//! Tests for `fold` / `unfurl` — the BATWD §IV maneuver (S1).
//!
//! The defining law: `fold ∘ unfurl ≡ id` (round-trip on a composite
//! spec). Plus: `unfurl` extracts the inner state, bridge, schema,
//! interface, and the self-exported face into siblings of one envelope;
//! `fold` reseals them under `config`.
//!
//! These are the composite-level analogue of `divide ↔ tensor` at the
//! value level. Same algebraic shape, one rung up the bigraph ladder
//! (`docs/bigraphs-all-the-way-down.md` §IV).

use indexmap::IndexMap;
use prism_schema::{
    algebra, fold, fold_at, unfurl, unfurl_into, Key, Value, COMPOSITE_TYPE, UNFURLED_TYPE,
};

fn val_str(s: &str) -> Value {
    Value::String(s.to_string())
}

fn wire_to(seg: &str) -> Value {
    Value::List(vec![Value::String(seg.to_string())])
}

/// A minimal but realistic composite spec — the shape chrysalis's
/// `build_composite_outer` emits after #47 (flat envelope + face slots).
fn cell_composite_spec() -> Value {
    let inner_state = Value::tree([
        ("mass", Value::float(5.0)),
        (
            "grow",
            Value::tree([
                ("_type", val_str("process")),
                ("address", val_str("local:Grow")),
                ("config", Value::tree([("rate", Value::float(0.5))])),
                ("inputs", Value::tree([("mass", wire_to("mass"))])),
                ("outputs", Value::tree([("mass", wire_to("mass"))])),
            ]),
        ),
    ]);
    let bridge = Value::tree([
        ("inputs", Value::tree([("glucose", wire_to("mass"))])),
        ("outputs", Value::tree([("mass", wire_to("mass"))])),
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
        ("inputs", Value::tree([("glucose", wire_to("glucose_pool"))])),
        ("outputs", Value::tree([("mass", wire_to("alice_mass"))])),
        // Self-face slots (the seed_self_face contract).
        ("mass", Value::float(5.0)),
    ])
}

#[test]
fn unfurl_extracts_inner_state_and_bridge() {
    let spec = cell_composite_spec();
    let unfurled = unfurl(&spec).expect("unfurl produces an envelope");
    let m = unfurled.as_map().expect("map");

    assert_eq!(m.get("_type").and_then(|v| v.as_str()), Some(UNFURLED_TYPE));
    assert_eq!(
        m.get("address").and_then(|v| v.as_str()),
        Some("local:Composite")
    );
    // Inner state is now a sibling, not buried under `config`.
    let state = m.get("state").expect("state sibling");
    let mass = state.get_field("mass").and_then(|v| v.as_f64());
    assert_eq!(mass, Some(5.0), "inner state's mass is at top of envelope");
    // Bridge is a sibling.
    assert!(m.contains_key("bridge"));
    // The face is exposed under its own key.
    let face = m.get("face").and_then(|v| v.as_map()).expect("face map");
    assert_eq!(face.get("mass").and_then(|v| v.as_f64()), Some(5.0));
}

#[test]
fn fold_inverts_unfurl_round_trip_identity() {
    // The defining law of the BATWD §IV pair.
    let spec = cell_composite_spec();
    let unfurled = unfurl(&spec).expect("unfurl");
    let resealed = fold(&unfurled).expect("fold");
    assert_eq!(resealed, spec, "fold(unfurl(spec)) ≡ spec");
}

#[test]
fn unfurl_rejects_non_composite_values() {
    // `unfurl` is type-aware: it only fires on `_type: "composite"`.
    let not_a_composite = Value::tree([
        ("_type", val_str("process")),
        ("address", val_str("local:Foo")),
    ]);
    assert!(unfurl(&not_a_composite).is_none());
    // A bare value isn't a composite either.
    assert!(unfurl(&Value::float(3.14)).is_none());
}

#[test]
fn fold_rejects_non_unfurled_values() {
    // `fold` is type-aware: only fires on `_type: "unfurled"`.
    let spec = cell_composite_spec();
    assert!(fold(&spec).is_none(), "fold should refuse a raw composite");
    assert!(fold(&Value::None).is_none());
}

#[test]
fn unfurl_preserves_the_face_for_round_trip() {
    // The self-exported face slots (seed_self_face) need to round-trip.
    // If we lost them, fold couldn't reconstruct the spec faithfully.
    let mut spec_map: IndexMap<Key, Value> = IndexMap::new();
    spec_map.insert(Key::from("_type"), val_str(COMPOSITE_TYPE));
    spec_map.insert(Key::from("address"), val_str("local:Composite"));
    spec_map.insert(
        Key::from("config"),
        Value::tree([
            ("state", Value::tree([("internal", Value::float(7.0))])),
            ("bridge", Value::tree([("inputs", Value::map()), ("outputs", Value::map())])),
            ("schema", Value::None),
        ]),
    );
    spec_map.insert(Key::from("inputs"), Value::map());
    spec_map.insert(Key::from("outputs"), Value::map());
    // Multiple face slots.
    spec_map.insert(Key::from("mass"), Value::float(5.0));
    spec_map.insert(Key::from("glucose"), Value::float(10.0));
    spec_map.insert(Key::from("acetate"), Value::float(2.0));
    let spec = Value::Map(spec_map);

    let unfurled = unfurl(&spec).unwrap();
    let resealed = fold(&unfurled).unwrap();
    assert_eq!(resealed, spec, "face with multiple slots round-trips");
}

#[test]
fn fold_and_unfurl_reachable_through_algebra_module() {
    // The closure-of-the-algebra discipline: every schema/state
    // operation goes through `prism_schema::algebra`. fold/unfurl
    // re-export there.
    let spec = cell_composite_spec();
    let unfurled = algebra::unfurl(&spec).unwrap();
    let resealed = algebra::fold(&unfurled).unwrap();
    assert_eq!(resealed, spec);
}

// ════════════════════════════════════════════════════════════════════
// Parent-context fold/unfurl (S1 part B)
// ════════════════════════════════════════════════════════════════════

/// A parent place graph with a composite at `cells.alice` plus
/// surrounding state — the minimal interesting setup.
fn parent_with_alice_composite() -> Value {
    Value::tree([
        ("glucose_pool", Value::float(100.0)),
        ("alice_mass_slot", Value::float(0.0)),
        (
            "cells",
            Value::tree([("alice", cell_composite_spec())]),
        ),
    ])
}

#[test]
fn unfurl_into_inlines_inner_state_at_composite_path() {
    // After unfurl_into, the composite at cells.alice is replaced by
    // its inner state — the spec wrapper is gone; the body shows
    // through directly. Sibling state (glucose_pool / alice_mass_slot)
    // is preserved.
    let parent = parent_with_alice_composite();
    let path = [Key::from("cells"), Key::from("alice")];
    let result = unfurl_into(&parent, &path).expect("unfurl_into");

    let alice_now = result
        .parent
        .get_path(&path)
        .expect("alice still at cells.alice");
    // Inner state's `mass` is at the top of cells.alice now — not
    // buried under config.state.
    let mass = alice_now.get_field("mass").and_then(|v| v.as_f64());
    assert_eq!(mass, Some(5.0), "inner state's mass is hoisted to cells.alice");
    // The composite spec sentinel is gone.
    assert!(
        alice_now.get_field("_type").and_then(|v| v.as_str()) != Some(COMPOSITE_TYPE),
        "no _type:composite at cells.alice after inline"
    );
    // The inner process `grow` is still present (and its wires are
    // untouched — they were already relative to its slot).
    let grow = alice_now.get_field("grow").expect("grow process present");
    assert_eq!(
        grow.get_field("_type").and_then(|v| v.as_str()),
        Some("process"),
        "inner process structure preserved"
    );
    // Sibling state untouched.
    assert_eq!(
        result.parent.get_field("glucose_pool").and_then(|v| v.as_f64()),
        Some(100.0),
        "sibling state preserved"
    );
}

#[test]
fn unfurl_into_then_fold_at_is_identity() {
    // The defining law of the parent-context pair.
    let parent = parent_with_alice_composite();
    let path = [Key::from("cells"), Key::from("alice")];
    let result = unfurl_into(&parent, &path).expect("unfurl_into");
    let resealed = fold_at(&result.parent, &path, &result.boundary).expect("fold_at");
    assert_eq!(
        resealed, parent,
        "fold_at(unfurl_into(parent, p).parent, p, .boundary) ≡ parent"
    );
}

#[test]
fn unfurl_into_rejects_paths_that_are_not_composites() {
    let parent = parent_with_alice_composite();
    // Sibling state is a scalar — not a composite spec.
    let pool_path = [Key::from("glucose_pool")];
    assert!(unfurl_into(&parent, &pool_path).is_none());
    // Missing path — None.
    let missing = [Key::from("does_not_exist")];
    assert!(unfurl_into(&parent, &missing).is_none());
}

#[test]
fn fold_at_rejects_non_boundary_metadata() {
    let parent = parent_with_alice_composite();
    let path = [Key::from("cells"), Key::from("alice")];
    let result = unfurl_into(&parent, &path).unwrap();
    // The composite-spec form is NOT a valid boundary (it's _type:composite,
    // not _type:unfurled).
    let bogus_boundary = parent.get_path(&path).cloned().unwrap();
    assert!(fold_at(&result.parent, &path, &bogus_boundary).is_none());
    // A bare scalar — None.
    assert!(fold_at(&result.parent, &path, &Value::None).is_none());
}

#[test]
fn unfurl_into_root_level_composite() {
    // The composite IS the parent (empty path). Edge case — the whole
    // root value gets replaced by its inner state, and fold_at reseals
    // back to the original spec.
    let spec = cell_composite_spec();
    let empty_path: [Key; 0] = [];
    let result = unfurl_into(&spec, &empty_path).expect("unfurl at root");
    // The parent's root IS now the inner state — `mass` and `grow` at
    // the top.
    assert_eq!(
        result.parent.get_field("mass").and_then(|v| v.as_f64()),
        Some(5.0),
        "root-level unfurl exposes inner state at root"
    );
    let resealed = fold_at(&result.parent, &empty_path, &result.boundary).expect("fold_at");
    assert_eq!(resealed, spec, "root-level round-trip");
}

#[test]
fn unfurl_into_one_of_multiple_sibling_composites() {
    // Two composites at sibling paths. Unfurl one; the other stays a
    // composite. Round-trip restores both.
    let parent = Value::tree([
        (
            "cells",
            Value::tree([
                ("alice", cell_composite_spec()),
                ("bob", cell_composite_spec()),
            ]),
        ),
    ]);
    let alice_path = [Key::from("cells"), Key::from("alice")];
    let result = unfurl_into(&parent, &alice_path).expect("unfurl alice");

    // Bob untouched — still a composite spec.
    let bob = result
        .parent
        .get_path(&[Key::from("cells"), Key::from("bob")])
        .expect("bob present");
    assert_eq!(
        bob.get_field("_type").and_then(|v| v.as_str()),
        Some(COMPOSITE_TYPE),
        "sibling composite is untouched"
    );
    // Alice is inlined.
    let alice = result.parent.get_path(&alice_path).expect("alice present");
    assert!(alice.get_field("_type").and_then(|v| v.as_str()) != Some(COMPOSITE_TYPE));

    // Round-trip restores Alice fully.
    let resealed = fold_at(&result.parent, &alice_path, &result.boundary).unwrap();
    assert_eq!(resealed, parent, "alice round-trips while bob is preserved");
}

#[test]
fn parent_context_round_trip_via_algebra_module() {
    // The closure-of-the-algebra: unfurl_into / fold_at are reachable
    // through `prism_schema::algebra`.
    let parent = parent_with_alice_composite();
    let path = [Key::from("cells"), Key::from("alice")];
    let result = algebra::unfurl_into(&parent, &path).unwrap();
    let resealed = algebra::fold_at(&result.parent, &path, &result.boundary).unwrap();
    assert_eq!(resealed, parent);
}

#[test]
fn unfurl_handles_missing_config_gracefully() {
    // A minimal composite with no `config` — the unfurled state /
    // bridge / schema default to empty / None. (No panic.)
    let spec = Value::tree([
        ("_type", val_str(COMPOSITE_TYPE)),
        ("address", val_str("local:Empty")),
        ("inputs", Value::map()),
        ("outputs", Value::map()),
    ]);
    let unfurled = unfurl(&spec).expect("still produces an envelope");
    let m = unfurled.as_map().unwrap();
    assert_eq!(m.get("state").cloned().unwrap_or(Value::None), Value::None);
    // Note: round-trip THROUGH a missing-config spec normalizes —
    // `fold` always emits the `config` key (with state/bridge/schema
    // inside), so the resealed value differs from the original. This
    // is expected: the law `fold ∘ unfurl ≡ id` holds for
    // WELL-FORMED composite specs (those with a `config`); the
    // graceful path is for tooling that walks weird inputs without
    // panicking.
}
