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
    algebra, fold, fold_at, refuse_links, unfurl, unfurl_into, Key, Value, COMPOSITE_TYPE,
    UNFURLED_TYPE,
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

// ════════════════════════════════════════════════════════════════════
// Wire re-fusion (S1 part C — BATWD §IV "re-fuse the cut links")
// ════════════════════════════════════════════════════════════════════

/// Build a parent place graph where the inner Grow process's wires
/// match a bridge entry — so wire re-fusion has something to rewrite.
///
/// Composite at cells.alice exposes the `glucose` input port (bridged
/// to internal `mass`) and the `mass` output port (bridged to internal
/// `mass`). Inner Grow reads + writes `[mass]` — both wires will be
/// re-fused after `refuse_links`.
fn parent_with_bridged_alice() -> Value {
    let inner_grow = Value::tree([
        ("_type", val_str("process")),
        ("address", val_str("local:Grow")),
        ("config", Value::tree([("rate", Value::float(0.5))])),
        ("inputs", Value::tree([("mass", wire_to("mass"))])),
        ("outputs", Value::tree([("mass", wire_to("mass"))])),
    ]);
    let inner_state = Value::tree([
        ("mass", Value::float(5.0)),
        ("grow", inner_grow),
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
    let composite_spec = Value::tree([
        ("_type", val_str(COMPOSITE_TYPE)),
        ("address", val_str("local:Composite")),
        ("config", config),
        // The composite's outer wires — these are the destinations that
        // refuse_links rewires inner processes to point to.
        ("inputs", Value::tree([("glucose", wire_to("glucose_pool"))])),
        ("outputs", Value::tree([("mass", wire_to("alice_mass_slot"))])),
    ]);
    Value::tree([
        ("glucose_pool", Value::float(100.0)),
        ("alice_mass_slot", Value::float(0.0)),
        ("cells", Value::tree([("alice", composite_spec)])),
    ])
}

#[test]
fn refuse_links_rewrites_inner_process_wires() {
    // The defining behaviour of #53: after refuse_links, the inner Grow
    // process's wires point DIRECTLY to the composite's former outer
    // endpoints (prefixed by `..` to shift the reference frame).
    let parent = parent_with_bridged_alice();
    let path = [Key::from("cells"), Key::from("alice")];
    let unfurled = unfurl_into(&parent, &path).expect("unfurl_into");
    let refused = refuse_links(&unfurled.parent, &path, &unfurled.boundary)
        .expect("refuse_links");

    // The inner Grow now sits at cells.alice.grow with its wires
    // rewritten.
    let grow = refused
        .get_path(&[Key::from("cells"), Key::from("alice"), Key::from("grow")])
        .expect("grow at cells.alice.grow");

    // Inputs.mass was `[mass]` (matched the bridge's inputs.glucose →
    // internal `[mass]`); composite's outer input wire for glucose was
    // `[glucose_pool]`. After re-fusion: `[..", glucose_pool]`.
    let input_mass = grow
        .get_field("inputs")
        .and_then(|v| v.get_field("mass"))
        .and_then(|v| v.as_list())
        .expect("inputs.mass is a list");
    let input_strs: Vec<&str> = input_mass.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        input_strs,
        vec!["..", "glucose_pool"],
        "inner Grow's mass-input rewritten through the bridge to glucose_pool"
    );

    // Outputs.mass was `[mass]` (matched bridge's outputs.mass →
    // internal `[mass]`); composite's outer output wire for mass was
    // `[alice_mass_slot]`. After re-fusion: `[".. ", alice_mass_slot]`.
    let output_mass = grow
        .get_field("outputs")
        .and_then(|v| v.get_field("mass"))
        .and_then(|v| v.as_list())
        .expect("outputs.mass is a list");
    let output_strs: Vec<&str> = output_mass.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        output_strs,
        vec!["..", "alice_mass_slot"],
        "inner Grow's mass-output rewritten through the bridge to alice_mass_slot"
    );
}

#[test]
fn refuse_links_with_no_bridge_match_leaves_wires_unchanged() {
    // A wire that DOESN'T match any bridge entry stays as-is. Setup: a
    // process reading from `[internal_only]` — a slot the composite
    // doesn't expose via the bridge.
    let inner_proc = Value::tree([
        ("_type", val_str("process")),
        ("address", val_str("local:NoOp")),
        ("inputs", Value::tree([("v", wire_to("internal_only"))])),
        ("outputs", Value::map()),
    ]);
    let inner_state = Value::tree([
        ("internal_only", Value::float(0.0)),
        ("proc", inner_proc),
    ]);
    let composite_spec = Value::tree([
        ("_type", val_str(COMPOSITE_TYPE)),
        ("address", val_str("local:Composite")),
        (
            "config",
            Value::tree([
                ("state", inner_state),
                ("bridge", Value::tree([
                    ("inputs", Value::map()),  // No bridge entries at all
                    ("outputs", Value::map()),
                ])),
                ("schema", Value::None),
            ]),
        ),
        ("inputs", Value::map()),
        ("outputs", Value::map()),
    ]);
    let parent = Value::tree([("alice", composite_spec)]);

    let path = [Key::from("alice")];
    let unfurled = unfurl_into(&parent, &path).unwrap();
    let refused = refuse_links(&unfurled.parent, &path, &unfurled.boundary).unwrap();

    let proc = refused
        .get_path(&[Key::from("alice"), Key::from("proc")])
        .unwrap();
    let v_wire = proc
        .get_field("inputs")
        .and_then(|v| v.get_field("v"))
        .and_then(|v| v.as_list())
        .unwrap();
    let strs: Vec<&str> = v_wire.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        strs,
        vec!["internal_only"],
        "non-bridge wire unchanged after refuse_links"
    );
}

#[test]
fn refuse_links_rejects_root_level_composite() {
    // No `..` to add for a root-level inline — refuse_links returns
    // None (the caller should keep the structural-inline form for
    // root-level cases, where there's no outer to point at).
    let spec = cell_composite_spec();
    let empty_path: [Key; 0] = [];
    let unfurled = unfurl_into(&spec, &empty_path).unwrap();
    let refused = refuse_links(&unfurled.parent, &empty_path, &unfurled.boundary);
    assert!(refused.is_none(), "root-level refuse_links refuses");
}

#[test]
fn refuse_links_walks_into_nested_specs() {
    // A process spec inside another process spec — both should have
    // their wires rewritten. Setup: a parent composite contains a sub-
    // process whose `_type` is "step" with its OWN wires that match
    // the parent composite's bridge.
    let nested_step = Value::tree([
        ("_type", val_str("step")),
        ("address", val_str("local:NestedStep")),
        ("inputs", Value::tree([("x", wire_to("mass"))])),
        ("outputs", Value::map()),
    ]);
    let inner_state = Value::tree([
        ("mass", Value::float(1.0)),
        ("nested", nested_step),
    ]);
    let composite_spec = Value::tree([
        ("_type", val_str(COMPOSITE_TYPE)),
        ("address", val_str("local:Composite")),
        (
            "config",
            Value::tree([
                ("state", inner_state),
                (
                    "bridge",
                    Value::tree([
                        ("inputs", Value::tree([("upstream", wire_to("mass"))])),
                        ("outputs", Value::map()),
                    ]),
                ),
                ("schema", Value::None),
            ]),
        ),
        (
            "inputs",
            Value::tree([("upstream", wire_to("source_pool"))]),
        ),
        ("outputs", Value::map()),
    ]);
    let parent = Value::tree([("alice", composite_spec)]);

    let path = [Key::from("alice")];
    let unfurled = unfurl_into(&parent, &path).unwrap();
    let refused = refuse_links(&unfurled.parent, &path, &unfurled.boundary).unwrap();

    let step = refused
        .get_path(&[Key::from("alice"), Key::from("nested")])
        .unwrap();
    let x_wire = step
        .get_field("inputs")
        .and_then(|v| v.get_field("x"))
        .and_then(|v| v.as_list())
        .unwrap();
    let strs: Vec<&str> = x_wire.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        strs,
        vec!["..", "source_pool"],
        "nested step's wire rewritten through the bridge"
    );
}

#[test]
fn refuse_links_handles_suffix_paths() {
    // A wire `[mass, sub]` where the bridge maps to internal `[mass]`
    // — the `[sub]` suffix should ride along after the outer wire.
    let inner_proc = Value::tree([
        ("_type", val_str("process")),
        ("address", val_str("local:Reader")),
        (
            "inputs",
            Value::tree([(
                "v",
                Value::List(vec![val_str("mass"), val_str("sub")]),
            )]),
        ),
        ("outputs", Value::map()),
    ]);
    let inner_state = Value::tree([
        ("mass", Value::tree([("sub", Value::float(7.0))])),
        ("proc", inner_proc),
    ]);
    let composite_spec = Value::tree([
        ("_type", val_str(COMPOSITE_TYPE)),
        ("address", val_str("local:Composite")),
        (
            "config",
            Value::tree([
                ("state", inner_state),
                (
                    "bridge",
                    Value::tree([
                        ("inputs", Value::tree([("glucose", wire_to("mass"))])),
                        ("outputs", Value::map()),
                    ]),
                ),
                ("schema", Value::None),
            ]),
        ),
        ("inputs", Value::tree([("glucose", wire_to("source"))])),
        ("outputs", Value::map()),
    ]);
    let parent = Value::tree([("alice", composite_spec)]);

    let path = [Key::from("alice")];
    let unfurled = unfurl_into(&parent, &path).unwrap();
    let refused = refuse_links(&unfurled.parent, &path, &unfurled.boundary).unwrap();

    let proc = refused
        .get_path(&[Key::from("alice"), Key::from("proc")])
        .unwrap();
    let v_wire = proc
        .get_field("inputs")
        .and_then(|v| v.get_field("v"))
        .and_then(|v| v.as_list())
        .unwrap();
    let strs: Vec<&str> = v_wire.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        strs,
        vec!["..", "source", "sub"],
        "bridge-prefix replaced with outer wire; the [sub] suffix rides along"
    );
}

#[test]
fn refuse_links_reachable_through_algebra_module() {
    // Closure-of-the-algebra check: refuse_links is exported from
    // `prism_schema::algebra` alongside fold/unfurl/fold_at/unfurl_into.
    let parent = parent_with_bridged_alice();
    let path = [Key::from("cells"), Key::from("alice")];
    let unfurled = algebra::unfurl_into(&parent, &path).unwrap();
    let refused = algebra::refuse_links(&unfurled.parent, &path, &unfurled.boundary).unwrap();
    // Sanity: refused parent has the bridge-rewritten Grow.
    let grow = refused
        .get_path(&[Key::from("cells"), Key::from("alice"), Key::from("grow")])
        .unwrap();
    let input_strs: Vec<&str> = grow
        .get_field("inputs")
        .and_then(|v| v.get_field("mass"))
        .and_then(|v| v.as_list())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(input_strs, vec!["..", "glucose_pool"]);
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
