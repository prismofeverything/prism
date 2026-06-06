//! Tests for `fire_across_composites` — the BRS-by-unfurl orchestrator
//! (S2 / BATWD §V / #43).
//!
//! A reaction whose redex spans multiple composites is fired by
//! unfurling them into the parent, matching against the now-flat
//! union, firing the rule, and folding each composite back. The first
//! user-visible payoff of S1's `unfurl_into` / `fold_at`.

use prism_schema::reaction::{Pattern, ReactionRule};
use prism_schema::{fire_across_composites, Key, Value, COMPOSITE_TYPE};

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
