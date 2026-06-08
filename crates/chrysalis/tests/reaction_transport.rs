//! **#61b — a schema-typed `map[Reaction]` link transports across a bridge.**
//!
//! With #61a the pool slot is typed `Map{Custom("Reaction")}`, so it crosses a
//! `rest:`/`stream:` bridge through the ORDINARY schema codec — no bridge
//! special-case. The bridge serializes each port value with the one serialize
//! door (`algebra::serialize_with`): the `Map` recurses per element, `Reaction`
//! dispatches `ReactionType::serialize` → the structural DATA form, and the
//! result is plain JSON (a runnable `Foreign` would `value_to_json` to `null`).
//! On the far side `realize_with` reconstructs the runnable
//! `Foreign(FOREIGN_REACTION, …)` per element, and it FIRES. This is the exact
//! per-port mechanism the rest bridge runs — exercised here without a live
//! socket so it is a fast, deterministic regression.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::process::Process;
use prism_bigraph::{BigraphicalReactiveSystem, Schema, Update, Value};
use prism_schema::reaction::{Pattern, ReactionRule};
use prism_schema::registry::TypeRegistry;
use prism_schema::value::Foreign;
use prism_schema::{algebra, Key, FOREIGN_REACTION};

use chrysalis::runtime::rule::ReactionType;

/// A structural A→B reaction in the runnable carrier form a `:: reaction` slot
/// stores.
fn reaction_a_to_b() -> Value {
    let redex = Pattern::sort("A", Vec::<(Key, Pattern)>::new());
    let reactum = Pattern::sort("B", Vec::<(Key, Pattern)>::new());
    let rule = ReactionRule::new(redex, reactum).with_label("Convert");
    Value::Foreign(Foreign::new(FOREIGN_REACTION, rule))
}

/// The `Reaction` type, registered exactly as a chrysalis Core registers it.
fn registry_with_reaction() -> TypeRegistry {
    let mut types = TypeRegistry::new();
    types.register_full(
        "Reaction",
        Schema::Any,
        None,
        Some(Arc::new(ReactionType)),
        Vec::new(),
    );
    types
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

#[test]
fn a_map_reaction_link_crosses_a_bridge_and_still_fires() {
    let registry = registry_with_reaction();
    let schema = map_reaction_schema();

    // The value a `map[Reaction]` link carries: a pool of reactions.
    let pool = Value::tree([("grow", reaction_a_to_b())]);

    // SEND SIDE — the bridge serializes the port value through the schema. The
    // `Map` recurses, `Reaction` → `ReactionType::serialize` → structural data;
    // no `Foreign` survives, so it is genuinely JSON-able.
    let wire = algebra::serialize_with(Some(&registry), &schema, &pool);
    let json = serde_json::to_string(&wire).expect("the reaction pool is JSON-able");
    assert!(
        json.contains("_pat") && json.contains("Rule"),
        "the reaction crossed as structural data, not an opaque Foreign: {json}"
    );

    // … crosses the wire …
    let arrived: Value = serde_json::from_str(&json).expect("JSON → data");

    // RECEIVE SIDE — realize through the same schema reconstructs the runnable
    // carrier per element.
    let landed = algebra::realize_with(Some(&registry), &schema, &arrived);
    let grow = landed.get_field("grow").expect("the reaction landed in the pool");
    let Value::Foreign(f) = grow else {
        panic!("expected a runnable reaction after realize, got {grow:?}")
    };
    assert_eq!(f.type_name, FOREIGN_REACTION, "reconstructed to the runnable form");
    let rule = f
        .downcast_ref::<ReactionRule>()
        .expect("a prism ReactionRule")
        .clone();

    // … and it FIRES on the far side: a BRS built from the transported rule
    // rewrites A → B.
    let after = drive(&BigraphicalReactiveSystem::new(vec![rule]), Value::tree([("a0", ion("A"))]), 2);
    let mut kinds = Vec::new();
    collect_types(&after, &mut kinds);
    assert!(
        kinds.iter().any(|t| t == "B"),
        "the wire-transported reaction fired on the far side (A→B): {after:?}"
    );
}

#[test]
fn a_reaction_pool_delta_crosses_via_serialize_update() {
    let registry = registry_with_reaction();
    let schema = map_reaction_schema(); // map[Reaction]

    // An UPDATE to the pool, not a state: `_add` a reaction. A state would be
    // `{grow: r}`; the delta wraps it in the `_add` sentinel. THIS is what
    // crosses a bridge when a BRS routes a new rule onto a `map[Reaction]` link.
    let delta = Value::tree([("_add", Value::tree([("grow", reaction_a_to_b())]))]);

    // serialize_UPDATE walks the `_add` sentinel and dispatches the Reaction leaf
    // → JSON-able. (serialize_WITH would treat `_add` as a Reaction-typed key and
    // leave the nested Foreign unserialized — which is exactly why an update
    // needs its own codec door.)
    let wire = algebra::serialize_update(Some(&registry), &schema, &delta);
    let json = serde_json::to_string(&wire).expect("the delta is JSON-able");
    assert!(
        json.contains("_add") && json.contains("_pat"),
        "the _add sentinel survives AND the reaction crossed as structural data: {json}"
    );

    // … crosses the wire … realize_update reconstructs the runnable carrier
    // inside the still-`_add`-wrapped delta.
    let arrived: Value = serde_json::from_str(&json).expect("JSON → data");
    let landed = algebra::realize_update(Some(&registry), &schema, &arrived);

    // Applying the transported delta to an empty pool installs a RUNNABLE rule.
    let pool = algebra::apply_with(Some(&registry), &schema, &Value::map(), &landed);
    let grow = pool.get_field("grow").expect("the reaction landed in the pool");
    let Value::Foreign(f) = grow else {
        panic!("expected a runnable reaction in the pool, got {grow:?}")
    };
    assert_eq!(f.type_name, FOREIGN_REACTION, "the _add'd reaction is runnable");
}

#[test]
fn serialize_update_is_structural_on_a_plain_data_delta() {
    // The delta codec is a conservative extension of the state codec: on a delta
    // with no `Custom`/`Foreign` leaves it is pure structure (a `Float` delta
    // serializes to itself), so every existing non-Custom port is unaffected.
    let registry = registry_with_reaction();
    let schema = Schema::Map { value: Box::new(Schema::float()) };
    let delta = Value::tree([
        ("_add", Value::tree([("x", Value::float(1.0))])),
        ("k", Value::float(2.0)), // a per-key Float increment
    ]);
    let wire = algebra::serialize_update(Some(&registry), &schema, &delta);
    assert_eq!(wire, delta, "a plain-data delta round-trips structurally");
    let back = algebra::realize_update(Some(&registry), &schema, &wire);
    assert_eq!(back, delta, "realize_update is identity on plain data");
}
