//! Engine-level proof of #27 step 2.5: a composite of `parallel:` processes runs
//! its invoke pass CONCURRENTLY. The engine invokes every due process (each
//! `ParallelProcess::invoke` submits to the shared pool and returns a slot-`Defer`),
//! flushes the pool runtime (auto-registered from the `Core`'s protocols — the
//! `Protocol::runtime()` seam), then collects. So N sleeping processes finish in
//! ~one delay, not N — wall-clock proof, built via a real typed schema (no
//! `Schema::Any`).

use std::any::Any;
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocol::ProtocolRegistry;
use prism_bigraph::protocols::ParallelProtocol;
use prism_bigraph::{Core, Engine, Key, Schema, Update, Value};

/// A process that sleeps `delay_ms` then emits `+1.0` on `out`. The sleep makes
/// concurrency observable: 4 in parallel ≈ one delay, sequential ≈ four.
#[derive(Debug)]
struct SlowProcess {
    delay_ms: u64,
}
impl Process for SlowProcess {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), Schema::float())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        std::thread::sleep(Duration::from_millis(self.delay_ms));
        Update::value(Value::tree([("out", Value::float(1.0))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String(s.to_string())).collect())
}

/// A `parallel:Slow` node whose `out` accumulates into the shared `total`.
fn slow_node() -> Value {
    Value::tree([
        ("address", Value::String("parallel:Slow".into())),
        ("config", Value::map()),
        ("inputs", Value::map()),
        ("outputs", Value::tree([("out", wire(&["total"]))])),
    ])
}

fn slow_link() -> Schema {
    Schema::ProcessLink {
        inputs: IndexMap::new(),
        outputs: IndexMap::from([(Key::from("out"), Schema::float())]),
        interval: 1.0,
    }
}

fn core(workers: usize) -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Slow", |_config| {
        ProcessNode::Process(Box::new(SlowProcess { delay_ms: 50 }))
    });
    let mut protocols = ProtocolRegistry::new();
    protocols.register(Arc::new(ParallelProtocol::new(workers)));
    Core::from(Arc::new(registry)).with_protocols(Arc::new(protocols))
}

#[test]
fn engine_invoke_pass_runs_parallel_processes_concurrently() {
    const N: usize = 4;
    // total + N parallel:Slow nodes, each emitting +1 to total per tick.
    let mut branches: IndexMap<Key, Schema> =
        IndexMap::from([(Key::from("total"), Schema::float())]);
    let mut state: IndexMap<Key, Value> = IndexMap::from([(Key::from("total"), Value::float(0.0))]);
    for i in 0..N {
        let name = format!("p{i}");
        branches.insert(Key::from(name.as_str()), slow_link());
        state.insert(Key::from(name.as_str()), slow_node());
    }
    let schema = Schema::Tree { branches };
    let state = Value::Map(state);

    // Pool of N workers so all sleeps can overlap.
    let mut engine = Engine::from_state(schema, state, core(N)).expect("engine");
    assert_eq!(
        engine.protocol_runtime_count(),
        1,
        "the parallel pool auto-registered as a protocol runtime via the Core"
    );

    let start = Instant::now();
    engine.run(1.0);
    let elapsed = start.elapsed();

    let total = engine.state().get_field("total").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    assert!((total - N as f64).abs() < 1e-9, "all {N} processes applied (+1 each): total={total}");
    assert!(
        elapsed < Duration::from_millis(150),
        "the {N} × 50ms sleeps ran CONCURRENTLY (one tick ≈ 50ms); sequential would be ~200ms, took {elapsed:?}"
    );
}
