//! #62 / grand-synthesis M1 — the `.ys` mesh link, FULLY DISTRIBUTED: two
//! `.ys`-declared `mesh` links converge over a LIVE rest bridge (a real socket).
//!
//! This COMPOSES the two transports, no new mechanism:
//!   • slice 5 (`mesh_link_runtime.rs`) — `mesh_links` reflection + the converged
//!     value written back through `Engine::state_mut`;
//!   • the live bridge (`prism-bigraph/tests/mesh_live_bridge.rs`) — each peer
//!     HOSTS its mesh slot as a `MeshReplica` at a rest server, and gossips over
//!     HTTP (`MeshReplica::gossip`, the push-pull CRDT round).
//! Deployable as-is by pointing each peer at the other's Tailscale IP instead of
//! `127.0.0.1`. The mesh is an ADDRESS, not a mode: the `.ys` is unchanged from
//! the in-process run — only the transport between the replicas differs.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{mesh_links, MeshReplica, RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Engine, Protocol};
use prism_schema::{Key, Schema, TypeRegistry, Value};

fn types() -> Arc<TypeRegistry> {
    Arc::new(TypeRegistry::new())
}

/// The canonical mesh-safe link in the demo `.ys` below.
fn link_schema() -> Schema {
    Schema::map(Schema::float())
}

fn run_peer(src: &str) -> Engine {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine");
    engine.discover_all_processes();
    engine.run(1.0);
    engine
}

/// HOST a mesh link's replica (seeded from `seed`) at a rest server, sharing
/// `slot` so the converged value can be read back. Every `initialize` builds a
/// replica sharing `slot`, so the peer's replica lives HERE, reachable over HTTP.
fn host_replica(seed: Value) -> (RestProcessServer, Arc<Mutex<Value>>) {
    let slot = Arc::new(Mutex::new(seed));
    let s = Arc::clone(&slot);
    let mut processes = ProcessRegistry::new();
    processes.register("Link", move |_| {
        ProcessNode::Process(Box::new(
            MeshReplica::shared(link_schema(), Arc::clone(&s), types()).expect("mesh-safe"),
        ))
    });
    let server = RestProcessServer::start(Core::from(Arc::new(processes))).expect("server");
    (server, slot)
}

/// A rest CLIENT onto a peer's hosted `Link` replica — the live-bridge handle a
/// local replica gossips through.
fn client_to(port: u16) -> Box<dyn Process> {
    let mut processes = ProcessRegistry::new();
    processes.register("Link", move |_| {
        ProcessNode::Process(Box::new(
            MeshReplica::shared(link_schema(), Arc::new(Mutex::new(Value::Map(IndexMap::new()))), types())
                .expect("mesh-safe"),
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

fn write_slot(e: &mut Engine, name: &str, value: Value) {
    if let Value::Map(m) = e.state_mut() {
        m.insert(Key::from(name), value);
    }
}

const PEER_A: &str = r#"
composite PeerA ->{ pool :: map[float] @ shared } (
  link shared :: map[float] mesh = { a: 1.0 }
)
PeerA[]
"#;

const PEER_B: &str = r#"
composite PeerB ->{ pool :: map[float] @ shared } (
  link shared :: map[float] mesh = { b: 2.0 }
)
PeerB[]
"#;

#[test]
fn two_ys_mesh_links_converge_over_a_live_rest_bridge() {
    let mut a = run_peer(PEER_A);
    let mut b = run_peer(PEER_B);

    for name in mesh_links(a.state()) {
        let seed_a = a.state().get_field(&name).cloned().unwrap();
        let seed_b = b.state().get_field(&name).cloned().unwrap();

        // Each peer HOSTS its mesh slot as a replica at its OWN rest server.
        let (server_a, slot_a) = host_replica(seed_a);
        let (server_b, slot_b) = host_replica(seed_b);
        std::thread::sleep(Duration::from_millis(50)); // servers up

        // Each peer gossips with the other OVER THE LIVE BRIDGE (push-pull). One
        // round converges both; the reverse is idempotent (anti-entropy) and shows
        // the symmetric peer↔peer topology — no coordinator.
        let la = MeshReplica::shared(link_schema(), Arc::clone(&slot_a), types()).unwrap();
        let lb = MeshReplica::shared(link_schema(), Arc::clone(&slot_b), types()).unwrap();
        la.gossip(client_to(server_b.port()).as_ref());
        lb.gossip(client_to(server_a.port()).as_ref());

        // Write the converged replicas back into each engine's slot.
        let va = slot_a.lock().unwrap().clone();
        let vb = slot_b.lock().unwrap().clone();
        write_slot(&mut a, &name, va);
        write_slot(&mut b, &name, vb);
    }

    // Both `.ys` engines' `mesh` link converged across the socket — no coordinator.
    let sa = a.state().get_field("shared").cloned().unwrap();
    let sb = b.state().get_field("shared").cloned().unwrap();
    assert_eq!(sa, sb, "the .ys mesh link converged over the live bridge: a={sa:?} b={sb:?}");
    assert_eq!(sa.get_field("a").and_then(|v| v.as_f64()), Some(1.0));
    assert_eq!(sa.get_field("b").and_then(|v| v.as_f64()), Some(2.0));
}
