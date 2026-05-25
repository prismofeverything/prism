//! `StreamProcess` (the `stream:` protocol's parent side): a lock-step driver of a
//! child `.ys` running `--serve-process`, over real OS pipes. The child runs its
//! entry as a real `Composite` and FORWARDS the composite's reconciled update
//! delta each tick (the pipe mirror of the `rest:` server) — so a child `.ys` is a
//! `Process` the engine steps like any local one, additive faces accumulating
//! correctly across the wire. This is a *delta-forwarder*, not a snapshot filter
//! (that's `serve_stream`, exercised in `tests/stream.rs`); a passthrough composite
//! with no inner dynamics therefore forwards nothing, exactly as it would locally.

use indexmap::IndexMap;
use prism_bigraph::{Core, Engine, Key, Process, ProtocolRegistry, Schema, Value};
use prism_schema::algebra;

use chrysalis::stream::{StreamProcess, StreamProtocol, float_record};

// A child composite with real inner dynamics: its inner `Tick` process adds +1 to
// `n` every tick, so the proxy forwards a +1 DELTA per tick (what a delta-forward
// process proxy does — vs an absolute-tracking snapshot filter).
const COUNTER: &str = "\
process Tick ~{n :: Float} ->{n :: Float} ( {n: 1.0} )

composite Counter[start :: Float = 0.0] ->{n :: Float @ n} (
  n: start |
  Tick ~{n: n} ->{n: n}
)
";

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String(s.to_string())).collect())
}

fn write_counter(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("prism_sp_{tag}_{}.ys", std::process::id()));
    std::fs::write(&p, COUNTER).expect("write counter.ys");
    p
}

#[test]
fn stream_process_drives_a_child_lock_step_over_pipes() {
    // Drive the Counter over pipes for 3 ticks. It has no inputs; each tick its
    // inner Tick adds +1 to `n`, so the child forwards a +1.0 DELTA each tick.
    let counter = write_counter("counter");
    let sp = StreamProcess::with_binary(counter.to_str().unwrap(), env!("CARGO_BIN_EXE_chrysalis"));

    let empty = Value::map();
    let updates: Vec<Value> = (0..3)
        .map(|_| sp.update(&empty, 1.0).into_value().unwrap_or(Value::None))
        .collect();
    drop(sp); // closes the pipe → child exits
    std::fs::remove_file(&counter).ok();

    // Folding the forwarded +1 deltas (as the engine would, applying each to the
    // parent's output state) tracks n = [1, 2, 3].
    let elem = float_record("n");
    let mut acc = Value::tree([("n", Value::float(0.0))]);
    let mut trajectory = Vec::new();
    for u in &updates {
        acc = algebra::apply(&elem, &acc, u);
        trajectory.push(acc.get_field("n").and_then(|v| v.as_f64()).unwrap_or(f64::NAN));
    }
    assert_eq!(
        trajectory,
        vec![1.0, 2.0, 3.0],
        "the proxy forwards a +1 delta each tick, driven lock-step over pipes; got {trajectory:?}"
    );
}

#[test]
fn engine_steps_a_stream_node_over_pipes() {
    // The full protocol: the ENGINE discovers a `stream:counter.ys` node and steps
    // it like any local process — its `n` output (a +1 delta/tick) wired to `total`,
    // which ACCUMULATES (additive Float) to 3 over 3 ticks.
    let counter = write_counter("counter_e2e");

    let driver = Value::tree([
        (
            "address",
            Value::from(format!("stream:{}", counter.to_str().unwrap()).as_str()),
        ),
        ("config", Value::map()),
        ("interval", Value::float(1.0)),
        ("inputs", Value::map()),
        ("outputs", Value::tree([("n", wire(&["total"]))])),
    ]);
    let state = Value::tree([("total", Value::float(0.0)), ("driver", driver)]);
    // `total` is an additive Float so the per-tick deltas accumulate (the driver is
    // address-discovered, so its schema can stay open).
    let schema = Schema::Tree {
        branches: IndexMap::from([
            (Key::from("total"), Schema::float()),
            (Key::from("driver"), Schema::Any),
        ]),
    };

    let mut protocols = ProtocolRegistry::new();
    protocols.register(std::sync::Arc::new(StreamProtocol {
        binary: Some(env!("CARGO_BIN_EXE_chrysalis").to_string()),
    }));
    let core = Core::new().with_protocols(std::sync::Arc::new(protocols));

    let mut engine = Engine::from_state(schema, state, core).expect("engine");
    engine.discover_all_processes();
    engine.run(3.0);
    let total = engine
        .state()
        .get_field("total")
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0);
    drop(engine);
    std::fs::remove_file(&counter).ok();

    assert_eq!(
        total, 3.0,
        "the engine stepped the stream:counter.ys child; its +1/tick deltas accumulated in `total` over pipes (got {total})"
    );
}
