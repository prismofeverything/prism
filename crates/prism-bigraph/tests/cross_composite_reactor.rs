//! The engine-level per-tick cross-composite reactor (#43, the BATWD §V
//! keystone — engine half).
//!
//! `prism-schema/tests/fire_across_composites.rs` proves the DELTA mechanism
//! (`cross_fire_delta`: unfurl → match flat → fire → re-fold to `config.state`,
//! composes with a concurrent grow, `_divide`/`_add` survive). This test proves
//! it END-TO-END through the Engine: a [`CrossCompositeReactor`] discovered as a
//! process node fires each tick, AUTO-DETECTS the composites it spans (no
//! caller-supplied paths), and emits a re-folded delta the engine applies —
//! reaching INSIDE composites the plain `BigraphicalReactiveSystem` only sees by
//! their face.
//!
//! Two cells `alice`/`bob` live as composite specs in a `cells` map. The reactor
//! holds a cross-composite bond reaction — `?w::Slot | ?e::Slot` => add a shared
//! `bond` edge to each interior (the place-graph stand-in for #40's link-graph
//! `?w ~{edge:~e} | ?e ~{edge:~e}`). After a tick, BOTH composites carry the
//! bond in their `config.state`, and (second test) a concurrent grow on a
//! sibling interior field survives — they COMPOSE through the engine's reconcile.
//!
//! Scope: the composites are INERT specs (no `address` → discovery leaves them
//! as data), isolating the reactor mechanism from the orthogonal "make
//! `config.state` the live source for a sub-engine composite" sync (a documented
//! follow-on). The link-graph surface matcher and the distributed form are the
//! next #43 slices.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;

use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::{Core, CrossCompositeReactor, Engine, Key, Schema, Update, Value};
use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};

fn val_str(s: &str) -> Value {
    Value::String(s.to_string())
}

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String((*s).into())).collect())
}

/// An INERT composite spec (no `address`, so engine discovery leaves it as data).
/// `config.state` is a `Slot` the cross-composite redex matches once unfurled.
fn inert_slot_composite(value: f64) -> Value {
    Value::tree([
        ("_type", val_str("composite")),
        (
            "config",
            Value::tree([
                ("state", Value::tree([("_type", val_str("Slot")), ("value", Value::float(value))])),
                ("bridge", Value::tree([("inputs", Value::map()), ("outputs", Value::map())])),
                ("schema", Value::None),
            ]),
        ),
        ("inputs", Value::map()),
        ("outputs", Value::map()),
    ])
}

/// A cross-composite bond reaction: bind TWO `Slot` composites (matched in the
/// unfurled frame) and ADD a shared `bond` edge to each interior. A computed
/// reactum emitting per-composite-key LOCALIZED `_add`s — the shape #40's
/// link-graph redex compiles to; composes with concurrent interior dynamics.
fn bond_rule(edge: &str) -> ReactionRule {
    let redex = Pattern::list([
        Pattern::Bind {
            name: Key::from("?w"),
            inner: Box::new(Pattern::sort("Slot", Vec::<(&str, Pattern)>::new())),
        },
        Pattern::Bind {
            name: Key::from("?e"),
            inner: Box::new(Pattern::sort("Slot", Vec::<(&str, Pattern)>::new())),
        },
    ]);
    let edge = edge.to_string();
    let reactum_fn: ReactumFn = Arc::new(move |b: &Bindings| {
        let mut out = prism_schema::StateMap::new();
        for key in b.key_map.values() {
            out.insert(
                key.clone(),
                Value::tree([("_add", Value::tree([("bond", val_str(&edge))]))]),
            );
        }
        Value::Map(out)
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("bond")
        .with_reactum_fn(reactum_fn)
}

/// The reactor process spec, wired `~{state: cells} ->{state: cells}` (absolute
/// `cells` from root).
fn reactor_spec() -> Value {
    Value::tree([
        ("_type", val_str("process")),
        ("address", val_str("local:CrossCompositeReactor")),
        ("config", Value::None),
        ("inputs", Value::tree([("state", wire(&["cells"]))])),
        ("outputs", Value::tree([("state", wire(&["cells"]))])),
    ])
}

fn make_core(rules: Vec<ReactionRule>, extra: impl Fn(&mut ProcessRegistry)) -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("CrossCompositeReactor", move |_config| {
        ProcessNode::Process(Box::new(CrossCompositeReactor::new(rules.clone())))
    });
    extra(&mut registry);
    Core::from(Arc::new(registry))
}

/// A FULLY-DECLARED state schema — no branch left to `schema_at_path`'s
/// `Any` fallback (a Tree is a closed record; a real compiled topology declares
/// every slot). `cells` is an OPEN `Map{RecursiveTree}` because the bond
/// reaction ADDS an interior `bond` field that the place-graph stand-in doesn't
/// pre-declare (the real `?w ~{edge:~e}` form declares it as a port); each
/// process slot is a `ProcessLink` so the reactor/grow are discovered
/// SCHEMA-FIRST, not via the `_type`-hint fallback.
fn engine_schema(process_slots: &[&str]) -> Schema {
    let mut branches = IndexMap::from([(
        Key::from("cells"),
        Schema::Map {
            value: Box::new(Schema::RecursiveTree { leaf: Box::new(Schema::Any) }),
        },
    )]);
    for name in process_slots {
        branches.insert(
            Key::from(*name),
            Schema::ProcessLink {
                inputs: IndexMap::from([(Key::from("state"), Schema::Any)]),
                outputs: IndexMap::from([(Key::from("state"), Schema::Any)]),
                interval: 1.0,
            },
        );
    }
    Schema::Tree { branches }
}

fn cell_interior(state: &Value, cell: &str, field: &str) -> Option<Value> {
    state
        .get_path(&[
            Key::from("cells"),
            Key::from(cell),
            Key::from("config"),
            Key::from("state"),
            Key::from(field),
        ])
        .cloned()
}

#[test]
fn reactor_fires_cross_composite_reaction_through_the_engine() {
    let state = Value::tree([
        (
            "cells",
            Value::tree([
                ("alice", inert_slot_composite(7.0)),
                ("bob", inert_slot_composite(13.0)),
            ]),
        ),
        ("reactor", reactor_spec()),
    ]);
    let core = make_core(vec![bond_rule("e1")], |_| {});

    let mut engine = Engine::from_state(engine_schema(&["reactor"]), state, core).expect("engine");
    engine.discover_all_processes();
    // The reactor node was discovered (auto), the inert composites were not.
    assert!(
        engine.node_names().iter().any(|n| *n == "reactor"),
        "reactor discovered as a process node; nodes={:?}",
        engine.node_names()
    );
    engine.run(1.0);

    // The cross-composite fire reached INSIDE both composites: each interior now
    // carries the shared bond — through one re-folded delta the engine applied.
    let s = engine.state();
    assert_eq!(cell_interior(s, "alice", "bond"), Some(val_str("e1")), "alice bonded");
    assert_eq!(cell_interior(s, "bob", "bond"), Some(val_str("e1")), "bob bonded");
    // The interior `value` (unmatched field) is untouched — the reactor emitted a
    // field-localized delta, not an interior overwrite.
    assert_eq!(cell_interior(s, "alice", "value"), Some(Value::float(7.0)));
    assert_eq!(cell_interior(s, "bob", "value"), Some(Value::float(13.0)));
    // Both are still composite specs (the boundary survived).
    assert_eq!(
        s.get_path(&[Key::from("cells"), Key::from("alice")])
            .and_then(|v| v.get_field("_type"))
            .and_then(|v| v.as_str()),
        Some("composite"),
    );
}

#[test]
fn reactor_is_a_noop_without_composites_or_matches() {
    // No composites in the wired subtree → the reactor does nothing (face-level
    // reactions are the plain BRS's job).
    let state = Value::tree([
        ("cells", Value::tree([("x", Value::float(1.0)), ("y", Value::float(2.0))])),
        ("reactor", reactor_spec()),
    ]);
    let core = make_core(vec![bond_rule("e1")], |_| {});
    let mut engine =
        Engine::from_state(engine_schema(&["reactor"]), state.clone(), core).expect("engine");
    engine.discover_all_processes();
    engine.run(2.0);
    // cells unchanged (no `bond`, no spurious structure).
    assert_eq!(
        engine.state().get_field("cells"),
        state.get_field("cells"),
        "no composites → reactor is a no-op"
    );
}

/// A parent-level grow Process: each tick, add +1.0 to every cell's interior
/// `value`. Stands in for "a cell grows the same tick the reactor fires" — its
/// delta must COMPOSE with the reactor's bond delta (different interior field).
#[derive(Debug)]
struct InteriorGrow;
impl Process for InteriorGrow {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("state".to_string(), Schema::Any)])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("state".to_string(), Schema::Any)])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, state: &Value, _interval: f64) -> Update {
        let Some(cells) = state.get_field("state").and_then(|v| v.as_map()) else {
            return Update::Noop;
        };
        let mut out = prism_schema::StateMap::new();
        for (key, _) in cells.iter().filter(|(k, _)| !k.starts_with('_')) {
            out.insert(
                key.clone(),
                Value::tree([("config", Value::tree([("state", Value::tree([("value", Value::float(1.0))]))]))]),
            );
        }
        Update::value(Value::tree([("state", Value::Map(out))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn reactor_delta_composes_with_a_concurrent_grow_through_the_engine() {
    // THE CRUX, end-to-end: the reactor's cross-composite bond AND a concurrent
    // grow on a sibling interior field BOTH land in one tick — reconciled
    // field-by-field by the engine, not clobbering each other.
    let state = Value::tree([
        (
            "cells",
            Value::tree([
                ("alice", inert_slot_composite(7.0)),
                ("bob", inert_slot_composite(13.0)),
            ]),
        ),
        ("reactor", reactor_spec()),
        (
            "grow",
            Value::tree([
                ("_type", val_str("process")),
                ("address", val_str("local:InteriorGrow")),
                ("config", Value::None),
                ("inputs", Value::tree([("state", wire(&["cells"]))])),
                ("outputs", Value::tree([("state", wire(&["cells"]))])),
            ]),
        ),
    ]);
    let core = make_core(vec![bond_rule("e1")], |reg| {
        reg.register("InteriorGrow", |_config| ProcessNode::Process(Box::new(InteriorGrow)));
    });

    // A STRUCTURED, FULLY-DECLARED schema (every slot typed — see
    // `engine_schema`) so the engine reconciles the two processes' deltas to
    // `cells` per-cell-recursively. Opaque `Any` at the root would be last-wins,
    // dropping a whole writer; `Map{RecursiveTree}` collates `_add` + per-key
    // deltas at every interior level — exactly the engine's per-branch reconcile.
    let mut engine =
        Engine::from_state(engine_schema(&["reactor", "grow"]), state, core).expect("engine");
    engine.discover_all_processes();
    engine.run(1.0);

    let s = engine.state();
    // Grow survived (7 + 1) AND the reaction's bond landed — composition.
    assert_eq!(
        cell_interior(s, "alice", "value"),
        Some(Value::float(8.0)),
        "grow on `value` composes (not clobbered by the bond reaction)"
    );
    assert_eq!(cell_interior(s, "bob", "value"), Some(Value::float(14.0)));
    assert_eq!(cell_interior(s, "alice", "bond"), Some(val_str("e1")), "alice bonded");
    assert_eq!(cell_interior(s, "bob", "bond"), Some(val_str("e1")), "bob bonded");
}
