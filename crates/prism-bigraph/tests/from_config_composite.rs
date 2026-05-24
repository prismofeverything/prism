//! Revives + verifies `Composite::from_config` — the ported
//! process-bigraph composite construction where a composite is just
//! `config = {state, bridge}` (matching upstream `Composite.config_schema`).
//!
//! This is the path chrysalis composites should compile to (a plain
//! spec with an `address`, instantiated via `from_config`) instead of
//! the bespoke `{_type, _process, outer-data}` wrapper it invented.
//! See docs/state-schema-unification.md and the upstream reference
//! `../process-bigraph/process_bigraph/processes/growth_division.py`.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Core, Engine, Key, Schema, Update, Value};

#[derive(Debug)]
struct Grow {
    rate: f64,
}

impl Process for Grow {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".to_string(), Schema::float())])
    }
    fn update(&self, state: &Value, interval: f64) -> Update {
        let mass = state
            .get_field("mass")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        // delta: mass grows by rate * interval
        Update::value(Value::tree([(
            "mass",
            Value::float(mass * self.rate * interval),
        )]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn wire(seg: &str) -> Value {
    Value::List(vec![Value::String(seg.to_string())])
}

#[test]
fn from_config_builds_and_runs_a_composite() {
    // The inner Grow process must be discoverable by the composite's
    // inner engine (from_config calls discover_all_processes).
    let mut registry = ProcessRegistry::new();
    registry.register("Grow", |config| {
        let rate = config
            .get_field("rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.1);
        ProcessNode::Process(Box::new(Grow { rate }))
    });
    let registry = Arc::new(registry);

    // Upstream composite shape: config = {state, bridge}. Inner state is
    // encapsulated; the bridge exposes `mass` to the outer world.
    let config = Value::tree([
        (
            "state",
            Value::tree([
                ("mass", Value::float(1.0)),
                (
                    "grow",
                    Value::tree([
                        ("address", Value::String("local:Grow".to_string())),
                        ("config", Value::tree([("rate", Value::float(0.5))])),
                        ("inputs", Value::tree([("mass", wire("mass"))])),
                        ("outputs", Value::tree([("mass", wire("mass"))])),
                    ]),
                ),
            ]),
        ),
        (
            "bridge",
            Value::tree([
                ("inputs", Value::map()),
                ("outputs", Value::tree([("mass", wire("mass"))])),
            ]),
        ),
    ]);

    let core = Core::from(Arc::clone(&registry));
    let composite =
        Composite::from_config(&config, &core).expect("from_config should build a composite");

    // Parent engine: the composite's bridged `mass` output lands in `cell_mass`.
    let mut parent = Topology::new();
    parent.state_schema = Schema::Any;
    parent.initial_state = Value::tree([("cell_mass", Value::float(1.0))]);
    parent.processes.insert(
        "cell".to_string(),
        ProcessSpec {
            process_type: "Composite".to_string(),
            config: Value::None,
            inputs: IndexMap::new(),
            outputs: IndexMap::from([("mass".to_string(), vec![Key::from("cell_mass")])]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );
    let mut inst = HashMap::new();
    inst.insert(
        "cell".to_string(),
        ProcessNode::Process(Box::new(composite)),
    );

    let mut engine = Engine::new(parent, inst);
    engine.run(3.0);

    let cell_mass = engine
        .state()
        .get_field("cell_mass")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    eprintln!("cell_mass after run(3.0) = {cell_mass}");
    assert!(
        cell_mass > 1.0,
        "from_config composite's inner Grow should grow the bridged mass (got {cell_mass})"
    );
}
