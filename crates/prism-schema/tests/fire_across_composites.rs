//! Tests for `fire_across_composites` — the BRS-by-unfurl orchestrator
//! (S2 / BATWD §V / #43).
//!
//! A reaction whose redex spans multiple composites is fired by
//! unfurling them into the parent, matching against the now-flat
//! union, firing the rule, and folding each composite back. The first
//! user-visible payoff of S1's `unfurl_into` / `fold_at`.

use std::sync::Arc;

use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};
use prism_schema::{
    algebra, cross_fire_delta, fire_across_composites, Key, Schema, StateMap, Value, COMPOSITE_TYPE,
};

fn val_str(s: &str) -> Value {
    Value::String(s.to_string())
}

fn wire(seg: &str) -> Value {
    Value::List(vec![Value::String(seg.to_string())])
}

/// Build a Slot composite at `path` with the given initial inner value.
/// Inner state: `{_type: "Slot", value: <v>}` (the sort tag lets the
/// matcher recognise it). The composite's bridge surfaces the inner
/// value as the outer `value` face on parent's `<path>_value` slot.
fn slot_composite(v: f64) -> Value {
    let inner_state = Value::tree([
        ("_type", val_str("Slot")),
        ("value", Value::float(v)),
    ]);
    let bridge = Value::tree([
        ("inputs", Value::map()),
        ("outputs", Value::tree([("value", wire("value"))])),
    ]);
    let config = Value::tree([
        ("state", inner_state),
        ("bridge", bridge),
        ("schema", val_str("encoded-schema")),
    ]);
    Value::tree([
        ("_type", val_str(COMPOSITE_TYPE)),
        ("address", val_str("local:Composite")),
        ("config", config),
        ("inputs", Value::map()),
        ("outputs", Value::tree([("value", wire("dest"))])),
    ])
}

/// Parent with two slot composites side-by-side.
fn parent_with_two_slots(alice_value: f64, bob_value: f64) -> Value {
    Value::tree([(
        "cells",
        Value::tree([
            ("alice", slot_composite(alice_value)),
            ("bob", slot_composite(bob_value)),
        ]),
    )])
}

fn inner_value_at(parent: &Value, cell: &str) -> Option<f64> {
    parent
        .get_path(&[
            Key::from("cells"),
            Key::from(cell),
            Key::from("config"),
            Key::from("state"),
            Key::from("value"),
        ])
        .and_then(|v| v.as_f64())
}

#[test]
fn fire_across_composites_with_no_match_is_a_round_trip_identity() {
    // A redex whose shape doesn't match anything in the parent.
    // The orchestrator unfurls, finds nothing, re-folds.
    // S1's identity `fold(unfurl(x)) ≡ x` plus zero edits = unchanged.
    let parent = parent_with_two_slots(1.0, 2.0);
    let alice = [Key::from("cells"), Key::from("alice")];
    let bob = [Key::from("cells"), Key::from("bob")];

    let rule = ReactionRule::new(
        // A redex that matches `_type:"Nonexistent"`.
        Pattern::map([(
            "nonsense",
            Pattern::sort("Nonexistent", Vec::<(&str, Pattern)>::new()),
        )]),
        Pattern::map([("nonsense", Pattern::Atom(Value::None))]),
    );

    let result =
        fire_across_composites(&parent, &rule, &[&alice, &bob]).expect("no-match round trip");
    assert!(!result.fired, "no match — fired flag is false");
    assert_eq!(result.parent, parent, "unfurl + fold without firing is the identity");
}

#[test]
fn fire_across_composites_modifies_state_inside_both_composites() {
    // A redex that NAMES both cells (matches at the `cells` container
    // map), captures alice's value, and rewrites both alice and bob.
    // Without the unfurl maneuver this redex CANNOT match — alice and
    // bob's outer composite specs hide their inner `value` slot. After
    // unfurl_into both, `cells.alice` and `cells.bob` ARE the inner
    // state shape (`{_type:"Slot", value:N}`), so the matcher sees
    // them and the rule fires.
    let parent = parent_with_two_slots(7.0, 13.0);
    let alice = [Key::from("cells"), Key::from("alice")];
    let bob = [Key::from("cells"), Key::from("bob")];

    let redex = Pattern::map([
        (
            "alice",
            Pattern::sort(
                "Slot",
                [(
                    "value",
                    Pattern::Bind {
                        name: Key::from("va"),
                        inner: Box::new(Pattern::Site),
                    },
                )],
            ),
        ),
        (
            "bob",
            Pattern::sort(
                "Slot",
                [(
                    "value",
                    Pattern::Bind {
                        name: Key::from("vb"),
                        inner: Box::new(Pattern::Site),
                    },
                )],
            ),
        ),
    ]);

    // The reactum: alice becomes {value: 99.0}, bob becomes
    // {value: 100.0}. (We're proving the orchestrator runs the
    // maneuver — not testing the binding/swap subtlety of the
    // matcher itself, which is exercised elsewhere.)
    let reactum = Pattern::map([
        (
            "alice",
            Pattern::sort(
                "Slot",
                [("value", Pattern::Atom(Value::float(99.0)))],
            ),
        ),
        (
            "bob",
            Pattern::sort(
                "Slot",
                [("value", Pattern::Atom(Value::float(100.0)))],
            ),
        ),
    ]);

    let rule = ReactionRule::new(redex, reactum).with_label("rewrite-both");

    let result = fire_across_composites(&parent, &rule, &[&alice, &bob])
        .expect("orchestrator returns Some");
    assert!(result.fired, "redex matched both unfurled cells");

    // The composites are re-sealed with the new inner state.
    assert_eq!(
        inner_value_at(&result.parent, "alice"),
        Some(99.0),
        "alice's inner value updated via the reactum"
    );
    assert_eq!(
        inner_value_at(&result.parent, "bob"),
        Some(100.0),
        "bob's inner value updated via the reactum"
    );

    // The cells slots are once again composite specs (folded back).
    let alice_spec = result.parent.get_path(&alice).expect("alice present");
    assert_eq!(
        alice_spec.get_field("_type").and_then(|v| v.as_str()),
        Some(COMPOSITE_TYPE),
        "alice is back to a composite spec"
    );
    let bob_spec = result.parent.get_path(&bob).expect("bob present");
    assert_eq!(
        bob_spec.get_field("_type").and_then(|v| v.as_str()),
        Some(COMPOSITE_TYPE),
        "bob is back to a composite spec"
    );
}

#[test]
fn fire_across_composites_rejects_invalid_composite_paths() {
    // A composite_path that doesn't point to a composite spec returns
    // None — the caller passed bogus paths.
    let parent = parent_with_two_slots(1.0, 2.0);
    let missing = [Key::from("does_not_exist")];

    let rule = ReactionRule::new(
        Pattern::map([("x", Pattern::Site)]),
        Pattern::map([("x", Pattern::Site)]),
    );

    let result = fire_across_composites(&parent, &rule, &[&missing]);
    assert!(result.is_none(), "bogus composite_path is rejected");
}

#[test]
fn fire_across_composites_with_single_composite_still_works() {
    // The degenerate case: only one composite_path. Demonstrates the
    // orchestrator handles n=1 (a non-cross-composite reaction routed
    // through the same maneuver).
    let parent = Value::tree([(
        "cells",
        Value::tree([("solo", slot_composite(42.0))]),
    )]);
    let solo = [Key::from("cells"), Key::from("solo")];

    let rule = ReactionRule::new(
        Pattern::map([(
            "solo",
            Pattern::sort(
                "Slot",
                [(
                    "value",
                    Pattern::Bind {
                        name: Key::from("v"),
                        inner: Box::new(Pattern::Site),
                    },
                )],
            ),
        )]),
        Pattern::map([(
            "solo",
            Pattern::sort("Slot", [("value", Pattern::Atom(Value::float(0.0)))]),
        )]),
    );

    let result = fire_across_composites(&parent, &rule, &[&solo]).expect("solo case");
    assert!(result.fired);
    assert_eq!(inner_value_at(&result.parent, "solo"), Some(0.0));
}

// ════════════════════════════════════════════════════════════════════
// cross_fire_delta — the DELTA form (the engine-facing reactor payload)
// ════════════════════════════════════════════════════════════════════
//
// Where `fire_across_composites` returns the folded whole PARENT,
// `cross_fire_delta` returns a re-folded structural fire-DELTA whose paths
// land at each composite's `config.state` interior. This is what an engine
// per-tick reactor emits so the cross-composite fire COMPOSES with the
// composites' own dynamics (a cell grows the same tick) — the CRUX.

/// A genuinely cross-composite reaction: bind TWO `Slot` composites (matched in
/// the unfurled frame as bare inner state) and ADD a shared `bond` edge field to
/// each interior. A computed reactum emitting per-composite-key LOCALIZED `_add`s
/// — the shape #40's link-graph redex (`?w ~{edge:~e} | ?e ~{edge:~e}`) compiles
/// to. NOT a container-level whole-replace (which would clobber concurrent grow).
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
        // For each matched composite key, a LOCALIZED interior delta: add a
        // `bond` field. Touches only `bond` — leaves `value` (and any concurrent
        // grow on it) alone.
        let mut out = StateMap::new();
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

fn inner_field(parent: &Value, cell: &str, field: &str) -> Option<Value> {
    parent
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
fn cross_fire_delta_is_equivalent_to_the_whole_parent_fold() {
    // The defining equivalence: applying the re-folded DELTA to the composite-
    // form parent yields exactly what `fire_across_composites` produces by
    // applying+folding the whole flat parent. Same fire, two shapes — the delta
    // form must agree with the whole-state form.
    let parent = parent_with_two_slots(7.0, 13.0);
    let alice = [Key::from("cells"), Key::from("alice")];
    let bob = [Key::from("cells"), Key::from("bob")];
    let rule = bond_rule("e1");

    let whole = fire_across_composites(&parent, &rule, &[&alice, &bob])
        .expect("whole-state fire");
    assert!(whole.fired);

    let cfd = cross_fire_delta(&parent, &rule, &[&alice, &bob]).expect("delta fire");
    assert!(cfd.fired);
    assert_eq!(cfd.label, "bond");

    // Apply the re-folded delta to the ORIGINAL composite-form parent.
    let applied = algebra::apply_with(None, &Schema::Any, &parent, &cfd.delta);

    assert_eq!(
        applied, whole.parent,
        "the re-folded delta reproduces the whole-parent fold exactly"
    );
    // And concretely: both composites carry the bond in their interior, and
    // their original `value` is untouched.
    assert_eq!(inner_field(&applied, "alice", "bond"), Some(val_str("e1")));
    assert_eq!(inner_field(&applied, "bob", "bond"), Some(val_str("e1")));
    assert_eq!(inner_value_at(&applied, "alice"), Some(7.0));
    assert_eq!(inner_value_at(&applied, "bob"), Some(13.0));
    // The composites are still composite specs (the delta didn't dissolve them).
    assert_eq!(
        applied
            .get_path(&alice)
            .and_then(|v| v.get_field("_type"))
            .and_then(|v| v.as_str()),
        Some(COMPOSITE_TYPE),
    );
}

#[test]
fn cross_fire_delta_composes_with_a_concurrent_grow() {
    // THE CRUX property. The reactor's delta must reconcile field-by-field with
    // a same-tick "grow" on a DIFFERENT interior field — not clobber it. We
    // reconcile the cross-fire delta with a grow delta (additive on `value`) and
    // apply: both the bond (reaction) AND the grown value (grow) must survive.
    let parent = parent_with_two_slots(7.0, 13.0);
    let alice = [Key::from("cells"), Key::from("alice")];
    let bob = [Key::from("cells"), Key::from("bob")];

    let cfd = cross_fire_delta(&parent, &bond_rule("e1"), &[&alice, &bob]).expect("delta");

    // A concurrent grow: +1.0 to each cell's interior `value` (a sibling field).
    let grow = Value::tree([(
        "cells",
        Value::tree([
            (
                "alice",
                Value::tree([("config", Value::tree([("state", Value::tree([("value", Value::float(1.0))]))]))]),
            ),
            (
                "bob",
                Value::tree([("config", Value::tree([("state", Value::tree([("value", Value::float(1.0))]))]))]),
            ),
        ]),
    )]);

    // Reconcile the two deltas (the engine's per-tick combine) then apply, using
    // a recursing schema — the engine reconciles against the real STRUCTURED
    // state schema (a Tree that descends per-branch), never opaque `Any`
    // (which is last-wins and would drop a whole writer). `RecursiveTree` is the
    // schema-faithful stand-in: it collates `_add`/`_remove` + per-key deltas at
    // every map level, exactly as the engine's per-branch reconcile does.
    let nested = Schema::RecursiveTree {
        leaf: Box::new(Schema::Any),
    };
    let combined =
        prism_schema::reconcile::reconcile_with(None, &nested, &[cfd.delta.clone(), grow])
            .expect("reconcile");
    let applied = algebra::apply_with(None, &nested, &parent, &combined);

    // Grow survived (7 + 1) AND the reaction's bond landed — they COMPOSED.
    assert_eq!(
        inner_value_at(&applied, "alice"),
        Some(8.0),
        "grow on `value` is preserved (not clobbered by the reaction)"
    );
    assert_eq!(inner_value_at(&applied, "bob"), Some(14.0));
    assert_eq!(inner_field(&applied, "alice", "bond"), Some(val_str("e1")));
    assert_eq!(inner_field(&applied, "bob", "bond"), Some(val_str("e1")));
}

#[test]
fn cross_fire_delta_preserves_raw_directives_at_the_interior() {
    // The re-fold maps PATHS, never diffs/overwrites — so the raw fire directives
    // (`_add`/`_remove`/`_divide`) survive verbatim under `config.state`, where
    // the engine enacts them schema-aware (a `_divide` splits the LIVE node).
    let parent = parent_with_two_slots(7.0, 13.0);
    let alice = [Key::from("cells"), Key::from("alice")];
    let bob = [Key::from("cells"), Key::from("bob")];

    let cfd = cross_fire_delta(&parent, &bond_rule("e1"), &[&alice, &bob]).expect("delta");

    // The delta nests to cells.alice.config.state and keeps `_add` intact.
    let interior = cfd
        .delta
        .get_path(&[
            Key::from("cells"),
            Key::from("alice"),
            Key::from("config"),
            Key::from("state"),
        ])
        .expect("delta lands at alice.config.state");
    assert!(
        interior.get_field("_add").is_some(),
        "the raw `_add` directive is preserved at the interior, not diffed away: {interior:?}"
    );
    // It did NOT replace the whole composite (no top-level _remove/_add of the
    // composite key at the cells container).
    let cells_delta = cfd
        .delta
        .get_path(&[Key::from("cells")])
        .expect("cells delta");
    assert!(
        cells_delta.get_field("_remove").is_none() && cells_delta.get_field("_add").is_none(),
        "no container-level whole-composite replace: {cells_delta:?}"
    );
}

#[test]
fn cross_fire_delta_with_no_match_is_a_noop() {
    // No match → fired=false, delta=None (a no-op the engine drops).
    let parent = parent_with_two_slots(1.0, 2.0);
    let alice = [Key::from("cells"), Key::from("alice")];
    let bob = [Key::from("cells"), Key::from("bob")];

    let rule = ReactionRule::new(
        Pattern::map([(
            "nope",
            Pattern::sort("Nonexistent", Vec::<(&str, Pattern)>::new()),
        )]),
        Pattern::map([("nope", Pattern::Atom(Value::None))]),
    );

    let cfd = cross_fire_delta(&parent, &rule, &[&alice, &bob]).expect("no-match returns Some");
    assert!(!cfd.fired);
    assert_eq!(cfd.delta, Value::None);
}
