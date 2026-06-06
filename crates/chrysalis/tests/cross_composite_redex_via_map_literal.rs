//! Cross-composite redex syntax (closing #40 — the principled path).
//!
//! Per the chrysalis design decision: a cross-composite redex is written
//! as a **map literal** — `{ alice: {state: ?a}, bob: {state: ?b} }`.
//! This is unambiguous (the `{}` form is always a map; keys are always
//! literal), works with the existing parser + Pattern lowering, and
//! contrasts cleanly with the sort form `K[args]~{ports}` which is for
//! controls/sorts.
//!
//! The discarded alternative `alice~{state: ?a} | bob~{state: ?b}`
//! conflated control-name with peer-key and forced case-based
//! heuristics for mixed-case chemistry sorts (`pERK`, `mLys`) — a
//! decaying compromise. The map literal form sidesteps the ambiguity
//! by being explicit about the semantic operation: keyed match by
//! literal name.
//!
//! See `docs/chrysalis-design.md` for the design rationale.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_schema::reaction::Pattern;
use prism_schema::{find_matches, MethodRegistry, Value};

use chrysalis::ast::{Expr, Program, StringLit};
use chrysalis::eval::Evaluator;
use chrysalis::runtime::rule::RuleBindings;

fn val_str(s: &str) -> Value {
    Value::String(s.to_string())
}

fn empty_evaluator() -> Evaluator {
    Evaluator::new(Arc::new(Program::new()), Arc::new(MethodRegistry::new()))
}

/// Convenience: build a chrysalis Expr::Map literal `{ k1: v1, k2: v2, … }`.
fn map_literal(entries: Vec<(&str, Expr)>) -> Expr {
    Expr::Map(
        entries
            .into_iter()
            .map(|(k, v)| (StringLit::plain(k), v))
            .collect(),
    )
}

#[test]
fn cross_composite_redex_lowers_to_a_keyed_map() {
    // `{ alice: { state: ?a }, bob: { state: ?b } }` — the
    // map-literal redex form. Lowers to
    // `Pattern::Map([("alice", {state: Site}), ("bob", {state: Site})])`.
    let redex = map_literal(vec![
        (
            "alice",
            map_literal(vec![("state", Expr::site("?a"))]),
        ),
        (
            "bob",
            map_literal(vec![("state", Expr::site("?b"))]),
        ),
    ]);

    let ev = empty_evaluator();
    let mut bindings = RuleBindings::new();
    let pattern = ev
        .eval_pattern(&redex, &IndexMap::new(), &mut bindings)
        .expect("lower");

    let outer = match pattern {
        Pattern::Map(m) => m,
        other => panic!("expected Pattern::Map, got {other:?}"),
    };
    assert_eq!(outer.len(), 2);
    assert!(outer.contains_key("alice"));
    assert!(outer.contains_key("bob"));
}

#[test]
fn cross_composite_map_literal_matches_two_peer_state() {
    // Build the redex via map literal and verify it matches a state
    // with two named siblings.
    let redex_expr = map_literal(vec![
        (
            "alice",
            map_literal(vec![("state", Expr::site("?a"))]),
        ),
        (
            "bob",
            map_literal(vec![("state", Expr::site("?b"))]),
        ),
    ]);
    let ev = empty_evaluator();
    let mut bindings = RuleBindings::new();
    let redex = ev
        .eval_pattern(&redex_expr, &IndexMap::new(), &mut bindings)
        .expect("lower");

    let state = Value::tree([
        ("alice", Value::tree([("state", val_str("active"))])),
        ("bob", Value::tree([("state", val_str("idle"))])),
    ]);

    let matches = find_matches(&state, &redex, None);
    assert!(
        !matches.is_empty(),
        "map-literal redex should match a two-peer state"
    );
    let m = &matches[0];
    assert!(m.bindings.key_map.contains_key("alice"));
    assert!(m.bindings.key_map.contains_key("bob"));
}

