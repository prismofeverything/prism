//! End-to-end: Rosen (M,R) closure (tier-1 benchmark #3), chrysalis → prism.
//!
//! Proves the homoiconic core: reactions match on a SHARED blueprint between
//! two co-located ions (anonymous parallel + a `where` guard, run on prism's
//! list/multiset matcher), and mechanism lineage flows through the
//! reactum's construction. Two checks:
//!   1. the shared-blueprint guard — `F | B` fires only when blueprints agree;
//!   2. the cycle — from `{F_X, A…}`, B's, φ's, and fresh F's appear, all
//!      carrying the X blueprint (no foreign lineage introduced).

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{BigraphicalReactiveSystem, BrsMode, Process, Update};
use prism_schema::{Key, ReactionRule, StateMap, Value, find_matches};

use chrysalis::ast::Expr;
use chrysalis::fixtures::mr;
use chrysalis::runtime::rule::{Rule, to_prism_rule};

/// Lower the chrysalis M/R reactions to prism `ReactionRule`s via the
/// compiler's evaluator (name → the converted rule).
fn prism_rules() -> IndexMap<String, ReactionRule> {
    let program = mr::program();
    let result = chrysalis::compile::compile(&program).expect("compile");
    let env: IndexMap<String, Value> = IndexMap::new();
    let mut out = IndexMap::new();
    for name in mr::rule_names() {
        let v = result
            .evaluator
            .eval_value(&Expr::term(&name).build(), &env)
            .expect("eval reaction");
        let Value::Foreign(f) = v else {
            panic!("{name} is not a Foreign rule")
        };
        let rule = f.downcast_ref::<Rule>().expect("chrysalis Rule");
        out.insert(
            name.clone(),
            to_prism_rule(rule, Arc::clone(&result.evaluator)),
        );
    }
    out
}

fn apply_delta(initial: &Value, delta: &Value) -> Value {
    fn walk(target: &mut Value, delta: &Value) {
        let (Some(dm), Some(tm)) = (delta.as_map(), target.as_map_mut()) else {
            *target = delta.clone();
            return;
        };
        if let Some(Value::List(rm)) = dm.get("_remove") {
            for k in rm {
                if let Some(s) = k.as_str() {
                    tm.shift_remove(s);
                }
            }
        }
        if let Some(Value::Map(adds)) = dm.get("_add") {
            for (k, v) in adds {
                tm.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in dm {
            if k == "_add" || k == "_remove" {
                continue;
            }
            match tm.get_mut(k) {
                Some(child) => walk(child, v),
                None => {
                    tm.insert(k.clone(), v.clone());
                }
            }
        }
    }
    let mut next = initial.clone();
    walk(&mut next, delta);
    next
}

fn wrap(subtree: Value) -> Value {
    let mut m = StateMap::new();
    m.insert(Key::from("state"), subtree);
    Value::Map(m)
}

/// Count ions of `_type ty` in the (flat) soup.
fn count(soup: &Value, ty: &str) -> usize {
    soup.as_map()
        .map(|m| {
            m.iter()
                .filter(|(k, v)| {
                    !k.starts_with('_') && v.get_field("_type").and_then(|t| t.as_str()) == Some(ty)
                })
                .count()
        })
        .unwrap_or(0)
}

/// Every ion of `_type ty` has `blueprint == bp`.
fn all_blueprints(soup: &Value, ty: &str, bp: &str) -> bool {
    soup.as_map()
        .map(|m| {
            m.iter()
                .filter(|(k, v)| {
                    !k.starts_with('_') && v.get_field("_type").and_then(|t| t.as_str()) == Some(ty)
                })
                .all(|(_, v)| v.get_field("blueprint").and_then(|b| b.as_str()) == Some(bp))
        })
        .unwrap_or(true)
}

#[test]
fn shared_blueprint_guard_gates_makephi() {
    let rule = prism_rules().shift_remove("MakePhi").expect("MakePhi");

    // Same blueprint → structurally matches AND the guard passes.
    let same = mr::soup(vec![("f", mr::f_ion("X")), ("b", mr::b_ion("X"))]);
    let m = find_matches(&same, &rule.redex, None);
    assert!(!m.is_empty(), "F | B present → structural match");
    assert!(
        rule.passes_guard(&m[0].bindings),
        "same blueprint X → guard passes"
    );

    // Different blueprints → still structurally matches (an F and a B exist),
    // but the shared-blueprint guard rejects it.
    let diff = mr::soup(vec![("f", mr::f_ion("X")), ("b", mr::b_ion("Y"))]);
    let m = find_matches(&diff, &rule.redex, None);
    assert!(!m.is_empty(), "F | B present → structural match");
    assert!(
        !rule.passes_guard(&m[0].bindings),
        "X vs Y blueprint → guard rejects (no cross-lineage φ)"
    );
}

#[test]
fn mr_cycle_runs_and_carries_lineage() {
    let rules: Vec<ReactionRule> = prism_rules().into_values().collect();
    let brs = BigraphicalReactiveSystem::with_config(
        rules,
        BrsMode::Gillespie,
        None,
        7,
        Some(10_000),
        1.0,
    );

    // One F of lineage X plus raw material; the cycle should turn over.
    let mut state = mr::soup(vec![
        ("f0", mr::f_ion("X")),
        ("a0", mr::a_ion()),
        ("a1", mr::a_ion()),
        ("a2", mr::a_ion()),
        ("a3", mr::a_ion()),
        ("a4", mr::a_ion()),
    ]);

    let mut saw_b = false;
    let mut saw_phi = false;
    for _ in 0..60 {
        if let Update::Value(delta) = brs.update(&wrap(state.clone()), 1.0) {
            if let Some(d) = delta.get_field("state") {
                state = apply_delta(&state, d);
            }
        }
        saw_b |= count(&state, "B") > 0;
        saw_phi |= count(&state, "Phi") > 0;
    }

    let fired: std::collections::HashSet<String> =
        brs.fired_log().into_iter().map(|e| e.rule_label).collect();
    // The whole cycle exercised: A→B, B+F→φ, φ+B→F.
    assert!(saw_b, "MakeB should have produced at least one B");
    assert!(saw_phi, "MakePhi should have produced at least one φ");
    assert!(
        fired.contains("MakeB") && fired.contains("MakePhi"),
        "expected MakeB and MakePhi to fire; fired {fired:?}"
    );

    // Lineage: nothing introduces a foreign blueprint, so every F / B / Phi
    // still carries X — the mechanism's lineage flowed through construction.
    for ty in ["F", "B", "Phi"] {
        assert!(
            all_blueprints(&state, ty, "X"),
            "every {ty} should carry the X lineage"
        );
    }
    // F is regenerated (the closure is self-maintaining), not exhausted.
    assert!(
        count(&state, "F") >= 1,
        "at least one F persists/regenerates"
    );
}
