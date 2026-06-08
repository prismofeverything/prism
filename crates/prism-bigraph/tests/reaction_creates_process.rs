//! **Demonstration / architecture probe (#5 / #59):** can a BRS reaction
//! *generate* live structure — a new process, a new composite — and does the
//! generative power require the **BRS** to hold the full [`Core`], or is the
//! **engine's** Core enough?
//!
//! The architecture under test is "BRS produces deltas → engine applies +
//! discovers + instantiates":
//!
//!   1. The BRS's `update` fires a reaction and RETURNS a localized delta. For a
//!      *generative* reaction the reactum's delta is `{_add: {name: <spec>}}` —
//!      a process/composite SPEC, which is just data (`{_type, address, config,
//!      inputs, outputs}`).
//!   2. The engine applies that delta (the BRS only needs `types` for the
//!      schema-aware apply), then `discover_processes` finds the new spec and
//!      instantiates it through the ENGINE's full Core —
//!      `core.protocols.instantiate(addr, config, &core)` — and `set_core`s
//!      the new node. So `local`/`rest`/`parallel`/`stream` and composite
//!      subengines all work, because the ENGINE holds the Core, not the BRS.
//!
//! Conclusion the tests below establish: a reaction can create a live process
//! AND a live composite (a subengine that inherits the full Core), with the BRS
//! producing only the spec delta. No expressivity is lost for *generation* by
//! keeping the BRS a pure delta producer — the Core lives one layer out, at the
//! applier. (Where a Core in the *reactum's evaluation context* would add power
//! is reflective / self-modifying reactions — a reactum that introspects the
//! available processes/types to decide what to build, or dispatches a
//! runtime-registered method; see the module note at the bottom.)

use std::any::Any;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::{BigraphicalReactiveSystem, Core, Engine, Key, Schema, StateMap, Update, Value};
use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};
use prism_schema::schema_to_value;

/// A process that emits +1 to its `n` output each tick (delta-typed, so it
/// accumulates on the wired slot).
#[derive(Debug)]
struct Counter;
impl Process for Counter {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("n".to_string(), Schema::delta())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("n".to_string(), Schema::delta())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        Update::value(Value::tree([("n", Value::float(1.0))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A place-graph-relative wire (`["count"]` → a sibling slot of the process).
fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String((*s).into())).collect())
}

/// A `local:Counter` process spec, wired (input+output) to the sibling `count`
/// slot. This is the *data* a generative reactum emits — no Core needed to build
/// it; the engine realizes it on discovery.
fn counter_proc_spec() -> Value {
    Value::tree([
        ("_type", Value::String("process".into())),
        ("address", Value::String("local:Counter".into())),
        ("config", Value::None),
        ("inputs", Value::tree([("n", wire(&["count"]))])),
        ("outputs", Value::tree([("n", wire(&["count"]))])),
    ])
}

/// A `local:Composite` spec: a sub-bigraph with an inner `Counter` writing to an
/// inner `m` slot, bridged out to the composite's `mass` port (wired to the
/// outer `mass` sibling slot). Building the live composite needs the full Core
/// (the subengine inherits types/processes/methods/protocols) — and the engine
/// supplies it at discovery, not the BRS.
fn counter_composite_spec() -> Value {
    let counter_proc = Value::tree([
        ("_type", Value::String("process".into())),
        ("address", Value::String("local:Counter".into())),
        ("config", Value::None),
        ("inputs", Value::tree([("n", wire(&["m"]))])),
        ("outputs", Value::tree([("n", wire(&["m"]))])),
    ]);
    let inner_state = Value::tree([("m", Value::float(0.0)), ("counter_proc", counter_proc)]);
    let bridge = Value::tree([
        ("inputs", Value::tree([("mass", wire(&["m"]))])),
        ("outputs", Value::tree([("mass", wire(&["m"]))])),
    ]);
    let inner_schema = Schema::Tree {
        branches: IndexMap::from([(Key::from("m"), Schema::delta())]),
    };
    let config = Value::tree([
        ("state", inner_state),
        ("bridge", bridge),
        ("schema", schema_to_value(&inner_schema)),
    ]);
    Value::tree([
        ("_type", Value::String("composite".into())),
        ("address", Value::String("local:Composite".into())),
        ("config", config),
        ("inputs", Value::tree([("mass", wire(&["mass"]))])),
        ("outputs", Value::tree([("mass", wire(&["mass"]))])),
    ])
}

/// Build a reaction `{ s: Seed } => { <added>: <spec> }`: match a `seed`-keyed
/// `Seed` ion, consume it, and `_add` the spec under `added_key`. The reactum is
/// a closure that returns the delta directly (a *computed* reactum) — the BRS
/// never sees a registry, only produces this data.
fn spawn_rule(added_key: &'static str, spec: fn() -> Value) -> ReactionRule {
    let no_fields: [(&str, Pattern); 0] = [];
    let redex = Pattern::map([("s", Pattern::sort("Seed", no_fields))]);
    let reactum_fn: ReactumFn = Arc::new(move |b: &Bindings| {
        let seed_key = b.key_map.get("s").map(|k| k.to_string()).unwrap_or_default();
        let mut add = StateMap::new();
        add.insert(Key::from(added_key), spec());
        Value::tree([
            ("_remove", Value::List(vec![Value::String(seed_key)])),
            ("_add", Value::Map(add)),
        ])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("spawn")
        .with_reactum_fn(reactum_fn)
}

/// A Core with `Counter`, `Composite`, and a `SpawnBrs` factory carrying `rule`.
/// `Core::from(registry)` — a registry-only Core (no custom types/methods) — is
/// all this needs; the point is the ENGINE threads it to discovered nodes.
fn make_core(rule: ReactionRule) -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Counter", |_c| ProcessNode::Process(Box::new(Counter)));

    // The Composite factory needs the Core handle so a discovered composite spec
    // can instantiate its inner subengine against the SAME Core.
    let handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle set");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("composite from_config"),
            ))
        });
    }

    let rule = Arc::new(rule);
    registry.register("SpawnBrs", move |_c| {
        ProcessNode::Process(Box::new(BigraphicalReactiveSystem::new(vec![(*rule).clone()])))
    });

    let core = Core::from(Arc::new(registry));
    let _ = handle.set(core.clone());
    core
}

/// Initial state: a `world` holding a `Seed` marker + a `count`/`mass` slot, and
/// a top-level `rxn` BRS wired over `world` (`~{state: world} ->{state: world}`).
fn initial(slot: &str) -> Value {
    Value::tree([
        (
            "world",
            Value::tree([
                ("seed", Value::tree([("_type", Value::String("Seed".into()))])),
                (slot, Value::float(0.0)),
            ]),
        ),
        (
            "rxn",
            Value::tree([
                ("_type", Value::String("process".into())),
                ("address", Value::String("local:SpawnBrs".into())),
                ("config", Value::None),
                ("inputs", Value::tree([("state", wire(&["world"]))])),
                ("outputs", Value::tree([("state", wire(&["world"]))])),
            ]),
        ),
    ])
}

#[test]
fn reaction_creates_a_live_process() {
    let mut engine =
        Engine::from_state(Schema::Any, initial("count"), make_core(spawn_rule("counter", counter_proc_spec)))
            .expect("engine");
    engine.discover_all_processes();

    // Control: the seed is present and nothing has been created at t=0.
    let s0 = engine.state();
    let world0 = s0.get_field("world").expect("world");
    assert!(world0.get_field("seed").is_some(), "seed present at t=0");
    assert!(world0.get_field("counter").is_none(), "no counter yet at t=0");

    engine.run(6.0);

    let s = engine.state().clone();
    let world = s.get_field("world").expect("world");
    // The reaction FIRED: the seed was consumed.
    assert!(world.get_field("seed").is_none(), "the reaction consumed the seed");
    // The reaction CREATED a process: the counter node exists in state.
    assert!(world.get_field("counter").is_some(), "the reaction created a counter process");
    // The created process is LIVE: count grew — the ENGINE instantiated the
    // reaction-created spec via its Core and scheduled it. The BRS only emitted
    // the spec as a delta.
    let count = world.get_field("count").and_then(|v| v.as_f64()).expect("count");
    assert!(count > 0.0, "the reaction-created Counter ran (count = {count})");
}

#[test]
fn reaction_creates_a_live_composite() {
    let mut engine = Engine::from_state(
        Schema::Any,
        initial("mass"),
        make_core(spawn_rule("cell", counter_composite_spec)),
    )
    .expect("engine");
    engine.discover_all_processes();

    let world0 = engine.state().get_field("world").expect("world").clone();
    assert!(world0.get_field("cell").is_none(), "no composite yet at t=0");

    engine.run(6.0);

    let s = engine.state().clone();
    let world = s.get_field("world").expect("world");
    // The reaction created a COMPOSITE (a sub-bigraph / subengine).
    assert!(world.get_field("seed").is_none(), "the reaction consumed the seed");
    assert!(world.get_field("cell").is_some(), "the reaction created a composite");
    // The composite's inner Counter ran and its bridge exported the value to the
    // outer `mass` slot — a whole subengine, built by the engine's Core from a
    // reaction-emitted spec, conserving the bigraphical boundary.
    let mass = world.get_field("mass").and_then(|v| v.as_f64()).expect("mass");
    assert!(mass > 0.0, "the reaction-created composite ran (mass = {mass})");
}

// ── Expressivity note (the answer to "do we lose anything?") ────────────────
//
// With "BRS produces deltas → engine applies + instantiates", a reaction can
// express any STRUCTURAL/TOPOLOGICAL rewrite — create/remove/divide processes
// and composites, over ANY protocol — because a spec is just data the engine
// realizes with its Core. The above proves the create cases end-to-end.
//
// The one semantic difference is intentional: a reaction-created process runs on
// the NEXT tick, not the firing tick (BSP snapshot consistency — engine.rs step
// ordering). That is a feature, not a loss.
//
// Where a Core in the REACTUM'S EVALUATION CONTEXT (vs the engine's applier)
// would add real power is *reflective* reactions: a reactum whose LOGIC reads
// the available processes/types/protocols to decide what to build, or dispatches
// a method registered at RUNTIME. Those are the AlChemy / reactions-making-
// reactions direction (#60/#61) — tracked separately, and the reason to thread
// ONE shared Core through the reactum evaluator when we get there.
