//! #62 / grand-synthesis M1 — N-PEER mesh convergence over the live bridge, with
//! NO coordinator. Three `MeshAgent`s, each HOSTING its replica at a rest server
//! and seeded with its OWN per-source key, gossip pairwise; after the rounds ALL
//! THREE hold the union {a, b, c}. The CRDT join is commutative + associative +
//! idempotent, so convergence is order-, duplication-, and pairing-independent at
//! any N. This is the mesh at the scale of `coordination.ys`'s three peers
//! (mesh, manifold, synth).

use std::sync::Arc;
use std::time::Duration;

use prism_bigraph::protocols::MeshAgent;
use prism_schema::{Schema, TypeRegistry, Value};

fn pool(key: &str, v: f64) -> Value {
    Value::tree([(key, Value::float(v))])
}

fn types() -> Arc<TypeRegistry> {
    Arc::new(TypeRegistry::new())
}

#[test]
fn three_peers_converge_over_the_live_bridge_with_no_coordinator() {
    let schema = Schema::map(Schema::float());
    let a = MeshAgent::host(schema.clone(), pool("a", 1.0), types()).unwrap();
    let b = MeshAgent::host(schema.clone(), pool("b", 2.0), types()).unwrap();
    let c = MeshAgent::host(schema.clone(), pool("c", 3.0), types()).unwrap();
    std::thread::sleep(Duration::from_millis(50)); // servers up

    // Before: each peer holds only its own per-source key.
    assert_eq!(a.replica().as_map().map(|m| m.len()), Some(1));

    // One round each: every agent gossips (push-pull) with the other two, over the
    // live bridge, in arbitrary order. No coordinator, no central copy.
    a.sync_round(&[b.port(), c.port()]);
    b.sync_round(&[a.port(), c.port()]);
    c.sync_round(&[a.port(), b.port()]);

    // All three converged to the union.
    let want = Value::tree([
        ("a", Value::float(1.0)),
        ("b", Value::float(2.0)),
        ("c", Value::float(3.0)),
    ]);
    for (name, agent) in [("a", &a), ("b", &b), ("c", &c)] {
        assert_eq!(agent.replica(), want, "peer {name} converged to the union {want:?}");
    }

    // Anti-entropy: another full round changes nothing (idempotent at N peers).
    a.sync_round(&[b.port(), c.port()]);
    b.sync_round(&[a.port(), c.port()]);
    assert_eq!(a.replica(), want, "repeated rounds are idempotent");
    assert_eq!(b.replica(), want);
}
