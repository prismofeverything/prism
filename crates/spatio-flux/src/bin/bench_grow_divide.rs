//! Benchmark grow-divide at increasing simulation durations.
//! Each agent is a Composite sub-engine containing Grow (process) + Divide (step).
//! Outputs CSV: duration,n_agents,wall_ms

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use indexmap::IndexMap;
use prism_bigraph::composite::{Bridge, Composite};
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Schema, Update, Value};

// ── Grow Process ──

#[derive(Clone, Debug)]
struct Grow { rate: f64 }

impl Process for Grow {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn interval(&self) -> f64 { 1.0 }
    fn update(&self, state: &Value, interval: f64) -> Update {
        let mass = state.as_map()
            .and_then(|m| m.get("mass"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Update::value(Value::tree([("mass", Value::float(self.rate * mass * interval))]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

// ── Divide Step ──
// Fires when mass > threshold. Outputs to 'environment' with _remove + _add.

#[derive(Clone, Debug)]
struct Divide { threshold: f64 }

impl Step for Divide {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("environment".into(), Schema::map(Schema::Any)),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("environment".into(), Schema::map(Schema::Any)),
        ])
    }
    fn update(&self, state: &Value) -> Update {
        let env = match state.as_map()
            .and_then(|m| m.get("environment"))
            .and_then(|v| v.as_map())
        {
            Some(e) => e,
            None => return Update::Noop,
        };

        let mut to_remove = Vec::new();
        let mut to_add: IndexMap<String, Value> = IndexMap::new();

        for (agent_id, agent_state) in env {
            let mass = agent_state.as_map()
                .and_then(|m| m.get("mass"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            if mass >= self.threshold {
                let half = mass / 2.0;
                let id_a = format!("{}_0", agent_id);
                let id_b = format!("{}_1", agent_id);

                let mut da = agent_state.clone();
                let mut db = agent_state.clone();
                if let Some(m) = da.as_map_mut() { m.insert("mass".into(), Value::float(half)); }
                if let Some(m) = db.as_map_mut() { m.insert("mass".into(), Value::float(half)); }

                to_remove.push(Value::String(agent_id.clone()));
                to_add.insert(id_a, da);
                to_add.insert(id_b, db);
            }
        }

        if to_remove.is_empty() {
            return Update::Noop;
        }

        let mut env_update = IndexMap::new();
        env_update.insert("_remove".into(), Value::List(to_remove));
        env_update.insert("_add".into(), Value::Map(to_add));

        Update::value(Value::tree([
            ("environment", Value::Map(env_update)),
        ]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

// ── Build a grow-divide agent as a Composite ──

fn make_agent_composite(growth_rate: f64, threshold: f64) -> Composite {
    let mut topology = Topology::new();

    // Internal state: mass + environment (bridged from parent)
    topology.initial_state = Value::tree([
        ("mass", Value::float(0.0)),
        ("environment", Value::map()),
    ]);
    topology.state_schema = Schema::Tree {
        branches: IndexMap::from([
            ("mass".into(), Schema::float()),
            ("environment".into(), Schema::map(Schema::Any)),
        ]),
    };

    // Grow process: mass += rate * mass * dt
    topology.processes.insert("grow".into(), ProcessSpec {
        process_type: "Grow".into(),
        config: Value::tree([("rate", Value::float(growth_rate))]),
        inputs: IndexMap::from([("mass".into(), vec!["mass".into()])]),
        outputs: IndexMap::from([("mass".into(), vec!["mass".into()])]),
        interval: Some(1.0),
        priority: 0.0,
    });

    // Divide step: fires when mass > threshold, outputs _add/_remove to environment
    topology.processes.insert("divide".into(), ProcessSpec {
        process_type: "Divide".into(),
        config: Value::tree([("threshold", Value::float(threshold))]),
        inputs: IndexMap::from([
            ("mass".into(), vec!["mass".into()]),
            ("environment".into(), vec!["environment".into()]),
        ]),
        outputs: IndexMap::from([
            ("environment".into(), vec!["environment".into()]),
        ]),
        interval: None, // Step — fires when mass changes
        priority: 0.0,
    });

    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("grow".into(), ProcessNode::Process(Box::new(Grow { rate: growth_rate })));
    instances.insert("divide".into(), ProcessNode::Step(Box::new(Divide { threshold })));

    let inner = Engine::new(topology, instances);

    // Bridge: mass and environment flow in and out
    let input_bridge = Bridge {
        mappings: IndexMap::from([
            ("mass".into(), vec!["mass".into()]),
            ("environment".into(), vec!["environment".into()]),
        ]),
    };
    let output_bridge = Bridge {
        mappings: IndexMap::from([
            ("mass".into(), vec!["mass".into()]),
            ("environment".into(), vec!["environment".into()]),
        ]),
    };

    Composite::new(
        inner,
        input_bridge,
        output_bridge,
        IndexMap::from([
            ("mass".into(), Schema::float()),
            ("environment".into(), Schema::map(Schema::Any)),
        ]),
        IndexMap::from([
            ("mass".into(), Schema::float()),
            ("environment".into(), Schema::map(Schema::Any)),
        ]),
        1.0,
    )
}

fn make_registry(growth_rate: f64, threshold: f64) -> ProcessRegistry {
    let mut reg = ProcessRegistry::new();
    let gr = growth_rate;
    let th = threshold;
    reg.register("GrowDivideAgent", move |_config| {
        ProcessNode::Process(Box::new(make_agent_composite(gr, th)))
    });
    reg.register("Divide", move |_config| {
        ProcessNode::Step(Box::new(Divide { threshold: th }))
    });
    reg
}

fn run_bench(duration: f64, growth_rate: f64, threshold: f64) -> (u128, usize) {
    let registry = Arc::new(make_registry(growth_rate, threshold));

    // Parent engine: environment map with agents, each containing a composite
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("environment".into(), Schema::map(Schema::Tree {
                branches: IndexMap::from([
                    ("mass".into(), Schema::float()),
                    ("grow_divide".into(), Schema::process(
                        IndexMap::from([
                            ("mass".into(), Schema::float()),
                            ("environment".into(), Schema::map(Schema::Any)),
                        ]),
                        IndexMap::from([
                            ("mass".into(), Schema::float()),
                            ("environment".into(), Schema::map(Schema::Any)),
                        ]),
                    )),
                ]),
            })),
        ]),
    };

    let state = Value::tree([
        ("environment", Value::tree([
            ("0", Value::tree([
                ("mass", Value::float(1.0)),
                ("grow_divide", Value::Map(IndexMap::from([
                    ("address".into(), Value::String("local:GrowDivideAgent".into())),
                    ("inputs".into(), Value::tree([
                        ("mass", Value::List(vec![Value::String("mass".into())])),
                        ("environment", Value::List(vec![
                            Value::String("..".into()),
                            Value::String("..".into()),
                            Value::String("environment".into()),
                        ])),
                    ])),
                    ("outputs".into(), Value::tree([
                        ("mass", Value::List(vec![Value::String("mass".into())])),
                        ("environment", Value::List(vec![
                            Value::String("..".into()),
                            Value::String("..".into()),
                            Value::String("environment".into()),
                        ])),
                    ])),
                ]))),
            ])),
        ])),
    ]);

    let start = Instant::now();
    let mut engine = Engine::from_state(schema, state, Arc::clone(&registry)).unwrap();
    engine.run(duration);
    let elapsed = start.elapsed().as_millis();

    let n_agents = engine.state()
        .get_path(&["environment".into()])
        .and_then(|v| v.as_map())
        .map(|m| m.len())
        .unwrap_or(0);

    (elapsed, n_agents)
}

fn main() {
    let growth_rate = 0.03;
    let threshold = 2.0;

    println!("duration,n_agents,wall_ms");
    for &dur in &[10.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 170.0] {
        let (ms, n) = run_bench(dur, growth_rate, threshold);
        println!("{dur},{n},{ms}");
        eprintln!("  t={dur}: {n} agents, {ms}ms");
    }
}
