//! S1 part D: the **engine-driven** consumer test that the
//! `fold`/`unfurl`/`refuse_links` triad produces a behaviorally
//! equivalent flat parent.
//!
//! BATWD §IV claims that after `unfurl_into` + `refuse_links` a
//! composite-containing parent has been turned into ONE LARGER FLAT
//! BIGRAPH that runs identically — place graph AND link graph are
//! continuous across the dissolved boundary. The previous tests
//! (`prism-schema/tests/fold_unfurl.rs`) prove the algebra's structural
//! correctness (round-trip identity + wire rewrite math). This test
//! proves it END-TO-END by running BOTH forms through the same Engine
//! and asserting the observable state matches.
//!
//! Setup:
//!   - A `Counter` process that emits +1 to its `n` output each tick.
//!   - Form A (composite-containing): parent has `cells.alice = <composite
//!     spec>` whose inner state includes the Counter; the composite's
//!     output port `n` bridges internally to `[counter]` and externally to
//!     parent's `[..", alice_mass]`.
//!   - Form B (flat): parent obtained by `unfurl_into` + `refuse_links` —
//!     `cells.alice = {counter: 0.0, counter_proc: {…rewritten wires…}}`.
//!
//! Run both for 3 ticks. Both should yield `alice_mass == 3.0` because
//! Counter wrote +1 per tick into the wire that resolves to
//! parent.alice_mass in both forms.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::{Core, Engine, Schema, Update, Value};
use prism_schema::{refuse_links, schema_to_value, unfurl_into, Key};

/// A process that emits +1 to its `n` output each tick.
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

fn make_core() -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Counter", |_config| ProcessNode::Process(Box::new(Counter)));
    // The Composite factory needs the Core handle so a discovered composite
    // spec can instantiate its inner subengine.
    let handle: Arc<std::sync::OnceLock<Core>> = Arc::new(std::sync::OnceLock::new());
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("from_config"),
            ))
        });
    }
    let registry = Arc::new(registry);
    let core = Core::from(Arc::clone(&registry));
    let _ = handle.set(core.clone());
    core
}

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String((*s).into())).collect())
}

/// Build a composite-containing parent. Counter sits inside `cells.alice`,
/// writing +1 to an internal `counter` slot each tick; the composite's
/// bridge exports that to its outer `n` port; the outer wire writes
/// to the parent's `alice_mass`.
fn composite_parent() -> Value {
    let counter_proc = Value::tree([
        ("_type", Value::String("process".into())),
        ("address", Value::String("local:Counter".into())),
        ("config", Value::None),
        // Counter has no real INPUT in its update logic, but the
        // process trait declares one. Wire it to the same internal
        // slot for parity; it just reads the current value.
        ("inputs", Value::tree([("n", wire(&["counter"]))])),
        ("outputs", Value::tree([("n", wire(&["counter"]))])),
    ]);
    let inner_state = Value::tree([
        ("counter", Value::float(0.0)),
        ("counter_proc", counter_proc),
    ]);
    let bridge = Value::tree([
        ("inputs", Value::tree([("n", wire(&["counter"]))])),
        ("outputs", Value::tree([("n", wire(&["counter"]))])),
    ]);
    let inner_schema = Schema::Tree {
        branches: IndexMap::from([(Key::from("counter"), Schema::delta())]),
    };
    let config = Value::tree([
        ("state", inner_state),
        ("bridge", bridge),
        ("schema", schema_to_value(&inner_schema)),
    ]);
    let composite_spec = Value::tree([
        ("_type", Value::String("composite".into())),
        ("address", Value::String("local:Composite".into())),
        ("config", config),
        ("inputs", Value::tree([("n", wire(&["..", "alice_mass"]))])),
        // The composite's OUTER wire: cells container's `..` = root,
        // then `alice_mass` — the parent slot we observe.
        ("outputs", Value::tree([("n", wire(&["..", "alice_mass"]))])),
    ]);
    Value::tree([
        ("alice_mass", Value::float(0.0)),
        ("cells", Value::tree([("alice", composite_spec)])),
    ])
}

fn read_alice_mass(state: &Value) -> f64 {
    state
        .get_field("alice_mass")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN)
}

#[test]
fn unfurl_then_refuse_links_yields_a_behaviorally_equivalent_flat_parent() {
    let composite = composite_parent();
    let path = [Key::from("cells"), Key::from("alice")];

    // ── Form A — run the composite-containing parent ──
    let mut engine_a =
        Engine::from_state(Schema::Any, composite.clone(), make_core()).expect("engine A");
    engine_a.discover_all_processes();
    engine_a.run(3.0);
    let alice_mass_composite = read_alice_mass(engine_a.state());

    // ── Form B — unfurl_into + refuse_links, then run the FLAT parent ──
    let unfurled = unfurl_into(&composite, &path).expect("unfurl_into");
    let flat = refuse_links(&unfurled.parent, &path, &unfurled.boundary).expect("refuse_links");
    // Sanity: the spec wrapper is gone at cells.alice (this is the
    // structural inline — what S1 part B proved).
    assert!(
        flat.get_path(&path)
            .and_then(|v| v.get_field("_type"))
            .and_then(|v| v.as_str())
            != Some("composite"),
        "after unfurl_into, no composite sentinel at cells.alice"
    );

    let mut engine_b = Engine::from_state(Schema::Any, flat, make_core()).expect("engine B");
    engine_b.discover_all_processes();
    engine_b.run(3.0);
    let alice_mass_flat = read_alice_mass(engine_b.state());

    // ── The defining BATWD §IV claim ──
    assert_eq!(
        alice_mass_composite, alice_mass_flat,
        "composite and flat forms produce identical alice_mass after 3 ticks \
         (composite={alice_mass_composite}, flat={alice_mass_flat})"
    );
    // Counter writes +1 per tick, 3 ticks → alice_mass = 3.0.
    assert_eq!(
        alice_mass_composite, 3.0,
        "counter accumulates 3 ticks of +1 → alice_mass = 3.0"
    );
}

/// A process whose output DEPENDS on its input: emits `+input` each tick.
/// This sharpens the equivalence test — if input wire rewiring is wrong,
/// the flat form's process reads the wrong value and the output diverges.
#[derive(Debug)]
struct Adder;
impl Process for Adder {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("x".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("y".to_string(), Schema::delta())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let x = state.get_field("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        Update::value(Value::tree([("y", Value::float(x))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn make_core_with_adder() -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Adder", |_config| ProcessNode::Process(Box::new(Adder)));
    let handle: Arc<std::sync::OnceLock<Core>> = Arc::new(std::sync::OnceLock::new());
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("from_config"),
            ))
        });
    }
    let registry = Arc::new(registry);
    let core = Core::from(Arc::clone(&registry));
    let _ = handle.set(core.clone());
    core
}

/// Parent with an Adder-driving composite. The composite reads from
/// parent's `alice_x` (constant 5.0) via its input bridge, writes +x
/// each tick to parent's `alice_y` via its output bridge.
fn adder_composite_parent() -> Value {
    let adder_proc = Value::tree([
        ("_type", Value::String("process".into())),
        ("address", Value::String("local:Adder".into())),
        ("config", Value::None),
        ("inputs", Value::tree([("x", wire(&["x"]))])),
        ("outputs", Value::tree([("y", wire(&["y"]))])),
    ]);
    let inner_state = Value::tree([
        ("x", Value::float(0.0)),
        ("y", Value::float(0.0)),
        ("adder_proc", adder_proc),
    ]);
    let bridge = Value::tree([
        ("inputs", Value::tree([("x", wire(&["x"]))])),
        ("outputs", Value::tree([("y", wire(&["y"]))])),
    ]);
    let inner_schema = Schema::Tree {
        branches: IndexMap::from([
            (Key::from("x"), Schema::float()),
            (Key::from("y"), Schema::delta()),
        ]),
    };
    let config = Value::tree([
        ("state", inner_state),
        ("bridge", bridge),
        ("schema", schema_to_value(&inner_schema)),
    ]);
    let composite_spec = Value::tree([
        ("_type", Value::String("composite".into())),
        ("address", Value::String("local:Composite".into())),
        ("config", config),
        ("inputs", Value::tree([("x", wire(&["..", "alice_x"]))])),
        ("outputs", Value::tree([("y", wire(&["..", "alice_y"]))])),
    ]);
    Value::tree([
        ("alice_x", Value::float(5.0)),
        ("alice_y", Value::float(0.0)),
        ("cells", Value::tree([("alice", composite_spec)])),
    ])
}

#[test]
fn behavioral_equivalence_holds_when_inner_process_depends_on_its_input() {
    // The Adder process emits +x each tick — so the END STATE depends
    // on x being correctly piped through. If input wire rewiring is
    // broken in the flat form, Adder reads the wrong value, output
    // diverges. This is the sharp half of S1 part D.
    let composite = adder_composite_parent();
    let path = [Key::from("cells"), Key::from("alice")];

    let mut engine_a =
        Engine::from_state(Schema::Any, composite.clone(), make_core_with_adder())
            .expect("engine A");
    engine_a.discover_all_processes();
    engine_a.run(3.0);
    let alice_y_composite = engine_a
        .state()
        .get_field("alice_y")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN);

    let unfurled = unfurl_into(&composite, &path).expect("unfurl_into");
    let flat = refuse_links(&unfurled.parent, &path, &unfurled.boundary).expect("refuse_links");
    let mut engine_b = Engine::from_state(Schema::Any, flat, make_core_with_adder())
        .expect("engine B");
    engine_b.discover_all_processes();
    engine_b.run(3.0);
    let alice_y_flat = engine_b
        .state()
        .get_field("alice_y")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN);

    assert_eq!(
        alice_y_composite, alice_y_flat,
        "composite + flat agree when output depends on input \
         (composite={alice_y_composite}, flat={alice_y_flat})"
    );
    // Adder emits +5 per tick (since alice_x = 5), 3 ticks → alice_y = 15.
    assert_eq!(
        alice_y_composite, 15.0,
        "adder accumulates 3 ticks of +5 → alice_y = 15"
    );
}
