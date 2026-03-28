//! Example: A simple growth-division simulation using prism.
//!
//! Two processes:
//! - Growth: increases biomass over time
//! - Division: a step triggered when biomass exceeds a threshold,
//!   resets biomass and increments a division counter

use std::any::Any;
use std::collections::HashMap;

use prism_bigraph::{
    Engine, Process, ProcessNode,
    Schema, Step, Topology, Update, Value,
};
use indexmap::IndexMap;

// ── Growth Process ──

#[derive(Debug)]
struct Growth {
    rate: f64,
}

impl Process for Growth {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("biomass".to_string(), Schema::float())])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("biomass".to_string(), Schema::float())])
    }

    fn interval(&self) -> f64 {
        1.0
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let biomass = state
            .as_map()
            .and_then(|m| m.get("biomass"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let growth = biomass * self.rate * interval;
        Update::value(Value::tree([("biomass", Value::float(growth))])) // delta
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

// ── Division Step ──

#[derive(Debug)]
struct Division {
    threshold: f64,
}

impl Step for Division {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("biomass".to_string(), Schema::float()),
            ("divisions".to_string(), Schema::integer()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("biomass".to_string(), Schema::float()),
            ("divisions".to_string(), Schema::integer()),
        ])
    }

    fn update(&self, state: &Value) -> Update {
        let map = match state.as_map() {
            Some(m) => m,
            None => return Update::Noop,
        };

        let biomass = map
            .get("biomass")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let divisions = map
            .get("divisions")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        if biomass >= self.threshold {
            Update::value(Value::tree([
                ("biomass", Value::float(-biomass / 2.0)), // delta: halve
                ("divisions", Value::Int(1)),              // delta: +1
            ]))
        } else {
            Update::Noop
        }
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

fn main() {
    // Build topology
    let mut topology = Topology::new();
    topology.initial_state = Value::tree([
        ("biomass", Value::float(1.0)),
        ("divisions", Value::Int(0)),
    ]);

    topology.add_process(
        "growth",
        "growth",
        Value::None,
        IndexMap::from([("biomass".into(), vec!["biomass".into()])]),
        IndexMap::from([("biomass".into(), vec!["biomass".into()])]),
        1.0,
    );

    topology.add_step(
        "division",
        "division",
        Value::None,
        IndexMap::from([
            ("biomass".into(), vec!["biomass".into()]),
            ("divisions".into(), vec!["divisions".into()]),
        ]),
        IndexMap::from([
            ("biomass".into(), vec!["biomass".into()]),
            ("divisions".into(), vec!["divisions".into()]),
        ]),
        0.0,
    );

    // Build instances directly
    let instances: HashMap<String, ProcessNode> = HashMap::from([
        (
            "growth".to_string(),
            ProcessNode::Process(Box::new(Growth { rate: 0.1 })),
        ),
        (
            "division".to_string(),
            ProcessNode::Step(Box::new(Division { threshold: 2.0 })),
        ),
    ]);

    // Run
    let mut engine = Engine::new(topology, instances);
    println!("t=0: {:}", engine.state());

    for t in 1..=20 {
        engine.run(1.0);
        let biomass = engine
            .get(&["biomass".into()])
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let divisions = engine
            .get(&["divisions".into()])
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        println!("t={t}: biomass={biomass:.3}, divisions={divisions}");
    }
}
