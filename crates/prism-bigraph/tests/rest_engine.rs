//! Engine-level proof of #21 (step 5): the invoke pass dispatches `rest:` nodes
//! CONCURRENTLY. `RestProcess::invoke` fires its HTTP round-trip on its own thread
//! and joins in the collect pass (mirror of the `stream:` change), and the
//! `RestProcessServer` handles each connection on its own thread — so N remote
//! processes that each sleep 50ms server-side finish in ~one delay, not N. The
//! analog of `parallel_engine.rs`, over real HTTP.

use std::any::Any;
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocol::ProtocolRegistry;
use prism_bigraph::protocols::{RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Engine, Key, Schema, Update, Value};

/// Sleeps `delay_ms` server-side then emits `+1.0` on `out` — the sleep makes
/// client-side concurrency observable (N over rest ≈ one delay, not N).
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

/// A `rest:Slow@host:port` node whose `out` accumulates into the shared `total`.
fn rest_node(port: u16) -> Value {
    let address = Value::Map(IndexMap::from([
        (Key::from("protocol"), Value::String("rest".into())),
        (
            Key::from("data"),
            Value::Map(IndexMap::from([
                (Key::from("process"), Value::String("Slow".into())),
                (Key::from("host"), Value::String("127.0.0.1".into())),
                (Key::from("port"), Value::String(port.to_string())),
            ])),
        ),
    ]));
    Value::tree([
        ("address", address),
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

/// The SERVER core: the `Slow` factory (runs remotely).
fn server_core() -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Slow", |_config| {
        ProcessNode::Process(Box::new(SlowProcess { delay_ms: 50 }))
    });
    Core::from(Arc::new(registry))
}

/// The CLIENT core: the same `Slow` name (so `check_references` is satisfied — the
/// rest nodes name it, though it runs on the server) + the `rest` protocol.
fn client_core() -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Slow", |_config| {
        ProcessNode::Process(Box::new(SlowProcess { delay_ms: 50 }))
    });
    let mut protocols = ProtocolRegistry::new();
    protocols.register(Arc::new(RestProtocol));
    Core::from(Arc::new(registry)).with_protocols(Arc::new(protocols))
}

#[test]
fn engine_invoke_pass_runs_rest_processes_concurrently() {
    const N: usize = 4;
    let server = RestProcessServer::start(server_core()).expect("start rest server");
    std::thread::sleep(Duration::from_millis(50));

    let mut branches: IndexMap<Key, Schema> =
        IndexMap::from([(Key::from("total"), Schema::float())]);
    let mut state: IndexMap<Key, Value> = IndexMap::from([(Key::from("total"), Value::float(0.0))]);
    for i in 0..N {
        let name = format!("p{i}");
        branches.insert(Key::from(name.as_str()), slow_link());
        state.insert(Key::from(name.as_str()), rest_node(server.port()));
    }
    let mut engine = Engine::from_state(
        Schema::Tree { branches },
        Value::Map(state),
        client_core(),
    )
    .expect("engine");
    engine.discover_all_processes();

    let start = Instant::now();
    engine.run(1.0);
    let elapsed = start.elapsed();

    let total = engine.state().get_field("total").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    assert!((total - N as f64).abs() < 1e-9, "all {N} rest processes applied (+1 each): total={total}");
    assert!(
        elapsed < Duration::from_millis(150),
        "the {N} × 50ms remote sleeps ran CONCURRENTLY (one tick ≈ 50ms + HTTP); \
         sequential would be ~{}ms, took {elapsed:?}",
        N * 50
    );
    drop(server);
}
