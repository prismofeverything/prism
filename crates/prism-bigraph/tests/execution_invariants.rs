//! Executable axioms for composite execution (#16) — the engine analog of
//! prism-schema's algebra-law tests. These assert the run-loop correctness
//! properties the engine-correctness arc established. If one regresses, composite
//! execution is no longer correct (and a workaround likely crept back in).

use std::any::Any;
use std::collections::HashMap;

use indexmap::IndexMap;

use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Key, Schema, Update, Value};

fn overwrite_float() -> Schema {
    Schema::Overwrite {
        inner: Box::new(Schema::float()),
    }
}

/// Copies input port `in` onto output port `out` (overwrite).
#[derive(Debug)]
struct Copy;
impl Process for Copy {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("in".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), overwrite_float())])
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let v = state
            .get_field("in")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Update::value(Value::tree([("out", Value::float(v))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Emits a fixed additive delta on `out`.
#[derive(Debug)]
struct AddDelta(f64);
impl Process for AddDelta {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), Schema::float())])
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        Update::value(Value::tree([("out", Value::float(self.0))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Binary op of inputs `x`,`y` onto `out` (overwrite).
#[derive(Debug)]
struct Op(&'static str);
impl Step for Op {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("x".to_string(), Schema::float()),
            ("y".to_string(), Schema::float()),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("out".to_string(), overwrite_float())])
    }
    fn update(&self, state: &Value) -> Update {
        let x = state.get_field("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = state.get_field("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let r = match self.0 {
            "+" => x + y,
            "*" => x * y,
            _ => 0.0,
        };
        Update::value(Value::tree([("out", Value::float(r))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn spec(
    ty: &str,
    inputs: &[(&str, &str)],
    outputs: &[(&str, &str)],
    interval: Option<f64>,
) -> ProcessSpec {
    ProcessSpec {
        process_type: ty.to_string(),
        config: Value::None,
        inputs: inputs
            .iter()
            .map(|(p, s)| (p.to_string(), vec![Key::from(*s)]))
            .collect(),
        outputs: outputs
            .iter()
            .map(|(p, s)| (p.to_string(), vec![Key::from(*s)]))
            .collect(),
        interval,
        priority: 0.0,
    }
}

/// **Invoke/apply separation.** Two processes copying each other's store in one
/// tick each read the PRE-tick value, so the values SWAP. A mid-tick leak (apply
/// before the other invokes) would make the second read the first's new value ⇒
/// no swap.
#[test]
fn processes_in_a_tick_share_one_pre_tick_snapshot() {
    let mut topo = Topology::new();
    topo.initial_state = Value::tree([("a", Value::float(1.0)), ("b", Value::float(2.0))]);
    topo.state_schema = Schema::tree([("a", overwrite_float()), ("b", overwrite_float())]);
    topo.processes.insert(
        "pa".into(),
        spec("Copy", &[("in", "b")], &[("out", "a")], Some(1.0)),
    );
    topo.processes.insert(
        "pb".into(),
        spec("Copy", &[("in", "a")], &[("out", "b")], Some(1.0)),
    );
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("pa".into(), ProcessNode::Process(Box::new(Copy)));
    instances.insert("pb".into(), ProcessNode::Process(Box::new(Copy)));

    let mut engine = Engine::new(topo, instances);
    engine.run(1.0);

    let a = engine.state().get_field("a").and_then(|v| v.as_f64());
    let b = engine.state().get_field("b").and_then(|v| v.as_f64());
    assert_eq!(
        (a, b),
        (Some(2.0), Some(1.0)),
        "values swap ⇒ both processes read the same pre-tick snapshot (no mid-tick leak)"
    );
}

/// **Reconciliation.** Two processes writing the same additive store in one tick
/// are SUMMED, not last-write-wins.
#[test]
fn concurrent_deltas_to_one_store_reconcile_by_sum() {
    let mut topo = Topology::new();
    topo.initial_state = Value::tree([("x", Value::float(0.0))]);
    topo.state_schema = Schema::tree([("x", Schema::float())]);
    topo.processes.insert(
        "p1".into(),
        spec("AddDelta", &[], &[("out", "x")], Some(1.0)),
    );
    topo.processes.insert(
        "p2".into(),
        spec("AddDelta", &[], &[("out", "x")], Some(1.0)),
    );
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("p1".into(), ProcessNode::Process(Box::new(AddDelta(1.0))));
    instances.insert("p2".into(), ProcessNode::Process(Box::new(AddDelta(2.0))));

    let mut engine = Engine::new(topo, instances);
    engine.run(1.0);

    assert_eq!(
        engine.state().get_field("x").and_then(|v| v.as_f64()),
        Some(3.0),
        "two deltas (+1, +2) to one store reconcile to +3, not last-wins"
    );
}

/// **Dependency layering.** A consumer step runs in a later layer than its
/// producer, so it sees the producer's output: `C = A+B` (layer 0), `D = B*C`
/// (layer 1) ⇒ `D = B*(A+B) = 21*(13+21) = 714`. (Not a mid-tick leak — layer
/// ordering: a consumer waits for its producer's whole layer to land.)
#[test]
fn step_consumer_runs_in_a_later_layer_than_its_producer() {
    let mut topo = Topology::new();
    topo.initial_state = Value::tree([
        ("a", Value::float(13.0)),
        ("b", Value::float(21.0)),
        ("c", Value::float(0.0)),
        ("d", Value::float(0.0)),
    ]);
    topo.state_schema = Schema::tree([
        ("a", Schema::float()),
        ("b", Schema::float()),
        ("c", overwrite_float()),
        ("d", overwrite_float()),
    ]);
    topo.processes.insert(
        "s1".into(),
        spec("Op", &[("x", "a"), ("y", "b")], &[("out", "c")], None),
    );
    topo.processes.insert(
        "s2".into(),
        spec("Op", &[("x", "b"), ("y", "c")], &[("out", "d")], None),
    );
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("s1".into(), ProcessNode::Step(Box::new(Op("+"))));
    instances.insert("s2".into(), ProcessNode::Step(Box::new(Op("*"))));

    let mut engine = Engine::new(topo, instances);
    engine.run(0.0); // settle the step DAG at init

    assert_eq!(
        engine.state().get_field("c").and_then(|v| v.as_f64()),
        Some(34.0),
        "producer C = A+B"
    );
    assert_eq!(
        engine.state().get_field("d").and_then(|v| v.as_f64()),
        Some(714.0),
        "consumer D = B*C saw the producer's C via layer ordering"
    );
}

/// Emits a fixed update on output port `out` (used to drive structural sentinels
/// into one reconciled layer).
#[derive(Debug)]
struct Emit(Value);
impl Step for Emit {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(
            "out".to_string(),
            Schema::map(Schema::tree([("v", Schema::float())])),
        )])
    }
    fn update(&self, _state: &Value) -> Update {
        Update::value(Value::tree([("out", self.0.clone())]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn one_cell_map() -> (Value, Schema) {
    (
        Value::tree([(
            "m",
            Value::tree([("a0", Value::tree([("v", Value::float(1.0))]))]),
        )]),
        Schema::tree([("m", Schema::map(Schema::tree([("v", Schema::float())])))]),
    )
}

/// **Remove-wins.** In one reconciled layer, a `_remove` of a key voids a
/// concurrent value-update to it — the key is gone, not re-created by the write.
#[test]
fn remove_wins_over_a_concurrent_write() {
    let (state, schema) = one_cell_map();
    let mut topo = Topology::new();
    topo.initial_state = state;
    topo.state_schema = schema;
    topo.processes
        .insert("rm".into(), spec("Emit", &[], &[("out", "m")], None));
    topo.processes
        .insert("wr".into(), spec("Emit", &[], &[("out", "m")], None));
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert(
        "rm".into(),
        ProcessNode::Step(Box::new(Emit(Value::tree([(
            "_remove",
            Value::List(vec![Value::String("a0".to_string())]),
        )])))),
    );
    instances.insert(
        "wr".into(),
        ProcessNode::Step(Box::new(Emit(Value::tree([(
            "a0",
            Value::tree([("v", Value::float(5.0))]),
        )])))),
    );

    let mut engine = Engine::new(topo, instances);
    engine.run(0.0);

    let has_a0 = engine
        .state()
        .get_field("m")
        .and_then(|m| m.get_field("a0"))
        .is_some();
    assert!(
        !has_a0,
        "_remove wins over a concurrent value-update (a0 is gone, not re-created)"
    );
}

/// **`_add`-compose.** In one reconciled layer, re-adding a key (`_add`, absolute)
/// composes with a concurrent additive write to it: `_add v=10` then `+5` ⇒ 15
/// (the pre-`_add`-base bug would yield 6, the old value 1 + 5).
#[test]
fn add_composes_with_a_concurrent_write() {
    let (state, schema) = one_cell_map();
    let mut topo = Topology::new();
    topo.initial_state = state;
    topo.state_schema = schema;
    topo.processes
        .insert("add".into(), spec("Emit", &[], &[("out", "m")], None));
    topo.processes
        .insert("delta".into(), spec("Emit", &[], &[("out", "m")], None));
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert(
        "add".into(),
        ProcessNode::Step(Box::new(Emit(Value::tree([(
            "_add",
            Value::tree([("a0", Value::tree([("v", Value::float(10.0))]))]),
        )])))),
    );
    instances.insert(
        "delta".into(),
        ProcessNode::Step(Box::new(Emit(Value::tree([(
            "a0",
            Value::tree([("v", Value::float(5.0))]),
        )])))),
    );

    let mut engine = Engine::new(topo, instances);
    engine.run(0.0);

    let v = engine
        .state()
        .get_field("m")
        .and_then(|m| m.get_field("a0"))
        .and_then(|a| a.get_field("v"))
        .and_then(|v| v.as_f64());
    assert_eq!(
        v,
        Some(15.0),
        "_add(v=10) composes with concurrent +5 ⇒ 15, not 6"
    );
}

/// **`_remove` + `_add` of the same key = replace.** When one tick both removes
/// and re-adds a key, `apply` runs `_remove` then `_add`, so the existing entry
/// is dropped and the freshly-added one takes its place (not gone, not the old
/// value). `reconcile` keeps both sentinels.
#[test]
fn remove_then_add_of_same_key_replaces() {
    let (state, schema) = one_cell_map();
    let mut topo = Topology::new();
    topo.initial_state = state;
    topo.state_schema = schema;
    topo.processes
        .insert("rm".into(), spec("Emit", &[], &[("out", "m")], None));
    topo.processes
        .insert("add".into(), spec("Emit", &[], &[("out", "m")], None));
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert(
        "rm".into(),
        ProcessNode::Step(Box::new(Emit(Value::tree([(
            "_remove",
            Value::List(vec![Value::String("a0".to_string())]),
        )])))),
    );
    instances.insert(
        "add".into(),
        ProcessNode::Step(Box::new(Emit(Value::tree([(
            "_add",
            Value::tree([("a0", Value::tree([("v", Value::float(99.0))]))]),
        )])))),
    );

    let mut engine = Engine::new(topo, instances);
    engine.run(0.0);

    let v = engine
        .state()
        .get_field("m")
        .and_then(|m| m.get_field("a0"))
        .and_then(|a| a.get_field("v"))
        .and_then(|v| v.as_f64());
    assert_eq!(
        v,
        Some(99.0),
        "_remove+_add of a0 in one tick replaces it: a0 = the new {{v:99}}, not old 1 nor gone"
    );
}
