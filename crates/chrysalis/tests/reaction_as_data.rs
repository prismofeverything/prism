//! **Reactions are DATA (#61).** A reaction is a pair of bigraphs sharing a
//! site-set — `{_type:"Rule", redex:{…}, reactum:{…}}` is exactly that, as a
//! plain `Value`. This proves the full homoiconic loop for reactions:
//!
//!   1. SERIALIZE a reaction to data (`Expr::to_value`),
//!   2. ROUND-TRIP it back (`Expr::from_value` on the `?c` Site / `=>` Rule
//!      variants — the gap #61 closed; these used to error),
//!   3. COMPILE the data to a runnable rule (`compile_reaction_value` — the
//!      eval-for-reactions: `Value → Expr → Pattern → ReactionRule`),
//!   4. RUN it.
//!
//! The reaction analog of the hand-built cell (#34) and `eval(ast)` (#32). Once a
//! reaction is plain data, it can be assembled from parts, inspected, and
//! serialized across a JSON / stream bridge — the substrate distributed AlChemy
//! needs (the `Foreign` blob can do none of those).

use prism_bigraph::process::Process;
use prism_bigraph::{BigraphicalReactiveSystem, Schema, Update, Value};
use prism_schema::reaction::ReactionRule;
use prism_schema::{FOREIGN_REACTION, algebra};

use chrysalis::ast::{Def, Expr};

fn ion(t: &str) -> Value {
    Value::tree([("_type", Value::String(t.into()))])
}

/// Every `_type` string anywhere under `v` (the fired result's shape depends on
/// the reaction; we only care which ions exist).
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

/// Drive a BRS directly: feed the subtree on the `state` port, apply its delta.
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
fn a_reaction_assembled_as_data_runs() {
    // Parse a reaction ONLY to obtain its body Expr (redex => reactum).
    let src = "reaction Convert ( (?a :: A) => (B) )";
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let rd = program
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Reaction(r) => Some(r),
            _ => None,
        })
        .expect("a reaction def");
    let rule_expr = Expr::Rule {
        redex: Box::new(rd.redex.clone()),
        reactum: Box::new(rd.reactum.clone()),
    };

    // (1) SERIALIZE the reaction to DATA — a plain Value (map literals).
    let data = rule_expr.to_value();
    assert_eq!(
        data.get_field("_type").and_then(|t| t.as_str()),
        Some("Rule"),
        "a reaction serializes to a plain data Value: {data:?}"
    );

    // (2) ROUND-TRIP: data → Expr. The `?c` Site + `=>` Rule variants now
    // reconstruct (the #61 gap — they used to return an error).
    let back = Expr::from_value(&data).expect("a reaction round-trips from data");
    assert!(
        matches!(back, Expr::Rule { .. }),
        "from_value reconstructs the Rule: {back:?}"
    );

    // (3) COMPILE the DATA into a runnable, transmittable reaction.
    let result = chrysalis::compile::compile(&program).expect("compile");
    let env = indexmap::IndexMap::new();
    let compiled = result
        .evaluator
        .compile_reaction_value(&data, &env)
        .expect("compile_reaction");
    let Value::Foreign(f) = &compiled else {
        panic!("expected a Foreign reaction value: {compiled:?}")
    };
    assert_eq!(
        f.type_name, FOREIGN_REACTION,
        "compiled to the transmittable form: {compiled:?}"
    );
    let rule: ReactionRule = f
        .downcast_ref::<ReactionRule>()
        .expect("a prism ReactionRule")
        .clone();

    // (4) RUN the assembled reaction: a BRS turns an `A` into a `B`.
    let brs = BigraphicalReactiveSystem::new(vec![rule]);
    let after = drive(&brs, Value::tree([("a0", ion("A"))]), 2);
    let mut kinds = Vec::new();
    collect_types(&after, &mut kinds);
    assert!(
        kinds.iter().any(|t| t == "B"),
        "the data-assembled reaction fired (A→B): {after:?}"
    );
    assert!(
        !kinds.iter().any(|t| t == "A"),
        "the A was consumed: {after:?}"
    );
}
