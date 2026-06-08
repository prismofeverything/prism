//! **Distributed AlChemy (#61) — a reaction crosses a JSON wire and runs.**
//!
//! The payoff of reactions-as-data: because a reaction serializes to a plain
//! `Value` (no `Foreign`), it crosses a serialization boundary as JSON and
//! recompiles + fires on the far side. This is the substrate distributed /
//! streaming AlChemy rides on — a reaction created on one node travels as data
//! (the same map-literal form) to another, where it becomes a live rule. The
//! runnable `Foreign(ReactionRule)` form *cannot* cross a wire; the data form
//! can, and `compile_reaction` turns it back into a rule at the destination.
//!
//! (The actual transport — a `stream:`/`rest:` bridge carrying the reaction
//! link — is a thin layer over this: the wire format is exactly this JSON.)

use prism_bigraph::process::Process;
use prism_bigraph::{BigraphicalReactiveSystem, Schema, Update, Value};
use prism_schema::reaction::ReactionRule;
use prism_schema::{FOREIGN_REACTION, algebra};

use chrysalis::ast::{Def, Expr};

fn ion(t: &str) -> Value {
    Value::tree([("_type", Value::String(t.into()))])
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

#[test]
fn a_reaction_crosses_a_json_wire_and_runs() {
    // A reaction, on the SENDING node — obtain its body as data.
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
    let data = Expr::Rule {
        redex: Box::new(rd.redex.clone()),
        reactum: Box::new(rd.reactum.clone()),
    }
    .to_value();

    // (1) SERIALIZE to JSON — the wire format. The data form is all maps /
    //     strings / lists (no `Foreign`), so it is fully JSON-able.
    let json = serde_json::to_string(&data).expect("reaction data → JSON");
    assert!(json.contains("Rule"), "the reaction is on the wire as data: {json}");

    // (2) … crosses the wire … the RECEIVING node deserializes the bytes.
    let arrived: Value = serde_json::from_str(&json).expect("JSON → reaction data");

    // (3) COMPILE on the far side (recover a runnable rule from the data) + RUN.
    let result = chrysalis::compile::compile(&program).expect("compile");
    let env = indexmap::IndexMap::new();
    let compiled = result
        .evaluator
        .compile_reaction_value(&arrived, &env)
        .expect("compile the arrived reaction");
    let Value::Foreign(f) = &compiled else {
        panic!("expected a runnable reaction: {compiled:?}")
    };
    assert_eq!(f.type_name, FOREIGN_REACTION, "recompiled to the runnable form");
    let rule: ReactionRule = f
        .downcast_ref::<ReactionRule>()
        .expect("a prism ReactionRule")
        .clone();

    let brs = BigraphicalReactiveSystem::new(vec![rule]);
    let after = drive(&brs, Value::tree([("a0", ion("A"))]), 2);
    let mut kinds = Vec::new();
    collect_types(&after, &mut kinds);
    assert!(
        kinds.iter().any(|t| t == "B"),
        "the wire-transmitted reaction fired on the far side (A→B): {after:?}"
    );
}
