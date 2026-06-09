//! #62 / Demo 2 LIVE — the streaming form. A [`LiveField`] gossips a mesh field
//! CONTINUOUSLY in the background while a tick loop reads + writes it, so the field
//! stays converged with the peer WHILE the engines run — no batch of explicit rounds
//! (those 400 rounds become a live stream). This is the integration point manifold's
//! Kuramoto tiles (and synth's A8 voices) plug into: each tile `sync`s its own keys
//! per tick and reads back the converged field.

use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use prism_bigraph::protocols::LiveField;
use prism_schema::{Schema, TypeRegistry, Value};

fn vec2(x: f64, y: f64) -> Value {
    Value::List(vec![Value::float(x), Value::float(y)])
}

fn types() -> Arc<TypeRegistry> {
    Arc::new(TypeRegistry::new())
}

/// The Demo-2 carrier: per-source `map[overwrite[array[[2], float]]]` (overwrite so a
/// per-tick refresh + gossip re-delivery both REPLACE — never accumulate).
fn carrier() -> Schema {
    Schema::map(Schema::overwrite(Schema::Array {
        shape: vec![2],
        element: Box::new(Schema::float()),
    }))
}

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

fn key_count(f: &Value) -> usize {
    f.as_map().map(|m| m.len()).unwrap_or(0)
}

#[test]
fn a_live_field_streams_convergence_while_the_loop_runs() {
    let tick = Duration::from_millis(10);
    // Two tiles with DISJOINT keys (a0,a1 / b0,b1) — single-writer-per-key.
    let mut a = LiveField::host(carrier(), Value::Map(IndexMap::new()), types()).unwrap();
    let mut b = LiveField::host(carrier(), Value::Map(IndexMap::new()), types()).unwrap();
    a.go_live(vec![b.port()], tick);
    b.go_live(vec![a.port()], tick);
    std::thread::sleep(Duration::from_millis(50)); // servers up

    // Each tile ticks: publish its own evolving keys, read the converged field. (The
    // dynamics is a placeholder — Kuramoto coupling is manifold's; the mesh's job is
    // to keep the field converged LIVE while the loop runs, which is what we assert.)
    for t in 0..20 {
        let phase = t as f64 * 0.1;
        a.sync(&Value::tree([("a0", vec2(phase, 0.0)), ("a1", vec2(0.0, phase))]));
        b.sync(&Value::tree([("b0", vec2(-phase, 0.0)), ("b1", vec2(0.0, -phase))]));
        std::thread::sleep(tick);
    }

    // Both tiles converged on the FULL field (all 4 keys) — live, no explicit round.
    assert!(
        wait_until(Duration::from_secs(20), || key_count(&a.field()) == 4
            && key_count(&b.field()) == 4),
        "both tiles see the full field live: a={:?} b={:?}",
        a.field(),
        b.field()
    );

    // A LIVE update on one tile streams to the other (background gossip carries it)
    // and OVERWRITES — no accumulation (the per-tick refresh property, end to end).
    a.sync(&Value::tree([("a0", vec2(9.0, 9.0))]));
    assert!(
        wait_until(Duration::from_secs(20), || b.field().get_field("a0").cloned()
            == Some(vec2(9.0, 9.0))),
        "B received A's live update, overwritten not accumulated: b.a0={:?}",
        b.field().get_field("a0")
    );
}
