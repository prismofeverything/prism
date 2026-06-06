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
use prism_schema::{algebra, fold, unfurl, Key, Value, COMPOSITE_TYPE, UNFURLED_TYPE};

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
