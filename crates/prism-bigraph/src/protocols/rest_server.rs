//! `rest` process **server** — exposes a [`ProcessRegistry`] over HTTP, the other
//! half of [`super::rest`] (the client). Port of upstream
//! `rest-process/rest_process/server.py` (`process_bigraph/server/rest.py`).
//!
//! Wire protocol (matches the client):
//! ```text
//! POST /process/{class}/initialize    body=config            → "process_id"
//! GET  /process/{class}/inputs/{id}                          → {port: type}
//! GET  /process/{class}/outputs/{id}                         → {port: type}
//! POST /process/{class}/update/{id}   body={state,interval}  → update | null
//! POST /process/{class}/end/{id}                             → null   (DELETES)
//! ```
//!
//! A [`super::RestProcess`] on another machine drives a process *here* exactly as
//! if it were local — the bridge boundary made real and enforced: the only
//! channel is the port interface, over HTTP. `end` deletes the instance, so a
//! client that ends every process it starts leaves nothing behind — observable
//! via [`RestProcessServer::live_count`].
//!
//! The HTTP layer is hand-rolled (`TcpListener` + minimal request parsing), the
//! same dependency-free approach as the `rest_protocol` test mock, since the wire
//! protocol is a handful of JSON routes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use prism_schema::algebra;
use prism_schema::schema::{json_to_value, value_to_json};

use crate::protocols::rest::record_schema;

use crate::core::Core;
use crate::process::ProcessNode;

/// Live process instances, keyed by the id `initialize` handed out. Shared across
/// connection-handler threads (each `Process`/`Step` is `Send + Sync`).
// Each process behind its OWN `Arc` so a handler can CLONE it out and release the
// map lock before running `update()` — otherwise concurrent updates serialize on
// the map (the rest-concurrency bottleneck, #21).
type Processes = Arc<Mutex<HashMap<String, Arc<ProcessNode>>>>;

/// An HTTP server exposing a [`ProcessRegistry`] over the rest-process wire
/// protocol. [`start`](RestProcessServer::start) binds an ephemeral port and
/// serves on a background thread; drop shuts it down and joins.
pub struct RestProcessServer {
    port: u16,
    processes: Processes,
    shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RestProcessServer {
    /// Start a server on `127.0.0.1:0` (an OS-chosen free port) serving `core`.
    /// Takes the WHOLE [`Core`] (never a registry subset — the Core-threading
    /// rule): `core.processes` builds the instances, `core.types` drives the
    /// boundary codec (a Custom-typed port crosses by schema), and the server
    /// `set_core`s each instance so a method-dispatching or composite node reaches
    /// `core.methods`/`core.protocols` too.
    pub fn start(core: Core) -> std::io::Result<Self> {
        Self::start_on(core, "127.0.0.1:0")
    }

    /// Start a server bound to `addr` (e.g. `"127.0.0.1:8088"`; port `0` = an
    /// OS-chosen free port — read it back via [`RestProcessServer::port`]).
    pub fn start_on(
        core: Core,
        addr: impl std::net::ToSocketAddrs,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        let processes: Processes = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let next_id = Arc::new(AtomicUsize::new(0));

        let thread = {
            let processes = Arc::clone(&processes);
            let shutdown = Arc::clone(&shutdown);
            thread::spawn(move || {
                while !shutdown.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let core = core.clone(); // cheap: every Core field is an Arc
                            let processes = Arc::clone(&processes);
                            let next_id = Arc::clone(&next_id);
                            thread::spawn(move || {
                                if let Err(e) = handle(stream, &core, &processes, &next_id) {
                                    eprintln!("rest server: {e}");
                                }
                            });
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            })
        };

        Ok(Self {
            port,
            processes,
            shutdown,
            thread: Some(thread),
        })
    }

    /// The bound port (use in a `rest` address's `port` field).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:{port}` — the base URL clients address.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Number of live process instances (initialized but not yet ended). Zero once
    /// a client has ended everything it started — the cleanup invariant.
    pub fn live_count(&self) -> usize {
        self.processes.lock().unwrap().len()
    }
}

impl Drop for RestProcessServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ── request handling (hand-rolled HTTP, like the rest_protocol test mock) ──

fn handle(
    mut stream: TcpStream,
    core: &Core,
    processes: &Processes,
    next_id: &AtomicUsize,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    // Request line: METHOD PATH HTTP/1.1
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    // Headers — we only need Content-Length.
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 || header == "\r\n" {
            break;
        }
        if let Some(rest) = header.to_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    let (status, payload) = route(&method, &path, &body, core, processes, next_id);

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn route(
    method: &str,
    path: &str,
    body: &str,
    core: &Core,
    processes: &Processes,
    next_id: &AtomicUsize,
) -> (&'static str, String) {
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method, segs.as_slice()) {
        // POST /process/{class}/initialize  body=config → "process_id"
        ("POST", ["process", class, "initialize"]) => {
            if !core.processes.contains(class) {
                return (
                    "404 Not Found",
                    json_string(&format!("process-not-found: {class}")),
                );
            }
            let config = json_to_value(&parse_json(body));
            // A composite document may reference inner processes this server's core
            // doesn't have — reject it rather than silently building a partial graph.
            let missing = core.missing_process_refs(&config);
            if !missing.is_empty() {
                return (
                    "400 Bad Request",
                    json_string(&format!(
                        "document references unregistered process(es): {}",
                        missing.join(", ")
                    )),
                );
            }
            match core.processes.create(class, config) {
                Some(mut node) => {
                    // Thread the WHOLE Core into the server-side instance, mirroring
                    // the engine's per-node `set_core` during discovery — so a
                    // method-dispatching process reaches `core.methods` and a
                    // composite's subengine inherits `core.types`/`core.protocols`.
                    // The method & protocol axes cross the boundary, not just types.
                    node.set_core(core.clone());
                    let id = format!("{class}-{}", next_id.fetch_add(1, Ordering::SeqCst));
                    processes.lock().unwrap().insert(id.clone(), Arc::new(node));
                    ("200 OK", json_string(&id))
                }
                None => (
                    "404 Not Found",
                    json_string(&format!("process-not-found: {class}")),
                ),
            }
        }
        // GET /process/{class}/inputs/{id}
        ("GET", ["process", _class, "inputs", id]) => port_response(processes, id, true),
        // GET /process/{class}/outputs/{id}
        ("GET", ["process", _class, "outputs", id]) => port_response(processes, id, false),
        // POST /process/{class}/update/{id}  body={state,interval} → update | null
        ("POST", ["process", _class, "update", id]) => {
            let parsed = parse_json(body);
            let state = json_to_value(parsed.get("state").unwrap_or(&serde_json::Value::Null));
            let interval = parsed
                .get("interval")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            // Clone the handle out and RELEASE the map lock BEFORE update(), so
            // concurrent updates (each on its own connection thread) run in
            // parallel instead of serializing on the shared map (#21).
            let node = {
                let map = processes.lock().unwrap();
                let Some(n) = map.get(*id) else {
                    return ("404 Not Found", "null".to_string());
                };
                Arc::clone(n)
            };
            // Decode the input + encode the output through the algebra (the one
            // codec door) so a typed/Custom port crosses by its SCHEMA, not a
            // schema-blind `value_to_json`. The input is a state (`realize_with`),
            // the output an update/delta (`serialize_update`). The registry is the
            // WHOLE Core's `types`, so a Custom/`Foreign` port dispatches its own
            // wire form instead of nulling. Mirrors the client; `json_to_value`/
            // `value_to_json` are the byte layer UNDER the door.
            let in_elem = record_schema(&node.inputs());
            let out_elem = record_schema(&node.outputs());
            let decoded = algebra::realize_with(Some(core.types.as_ref()), &in_elem, &state);
            let update = match &*node {
                ProcessNode::Process(p) => p.update(&decoded, interval),
                ProcessNode::Step(s) => s.update(&decoded),
            };
            match update.into_value() {
                Some(v) => (
                    "200 OK",
                    value_to_json(&algebra::serialize_update(Some(core.types.as_ref()), &out_elem, &v))
                        .to_string(),
                ),
                None => ("200 OK", "null".to_string()),
            }
        }
        // POST /process/{class}/end/{id} → null  (DELETE the instance)
        ("POST", ["process", _class, "end", id]) => {
            processes.lock().unwrap().remove(*id);
            ("200 OK", "null".to_string())
        }
        _ => ("404 Not Found", json_string("not found")),
    }
}

/// `inputs()` / `outputs()` → `{port: "<type>"}`, where `<type>` is the port's
/// Schema rendered as a type expression (`Schema`'s Display — the inverse of
/// `prism_schema::parse_type_expression`). The client reconstructs the REAL
/// Schema, so cross-boundary apply/reconcile speaks actual schemas — not `Any`.
fn port_response(processes: &Processes, id: &str, inputs: bool) -> (&'static str, String) {
    let map = processes.lock().unwrap();
    let Some(node) = map.get(id) else {
        return ("404 Not Found", "null".to_string());
    };
    let ports = if inputs {
        node.inputs()
    } else {
        node.outputs()
    };
    let obj: serde_json::Map<String, serde_json::Value> = ports
        .iter()
        .map(|(k, schema)| (k.clone(), serde_json::Value::String(schema.to_string())))
        .collect();
    ("200 OK", serde_json::Value::Object(obj).to_string())
}

fn parse_json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}
