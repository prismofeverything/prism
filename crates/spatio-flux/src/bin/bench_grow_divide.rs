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
// Fires when mass > threshold. Knows its own agent_id from config.
// Reads only its own mass (trigger), outputs _add/_remove to environment.

#[derive(Clone, Debug)]
struct Divide { threshold: f64, agent_id: String }

impl Step for Divide {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("trigger".into(), Schema::float()),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("environment".into(), Schema::map(Schema::Any)),
        ])
    }
    fn update(&self, state: &Value) -> Update {
        let mass = state.get_field("trigger")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if mass < self.threshold {
            return Update::Noop;
        }

        let half = mass / 2.0;
        let id_a = format!("{}_0", self.agent_id);
        let id_b = format!("{}_1", self.agent_id);

        // Build minimal daughter state — grow_divide spec included
        // so the parent engine can discover and instantiate the composite.
        let make_daughter = |id: &str| -> Value {
            Value::tree([
                ("mass", Value::float(half)),
                ("grow_divide", Value::Map(IndexMap::from([
                    ("address".into(), Value::String("local:GrowDivideAgent".into())),
                    ("config".into(), Value::tree([("agent_id", Value::String(id.into()))])),
                    ("inputs".into(), Value::tree([
                        ("mass", Value::List(vec![Value::String("mass".into())])),
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
            ])
        };

        let mut env_update = IndexMap::new();
        env_update.insert("_remove".into(), Value::List(vec![
            Value::String(self.agent_id.clone()),
        ]));
        env_update.insert("_add".into(), Value::Map(IndexMap::from([
            (prism_schema::Key::from(id_a.as_str()), make_daughter(&id_a)),
            (prism_schema::Key::from(id_b.as_str()), make_daughter(&id_b)),
        ])));

        Update::value(Value::tree([
            ("environment", Value::Map(env_update)),
        ]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

// ── Build a grow-divide agent as a Composite ──

fn make_agent_composite(growth_rate: f64, threshold: f64, agent_id: &str) -> Composite {
    let mut topology = Topology::new();

    // Internal state: just mass (environment is output-only, not bridged in)
    topology.initial_state = Value::tree([
        ("mass", Value::float(0.0)),
    ]);
    topology.state_schema = Schema::Tree {
        branches: IndexMap::from([
            ("mass".into(), Schema::float()),
        ]),
    };

    // Grow process
    topology.processes.insert("grow".into(), ProcessSpec {
        process_type: "Grow".into(),
        config: Value::tree([("rate", Value::float(growth_rate))]),
        inputs: IndexMap::from([("mass".into(), vec!["mass".into()])]),
        outputs: IndexMap::from([("mass".into(), vec!["mass".into()])]),
        interval: Some(1.0),
        priority: 0.0,
    });

    // Divide step: reads mass as trigger, outputs to environment
    topology.processes.insert("divide".into(), ProcessSpec {
        process_type: "Divide".into(),
        config: Value::None,
        inputs: IndexMap::from([
            ("trigger".into(), vec!["mass".into()]),
        ]),
        outputs: IndexMap::from([
            ("environment".into(), vec!["environment".into()]),
        ]),
        interval: None,
        priority: 0.0,
    });

    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("grow".into(), ProcessNode::Process(Box::new(Grow { rate: growth_rate })));
    instances.insert("divide".into(), ProcessNode::Step(Box::new(Divide {
        threshold,
        agent_id: agent_id.to_string(),
    })));

    let inner = Engine::new(topology, instances);

    // Bridge: mass flows in AND out, environment only flows out
    let input_bridge = Bridge {
        mappings: IndexMap::from([
            ("mass".into(), vec!["mass".into()]),
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
    reg.register("GrowDivideAgent", move |config| {
        let agent_id = config.as_map()
            .and_then(|m| m.get("agent_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();
        ProcessNode::Process(Box::new(make_agent_composite(gr, th, &agent_id)))
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
                        IndexMap::from([("mass".into(), Schema::float())]),
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
                    ("config".into(), Value::tree([("agent_id", Value::String("0".into()))])),
                    ("inputs".into(), Value::tree([
                        ("mass", Value::List(vec![Value::String("mass".into())])),
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
    // Use rate=0.1 to match Python's grow_divide_agent default.
    // (Python's Divide step creates daughters with the default rate,
    // not the parent's configured rate.)
    let growth_rate = 0.1;
    let threshold = 2.0;

    println!("duration,n_agents,wall_ms");
    for &dur in &[5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 55.0, 60.0, 65.0, 70.0, 75.0] {
        let (ms, n) = run_bench(dur, growth_rate, threshold);
        println!("{dur},{n},{ms}");
        eprintln!("  t={dur}: {n} agents, {ms}ms");
    }
}
