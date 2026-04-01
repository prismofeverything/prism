//! Profile steady-state vs division-tick cost.

use std::collections::HashMap;
use std::sync::Arc;
use std::any::Any;
use std::time::Instant;

use indexmap::IndexMap;
use prism_bigraph::composite::{Bridge, Composite};
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode, Step};
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Schema, Update, Value};

#[derive(Clone, Debug)]
struct Grow { rate: f64 }
impl Process for Grow {
    fn inputs(&self) -> IndexMap<String, Schema> { IndexMap::from([("mass".into(), Schema::float())]) }
    fn outputs(&self) -> IndexMap<String, Schema> { IndexMap::from([("mass".into(), Schema::float())]) }
    fn interval(&self) -> f64 { 1.0 }
    fn update(&self, state: &Value, interval: f64) -> Update {
        let mass = state.get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
        Update::value(Value::tree([("mass", Value::float(self.rate * mass * interval))]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

#[derive(Clone, Debug)]
struct Divide { threshold: f64, agent_id: String }
impl Step for Divide {
    fn inputs(&self) -> IndexMap<String, Schema> { IndexMap::from([("trigger".into(), Schema::float())]) }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("environment".into(), Schema::Overwrite { inner: Box::new(Schema::map(Schema::Any)) })])
    }
    fn update(&self, state: &Value) -> Update {
        let mass = state.get_field("trigger").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if mass < self.threshold { return Update::Noop; }
        let half = mass / 2.0;
        let id_a = format!("{}_0", self.agent_id);
        let id_b = format!("{}_1", self.agent_id);
        let make_daughter = |id: &str| -> Value {
            Value::tree([
                ("mass", Value::float(half)),
                ("grow_divide", Value::Map(IndexMap::from([
                    ("address".into(), Value::String("local:GrowDivideAgent".into())),
                    ("config".into(), Value::tree([("agent_id", Value::String(id.into()))])),
                    ("inputs".into(), Value::tree([("mass", Value::List(vec![Value::String("mass".into())]))])),
                    ("outputs".into(), Value::tree([
                        ("mass", Value::List(vec![Value::String("mass".into())])),
                        ("environment", Value::List(vec![
                            Value::String("..".into()), Value::String("..".into()), Value::String("environment".into()),
                        ])),
                    ])),
                ]))),
            ])
        };
        let mut env_update = IndexMap::new();
        env_update.insert("_remove".into(), Value::List(vec![Value::String(self.agent_id.clone())]));
        env_update.insert("_add".into(), Value::Map(IndexMap::from([
            (prism_schema::Key::from(id_a.as_str()), make_daughter(&id_a)),
            (prism_schema::Key::from(id_b.as_str()), make_daughter(&id_b)),
        ])));
        Update::value(Value::tree([("environment", Value::Map(env_update))]))
    }
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

fn make_composite(growth_rate: f64, threshold: f64, agent_id: &str) -> Composite {
    let mut topo = Topology::new();
    topo.initial_state = Value::tree([("mass", Value::float(0.0))]);
    topo.state_schema = Schema::Tree { branches: IndexMap::from([("mass".into(), Schema::float())]) };
    topo.processes.insert("grow".into(), ProcessSpec {
        process_type: "Grow".into(),
        config: Value::tree([("rate", Value::float(growth_rate))]),
        inputs: IndexMap::from([("mass".into(), vec!["mass".into()])]),
        outputs: IndexMap::from([("mass".into(), vec!["mass".into()])]),
        interval: Some(1.0), priority: 0.0,
    });
    topo.processes.insert("divide".into(), ProcessSpec {
        process_type: "Divide".into(), config: Value::None,
        inputs: IndexMap::from([("trigger".into(), vec!["mass".into()])]),
        outputs: IndexMap::from([("environment".into(), vec!["environment".into()])]),
        interval: None, priority: 0.0,
    });
    let mut inst = HashMap::new();
    inst.insert("grow".into(), ProcessNode::Process(Box::new(Grow { rate: growth_rate })));
    inst.insert("divide".into(), ProcessNode::Step(Box::new(Divide { threshold, agent_id: agent_id.into() })));
    let inner = Engine::new(topo, inst);
    Composite::new(inner,
        Bridge { mappings: IndexMap::from([("mass".into(), vec!["mass".into()])]) },
        Bridge { mappings: IndexMap::from([
            ("mass".into(), vec!["mass".into()]),
            ("environment".into(), vec!["environment".into()]),
        ]) },
        IndexMap::from([("mass".into(), Schema::float())]),
        IndexMap::from([
            ("mass".into(), Schema::float()),
            ("environment".into(), Schema::map(Schema::Any)),
        ]),
        1.0,
    )
}

fn main() {
    let growth_rate = 0.1;
    let threshold = 2.0;

    let mut reg = ProcessRegistry::new();
    let gr = growth_rate;
    let th = threshold;
    reg.register("GrowDivideAgent", move |config| {
        let agent_id = config.as_map()
            .and_then(|m| m.get("agent_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("0").to_string();
        ProcessNode::Process(Box::new(make_composite(gr, th, &agent_id)))
    });

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
                    ("inputs".into(), Value::tree([("mass", Value::List(vec![Value::String("mass".into())]))])),
                    ("outputs".into(), Value::tree([
                        ("mass", Value::List(vec![Value::String("mass".into())])),
                        ("environment", Value::List(vec![
                            Value::String("..".into()), Value::String("..".into()), Value::String("environment".into()),
                        ])),
                    ])),
                ]))),
            ])),
        ])),
    ]);

    let registry = Arc::new(reg);
    let mut engine = Engine::from_state(schema, state, Arc::clone(&registry)).unwrap();

    println!("tick | agents | tick_ms | event");
    for tick in 0..200 {
        let pre_agents = engine.state().get_field("environment")
            .and_then(|v| v.as_map()).map(|m| m.len()).unwrap_or(0);
        if pre_agents > 128 {
            println!("stopping at {} agents", pre_agents);
            break;
        }
        let t0 = Instant::now();
        engine.run(1.0);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let post_agents = engine.state().get_field("environment")
            .and_then(|v| v.as_map()).map(|m| m.len()).unwrap_or(0);
        let event = if post_agents != pre_agents { "DIVIDE" } else { "" };
        println!("{tick:>4} | {post_agents:>6} | {ms:>7.3} | {event}");
    }
}
