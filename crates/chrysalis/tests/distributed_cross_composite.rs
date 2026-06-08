//! **#43 — the DISTRIBUTED cross-composite reactor.** A cross-composite coupling
//! reaction crosses a LIVE `rest:` bridge and fires on REMOTE composites,
//! modifying them IN PLACE server-side.
//!
//! This is the combination of three proven pieces, over a real socket:
//!   - the reaction TRANSPORT (#61b, `reaction_rest.rs`) — a reaction lives only
//!     on the client, crosses to a `RestProcessServer` as structural data, and is
//!     reconstructed runnable by the server's threaded-Core codec;
//!   - the cross-composite LINK-GRAPH match — `?west … | ?east …` couples two
//!     sealed composites on a shared `edge` link (no unfurl; runs through the
//!     server's `BigraphicalReactiveSystem`);
//!   - the IN-PLACE keying — a reactum keyed by the binders `{ ?west: …, ?east: …
//!     }` modifies the coupled pair at their OWN keys (`key_map[?west]` →
//!     `remap_keys`), not under fresh keys.
//!
//! The reaction is STRUCTURAL (closure-free), so it serializes (`to_data_value`)
//! and crosses the wire. The payoff: "send the reaction to where the composites
//! live" — the BATWD §V remote case, end-to-end over HTTP.

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
use prism_schema::{algebra, Key, FOREIGN_REACTION};

use chrysalis::runtime::rule::ReactionType;

fn map_reaction_schema() -> Schema {
    Schema::Map {
        value: Box::new(Schema::Custom {
            name: "Reaction".into(),
            parameters: IndexMap::new(),
        }),
    }
}

fn wire() -> Value {
    Value::List(vec![Value::String("_links".into()), Value::String("e".into())])
}

/// A sealed `Cell` composite-ish node exposing an `edge` input port (wired to the
/// shared link) and carrying `mass` — the matchable, modifiable surface.
fn cell() -> Value {
    Value::tree([
        ("_type", Value::String("Cell".into())),
        ("inputs", Value::tree([("edge", wire())])),
        ("mass", Value::float(1.0)),
    ])
}

/// Two cells coupled on the SAME edge link → the redex's shared `~e` unifies.
fn coupled_cells() -> Value {
    Value::tree([("a", cell()), ("b", cell())])
}

/// The cross-composite IN-PLACE coupling reaction, as a runnable carrier. Pure
/// `Pattern` (no closure) → it serializes and crosses the wire.
/// `?west :: Cell[bonded: absent] ~{edge: ~e} | ?east :: … => { ?west: Cell[bonded:
/// ~e] ~{edge: ~e}, ?east: … }` (NAC `bonded: absent` → fires once).
fn coupling_reaction() -> Value {
    let cell_redex = || {
        Pattern::sort(
            "Cell",
            [
                ("bonded", Pattern::Absent),
                ("inputs", Pattern::map([("edge", Pattern::link_var("e"))])),
            ],
        )
    };
    let redex = Pattern::list([
        Pattern::Bind { name: Key::from("?west"), inner: Box::new(cell_redex()) },
        Pattern::Bind { name: Key::from("?east"), inner: Box::new(cell_redex()) },
    ]);
    let cell_reactum = || {
        Pattern::sort(
            "Cell",
            [
                ("bonded", Pattern::link_var("e")),
                ("inputs", Pattern::map([("edge", Pattern::link_var("e"))])),
            ],
        )
    };
    let reactum = Pattern::map([("?west", cell_reactum()), ("?east", cell_reactum())]);
    let rule = ReactionRule::new(redex, reactum).with_label("Couple");
    Value::Foreign(Foreign::new(FOREIGN_REACTION, rule))
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

/// SERVER-SIDE process: receive a `map[Reaction]` pool + a `cells` container, build
/// a BRS from the (reconstructed-runnable) reactions, fire it on the cells, return
/// the result. It runs over rest, so the reactions must have survived the crossing
/// as data — and the cross-composite ones must fire IN PLACE here.
#[derive(Debug)]
struct RemoteReactor;
impl Process for RemoteReactor {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("pool".to_string(), map_reaction_schema()),
            ("cells".to_string(), Schema::Any),
        ])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("cells".to_string(), Schema::Any)])
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
        let cells = state
            .as_map()
            .and_then(|m| m.get("cells"))
            .cloned()
            .unwrap_or(Value::None);
        let fired = drive(&BigraphicalReactiveSystem::new(rules), cells, 2);
        Update::value(Value::tree([("cells", fired)]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn reactor_core() -> Core {
    let mut types = TypeRegistry::new();
    types.register_full("Reaction", Schema::Any, None, Some(Arc::new(ReactionType)), Vec::new());
    let mut processes = ProcessRegistry::new();
    processes.register("RemoteReactor", |_| ProcessNode::Process(Box::new(RemoteReactor)));
    Core::new()
        .with_types(Arc::new(types))
        .with_processes(Arc::new(processes))
}

#[test]
fn a_cross_composite_reaction_crosses_a_live_rest_bridge_and_couples_remote_composites() {
    let core = reactor_core();
    let server = RestProcessServer::start(core.clone()).expect("start rest server");
    std::thread::sleep(Duration::from_millis(50));

    let addr = Value::Map(IndexMap::from_iter([
        ("process".into(), Value::String("RemoteReactor".into())),
        ("host".into(), Value::String("127.0.0.1".into())),
        ("port".into(), Value::Int(server.port() as i64)),
    ]));
    let node = RestProtocol
        .instantiate(&addr, Value::None, &core)
        .expect("rest RemoteReactor");
    let ProcessNode::Process(client) = node else {
        panic!("expected a Process");
    };

    // The cross-composite coupling reaction lives ONLY on the client; the cells
    // live on the server. The reaction must cross as data and fire there.
    let state = Value::tree([
        ("pool", Value::tree([("couple", coupling_reaction())])),
        ("cells", coupled_cells()),
    ]);
    let out = client.update(&state, 1.0).into_value().expect("an update came back");
    let cells = out.get_field("cells").expect("the cells port");
    let cm = cells.as_map().expect("cells map");

    // IN PLACE: the SAME keys a, b (not fresh) — the coupling modified them where
    // they live, on the remote server.
    let a = cm.get("a").unwrap_or_else(|| panic!("cell a in place; cells={cells:?}"));
    let b = cm.get("b").expect("cell b in place");
    // Both gained the shared bond — the cross-composite reaction COUPLED them
    // server-side, after crossing the live bridge.
    assert!(
        a.get_field("bonded").is_some_and(|v| !matches!(v, Value::None)),
        "remote cell a bonded in place: {a:?}"
    );
    assert!(
        b.get_field("bonded").is_some_and(|v| !matches!(v, Value::None)),
        "remote cell b bonded in place: {b:?}"
    );
    // Still a Cell (modified, not replaced by a different sort). Field
    // preservation via rest-capture is shown in the local `.ys` demo; this test
    // focuses on the transport + the in-place coupling firing remotely.
    assert_eq!(a.get_field("_type").and_then(|v| v.as_str()), Some("Cell"));
}
