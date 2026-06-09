//! #62 / grand-synthesis M1 slice 3 — CONTINUOUS gossip / anti-entropy.
//!
//! Each `MeshAgent` runs a BACKGROUND gossip loop (`start_gossip`); the mesh
//! converges and STAYS converged with no explicit `sync_round` calls, and a live
//! `contribute` on ANY peer propagates to all (eventual consistency). The CRDT join
//! (the schema `merge`) makes it order-, duplication-, and timing-independent — so
//! the mesh is self-maintaining, not one-shot. This is what a long-lived peer (a
//! coordination daemon, a streaming voice, a colony subdomain) needs.

use std::sync::Arc;
use std::time::{Duration, Instant};

use prism_bigraph::protocols::MeshAgent;
use prism_schema::{Schema, TypeRegistry, Value};

fn pool(key: &str, v: f64) -> Value {
    Value::tree([(key, Value::float(v))])
}

fn types() -> Arc<TypeRegistry> {
    Arc::new(TypeRegistry::new())
}

/// Poll `cond` until true or `timeout` elapses — the standard eventual-consistency
/// check (wait for the condition, bounded; not a fixed sleep).
fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cond()
}

#[test]
fn continuous_gossip_converges_and_propagates_live_updates() {
    let schema = Schema::map(Schema::float());
    let a = MeshAgent::host(schema.clone(), pool("a", 1.0), types()).unwrap();
    let b = MeshAgent::host(schema.clone(), pool("b", 2.0), types()).unwrap();
    let c = MeshAgent::host(schema.clone(), pool("c", 3.0), types()).unwrap();
    std::thread::sleep(Duration::from_millis(50)); // servers up

    // Each agent gossips CONTINUOUSLY with the other two — no explicit sync_round.
    let tick = Duration::from_millis(10);
    let _ga = a.start_gossip(vec![b.port(), c.port()], tick);
    let _gb = b.start_gossip(vec![a.port(), c.port()], tick);
    let _gc = c.start_gossip(vec![a.port(), b.port()], tick);

    // Eventually all three hold the union {a,b,c} — converged with no coordinator
    // and no explicit calls (the background loops + the CRDT join do it).
    let union3 = Value::tree([
        ("a", Value::float(1.0)),
        ("b", Value::float(2.0)),
        ("c", Value::float(3.0)),
    ]);
    assert!(
        wait_until(Duration::from_secs(3), || a.replica() == union3
            && b.replica() == union3
            && c.replica() == union3),
        "continuous gossip converged all 3: a={:?} b={:?} c={:?}",
        a.replica(),
        b.replica(),
        c.replica()
    );

    // A LIVE update on one peer (its own per-source key) propagates to all — pure
    // anti-entropy, no explicit push: `contribute` then the loops carry it.
    a.contribute(&pool("a2", 9.0));
    let union4 = Value::tree([
        ("a", Value::float(1.0)),
        ("a2", Value::float(9.0)),
        ("b", Value::float(2.0)),
        ("c", Value::float(3.0)),
    ]);
    assert!(
        wait_until(Duration::from_secs(3), || b.replica() == union4
            && c.replica() == union4),
        "the live contribute propagated to all peers: b={:?} c={:?}",
        b.replica(),
        c.replica()
    );
    // The `_g*` handles drop here → the gossip loops stop + join cleanly.
}
