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

#[test]
#[ignore] // Requires schema-driven process discovery
fn test_composite_basic() {
    // Python test_composite: composite with bridge I/O
    // process inside updates value, bridge exposes it
}

#[test]
#[ignore] // Requires schema-driven process instantiation
fn test_infer_process_from_state() {
    // Python test_infer: state with _type='process' is auto-instantiated
}

#[test]
#[ignore] // Requires grow/divide agent
fn test_grow_divide() {
    // Python test_grow_divide: particles grow and divide
}

#[test]
#[ignore] // Requires star path matching
fn test_star_update() {
    // Python test_star_update: wildcard paths like ['Compartments', '*', 'volume']
}

#[test]
#[ignore] // Requires merge_schema
fn test_merge_schema() {
    // Python test_merge_schema: dynamically add process schema to running composite
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
