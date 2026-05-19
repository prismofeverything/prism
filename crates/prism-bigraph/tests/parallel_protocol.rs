//! End-to-end test for the `parallel` protocol routed through Engine
//! discovery.
//!
//! Validates the protocol abstraction by running the same process
//! definition (a simple `Grow` process) through two different
//! protocols — `local` and `parallel` — and confirming the results
//! are identical. That's the protocol-agnostic property.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;

use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::ports::PortSchema;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::update::Update;
use prism_bigraph::{Engine, ParallelProtocol};
use prism_schema::{Key, Schema, Value};

#[derive(Clone, Debug)]
struct Grow {
    rate: f64,
}
impl Process for Grow {
    fn inputs(&self) -> PortSchema {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn outputs(&self) -> PortSchema {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, state: &Value, interval: f64) -> Update {
        let mass = state
            .as_map()
            .and_then(|m| m.get("mass"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Update::value(Value::tree([(
            "mass",
            Value::float(self.rate * mass * interval),
        )]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn make_registry() -> Arc<ProcessRegistry> {
    let mut r = ProcessRegistry::new();
    r.register("Grow", |config| {
        let rate = config
            .as_map()
            .and_then(|m| m.get("rate"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.1);
        ProcessNode::Process(Box::new(Grow { rate }))
    });
    Arc::new(r)
}

fn run_engine(protocol: &str, duration: f64) -> f64 {
    let registry = make_registry();
    let initial_mass = 1.0;

    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("mass".into(), Schema::float()),
            (
                "grow".into(),
                Schema::process(
                    IndexMap::from([("mass".into(), Schema::float())]),
                    IndexMap::from([("mass".into(), Schema::float())]),
                ),
            ),
        ]),
    };

    let state = Value::tree([
        ("mass", Value::float(initial_mass)),
        (
            "grow",
            Value::Map(IndexMap::from([
                (
                    Key::from("address"),
                    Value::String(format!("{protocol}:Grow")),
                ),
                (
                    Key::from("config"),
                    Value::tree([("rate", Value::float(0.1))]),
                ),
                (
                    Key::from("inputs"),
                    Value::tree([("mass", Value::List(vec![Value::String("..".into()), Value::String("mass".into())]))]),
                ),
                (
                    Key::from("outputs"),
                    Value::tree([("mass", Value::List(vec![Value::String("..".into()), Value::String("mass".into())]))]),
                ),
            ])),
        ),
    ]);

    let mut protocols = prism_bigraph::protocol::ProtocolRegistry::new();
    protocols.register(Arc::new(ParallelProtocol::default()));
    let mut engine = Engine::from_state_with_protocols(
        schema,
        state,
        registry,
        Arc::new(protocols),
    )
    .expect("init");
    engine.discover_all_processes();
    engine.run(duration);

    engine
        .state()
        .get_path(&["mass".into()])
        .and_then(|v| v.as_f64())
        .expect("mass present")
}

#[test]
fn local_and_parallel_produce_identical_results() {
    let duration = 5.0;
    let local_mass = run_engine("local", duration);
    let parallel_mass = run_engine("parallel", duration);

    assert!(
        (local_mass - parallel_mass).abs() < 1e-9,
        "expected identical results across protocols: local={local_mass}, parallel={parallel_mass}"
    );
    // Sanity: 1.0 * 1.1^5 ≈ 1.611 (compounded multiplicative growth)
    assert!(local_mass > 1.5, "expected growth, got {local_mass}");
}
