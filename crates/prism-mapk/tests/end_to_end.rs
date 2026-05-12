//! End-to-end check: a short Gillespie run actually exercises every
//! rule in the MAPK BRS and conserves the total ERK + pERK count.

use prism_bigraph::{BigraphicalReactiveSystem, BrsMode, Process, Update};
use prism_mapk::{initial_mapk_state, mapk_rules};
use prism_schema::{Key, StateMap, Value};

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

/// Count all ERK + pERK nodes anywhere under the cell (excluding
/// compartment subtrees — these get walked separately).
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

#[test]
fn brs_runs_and_conserves_total_erk() {
    let initial = initial_mapk_state();
    let initial_count = total_erk_perk(&initial);
    assert_eq!(
        initial_count, 4,
        "initial state should have 4 ERK/pERK total (3 ERK + 1 pERK)"
    );

    let brs = BigraphicalReactiveSystem::with_config(
        mapk_rules(),
        BrsMode::Gillespie,
        None,
        7,
        Some(10_000),
        1.0,
    );

    let mut state = initial.clone();
    for _ in 0..50 {
        let update = brs.update(&wrap_state(state.clone()), 1.0);
        if let Update::Value(delta) = update {
            if let Some(state_delta) = delta.get_field("state") {
                state = apply_delta(&state, state_delta);
            }
        }
    }

    let after_count = total_erk_perk(&state);
    assert_eq!(
        after_count, initial_count,
        "MAPK rules must conserve total ERK + pERK"
    );

    let fired = brs.fired_log();
    assert!(
        fired.len() > 5,
        "expected several firings over 50 ticks, got {}",
        fired.len()
    );

    // Cycling rules should all show up over a long enough run.
    let labels: std::collections::HashSet<&str> =
        fired.iter().map(|e| e.rule_label.as_str()).collect();
    assert!(labels.contains("phosphorylate"));
    assert!(labels.contains("translocate_perk_in"));
}
