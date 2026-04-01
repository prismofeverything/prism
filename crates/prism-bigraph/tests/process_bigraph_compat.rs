//! Tests ported from Python process-bigraph/tests.py
//!
//! These verify that prism-bigraph's engine, process scheduling,
//! step dependency resolution, and composite behavior match the
//! Python process-bigraph reference implementation.
//!
//! Tests marked `#[ignore]` require features not yet implemented.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Schema, Value};

// ═══════════════════════════════════════════════════════════
// Test processes (analogs of Python IncreaseProcess, OperatorStep)
// ═══════════════════════════════════════════════════════════

/// Analog of Python's IncreaseProcess: adds rate * level * dt each tick
#[derive(Clone, Debug)]
struct IncreaseProcess {
    rate: f64,
}

impl Process for IncreaseProcess {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("level".into(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("level".into(), Schema::float())])
    }
    fn interval(&self) -> f64 { 1.0 }
    fn update(&self, state: &Value, interval: f64) -> prism_bigraph::Update {
        let level = state.as_map()
            .and_then(|m| m.get("level"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let delta = self.rate * level * interval;
        prism_bigraph::Update::value(Value::tree([
            ("level", Value::float(delta)),
        ]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// Analog of Python's OperatorStep: applies an operator to two inputs
#[derive(Clone, Debug)]
struct OperatorStep {
    operator: String,
}

impl Step for OperatorStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("a".into(), Schema::float()),
            ("b".into(), Schema::float()),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("c".into(), Schema::Overwrite { inner: Box::new(Schema::float()) }),
        ])
    }
    fn update(&self, state: &Value) -> prism_bigraph::Update {
        let map = state.as_map().unwrap();
        let a = map.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b = map.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let c = match self.operator.as_str() {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => if b != 0.0 { a / b } else { 0.0 },
            _ => 0.0,
        };
        prism_bigraph::Update::value(Value::tree([
            ("c", Value::float(c)),
        ]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

// ═══════════════════════════════════════════════════════════
// Process tests
// ═══════════════════════════════════════════════════════════

/// Python test_process: IncreaseProcess with rate=0.2
#[test]
fn test_process_update() {
    let proc = IncreaseProcess { rate: 0.2 };
    let state = Value::tree([("level", Value::float(5.5))]);
    let update = proc.update(&state, 1.0);
    let val = update.into_value().unwrap();
    let delta = val.as_map().unwrap().get("level").unwrap().as_f64().unwrap();
    assert!((delta - 1.1).abs() < 1e-10); // 0.2 * 5.5 = 1.1
}

/// Python test_process: apply update to state
#[test]
fn test_process_apply() {
    let proc = IncreaseProcess { rate: 0.2 };
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("level".into(), Schema::float()),
        ]),
    };
    let state = Value::tree([("level", Value::float(5.5))]);
    let update = proc.update(&state, 1.0).into_value().unwrap();
    let result = schema.apply_update(&state, &update);
    let level = result.as_map().unwrap().get("level").unwrap().as_f64().unwrap();
    assert!((level - 6.6).abs() < 1e-10); // 5.5 + 1.1 = 6.6
}

// ═══════════════════════════════════════════════════════════
// Step initialization and dependency resolution
// ═══════════════════════════════════════════════════════════

/// Python test_step_initialization: steps fire on init based on dependencies
#[test]
fn test_step_initialization() {
    // A=13, B=21, step1: C = A + B, step2: D = B * C
    let mut topology = Topology::new();
    topology.initial_state = Value::tree([
        ("A", Value::float(13.0)),
        ("B", Value::float(21.0)),
        ("C", Value::float(0.0)),
        ("D", Value::float(0.0)),
    ]);
    topology.state_schema = Schema::Tree {
        branches: IndexMap::from([
            ("A".into(), Schema::float()),
            ("B".into(), Schema::float()),
            ("C".into(), Schema::Overwrite { inner: Box::new(Schema::float()) }),
            ("D".into(), Schema::Overwrite { inner: Box::new(Schema::float()) }),
        ]),
    };

    topology.processes.insert("step1".into(), ProcessSpec {
        process_type: "OperatorStep".into(),
        config: Value::None,
        inputs: IndexMap::from([
            ("a".into(), vec!["A".into()]),
            ("b".into(), vec!["B".into()]),
        ]),
        outputs: IndexMap::from([
            ("c".into(), vec!["C".into()]),
        ]),
        interval: None, // Step
        priority: 0.0,
    });

    topology.processes.insert("step2".into(), ProcessSpec {
        process_type: "OperatorStep".into(),
        config: Value::None,
        inputs: IndexMap::from([
            ("a".into(), vec!["B".into()]),
            ("b".into(), vec!["C".into()]),
        ]),
        outputs: IndexMap::from([
            ("c".into(), vec!["D".into()]),
        ]),
        interval: None, // Step
        priority: 0.0,
    });

    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("step1".into(), ProcessNode::Step(Box::new(
        OperatorStep { operator: "+".into() }
    )));
    instances.insert("step2".into(), ProcessNode::Step(Box::new(
        OperatorStep { operator: "*".into() }
    )));

    let mut engine = Engine::new(topology, instances);

    // Steps should fire during first tick or initialization cascade
    // step1: C = 13 + 21 = 34
    // step2: D = 21 * 34 = 714
    engine.run(0.0);

    let d = engine.state().get_path(&["D".into()]).unwrap();
    assert_eq!(d.as_f64().unwrap(), (13.0 + 21.0) * 21.0);
}

/// Python test_dependencies: chain of step dependencies
#[test]
fn test_step_dependencies() {
    let mut topology = Topology::new();
    topology.initial_state = Value::tree([
        ("a", Value::float(11.111)),
        ("b", Value::float(22.2)),
        ("c", Value::float(555.555)),
        ("d", Value::float(0.0)),
        ("e", Value::float(0.0)),
        ("f", Value::float(0.0)),
        ("g", Value::float(0.0)),
        ("h", Value::float(0.0)),
        ("i", Value::float(0.0)),
    ]);
    topology.state_schema = Schema::Tree {
        branches: ["a","b","c","d","e","f","g","h","i"].iter()
            .map(|k| (k.to_string(), Schema::Overwrite { inner: Box::new(Schema::float()) }))
            .collect(),
    };

    // Step 1: e = a + b
    topology.processes.insert("1".into(), ProcessSpec {
        process_type: "op".into(), config: Value::None,
        inputs: IndexMap::from([("a".into(), vec!["a".into()]), ("b".into(), vec!["b".into()])]),
        outputs: IndexMap::from([("c".into(), vec!["e".into()])]),
        interval: None, priority: 0.0,
    });
    // Step 2.1: f = c - e
    topology.processes.insert("2.1".into(), ProcessSpec {
        process_type: "op".into(), config: Value::None,
        inputs: IndexMap::from([("a".into(), vec!["c".into()]), ("b".into(), vec!["e".into()])]),
        outputs: IndexMap::from([("c".into(), vec!["f".into()])]),
        interval: None, priority: 0.0,
    });
    // Step 2.2: g = d - e
    topology.processes.insert("2.2".into(), ProcessSpec {
        process_type: "op".into(), config: Value::None,
        inputs: IndexMap::from([("a".into(), vec!["d".into()]), ("b".into(), vec!["e".into()])]),
        outputs: IndexMap::from([("c".into(), vec!["g".into()])]),
        interval: None, priority: 0.0,
    });
    // Step 3: h = f * g
    topology.processes.insert("3".into(), ProcessSpec {
        process_type: "op".into(), config: Value::None,
        inputs: IndexMap::from([("a".into(), vec!["f".into()]), ("b".into(), vec!["g".into()])]),
        outputs: IndexMap::from([("c".into(), vec!["h".into()])]),
        interval: None, priority: 0.0,
    });
    // Step 4: i = e + h
    topology.processes.insert("4".into(), ProcessSpec {
        process_type: "op".into(), config: Value::None,
        inputs: IndexMap::from([("a".into(), vec!["e".into()]), ("b".into(), vec!["h".into()])]),
        outputs: IndexMap::from([("c".into(), vec!["i".into()])]),
        interval: None, priority: 0.0,
    });

    let ops = [("+", "1"), ("-", "2.1"), ("-", "2.2"), ("*", "3"), ("+", "4")];
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    for (op, name) in ops {
        instances.insert(name.into(), ProcessNode::Step(Box::new(
            OperatorStep { operator: op.into() }
        )));
    }

    let mut engine = Engine::new(topology, instances);
    engine.run(0.0);

    let h = engine.state().get_path(&["h".into()]).unwrap().as_f64().unwrap();
    // e = 11.111 + 22.2 = 33.311
    // f = 555.555 - 33.311 = 522.244
    // g = 0 - 33.311 = -33.311
    // h = 522.244 * -33.311 = -17396.469...
    assert!((h - (-17396.469884)).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════
// Engine with temporal processes
// ═══════════════════════════════════════════════════════════

/// Test a process running for multiple ticks
#[test]
fn test_engine_run() {
    let mut topology = Topology::new();
    topology.initial_state = Value::tree([
        ("level", Value::float(1.0)),
    ]);
    topology.state_schema = Schema::Tree {
        branches: IndexMap::from([
            ("level".into(), Schema::float()),
        ]),
    };
    topology.processes.insert("increase".into(), ProcessSpec {
        process_type: "IncreaseProcess".into(),
        config: Value::None,
        inputs: IndexMap::from([("level".into(), vec!["level".into()])]),
        outputs: IndexMap::from([("level".into(), vec!["level".into()])]),
        interval: Some(1.0),
        priority: 0.0,
    });

    let mut instances = HashMap::new();
    instances.insert("increase".into(), ProcessNode::Process(Box::new(
        IncreaseProcess { rate: 0.1 }
    )));

    let mut engine = Engine::new(topology, instances);
    engine.run(10.0);

    let level = engine.state().get_path(&["level".into()]).unwrap().as_f64().unwrap();
    // After 10 ticks of 10% growth: 1.0 * 1.1^10 ≈ 2.5937
    assert!((level - 2.5937424601).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════
// Composite tests (require Schema::Link for full compat)
// ═══════════════════════════════════════════════════════════

/// Python test_composite: build engine from schema + state with embedded process
#[test]
fn test_composite_basic() {
    use prism_bigraph::factory::ProcessRegistry;

    let mut registry = ProcessRegistry::new();
    registry.register("IncreaseProcess", |config| {
        let rate = config.as_map()
            .and_then(|m| m.get("rate"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.1);
        ProcessNode::Process(Box::new(IncreaseProcess { rate }))
    });

    // Schema declares 'increase' as a process, 'value' as float
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("increase".into(), Schema::process(
                IndexMap::from([("level".into(), Schema::float())]),
                IndexMap::from([("level".into(), Schema::float())]),
            )),
            ("value".into(), Schema::float()),
        ]),
    };

    // State has the process spec embedded
    let state = Value::tree([
        ("increase", Value::Map(IndexMap::from([
            ("address".to_string(), Value::String("local:IncreaseProcess".into())),
            ("config".to_string(), Value::tree([("rate", Value::float(0.3))])),
            ("inputs".to_string(), Value::tree([("level", Value::List(vec![Value::String("value".into())]))])),
            ("outputs".to_string(), Value::tree([("level", Value::List(vec![Value::String("value".into())]))])),
        ]))),
        ("value", Value::float(11.11)),
    ]);

    let registry = Arc::new(registry);
    let mut engine = Engine::from_state(schema, state, registry).unwrap();
    engine.run(10.0);

    let value = engine.state().get_path(&["value".into()]).unwrap().as_f64().unwrap();
    // After 10 ticks of 30% growth: 11.11 * 1.3^10 ≈ 153.0
    assert!(value > 100.0, "expected growth, got {value}");
}

/// Python test_infer: state with _type='process' infers schema as Link
#[test]
fn test_infer_process_from_state() {
    // When state has '_type': 'process', Schema::infer should return Link
    let state = Value::tree([
        ("increase", Value::Map(IndexMap::from([
            ("_type".to_string(), Value::String("process".into())),
            ("address".to_string(), Value::String("local:IncreaseProcess".into())),
            ("config".to_string(), Value::tree([("rate", Value::String("0.3".into()))])),
            ("inputs".to_string(), Value::tree([("level", Value::List(vec![Value::String("value".into())]))])),
            ("outputs".to_string(), Value::tree([("level", Value::List(vec![Value::String("value".into())]))])),
        ]))),
        ("value", Value::String("11.11".into())),
    ]);

    // Infer schema from state
    let schema = Schema::infer(&state);
    match &schema {
        Schema::Tree { branches } => {
            // 'increase' should be inferred as Link
            assert!(matches!(branches.get("increase"), Some(Schema::Link { temporal: Some(true), .. })),
                "expected Link for 'increase', got {:?}", branches.get("increase"));
            // 'value' should be inferred as float (string "11.11" parses as number)
            assert!(matches!(branches.get("value"), Some(Schema::Float { .. })),
                "expected Float for 'value', got {:?}", branches.get("value"));
        }
        _ => panic!("expected Tree, got {:?}", schema),
    }

    // Realize the state with the inferred schema
    let realized = schema.realize(&state);
    let map = realized.as_map().unwrap();
    // "11.11" string should be realized as float
    assert_eq!(map.get("value").unwrap().as_f64().unwrap(), 11.11);
}

/// Simple growth process: mass += rate * mass * dt
#[derive(Clone, Debug)]
struct GrowProcess {
    rate: f64,
}

impl Process for GrowProcess {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn interval(&self) -> f64 { 1.0 }
    fn update(&self, state: &Value, interval: f64) -> prism_bigraph::Update {
        let mass = state.as_map()
            .and_then(|m| m.get("mass"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        prism_bigraph::Update::value(Value::tree([
            ("mass", Value::float(self.rate * mass * interval)),
        ]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// Division step: when mass > threshold, output _remove self + _add two daughters.
/// Outputs to the parent environment map.
#[derive(Clone, Debug)]
struct DivideStep {
    threshold: f64,
    agent_id: String,
}

impl Step for DivideStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("mass".into(), Schema::float()),
            ("environment".into(), Schema::map(Schema::Any)),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("environment".into(), Schema::map(Schema::Any)),
        ])
    }
    fn update(&self, state: &Value) -> prism_bigraph::Update {
        let map = state.as_map().unwrap();
        let mass = map.get("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);

        if mass < self.threshold {
            return prism_bigraph::Update::Noop;
        }

        // Get the current agent's full state to clone for daughters
        let env = map.get("environment").and_then(|v| v.as_map()).unwrap();
        let agent_state = match env.get(&self.agent_id) {
            Some(s) => s,
            None => return prism_bigraph::Update::Noop,
        };

        // Create two daughters with half mass
        let half_mass = mass / 2.0;
        let mut daughter_a = agent_state.clone();
        let mut daughter_b = agent_state.clone();
        if let Some(m) = daughter_a.as_map_mut() {
            m.insert("mass".to_string(), Value::float(half_mass));
        }
        if let Some(m) = daughter_b.as_map_mut() {
            m.insert("mass".to_string(), Value::float(half_mass));
        }

        let id_a = format!("{}_0", self.agent_id);
        let id_b = format!("{}_1", self.agent_id);

        let mut env_update = IndexMap::new();
        env_update.insert("_remove".to_string(), Value::List(vec![
            Value::String(self.agent_id.clone()),
        ]));
        env_update.insert("_add".to_string(), Value::Map(IndexMap::from([
            (id_a, daughter_a),
            (id_b, daughter_b),
        ])));

        prism_bigraph::Update::value(Value::tree([
            ("environment", Value::Map(env_update)),
        ]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// Python test_grow_divide: agents grow and divide, creating new agents.
/// Each agent has a Grow process and a Divide step. Division creates
/// two daughter agents, each with their own Grow and Divide.
#[test]
fn test_grow_divide() {
    use prism_bigraph::factory::ProcessRegistry;

    let initial_mass = 1.0;
    let division_threshold = 2.0;
    let growth_rate = 0.03;

    let mut registry = ProcessRegistry::new();
    registry.register("Grow", |config| {
        let rate = config.as_map()
            .and_then(|m| m.get("rate"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.03);
        ProcessNode::Process(Box::new(GrowProcess { rate }))
    });
    // DivideStep needs agent_id from config, so we register a factory
    registry.register("Divide", |config| {
        let agent_id = config.as_map()
            .and_then(|m| m.get("agent_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();
        let threshold = config.as_map()
            .and_then(|m| m.get("threshold"))
            .and_then(|v| v.as_f64())
            .unwrap_or(2.0);
        ProcessNode::Step(Box::new(DivideStep { threshold, agent_id }))
    });

    // Build the environment with one agent
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("environment".into(), Schema::map(Schema::Tree {
                branches: IndexMap::from([
                    ("mass".into(), Schema::float()),
                    ("grow".into(), Schema::process(
                        IndexMap::from([("mass".into(), Schema::float())]),
                        IndexMap::from([("mass".into(), Schema::float())]),
                    )),
                    ("divide".into(), Schema::step(
                        IndexMap::from([
                            ("mass".into(), Schema::float()),
                            ("environment".into(), Schema::map(Schema::Any)),
                        ]),
                        IndexMap::from([
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
                ("mass", Value::float(initial_mass)),
                ("grow", Value::Map(IndexMap::from([
                    ("address".to_string(), Value::String("local:Grow".into())),
                    ("config".to_string(), Value::tree([("rate", Value::float(growth_rate))])),
                    ("inputs".to_string(), Value::tree([("mass", Value::List(vec![
                        Value::String("..".into()), Value::String("mass".into()),
                    ]))])),
                    ("outputs".to_string(), Value::tree([("mass", Value::List(vec![
                        Value::String("..".into()), Value::String("mass".into()),
                    ]))])),
                ]))),
                ("divide", Value::Map(IndexMap::from([
                    ("address".to_string(), Value::String("local:Divide".into())),
                    ("config".to_string(), Value::tree([
                        ("agent_id", Value::String("0".into())),
                        ("threshold", Value::float(division_threshold)),
                    ])),
                    ("inputs".to_string(), Value::tree([
                        ("mass", Value::List(vec![
                            Value::String("..".into()), Value::String("mass".into()),
                        ])),
                        ("environment", Value::List(vec![
                            Value::String("..".into()), Value::String("..".into()),
                            Value::String("environment".into()),
                        ])),
                    ])),
                    ("outputs".to_string(), Value::tree([
                        ("environment", Value::List(vec![
                            Value::String("..".into()), Value::String("..".into()),
                            Value::String("environment".into()),
                        ])),
                    ])),
                ]))),
            ])),
        ])),
    ]);

    let registry = Arc::new(registry);
    let mut engine = Engine::from_state(schema, state, Arc::clone(&registry)).unwrap();

    // Run for enough time that mass doubles (1.0 * e^(0.03*t) > 2.0 → t ≈ 23s)
    // But growth is multiplicative per tick, so 1.0 * 1.03^t > 2.0 → t ≈ 24 ticks
    engine.run(30.0);

    // After 30s, agent "0" should have divided
    let env = engine.state().get_path(&["environment".into()])
        .and_then(|v| v.as_map())
        .unwrap();

    // Original agent "0" should be gone (divided)
    // Daughters "0_0" and "0_1" should exist
    let agent_ids: Vec<&String> = env.keys().collect();
    assert!(agent_ids.len() >= 2,
        "expected at least 2 agents after division, got {}: {:?}", agent_ids.len(), agent_ids);
    assert!(!env.contains_key("0") || env.len() > 1,
        "expected division to have occurred");
}

/// Analog of Python's WriteCounts step: counts = concentrations * volume
#[derive(Clone, Debug)]
struct WriteCountsStep;

impl Step for WriteCountsStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("volumes".into(), Schema::map(Schema::float())),
            ("concentrations".into(), Schema::map(Schema::map(Schema::float()))),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("counts".into(), Schema::map(Schema::map(Schema::Overwrite {
                inner: Box::new(Schema::integer()),
            }))),
        ])
    }
    fn update(&self, state: &Value) -> prism_bigraph::Update {
        let map = state.as_map().unwrap();
        let volumes = map.get("volumes").and_then(|v| v.as_map()).unwrap();
        let concentrations = map.get("concentrations").and_then(|v| v.as_map()).unwrap();

        // For each compartment key, multiply concentrations by volume
        let mut counts = IndexMap::new();
        for (comp_id, vol_val) in volumes {
            let volume = vol_val.as_f64().unwrap_or(1.0);
            if let Some(concs) = concentrations.get(comp_id).and_then(|v| v.as_map()) {
                let mut comp_counts = IndexMap::new();
                for (mol, conc_val) in concs {
                    let conc = conc_val.as_f64().unwrap_or(0.0);
                    let count = (conc * volume).round() as i64;
                    comp_counts.insert(mol.clone(), Value::Int(count));
                }
                counts.insert(comp_id.clone(), Value::Map(comp_counts));
            }
        }
        prism_bigraph::Update::value(Value::tree([
            ("counts", Value::Map(counts)),
        ]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// Python test_star_update: wildcard paths fan out across map entries
#[test]
fn test_star_update() {
    let mut topology = Topology::new();

    // Compartments with volumes and concentrations
    let compartments = Value::tree([
        ("0", Value::tree([
            ("Shared Environment", Value::tree([
                ("concentrations", Value::tree([
                    ("biomass", Value::float(5.484)),
                ])),
                ("counts", Value::tree([
                    ("biomass", Value::float(0.0)),
                ])),
                ("volume", Value::float(100.0)),
            ])),
        ])),
        ("1", Value::tree([
            ("Shared Environment", Value::tree([
                ("concentrations", Value::tree([
                    ("biomass", Value::float(5.209)),
                ])),
                ("counts", Value::tree([
                    ("biomass", Value::float(0.0)),
                ])),
                ("volume", Value::float(200.0)),
            ])),
        ])),
        ("2", Value::tree([
            ("Shared Environment", Value::tree([
                ("concentrations", Value::tree([
                    ("biomass", Value::float(9.635)),
                ])),
                ("counts", Value::tree([
                    ("biomass", Value::float(0.0)),
                ])),
                ("volume", Value::float(300.0)),
            ])),
        ])),
    ]);

    topology.initial_state = Value::tree([
        ("Compartments", compartments),
    ]);
    topology.state_schema = Schema::Tree {
        branches: IndexMap::from([
            ("Compartments".into(), Schema::map(Schema::Tree {
                branches: IndexMap::from([
                    ("Shared Environment".into(), Schema::Tree {
                        branches: IndexMap::from([
                            ("counts".into(), Schema::map(Schema::Overwrite {
                                inner: Box::new(Schema::integer()),
                            })),
                            ("concentrations".into(), Schema::map(Schema::float())),
                            ("volume".into(), Schema::float()),
                        ]),
                    }),
                ]),
            })),
        ]),
    };

    // Step wired with star paths
    topology.processes.insert("write".into(), ProcessSpec {
        process_type: "WriteCounts".into(),
        config: Value::None,
        inputs: IndexMap::from([
            ("volumes".into(), vec!["Compartments".into(), "*".into(), "Shared Environment".into(), "volume".into()]),
            ("concentrations".into(), vec!["Compartments".into(), "*".into(), "Shared Environment".into(), "concentrations".into()]),
        ]),
        outputs: IndexMap::from([
            ("counts".into(), vec!["Compartments".into(), "*".into(), "Shared Environment".into(), "counts".into()]),
        ]),
        interval: None,
        priority: 0.0,
    });

    let mut instances = HashMap::new();
    instances.insert("write".into(), ProcessNode::Step(Box::new(WriteCountsStep)));

    let mut engine = Engine::new(topology, instances);
    engine.run(0.0);

    // Check: compartment 2, biomass count = 9.635 * 300 ≈ 2890-2891
    let count = engine.state()
        .get_path(&["Compartments".into(), "2".into(), "Shared Environment".into(), "counts".into(), "biomass".into()])
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    assert!(count == 2890 || count == 2891, "expected ~2890, got {count}");

    // Check all compartments got updated (not just one)
    let count_0 = engine.state()
        .get_path(&["Compartments".into(), "0".into(), "Shared Environment".into(), "counts".into(), "biomass".into()])
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    assert_eq!(count_0, 548); // 5.484 * 100 ≈ 548
}

/// Python test_merge_schema: dynamically add a process to a running engine
/// by merging new schema that declares a Link node.
#[test]
fn test_merge_schema() {
    use prism_bigraph::factory::ProcessRegistry;

    let mut registry = ProcessRegistry::new();
    registry.register("IncreaseProcess", |config| {
        let rate = config.as_map()
            .and_then(|m| m.get("rate"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.1);
        ProcessNode::Process(Box::new(IncreaseProcess { rate }))
    });

    // Start with a simple engine — just data, no processes
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("a".into(), Schema::float()),
        ]),
    };
    let state = Value::tree([("a", Value::float(11.0))]);
    let registry = Arc::new(registry);
    let mut engine = Engine::from_state(schema, state, Arc::clone(&registry)).unwrap();

    // Verify: no processes yet
    assert_eq!(engine.node_names().len(), 0);

    // Merge a new process schema + default state
    let increase_schema = Schema::Tree {
        branches: IndexMap::from([
            ("increase".into(), Schema::process(
                IndexMap::from([("level".into(), Schema::float())]),
                IndexMap::from([("level".into(), Schema::float())]),
            )),
        ]),
    };
    let increase_state = Value::tree([
        ("increase", Value::Map(IndexMap::from([
            ("address".to_string(), Value::String("local:IncreaseProcess".into())),
            ("config".to_string(), Value::tree([("rate", Value::float(0.0001))])),
            ("inputs".to_string(), Value::tree([("level", Value::List(vec![Value::String("a".into())]))])),
            ("outputs".to_string(), Value::tree([("level", Value::List(vec![Value::String("a".into())]))])),
        ]))),
    ]);

    engine.merge_schema(increase_schema, increase_state);

    // Verify: process was instantiated
    assert!(engine.node_names().contains(&"increase"),
        "expected 'increase' process, got {:?}", engine.node_names());

    // Run and verify the process affects state
    let before = engine.state().get_path(&["a".into()]).unwrap().as_f64().unwrap();
    engine.run(10.0);
    let after = engine.state().get_path(&["a".into()]).unwrap().as_f64().unwrap();
    assert!(after > before, "process should have increased a: {before} -> {after}");
}

/// Python test_match_star_path
#[test]
fn test_match_star_path() {
    fn match_star(path: &[&str], pattern: &[&str]) -> bool {
        if path.len() != pattern.len() { return false; }
        path.iter().zip(pattern.iter()).all(|(p, q)| *q == "*" || p == q)
    }
    assert!(match_star(&["first", "list", "test"], &["first", "*", "test"]));
    assert!(!match_star(&["first", "list", "tent"], &["first", "*", "test"]));
    assert!(match_star(&["first", "list", "test"], &["first", "list", "test"]));
}
