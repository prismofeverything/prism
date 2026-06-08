//! **#61b CLOSED — a reaction crosses a LIVE `rest:` bridge and fires server-side.**
//!
//! `reaction_transport.rs` proves the per-port codec carries a `map[Reaction]`
//! across a boundary, but through a hand-rolled `serde_json` round-trip "without a
//! live socket". This is the real thing: a `RestProcessServer` + `RestProcess` over
//! HTTP. The reaction exists ONLY on the client; it crosses to the server as
//! structural data (a runnable `Foreign(FOREIGN_REACTION,…)` would `value_to_json`
//! to `null`), is reconstructed to the runnable carrier by the server's codec —
//! which works ONLY because the whole Core (with the `Reaction` type) is threaded
//! into the server — and FIRES there (A→B). The payoff of the Core-threading arc:
//! reactions-as-data ride a real transport.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{RestProcessServer, RestProtocol};
use prism_bigraph::{BigraphicalReactiveSystem, Core, Protocol, Schema, Update, Value};
use prism_schema::reaction::{Pattern, ReactionRule};
use prism_schema::registry::TypeRegistry;
use prism_schema::value::Foreign;
use prism_schema::{algebra, FOREIGN_REACTION, Key};

use chrysalis::runtime::rule::ReactionType;

/// A structural A→B reaction in the runnable carrier form a `:: Reaction` slot holds.
fn reaction_a_to_b() -> Value {
    let redex = Pattern::sort("A", Vec::<(Key, Pattern)>::new());
    let reactum = Pattern::sort("B", Vec::<(Key, Pattern)>::new());
    let rule = ReactionRule::new(redex, reactum).with_label("Convert");
    Value::Foreign(Foreign::new(FOREIGN_REACTION, rule))
}

fn map_reaction_schema() -> Schema {
    Schema::Map {
        value: Box::new(Schema::Custom {
            name: "Reaction".into(),
            parameters: IndexMap::new(),
        }),
    }
}

fn ion(t: &str) -> Value {
    Value::tree([("_type", Value::String(t.into()))])
}

fn collect_types(v: &Value, out: &mut Vec<String>) {
    if let Some(m) = v.as_map() {
        if let Some(t) = m.get("_type").and_then(|t| t.as_str()) {
            out.push(t.to_string());
        }
        for vv in m.values() {
            collect_types(vv, out);
        }
    }
}

/// Drive a BRS over a `{state: subtree}` view, applying each tick's delta.
fn drive(brs: &BigraphicalReactiveSystem, mut subtree: Value, ticks: usize) -> Value {
    for _ in 0..ticks {
        let view = Value::tree([("state", subtree.clone())]);
        if let Update::Value(out) = brs.update(&view, 1.0) {
            if let Some(delta) = out.get_field("state") {
                subtree = algebra::apply_with(None, &Schema::Any, &subtree, delta);
            }
        }
    }
    subtree
}

/// A process that RECEIVES a `map[Reaction]` pool + a `seed` sub-bigraph, builds a
/// BRS from the (reconstructed-runnable) reactions, and fires it on the seed. It
/// runs SERVER-SIDE over rest, so the reactions it fires must have survived the
/// crossing as data and been rebuilt — proving the wire carries runnable reactions.
#[derive(Debug)]
struct RunPool;
impl Process for RunPool {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("pool".to_string(), map_reaction_schema()),
            ("seed".to_string(), Schema::Any),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("result".to_string(), Schema::Any)])
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let rules: Vec<ReactionRule> = state
            .as_map()
            .and_then(|m| m.get("pool"))
            .and_then(|p| p.as_map())
            .map(|m| {
                m.values()
                    .filter_map(|v| match v {
                        Value::Foreign(f) if f.type_name == FOREIGN_REACTION => {
                            f.downcast_ref::<ReactionRule>().cloned()
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let seed = state
            .as_map()
            .and_then(|m| m.get("seed"))
            .cloned()
            .unwrap_or(Value::None);
        let fired = drive(&BigraphicalReactiveSystem::new(rules), seed, 2);
        Update::value(Value::tree([("result", fired)]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A Core carrying the `Reaction` type (so the boundary codec can transport it) and
/// the `RunPool` factory — shared by server and client.
fn pool_core() -> Core {
    let mut types = TypeRegistry::new();
    types.register_full(
        "Reaction",
        Schema::Any,
        None,
        Some(Arc::new(ReactionType)),
        Vec::new(),
    );
    let mut processes = ProcessRegistry::new();
    processes.register("RunPool", |_| ProcessNode::Process(Box::new(RunPool)));
    Core::new()
        .with_types(Arc::new(types))
        .with_processes(Arc::new(processes))
}

#[test]
fn a_reaction_crosses_a_live_rest_bridge_and_fires_on_the_server() {
    let core = pool_core();
    let server = RestProcessServer::start(core.clone()).expect("start rest server");
    std::thread::sleep(Duration::from_millis(50));

    let addr = Value::Map(IndexMap::from_iter([
        ("process".into(), Value::String("RunPool".into())),
        ("host".into(), Value::String("127.0.0.1".into())),
        ("port".into(), Value::Int(server.port() as i64)),
    ]));
    let node = RestProtocol
        .instantiate(&addr, Value::None, &core)
        .expect("rest RunPool");
    let ProcessNode::Process(client) = node else {
        panic!("expected a Process");
    };

    // The reaction lives ONLY on the client; it must cross to the server as data.
    let state = Value::tree([
        ("pool", Value::tree([("grow", reaction_a_to_b())])),
        ("seed", Value::tree([("a0", ion("A"))])),
    ]);
    let out = client
        .update(&state, 1.0)
        .into_value()
        .expect("an update came back");
    let result = out.get_field("result").expect("the result port");

    let mut kinds = Vec::new();
    collect_types(result, &mut kinds);
    assert!(
        kinds.iter().any(|t| t == "B"),
        "the reaction crossed the LIVE rest bridge and FIRED server-side (A→B): {result:?}"
    );
    assert!(
        !kinds.iter().any(|t| t == "A"),
        "the A node was consumed by the firing (so it really ran, not just crossed): {result:?}"
    );
}
