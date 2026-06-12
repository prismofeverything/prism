//! The `registry` transport (#67 Phase 4, the `mesh` deliverable): a package
//! registry as a keyed, immutable Value store over HTTP — the remote backend
//! `RemoteRegistry` (chrysalis) calls. Proves the seam end-to-end over a REAL
//! socket + the CRDT-of-theories property that makes the registry a mesh node.

use prism_bigraph::protocols::registry::{
    self, package_schema, registry_carrier_schema, RegistryServer,
};
use prism_schema::{algebra, Key, StateMap, Value};

/// A package as `.ys`-as-data: `map[relpath → file-string]`.
fn pkg(files: &[(&str, &str)]) -> Value {
    let mut m = StateMap::new();
    for (k, v) in files {
        m.insert(Key::from(*k), Value::String((*v).to_string()));
    }
    Value::Map(m)
}

#[test]
fn the_carrier_is_a_crdt_of_theories() {
    // The closure invariant PROVES "the registry is a CRDT of theories": its
    // carrier map[name → map[version → Const<package>]] is a join-semilattice, so
    // replication converges with no coordinator.
    assert!(
        algebra::mesh_safety(&registry_carrier_schema()).is_ok(),
        "the registry carrier must be mesh-safe (grow-only key-union)"
    );
}

#[test]
fn publish_then_fetch_round_trips_over_http() {
    let server = RegistryServer::start().unwrap();
    let base = server.base_url();
    let greet = pkg(&[
        ("project.ys", "def package = { name: 'greet', version: '1.0.0' }"),
        ("lib.ys", "process Tick ~{n :: Float} ->{n :: Float} ( {n: 1.0} )"),
    ]);

    // publish (client → server), then fetch (server → client) over a real socket.
    registry::publish(&base, "greet", "1.0.0", &greet).expect("publish");
    let fetched = registry::fetch(&base, "greet", "1.0.0").expect("fetch");

    // The package round-trips structurally identical through the boundary codec.
    assert_eq!(fetched, greet, "fetched package must equal what was published");

    // versions() lists the published version (a string — chrysalis parses to Version).
    let mut vs = registry::versions(&base, "greet").unwrap();
    vs.sort();
    assert_eq!(vs, vec!["1.0.0".to_string()]);

    // An unknown package has no versions (empty, not an error — matches LocalRegistry).
    assert!(registry::versions(&base, "ghost").unwrap().is_empty());

    // Fetching an absent version is an error.
    assert!(registry::fetch(&base, "greet", "9.9.9").is_err());
}

#[test]
fn a_published_version_is_immutable_over_http() {
    let server = RegistryServer::start().unwrap();
    let base = server.base_url();
    registry::publish(&base, "foo", "1.0.0", &pkg(&[("project.ys", "x")])).unwrap();

    // Re-publishing the SAME name@version is refused (409 → an error) — a published
    // theory never silently changes under a consumer.
    let err = registry::publish(&base, "foo", "1.0.0", &pkg(&[("project.ys", "y")]))
        .expect_err("re-publish must be refused");
    let msg = format!("{err}");
    assert!(msg.contains("immutable") || msg.contains("already published"), "got: {msg}");

    // A different version of the same package is fine (the registry is grow-only).
    registry::publish(&base, "foo", "1.1.0", &pkg(&[("project.ys", "z")])).unwrap();
    let mut vs = registry::versions(&base, "foo").unwrap();
    vs.sort();
    assert_eq!(vs, vec!["1.0.0".to_string(), "1.1.0".to_string()]);
    assert_eq!(server.entry_count(), 2);
}

#[test]
fn many_packages_and_versions_over_http() {
    let server = RegistryServer::start().unwrap();
    let base = server.base_url();
    for (name, ver) in [("synth", "0.1.0"), ("synth", "0.2.0"), ("quantum", "0.1.0")] {
        registry::publish(&base, name, ver, &pkg(&[("project.ys", name)])).unwrap();
    }
    let mut synth = registry::versions(&base, "synth").unwrap();
    synth.sort();
    assert_eq!(synth, vec!["0.1.0".to_string(), "0.2.0".to_string()]);
    assert_eq!(registry::versions(&base, "quantum").unwrap(), vec!["0.1.0".to_string()]);
    // each package fetches back its own content
    let q = registry::fetch(&base, "quantum", "0.1.0").unwrap();
    assert_eq!(q, pkg(&[("project.ys", "quantum")]));
}

#[test]
fn two_registries_converge_no_coordinator() {
    // The mesh payoff: because the carrier is mesh-safe, two registries' snapshots
    // `merge` (the schema's state-based CRDT join — the SAME algebra join every mesh
    // boundary uses) to the UNION, with no coordinator. The registry replicates as a
    // mesh node. Each peer publishes a different package:
    let east = RegistryServer::start().unwrap();
    let west = RegistryServer::start().unwrap();
    registry::publish(&east.base_url(), "bio", "1.0.0", &pkg(&[("project.ys", "bio")])).unwrap();
    registry::publish(&west.base_url(), "synth", "1.0.0", &pkg(&[("project.ys", "synth")])).unwrap();

    let carrier = registry_carrier_schema();
    let e = east.snapshot();
    let w = west.snapshot();

    // merge converges to the union…
    let merged = algebra::merge(&carrier, &e, &w);
    let union = algebra::merge(&carrier, &w, &e);
    assert_eq!(merged, union, "merge is commutative (a join-semilattice)");

    // …and is idempotent (re-delivering a peer's state does not double anything).
    let again = algebra::merge(&carrier, &merged, &e);
    assert_eq!(again, merged, "merge is idempotent (CALM — coordination-free)");

    // The converged registry holds BOTH theories.
    let top = merged.as_map().expect("carrier is a map");
    assert!(top.contains_key("bio"), "converged registry has bio");
    assert!(top.contains_key("synth"), "converged registry has synth");
}

#[test]
fn package_schema_is_the_recommended_shape() {
    // map[relpath → string] conforms to package_schema (a sanity check the codec
    // element schema matches the recommended package shape).
    let p = pkg(&[("a.ys", "1"), ("b/c.ys", "2")]);
    let encoded = algebra::serialize_with(None, &package_schema(), &p);
    let decoded = algebra::realize_with(None, &package_schema(), &encoded);
    assert_eq!(decoded, p);
}
