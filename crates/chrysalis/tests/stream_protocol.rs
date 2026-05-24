//! `StreamProcess` (the `stream:` protocol's parent side): a lock-step driver of a
//! child `.ys` running `--serve-stream`, over real OS pipes. Each `update` sends
//! one input frame and reads one output frame — so a child `.ys` is a `Process`
//! the engine could step like any local one. Proves the parent side of #19.

use prism_bigraph::{Core, Engine, Process, ProtocolRegistry, Schema, Value};
use prism_schema::algebra;

use chrysalis::stream::{StreamProcess, StreamProtocol, float_record};

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String(s.to_string())).collect())
}

#[test]
fn stream_process_drives_a_child_lock_step_over_pipes() {
    // Child: echoes its driven input `n` to output `out` (one tick at a time).
    let echo = std::env::temp_dir().join(format!("prism_sp_echo_{}.ys", std::process::id()));
    std::fs::write(
        &echo,
        "composite Echo ~{n :: Float @ v} ->{out :: Float @ v} (\n  v: n\n)\n",
    )
    .expect("write echo.ys");

    let sp = StreamProcess::with_binary(echo.to_str().unwrap(), env!("CARGO_BIN_EXE_chrysalis"));

    // Drive it n = 0,1,2 — one engine-style update per tick.
    let frame = |n: f64| Value::tree([("n", Value::float(n))]);
    let updates: Vec<Value> = (0..3)
        .map(|n| {
            sp.update(&frame(n as f64), 1.0)
                .into_value()
                .unwrap_or(Value::None)
        })
        .collect();
    drop(sp); // closes the pipe → child exits
    std::fs::remove_file(&echo).ok();

    // The child returns an output delta-log (the per-tick updates). Folding them
    // (as the engine would, applying each to the parent's output state) tracks the
    // child's echo of n = [0, 1, 2].
    let elem = float_record("out");
    let mut acc = Value::tree([("out", Value::float(0.0))]);
    let mut trajectory = Vec::new();
    for u in &updates {
        acc = algebra::apply(&elem, &acc, u);
        trajectory.push(
            acc.get_field("out")
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN),
        );
    }
    assert_eq!(
        trajectory,
        vec![0.0, 1.0, 2.0],
        "the parent's output tracks the child's echo, driven lock-step over pipes; got {trajectory:?}"
    );
}

#[test]
fn engine_steps_a_stream_node_over_pipes() {
    // The full protocol: the ENGINE discovers a `stream:echo.ys` node and steps it
    // like any local process — input `n`=5 wired in, output `out` wired to `total`.
    let echo = std::env::temp_dir().join(format!("prism_sp_e2e_{}.ys", std::process::id()));
    std::fs::write(
        &echo,
        "composite Echo ~{n :: Float @ v} ->{out :: Float @ v} (\n  v: n\n)\n",
    )
    .expect("write echo.ys");

    let driver = Value::tree([
        (
            "address",
            Value::from(format!("stream:{}", echo.to_str().unwrap()).as_str()),
        ),
        ("config", Value::map()),
        ("interval", Value::float(1.0)),
        ("inputs", Value::tree([("n", wire(&["n"]))])),
        ("outputs", Value::tree([("out", wire(&["total"]))])),
    ]);
    let state = Value::tree([
        ("n", Value::float(5.0)),
        ("total", Value::float(0.0)),
        ("driver", driver),
    ]);

    // A Core whose `stream` protocol points at the cargo-built binary.
    let mut protocols = ProtocolRegistry::new();
    protocols.register(std::sync::Arc::new(StreamProtocol {
        binary: Some(env!("CARGO_BIN_EXE_chrysalis").to_string()),
    }));
    let core = Core::new().with_protocols(std::sync::Arc::new(protocols));

    let mut engine = Engine::from_state(Schema::Any, state, core).expect("engine");
    engine.discover_all_processes();
    engine.run(3.0);
    let total = engine
        .state()
        .get_field("total")
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0);
    drop(engine);
    std::fs::remove_file(&echo).ok();

    assert_eq!(
        total, 5.0,
        "the engine stepped the stream:echo.ys child; its echo of n=5 reached `total` over pipes (got {total})"
    );
}
