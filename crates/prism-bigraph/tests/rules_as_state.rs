//! **#60 made load-bearing — RULES-AS-STATE (the reaction loop / #61 AlChemy).**
//!
//! The basis decision of #60: reactions are the dynamical generator;
//! `_add`/`_remove`/`_divide` are the schema algebra's delta vocabulary; and the
//! bridge is the duality *"a delta is a degenerate reaction; a reaction is a
//! guarded delta."* This test makes that duality carry weight: the BRS reads its
//! active ruleset from the state subtree each tick (seed rules ∪ reaction-values
//! under `_rules`), so a reactum can `_add` a REACTION and have it fire later.
//!
//! `Spawn` (the only seed rule) consumes a `Trigger` and INSTALLS a `Grow` rule
//! into `_rules`. `Grow` exists nowhere as a seed; only after `Spawn` runs does
//! it become active and convert a `Seed` into a `Plant`. A `Plant` appearing is
//! therefore proof that **a reaction added a reaction that then fired** — the
//! reaction loop closed, with `_add` of a reaction-value as the one move.

use std::sync::Arc;

use prism_bigraph::process::Process;
use prism_bigraph::{BigraphicalReactiveSystem, Foreign, Update};
use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};
use prism_schema::{FOREIGN_REACTION, Key, Schema, StateMap, Value, algebra};

/// A bare control ion `{_type: t}`.
fn ion(t: &str) -> Value {
    Value::tree([("_type", Value::String(t.into()))])
}

/// `Grow`: a `Seed` becomes a `Plant`.
fn grow_rule() -> ReactionRule {
    let no_fields: [(&str, Pattern); 0] = [];
    let redex = Pattern::map([("s", Pattern::sort("Seed", no_fields))]);
    let rf: ReactumFn = Arc::new(|b: &Bindings| {
        let seed_key = b.key_map.get("s").map(|k| k.to_string()).unwrap_or_default();
        let mut add = StateMap::new();
        add.insert(Key::from("plant"), ion("Plant"));
        Value::tree([
            ("_remove", Value::List(vec![Value::String(seed_key)])),
            ("_add", Value::Map(add)),
        ])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("grow")
        .with_reactum_fn(rf)
}

/// `Spawn`: a `Trigger` is consumed and a `Grow` rule is INSTALLED into `_rules`
/// as a reaction-value (`Foreign(FOREIGN_REACTION, ReactionRule)` — the #42 wire
/// form). The reactum's effect-delta is just `_remove` + a nested `_add` of a
/// reaction — a reaction producing a reaction.
fn spawn_rule() -> ReactionRule {
    let no_fields: [(&str, Pattern); 0] = [];
    let redex = Pattern::map([("t", Pattern::sort("Trigger", no_fields))]);
    let rf: ReactumFn = Arc::new(|b: &Bindings| {
        let trig = b.key_map.get("t").map(|k| k.to_string()).unwrap_or_default();
        let grow = Value::Foreign(Foreign::new(FOREIGN_REACTION, grow_rule()));
        let mut add_rules = StateMap::new();
        add_rules.insert(Key::from("grow"), grow);
        Value::tree([
            ("_remove", Value::List(vec![Value::String(trig)])),
            ("_rules", Value::tree([("_add", Value::Map(add_rules))])),
        ])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("spawn")
        .with_reactum_fn(rf)
}

/// The BRS reads its subtree off the `state` input port.
fn wrap(subtree: Value) -> Value {
    Value::tree([("state", subtree)])
}

/// Apply the BRS's emitted state-delta back to the subtree (Any-typed: the
/// algebra interprets `_remove`/`_add` and passes the Foreign rule-value
/// through). This is the engine's apply step, in miniature.
fn drive(brs: &BigraphicalReactiveSystem, mut subtree: Value, ticks: usize) -> Value {
    for _ in 0..ticks {
        if let Update::Value(out) = brs.update(&wrap(subtree.clone()), 1.0) {
            if let Some(delta) = out.get_field("state") {
                subtree = algebra::apply_with(None, &Schema::Any, &subtree, delta);
            }
        }
    }
    subtree
}

#[test]
fn a_reaction_installs_a_reaction_that_then_fires() {
    let brs = BigraphicalReactiveSystem::new(vec![spawn_rule()]);
    let initial = Value::tree([
        ("trigger", ion("Trigger")),
        ("seed", ion("Seed")),
        ("_rules", Value::map()),
    ]);

    // One tick: Spawn fires (Trigger present) and installs Grow into `_rules` —
    // but Grow has NOT fired yet (it was added this tick). The Seed is untouched.
    let after1 = drive(&brs, initial.clone(), 1);
    let m1 = after1.as_map().expect("map");
    assert!(!m1.contains_key("trigger"), "Spawn consumed the trigger: {m1:?}");
    assert!(
        m1.contains_key("seed"),
        "Seed not yet converted after one tick (Grow added, not yet fired): {m1:?}"
    );
    let rules = after1
        .get_field("_rules")
        .and_then(|v| v.as_map())
        .expect("_rules map");
    assert!(
        rules.contains_key("grow"),
        "Spawn installed the Grow rule into _rules (rules-as-state): {rules:?}"
    );

    // More ticks: Grow — now an ACTIVE rule, read from state — converts Seed→Plant.
    let after = drive(&brs, initial, 4);
    let m = after.as_map().expect("map");
    assert!(
        !m.contains_key("seed"),
        "the reaction-added Grow rule consumed the seed: {m:?}"
    );
    assert!(
        m.contains_key("plant"),
        "Grow (a reaction ADDED by a reaction) fired — the reaction loop closed: {m:?}"
    );
}
