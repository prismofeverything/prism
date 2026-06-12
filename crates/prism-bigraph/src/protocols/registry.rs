//! `registry` — a package **registry as a keyed, immutable Value store over HTTP**:
//! the remote backend for chrysalis's package registry (#67 Phase 4). The transport
//! `chrysalis publish` / `chrysalis add` / the resolver drive when a registry is a
//! URL instead of a local dir.
//!
//! ## The registry IS a mesh of theories (reuse, not clone)
//!
//! A published `(name, version)` is **immutable**, and the keys are write-once, so
//! the whole store is a **grow-only** `map[name → map[version → Const<package>]]` —
//! and [`algebra::mesh_safety`] classifies that carrier as a join-**SEMILATTICE**
//! (key-union, idempotent + commutative; [`registry_carrier_schema`]). That is the
//! *proof*, not the slogan, that **the registry is a CRDT of theories**: two
//! registries converge with no coordinator (`merge` of their snapshots = the union).
//! It is the structural reason the remote backend is "the mesh-transport
//! generalization" — the **keyed projection** of the mesh's full-state gossip (a
//! `chrysalis add foo` is a *keyed* fetch, not a whole-replica pull).
//!
//! So this reuses the mesh transport rather than cloning a second HTTP stack:
//! - the **same** [`HttpServer`] accept-loop door as [`super::rest_server`] (one
//!   plumbing, two handlers);
//! - the **same** boundary byte-codec (`value_to_json` / `json_to_value`) a package
//!   crosses on, since a package is plain `.ys`-as-data (`map[path → string]`, no
//!   Custom-typed slot needing the algebra's type-dispatch door);
//! - the **same** [`algebra`] join (`merge`) the carrier converges under.
//!
//! ## Wire protocol
//! ```text
//! GET  /registry/{name}            → ["1.0.0", "1.2.0", …]          (version strings)
//! GET  /registry/{name}/{version}  → {"value": <package>, "checksum": "<hex>"}
//! PUT  /registry/{name}/{version}  body=<package> → {"checksum":"<hex>"} | 409 (immutable)
//! ```
//!
//! ## The seam `RemoteRegistry` (chrysalis) calls
//!
//! [`versions`] / [`fetch`] / [`publish`] are the client. The package `Value` shape
//! is the CALLER's choice (recommended: `map[relpath → file-string]`); this transport
//! is payload-agnostic. `RemoteRegistry::source` fetches the `Value`, materializes a
//! cache dir, and returns the `PathBuf` — so the resolver loads it exactly like a path
//! dependency, and the `Registry` trait is unchanged. Version *strings* cross the wire
//! (chrysalis owns the `Version`/semver semantics — the poset is upstream of the
//! transport). Integrity: [`fetch`] re-derives the [`checksum`] over the received
//! value and rejects a mismatch.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use prism_schema::{
    algebra,
    schema::{json_to_value, value_to_json},
    Key, Schema, StateMap, Value,
};

use crate::protocol::ProtocolError;
use crate::protocols::http::{self, HttpServer};

const DEFAULT_TIMEOUT_SECS: u64 = 30;

// ── schemas: the wire codec element + the CRDT proof ─────────────────

/// A published package as `.ys`-as-data: a map of relative path → file contents
/// (a nested dir flattens to `ys/demo.ys` → its text). It is `map[string]`, the
/// element schema the boundary codec serializes a package against. The transport
/// does not *require* this shape (it carries any `Value`), but it is the recommended
/// one and the schema the codec/checksum default to.
pub fn package_schema() -> Schema {
    Schema::map(Schema::string())
}

/// The whole registry STORE as a schema: `map[name → map[version → Const<package>]]`.
/// A published `(name, version)` is IMMUTABLE (`Const`) and write-once, so the store
/// is GROW-ONLY — and [`algebra::mesh_safety`] returns `Ok` for it (key-union ⇒ a
/// join-semilattice). The executable proof that **the registry is a CRDT of
/// theories** (asserted in `tests/registry_transport.rs`): replication is
/// coordination-free, so a mirror registry can `merge` snapshots and converge.
pub fn registry_carrier_schema() -> Schema {
    Schema::map(Schema::map(Schema::const_of(package_schema())))
}

// ── checksum: a deterministic, dependency-free content hash ───────────

/// A content checksum over a package `Value` — FNV-1a over a CANONICAL walk (map
/// keys sorted, every variant tagged), so it is **deterministic across processes
/// and builds** and independent of wire/JSON key ordering. Integrity against
/// corruption on the wire (and a content-address for the lockfile to pin). It is
/// NOT cryptographic — adversarial tamper-resistance (sha256) lands with the
/// transport's auth / capability-ref layer (the deferred extension point).
pub fn checksum(value: &Value) -> String {
    let mut h = Fnv::new();
    hash_value(&mut h, value);
    format!("{:016x}", h.0)
}

struct Fnv(u64);
impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325) // FNV offset basis
    }
    fn byte(&mut self, b: u8) {
        self.0 ^= b as u64;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3); // FNV prime
    }
    fn bytes(&mut self, bs: &[u8]) {
        for &b in bs {
            self.byte(b);
        }
    }
}

/// Canonical structural hash: a per-variant tag byte disambiguates (so `Int(0)`,
/// `Bool(false)`, `None` differ), map keys are SORTED (order-independent), and a
/// `0` terminator separates variable-length runs (so `["a","b"]` ≠ `["ab"]`).
fn hash_value(h: &mut Fnv, v: &Value) {
    match v {
        Value::None => h.byte(0),
        Value::Bool(b) => {
            h.byte(1);
            h.byte(*b as u8);
        }
        Value::Int(n) => {
            h.byte(2);
            h.bytes(&n.to_le_bytes());
        }
        Value::Float(f) => {
            h.byte(3);
            h.bytes(&f.0.to_le_bytes());
        }
        Value::String(s) => {
            h.byte(4);
            h.bytes(s.as_bytes());
            h.byte(0);
        }
        Value::Bytes(b) => {
            h.byte(5);
            h.bytes(b);
            h.byte(0);
        }
        Value::List(items) => {
            h.byte(6);
            for it in items {
                hash_value(h, it);
            }
            h.byte(0);
        }
        Value::Map(m) => {
            h.byte(7);
            let mut keys: Vec<&Key> = m.keys().collect();
            keys.sort();
            for k in keys {
                h.bytes(k.as_bytes());
                h.byte(0);
                hash_value(h, &m[k]);
            }
            h.byte(0);
        }
        // Struct / Foreign are not plain package data; a stable Debug encoding keeps
        // the hash total (they never appear in a `map[path → string]` package).
        other => {
            h.byte(8);
            h.bytes(format!("{other:?}").as_bytes());
        }
    }
}

// ── RegistryServer ───────────────────────────────────────────────────

/// The store: `name → version → package`. Behind one `Mutex` (publishes are rare;
/// fetches clone the small package out). Grow-only + write-once per the immutability
/// rule — the in-memory realization of [`registry_carrier_schema`].
type Store = Arc<Mutex<HashMap<String, HashMap<String, Value>>>>;

/// An HTTP server exposing a package registry — a [`route`] handler over the shared
/// [`HttpServer`] plumbing (the same door [`super::rest_server`] serves processes
/// on). Drop shuts it down and joins (the `HttpServer` field's own `Drop`).
pub struct RegistryServer {
    server: HttpServer,
    store: Store,
}

impl RegistryServer {
    /// Start a registry server on `127.0.0.1:0` (an OS-chosen free port — read it
    /// back via [`port`](RegistryServer::port) / [`base_url`](RegistryServer::base_url)).
    pub fn start() -> std::io::Result<Self> {
        Self::start_on("127.0.0.1:0")
    }

    /// Start bound to `addr` (e.g. `"0.0.0.0:8910"` to serve a mesh of consumers;
    /// port `0` = an OS-chosen free port).
    pub fn start_on(addr: impl std::net::ToSocketAddrs) -> std::io::Result<Self> {
        let store: Store = Arc::new(Mutex::new(HashMap::new()));
        let handler = {
            let store = Arc::clone(&store);
            move |req: &http::Request| route(req, &store)
        };
        let server = HttpServer::start(addr, handler)?;
        Ok(Self { server, store })
    }

    /// The bound port.
    pub fn port(&self) -> u16 {
        self.server.port()
    }

    /// `http://127.0.0.1:{port}` — the base URL clients address.
    pub fn base_url(&self) -> String {
        self.server.base_url()
    }

    /// Number of `(name, version)` entries published — for tests / introspection.
    pub fn entry_count(&self) -> usize {
        self.store.lock().unwrap().values().map(|m| m.len()).sum()
    }

    /// Publish `name@version` = `pkg` directly (no HTTP) — the in-process analogue of
    /// [`publish`], e.g. to pre-seed a mirror. IMMUTABLE: refuses an existing
    /// `(name, version)`.
    pub fn insert(&self, name: &str, version: &str, pkg: Value) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        let versions = store.entry(name.to_string()).or_default();
        if versions.contains_key(version) {
            return Err(format!(
                "`{name}@{version}` is already published (a registry version is immutable)"
            ));
        }
        versions.insert(version.to_string(), pkg);
        Ok(())
    }

    /// The store as the mesh CARRIER value (`map[name → map[version → package]]`,
    /// the shape of [`registry_carrier_schema`]). Because that carrier is mesh-safe,
    /// two servers' snapshots `algebra::merge` to the union — the registry replicates
    /// as a mesh node, no coordinator. The hook for mirror/replica registries.
    pub fn snapshot(&self) -> Value {
        let store = self.store.lock().unwrap();
        let mut outer = StateMap::new();
        for (name, versions) in store.iter() {
            let mut inner = StateMap::new();
            for (ver, pkg) in versions.iter() {
                inner.insert(Key::from(ver.as_str()), pkg.clone());
            }
            outer.insert(Key::from(name.as_str()), Value::Map(inner));
        }
        Value::Map(outer)
    }
}

/// The registry handler: GET versions / GET package / PUT package. The HTTP plumbing
/// (accept loop, parsing, response writing) lives in [`super::http`].
fn route(req: &http::Request, store: &Store) -> (&'static str, String) {
    let segs: Vec<&str> = req
        .path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    match (req.method.as_str(), segs.as_slice()) {
        // GET /registry/{name} → the version list (empty for an unknown package,
        // mirroring LocalRegistry — "no such package" is an empty set, not an error).
        ("GET", ["registry", name]) => {
            let store = store.lock().unwrap();
            let versions: Vec<serde_json::Value> = store
                .get(*name)
                .map(|m| {
                    m.keys()
                        .map(|v| serde_json::Value::String(v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            ("200 OK", serde_json::Value::Array(versions).to_string())
        }
        // GET /registry/{name}/{version} → {value, checksum}
        ("GET", ["registry", name, version]) => {
            let store = store.lock().unwrap();
            match store.get(*name).and_then(|m| m.get(*version)) {
                Some(pkg) => {
                    let value = value_to_json(&algebra::serialize_with(
                        None,
                        &package_schema(),
                        pkg,
                    ));
                    let body = serde_json::json!({ "value": value, "checksum": checksum(pkg) });
                    ("200 OK", body.to_string())
                }
                None => (
                    "404 Not Found",
                    json_string(&format!("not found: {name}@{version}")),
                ),
            }
        }
        // PUT /registry/{name}/{version} body=package → store IMMUTABLY (409 if present)
        ("PUT", ["registry", name, version]) => {
            let pkg = algebra::realize_with(
                None,
                &package_schema(),
                &json_to_value(&parse_json(&req.body)),
            );
            let mut store = store.lock().unwrap();
            let versions = store.entry((*name).to_string()).or_default();
            if versions.contains_key(*version) {
                return (
                    "409 Conflict",
                    json_string(&format!(
                        "already published: {name}@{version} (a registry version is immutable)"
                    )),
                );
            }
            let sum = checksum(&pkg);
            versions.insert((*version).to_string(), pkg);
            ("201 Created", serde_json::json!({ "checksum": sum }).to_string())
        }
        _ => ("404 Not Found", json_string("not found")),
    }
}

fn parse_json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

// ── the client (the seam `RemoteRegistry` calls) ─────────────────────

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()
}

fn other(msg: impl Into<String>) -> ProtocolError {
    ProtocolError::Other {
        protocol: "registry".into(),
        message: msg.into(),
    }
}

/// All published versions of `name` (as STRINGS — the caller owns version/semver
/// semantics). Empty ⇒ no such package (matching `LocalRegistry`).
pub fn versions(base_url: &str, name: &str) -> Result<Vec<String>, ProtocolError> {
    let url = format!("{base_url}/registry/{name}");
    let resp = agent()
        .get(&url)
        .call()
        .map_err(|e| other(format!("GET {url}: {e}")))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| other(format!("read versions {url}: {e}")))?;
    let arr = json
        .as_array()
        .ok_or_else(|| other(format!("versions {url}: expected a JSON array")))?;
    Ok(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
}

/// Fetch `name@version` — the package `Value`, integrity-checked: the server returns
/// `{value, checksum}`, and this re-derives the checksum over the received value and
/// REJECTS a mismatch (corruption / tampering on the wire).
pub fn fetch(base_url: &str, name: &str, version: &str) -> Result<Value, ProtocolError> {
    let url = format!("{base_url}/registry/{name}/{version}");
    let resp = agent()
        .get(&url)
        .call()
        .map_err(|e| map_status(e, &url, name, version))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| other(format!("read package {url}: {e}")))?;
    let value_json = json
        .get("value")
        .ok_or_else(|| other(format!("package {url}: response missing `value`")))?;
    let expected = json
        .get("checksum")
        .and_then(|c| c.as_str())
        .ok_or_else(|| other(format!("package {url}: response missing `checksum`")))?;
    let pkg = algebra::realize_with(None, &package_schema(), &json_to_value(value_json));
    let got = checksum(&pkg);
    if got != expected {
        return Err(other(format!(
            "checksum mismatch for {name}@{version}: expected {expected}, got {got} \
             (corruption or tampering)"
        )));
    }
    Ok(pkg)
}

/// Publish `name@version` = `pkg` to the remote registry. IMMUTABLE: an existing
/// `(name, version)` is refused (the server answers 409 → an error here), so a
/// published theory never silently changes under a consumer.
pub fn publish(
    base_url: &str,
    name: &str,
    version: &str,
    pkg: &Value,
) -> Result<(), ProtocolError> {
    let url = format!("{base_url}/registry/{name}/{version}");
    let body = value_to_json(&algebra::serialize_with(None, &package_schema(), pkg));
    agent()
        .put(&url)
        .send_json(body)
        .map(|_| ())
        .map_err(|e| map_status(e, &url, name, version))
}

/// Map a `ureq` HTTP error to a registry [`ProtocolError`] — naming the two states
/// the wire encodes (409 = immutable-already-published, 404 = absent).
fn map_status(e: ureq::Error, url: &str, name: &str, version: &str) -> ProtocolError {
    match e {
        ureq::Error::Status(409, _) => other(format!(
            "`{name}@{version}` is already published (a registry version is immutable)"
        )),
        ureq::Error::Status(404, _) => {
            other(format!("`{name}@{version}` is not in the registry"))
        }
        e => other(format!("{url}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(files: &[(&str, &str)]) -> Value {
        let mut m = StateMap::new();
        for (k, v) in files {
            m.insert(Key::from(*k), Value::String((*v).to_string()));
        }
        Value::Map(m)
    }

    #[test]
    fn carrier_is_mesh_safe() {
        // The registry IS a CRDT of theories — the closure invariant proves it.
        assert!(algebra::mesh_safety(&registry_carrier_schema()).is_ok());
    }

    #[test]
    fn checksum_is_deterministic_and_order_independent() {
        let a = pkg(&[("project.ys", "def package = {}"), ("ys/demo.ys", "Foo")]);
        let b = pkg(&[("ys/demo.ys", "Foo"), ("project.ys", "def package = {}")]);
        assert_eq!(checksum(&a), checksum(&b), "key order must not change the checksum");
        let c = pkg(&[("project.ys", "def package = {}"), ("ys/demo.ys", "Bar")]);
        assert_ne!(checksum(&a), checksum(&c), "different content must differ");
    }

    #[test]
    fn insert_is_immutable() {
        let server = RegistryServer::start().unwrap();
        server.insert("foo", "1.0.0", pkg(&[("project.ys", "x")])).unwrap();
        assert!(server.insert("foo", "1.0.0", pkg(&[("project.ys", "y")])).is_err());
        assert_eq!(server.entry_count(), 1);
    }
}
