//! Probe: print resolved input wires for the bench's grow process.
use std::collections::HashMap;
use std::sync::Arc;
use std::any::Any;
use indexmap::IndexMap;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::{Engine, Schema, Update, Value};

#[derive(Clone, Debug)]
struct PrintGrow;
impl Process for PrintGrow {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".into(), Schema::float())])
    }
    fn interval(&self) -> f64 { 1.0 }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        eprintln!("PrintGrow.update saw state = {:?}", state);
        Update::Noop
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

fn main() {
    use prism_schema::Key;
    let mut reg = ProcessRegistry::new();
    reg.register("PrintGrow", |_config| ProcessNode::Process(Box::new(PrintGrow)));
    let reg = Arc::new(reg);

    // Same shape as bench
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("environment".into(), Schema::map(Schema::Tree {
                branches: IndexMap::from([
                    ("mass".into(), Schema::float()),
                    ("grow".into(), Schema::process(
                        IndexMap::from([("mass".into(), Schema::float())]),
                        IndexMap::from([("mass".into(), Schema::float())]),
                    )),
                ]),
            })),
        ]),
    };

    let state = Value::tree([
        ("environment", Value::tree([
            ("0", Value::tree([
                ("mass", Value::float(1.0)),
                ("grow", Value::Map(IndexMap::from([
                    (Key::from("address"), Value::String("local:PrintGrow".into())),
                    (Key::from("config"), Value::None),
                    (Key::from("inputs"), Value::tree([("mass", Value::List(vec![
                        Value::String("..".into()), Value::String("mass".into()),
                    ]))])),
                    (Key::from("outputs"), Value::tree([("mass", Value::List(vec![
                        Value::String("..".into()), Value::String("mass".into()),
                    ]))])),
                ]))),
            ])),
        ])),
    ]);

    let engine = Engine::from_state(schema, state, reg).unwrap();
    eprintln!("registered specs:");
    for (name, spec) in engine.specs().iter() {
        eprintln!("  {name} inputs = {:?}", spec.inputs);
        eprintln!("  {name} outputs = {:?}", spec.outputs);
    }
    let mut eng = engine;
    eng.run(1.0);
}
