//! #26 (the peer-bridge keystone) — SLICE 1: a SHARED LINK with NO central
//! store, REPLICATED across two peer engines and converging over live bridges.
//!
//! "Where is a peer-to-peer shared link stored?" — nowhere single. Each peer
//! holds a REPLICA; the shared link IS the agreement between them (the place
//! graph has locations, but a link is a link-graph HYPEREDGE — a relation, not a
//! located object). The realization here is the symmetric/replicated one: each
//! peer co-owns the link as a local replica, and the peers exchange their
//! contributions over a live rest bridge until the replicas CONVERGE — no
//! coordinator, no home node.
//!
//! Convergence with no coordinator needs the merge to be a CRDT (commutative /
//! associative / idempotent). The link's SCHEMA reconcile is that merge; here the
//! contributions are disjoint per-peer keys, so the merge is a key UNION (a G-Set
//! — trivially a CRDT; the same shape a distributed `map[Reaction]` pool has).
//!
//! SLICE 1 is two replicas exchanging once each (testable on localhost,
//! deployable by pointing each peer at the other's Tailscale IP). Continuous
//! per-tick gossip and N-peer meshes are later slices.

use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Protocol, Schema, StateMap, Update, Value};

/// One peer's local REPLICA of the shared link. The peer co-owns the link as
/// this slot — there is no central copy. Receiving a peer's `contribution`
/// MERGES it in (a CRDT union; the link's schema-reconcile in general) and
/// returns the merged replica. The peer's engine reads/writes this same `Arc`.
#[derive(Debug)]
struct Replica {
    slot: Arc<Mutex<StateMap>>,
}
impl Process for Replica {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("contribution".to_string(), Schema::Any)])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("link".to_string(), Schema::Any)])
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut slot = self.slot.lock().unwrap();
        if let Some(contribution) = state.get_field("contribution").and_then(|v| v.as_map()) {
            for (k, v) in contribution {
                slot.insert(k.clone(), v.clone());
            }
        }
        Update::value(Value::tree([("link", Value::Map(slot.clone()))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A peer node: a rest SERVER hosting its own `Replica` (its co-owned copy of the
/// link) over `slot`. Returns the server + the port. The peer's replica lives
/// HERE, at this node — not at a coordinator.
fn peer_node(slot: Arc<Mutex<StateMap>>) -> RestProcessServer {
    let mut processes = ProcessRegistry::new();
    processes.register("Replica", move |_| {
        ProcessNode::Process(Box::new(Replica { slot: Arc::clone(&slot) }))
    });
    let core = Core::from(Arc::new(processes));
    RestProcessServer::start(core).expect("peer server")
}

/// A client onto a peer's `Replica` — how one peer pushes its contribution to
/// the OTHER peer's replica.
fn link_to(port: u16) -> Box<dyn Process> {
    let core = {
        let mut processes = ProcessRegistry::new();
        processes.register("Replica", |_| {
            ProcessNode::Process(Box::new(Replica { slot: Arc::new(Mutex::new(IndexMap::new())) }))
        });
        Core::from(Arc::new(processes))
    };
    let addr = Value::Map(IndexMap::from_iter([
        ("process".into(), Value::String("Replica".into())),
        ("host".into(), Value::String("127.0.0.1".into())),
        ("port".into(), Value::Int(port as i64)),
    ]));
    match RestProtocol.instantiate(&addr, Value::None, &core).expect("rest Replica") {
        ProcessNode::Process(p) => p,
        _ => panic!("expected a Process"),
    }
}

fn push(client: &dyn Process, k: &str, v: &str) {
    let state = Value::tree([("contribution", Value::tree([(k, Value::String(v.into()))]))]);
    client.update(&state, 1.0);
}

#[test]
fn a_shared_link_replicated_across_two_peers_converges_with_no_coordinator() {
    // Each peer's replica lives at the peer — there is NO central store.
    let alice_slot: Arc<Mutex<StateMap>> =
        Arc::new(Mutex::new(IndexMap::from_iter([("alice".into(), Value::String("left".into()))])));
    let bob_slot: Arc<Mutex<StateMap>> =
        Arc::new(Mutex::new(IndexMap::from_iter([("bob".into(), Value::String("right".into()))])));

    let alice = peer_node(Arc::clone(&alice_slot));
    let bob = peer_node(Arc::clone(&bob_slot));
    std::thread::sleep(Duration::from_millis(50));

    // Before exchange: each peer only has its OWN contribution.
    assert_eq!(alice_slot.lock().unwrap().len(), 1);
    assert_eq!(bob_slot.lock().unwrap().len(), 1);

    // Symmetric exchange over live bridges: alice → bob's replica, bob → alice's.
    let alice_to_bob = link_to(bob.port());
    let bob_to_alice = link_to(alice.port());
    push(alice_to_bob.as_ref(), "alice", "left"); // alice's contribution lands in bob's replica
    push(bob_to_alice.as_ref(), "bob", "right"); // bob's contribution lands in alice's replica

    // Both replicas now hold BOTH contributions — converged, with no coordinator
    // and no central copy. The shared link IS this agreement between the two
    // co-owned replicas.
    let a = alice_slot.lock().unwrap().clone();
    let b = bob_slot.lock().unwrap().clone();
    assert_eq!(a, b, "the two replicas converged to the same link: alice={a:?} bob={b:?}");
    assert_eq!(a.get("alice").and_then(|v| v.as_str()), Some("left"));
    assert_eq!(a.get("bob").and_then(|v| v.as_str()), Some("right"));

    // Idempotent (CRDT): re-pushing the same contribution changes nothing — so the
    // convergence is robust to retries / duplicate delivery, no coordinator needed.
    push(alice_to_bob.as_ref(), "alice", "left");
    assert_eq!(*bob_slot.lock().unwrap(), b, "re-delivery is idempotent (CRDT union)");
}
