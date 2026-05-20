//! Port of `../process-bigraph/process_bigraph/processes/growth_division.py`:
//! a cell is a **subengine composite** (`from_config` with `{state, bridge}`)
//! that grows internally and **divides itself** by writing daughters *up*
//! through `outputs.environment = ['..']`. Division is observed by the cell
//! COUNT in the environment changing — never by reading a cell's inner mass.
//!
//! This proves subengine composites + internal division work end to end,
//! the upstream way (no flattening, no reaching into a composite).

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Key, Schema, Update, Value};

// ── Grow: mass += mass * rate * interval (a delta) ──────────────────
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
        let mass = state.get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
        Update::value(Value::tree([("mass", Value::float(mass * self.rate * interval))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Divide: when inner mass crosses threshold, replace self with two
//    daughters at the parent (environment) via the bridge's `['..']`. ──
#[derive(Debug)]
struct Divide {
    threshold: f64,
    agent_id: String,
}
impl Step for Divide {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("trigger".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("environment".to_string(), Schema::map(Schema::Any))])
    }
    fn update(&self, state: &Value) -> Update {
        let trigger = state.get_field("trigger").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if trigger <= self.threshold {
            return Update::Noop;
        }
        let d0 = format!("{}_0", self.agent_id);
        let d1 = format!("{}_1", self.agent_id);
        let half = trigger / 2.0;
        Update::value(Value::tree([(
            "environment",
            Value::Map(IndexMap::from([
                (
                    Key::from("_remove"),
                    Value::List(vec![Value::String(self.agent_id.clone())]),
                ),
                (
                    Key::from("_add"),
                    Value::tree([
                        (d0.as_str(), cell_spec(half, &d0)),
                        (d1.as_str(), cell_spec(half, &d1)),
                    ]),
                ),
            ])),
        )]))
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

/// A cell = a subengine composite `{address, config:{state, bridge}, …}`.
/// Inner state: `{mass, grow, divide}`. The cell exposes `environment`
/// (where Divide writes `_add`/`_remove`) up to its parent via `['..']`.
fn cell_spec(mass: f64, id: &str) -> Value {
    let grow = Value::tree([
        ("address", Value::String("local:Grow".to_string())),
        ("config", Value::tree([("rate", Value::float(0.6))])),
        ("inputs", Value::tree([("mass", wire(&["mass"]))])),
        ("outputs", Value::tree([("mass", wire(&["mass"]))])),
    ]);
    let divide = Value::tree([
        ("address", Value::String("local:Divide".to_string())),
        (
            "config",
            Value::tree([
                ("threshold", Value::float(2.0)),
                ("agent_id", Value::String(id.to_string())),
            ]),
        ),
        ("inputs", Value::tree([("trigger", wire(&["mass"]))])),
        ("outputs", Value::tree([("environment", wire(&["environment"]))])),
    ]);
    let state = Value::tree([
        ("mass", Value::float(mass)),
        ("grow", grow),
        ("divide", divide),
        ("environment", Value::map()),
    ]);
    let bridge = Value::tree([
        ("inputs", Value::map()),
        // expose the inner `environment` slot as the composite's port
        ("outputs", Value::tree([("environment", wire(&["environment"]))])),
    ]);
    Value::tree([
        ("address", Value::String("local:Composite".to_string())),
        ("config", Value::tree([("state", state), ("bridge", bridge)])),
        ("inputs", Value::map()),
        // The composite's `environment` output lands on its PARENT store
        // (the map holding the cells). prism is parent-relative, so the
        // empty wire `[]` == "my parent" (upstream's self-relative `['..']`).
        ("outputs", Value::tree([("environment", wire(&[]))])),
    ])
}

#[test]
fn subengine_cell_grows_and_divides_via_bridge() {
    let mut registry = ProcessRegistry::new();
    registry.register("Grow", |config| {
        let rate = config.get_field("rate").and_then(|v| v.as_f64()).unwrap_or(0.1);
        ProcessNode::Process(Box::new(Grow { rate }))
    });
    registry.register("Divide", |config| {
        let threshold = config.get_field("threshold").and_then(|v| v.as_f64()).unwrap_or(2.0);
        let agent_id = config
            .get_field("agent_id")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        ProcessNode::Step(Box::new(Divide { threshold, agent_id }))
    });
    // The cell composite is discovered/instantiated through from_config too.
    let handle: Arc<std::sync::OnceLock<Arc<ProcessRegistry>>> =
        Arc::new(std::sync::OnceLock::new());
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let registry = handle.get().cloned().expect("registry handle");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, registry).expect("from_config"),
            ))
        });
    }
    let registry = Arc::new(registry);
    let _ = handle.set(Arc::clone(&registry));

    // Top engine: an `environment` map holding one cell, plus the cell's
    // process spec so it is discovered.
    let mut topo = Topology::new();
    topo.state_schema = Schema::Any;
    topo.initial_state = Value::tree([(
        "environment",
        Value::tree([("cell", cell_spec(1.6, "cell"))]),
    )]);
    let mut engine = Engine::from_state(Schema::Any, topo.initial_state.clone(), Arc::clone(&registry))
        .expect("engine");
    engine.discover_all_processes();

    let count = |e: &Engine| {
        e.state()
            .get_field("environment")
            .and_then(|v| v.as_map().map(|m| m.keys().filter(|k| !k.starts_with('_')).count()))
            .unwrap_or(0)
    };
    eprintln!("cells before: {}", count(&engine));
    engine.run(8.0);
    let after = count(&engine);
    eprintln!(
        "cells after run(8.0): {after}\n  root keys: {:?}\n  env keys: {:?}",
        engine.state().as_map().map(|m| m.keys().collect::<Vec<_>>()),
        engine
            .state()
            .get_field("environment")
            .and_then(|v| v.as_map().map(|m| m.keys().collect::<Vec<_>>()))
    );
    assert!(
        after >= 2,
        "subengine cell should grow past threshold and divide (got {after} cells)"
    );
}
