//! End-to-end: MAPK signalling (tier-1 benchmark #2), chrysalis → prism BRS.
//!
//! Mirrors `crates/prism-mapk/tests/end_to_end.rs` — same acceptance —
//! but the seven rules are built from a chrysalis AST fixture and lowered
//! to `prism_schema::ReactionRule`s (via `to_prism_rule`), then run on
//! prism's `BigraphicalReactiveSystem`. Proves chrysalis can express the
//! full pattern-composition / link-graph / nesting suite and reproduce the
//! reference dynamics on the real prism engine — no chrysalis BRS.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{BigraphicalReactiveSystem, BrsMode, Engine, Process, Update};
use prism_schema::{Key, ReactionRule, StateMap, Value};

use chrysalis::ast::Expr;
use chrysalis::fixtures::mapk;
use chrysalis::runtime::rule::{Rule, to_prism_rule};

/// Total ERK + pERK nodes anywhere under the cell.
fn total_erk_perk(state: &Value) -> usize {
    fn walk(v: &Value, count: &mut usize) {
        if let Some(iter) = v.iter_fields() {
            for (k, child) in iter {
                if k.starts_with('_') {
                    continue;
                }
                match child.get_field("_type").and_then(|t| t.as_str()) {
                    Some("ERK") | Some("pERK") => *count += 1,
                    _ => {}
                }
                walk(child, count);
            }
        }
    }
    let mut c = 0;
    walk(state, &mut c);
    c
}

fn wrap_state(subtree: Value) -> Value {
    let mut m = StateMap::new();
    m.insert(Key::from("state"), subtree);
    Value::Map(m)
}

fn apply_delta(initial: &Value, delta: &Value) -> Value {
    fn walk(target: &mut Value, delta: &Value) {
        let delta_map = match delta.as_map() {
            Some(m) => m,
            None => {
                *target = delta.clone();
                return;
            }
        };
        let target_map = match target.as_map_mut() {
            Some(m) => m,
            None => {
                *target = delta.clone();
                return;
            }
        };
        if let Some(Value::List(rm)) = delta_map.get("_remove") {
            for k in rm {
                if let Some(s) = k.as_str() {
                    target_map.shift_remove(s);
                }
            }
        }
        if let Some(Value::Map(adds)) = delta_map.get("_add") {
            for (k, v) in adds {
                target_map.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in delta_map {
            if k == "_add" || k == "_remove" {
                continue;
            }
            match target_map.get_mut(k) {
                Some(child) => walk(child, v),
                None => {
                    target_map.insert(k.clone(), v.clone());
                }
            }
        }
    }
    let mut next = initial.clone();
    walk(&mut next, delta);
    next
}

/// Lower the chrysalis MAPK reactions to prism `ReactionRule`s, reusing the
/// compiler's own evaluator (the exact path `compile` takes).
fn prism_rules() -> Vec<ReactionRule> {
    let program = mapk::program();
    let result = chrysalis::compile::compile(&program).expect("compile");
    let env: IndexMap<String, Value> = IndexMap::new();
    mapk::rule_names()
        .iter()
        .map(|name| {
            let v = result
                .evaluator
                .eval_value(&Expr::term(name).build(), &env)
                .expect("eval reaction value");
            let Value::Foreign(f) = v else {
                panic!("reaction {name} did not evaluate to a Foreign rule")
            };
            let rule = f
                .downcast_ref::<Rule>()
                .unwrap_or_else(|| panic!("reaction {name} carrier is not a chrysalis Rule"));
            to_prism_rule(rule, Arc::clone(&result.evaluator))
        })
        .collect()
}

#[test]
fn chrysalis_mapk_conserves_erk_and_cycles_through_prism_brs() {
    let initial = mapk::initial_state();
    assert_eq!(total_erk_perk(&initial), 4, "3 ERK + 1 pERK initially");

    // Gillespie, seed 7 — same configuration as prism-mapk's reference test.
    let brs = BigraphicalReactiveSystem::with_config(
        prism_rules(),
        BrsMode::Gillespie,
        None,
        7,
        Some(10_000),
        1.0,
    );

    let mut state = initial.clone();
    for _ in 0..50 {
        if let Update::Value(delta) = brs.update(&wrap_state(state.clone()), 1.0) {
            if let Some(state_delta) = delta.get_field("state") {
                state = apply_delta(&state, state_delta);
            }
        }
    }

    // Conservation: the rules only interconvert/move ERK & pERK.
    assert_eq!(
        total_erk_perk(&state),
        4,
        "MAPK rules must conserve total ERK + pERK"
    );

    // Liveness: the cycle actually turned over.
    let fired = brs.fired_log();
    assert!(
        fired.len() > 5,
        "expected several firings over 50 ticks, got {}",
        fired.len()
    );
    let labels: std::collections::HashSet<&str> =
        fired.iter().map(|e| e.rule_label.as_str()).collect();
    assert!(
        labels.contains("phosphorylate"),
        "phosphorylate should fire; fired {labels:?}"
    );
    assert!(
        labels.contains("translocate_perk_in"),
        "translocate_perk_in should fire (the cycle reaches the nucleus); fired {labels:?}"
    );
}

#[test]
fn chrysalis_mapk_runs_through_the_engine() {
    // The full pipeline: compile → Engine → BRS factory → to_prism_rule.
    let program = mapk::program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();

    let before = total_erk_perk(
        result
            .initial_state
            .get_field("cell")
            .unwrap_or(&result.initial_state),
    );
    engine.run(50.0);
    let after_state = engine.state();
    let cell = after_state.get_field("cell").unwrap_or(after_state);
    assert_eq!(
        total_erk_perk(cell),
        before,
        "ERK + pERK conserved across the engine run"
    );
}
