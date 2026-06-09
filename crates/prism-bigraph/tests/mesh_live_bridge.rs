//! #62 / grand-synthesis M1 — a `mesh:` shared link converging over a LIVE rest
//! bridge, through the FIRST-CLASS protocol (`MeshReplica` — gated + algebra-merge).
//!
//! The protocol-level generalization of `peer_shared_link.rs`'s slice 1: two peers
//! each HOST their replica at a rest server, exchange contribution STATES over
//! HTTP, and converge with **no coordinator**. The difference from slice 1: the
//! replica is the first-class `MeshReplica` (built only through the mesh-safe
//! GATE), and the merge is the schema's `merge` (the state-based CRDT join — the
//! boundary codec carries the STATE, so no delta-in support is needed). The link
//! type is the canonical mesh-safe form, a per-source `map[float]` pool.
//!
//! Deployable as written by pointing each peer at the other's Tailscale IP
//! instead of `127.0.0.1`; localhost here keeps it a `cargo test`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{MeshReplica, RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Protocol, Schema, Value};
use prism_schema::TypeRegistry;

/// The canonical mesh-safe link: a per-source `map[float]` pool (each peer owns
/// its own key).
fn link_schema() -> Schema {
    Schema::map(Schema::float())
}

/// A peer node: a rest SERVER hosting its own gated `MeshReplica` over `slot`.
/// Every `initialize` builds a replica SHARING `slot`, so the peer's replica
/// lives HERE — not at a coordinator. The factory uses `MeshReplica::shared`,
/// which GATES the schema through `mesh_safety`; an unsafe link would refuse.
fn peer_node(slot: Arc<Mutex<Value>>) -> RestProcessServer {
    let types = Arc::new(TypeRegistry::new());
    let mut processes = ProcessRegistry::new();
    processes.register("Link", move |_| {
        ProcessNode::Process(Box::new(
            MeshReplica::shared(link_schema(), Arc::clone(&slot), Arc::clone(&types))
                .expect("mesh-safe link"),
        ))
    });
    RestProcessServer::start(Core::from(Arc::new(processes))).expect("peer server")
}

/// A rest CLIENT onto a peer's `Link` replica — how one peer ships its
/// contribution STATE to the OTHER peer's replica over HTTP. (The client builds
/// the HTTP forwarder and fetches the server's port schema; the throwaway local
/// factory just satisfies the registry.)
fn link_to(port: u16) -> Box<dyn Process> {
    let types = Arc::new(TypeRegistry::new());
    let mut processes = ProcessRegistry::new();
    processes.register("Link", move |_| {
        ProcessNode::Process(Box::new(
            MeshReplica::shared(
                link_schema(),
                Arc::new(Mutex::new(Value::Map(IndexMap::new()))),
                Arc::clone(&types),
            )
            .expect("mesh-safe link"),
        ))
    });
    let core = Core::from(Arc::new(processes));
    let addr = Value::Map(IndexMap::from_iter([
        ("process".into(), Value::String("Link".into())),
        ("host".into(), Value::String("127.0.0.1".into())),
        ("port".into(), Value::Int(port as i64)),
    ]));
    match RestProtocol.instantiate(&addr, Value::None, &core).expect("rest Link") {
        ProcessNode::Process(p) => p,
        _ => panic!("expected a Process"),
    }
}

/// Ship a contribution STATE to a peer's replica over the live bridge.
fn push(client: &dyn Process, contribution: Value) {
    client.update(&Value::tree([("contribution", contribution)]), 1.0);
}

#[test]
fn a_mesh_link_converges_over_a_live_rest_bridge() {
    // Each peer's replica lives at the peer — NO central store. Per-source keys.
    let alice_slot: Arc<Mutex<Value>> =
        Arc::new(Mutex::new(Value::tree([("alice", Value::float(1.0))])));
    let bob_slot: Arc<Mutex<Value>> =
        Arc::new(Mutex::new(Value::tree([("bob", Value::float(2.0))])));

    let alice = peer_node(Arc::clone(&alice_slot));
    let bob = peer_node(Arc::clone(&bob_slot));
    std::thread::sleep(Duration::from_millis(50));

    // Before exchange: each peer holds only its OWN contribution.
    assert_eq!(alice_slot.lock().unwrap().as_map().map(|m| m.len()), Some(1));
    assert_eq!(bob_slot.lock().unwrap().as_map().map(|m| m.len()), Some(1));

    // Symmetric exchange over LIVE rest bridges: alice ships her state to bob's
    // replica, bob ships his to alice's. The server-side `MeshReplica` merges each
    // via the schema's `merge` — the boundary codec carries the STATE (no delta-in).
    let to_bob = link_to(bob.port());
    let to_alice = link_to(alice.port());
    push(to_bob.as_ref(), Value::tree([("alice", Value::float(1.0))]));
    push(to_alice.as_ref(), Value::tree([("bob", Value::float(2.0))]));

    // Both replicas now hold BOTH contributions — converged, with no coordinator
    // and no central copy. The shared link IS this agreement between the replicas.
    let a = alice_slot.lock().unwrap().clone();
    let b = bob_slot.lock().unwrap().clone();
    assert_eq!(a, b, "the two replicas converged: alice={a:?} bob={b:?}");
    assert_eq!(a.get_field("alice").and_then(|v| v.as_f64()), Some(1.0));
    assert_eq!(a.get_field("bob").and_then(|v| v.as_f64()), Some(2.0));

    // Idempotent (CRDT): re-shipping the same state changes nothing — robust to
    // retries / anti-entropy, no coordinator needed.
    push(to_bob.as_ref(), Value::tree([("alice", Value::float(1.0))]));
    assert_eq!(*bob_slot.lock().unwrap(), b, "re-delivery is idempotent");
}
