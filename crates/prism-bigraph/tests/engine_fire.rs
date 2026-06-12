//! `Engine::fire` — the on-demand INTENT surface a world-boundary (a `web:` browser
//! click, a control) drives, distinct from the per-tick BRS. It reuses the reaction
//! spine (`find_matches` / `fire_rule_at` / `apply_fire`) + the same post-apply
//! propagation a tick gets. (mesh's `to_core` ask, for the `web:` boundary.)
//!
//! The rule is the SAME single-node `relabel_rule` the functor lift uses — a functor
//! mapping fired as an intent — so the render-OUT (functor) and intent-IN (fire) halves
//! of the world-boundary face share one reaction primitive.

use prism_bigraph::{Core, Engine, Key, Schema, Value};
use prism_schema::functor::relabel_rule;

fn gate_state() -> Value {
    // a tiny bigraph: one node `gate` whose control is `Idle`.
    Value::tree([("gate", Value::tree([("_type", Value::String("Idle".into()))]))])
}

#[test]
fn fire_rewrites_a_node_on_demand_and_reports_the_change() {
    let mut engine = Engine::from_state(Schema::Any, gate_state(), Core::new()).unwrap();
    // a single-node redex Idle -> Active — the same functor rule, fired as an intent.
    let rule = relabel_rule("Idle", "Active");

    let changed = engine.fire(&rule, &[Key::from("gate")]);

    assert_eq!(
        engine
            .state()
            .get_path(&[Key::from("gate"), Key::from("_type")]),
        Some(&Value::String("Active".into())),
        "the targeted node was rewritten on demand",
    );
    assert!(
        changed.contains(&vec![Key::from("gate")]),
        "fire reports the changed path for the boundary to broadcast its delta",
    );
}

#[test]
fn fire_is_a_noop_where_the_rule_does_not_match() {
    let mut engine = Engine::from_state(Schema::Any, gate_state(), Core::new()).unwrap();
    let rule = relabel_rule("Running", "Stopped"); // no `Running` node here

    let changed = engine.fire(&rule, &[Key::from("gate")]);

    assert!(changed.is_empty(), "no match -> nothing changed");
    assert_eq!(
        engine
            .state()
            .get_path(&[Key::from("gate"), Key::from("_type")]),
        Some(&Value::String("Idle".into())),
        "state untouched when the rule does not match at `at`",
    );
}
