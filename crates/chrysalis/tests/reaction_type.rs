//! The `reaction` TYPE — the foundation of chrysalis AlChemy. Its `realize`
//! REIFIES a reaction value — the transparent `{_pat:"Rule"}` DATA a structural
//! reaction now is (Stage 4c), or the `FOREIGN_RULE` carrier a computed reaction
//! keeps — into the closure-free `Foreign(FOREIGN_REACTION, ReactionRule)` form
//! the BRS reads. That is the AUTO-convert: a
//! `:: reaction` / `:: map[reaction]` slot turns a reaction reference into the
//! exact value `BigraphicalReactiveSystem` reads as a rule (rules-as-state) and
//! that crosses a `:: bigraph` bridge (#42) — one value for store / link-share /
//! send. So an `.ys` reactum can write `=> Grow` with no reification verb.

use chrysalis::ast::Expr;
use chrysalis::runtime::rule::ReactionType;
use prism_schema::registry::{TypeMethods, TypeRegistry};
use prism_schema::{FOREIGN_REACTION, Schema, Value};

/// Compile a tiny program and evaluate a structural reaction reference to its
/// (chrysalis-`Rule`) value.
fn grow_rule_value() -> Value {
    let src = "reaction Grow ( (?a :: A) => (B) )";
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let env = indexmap::IndexMap::new();
    result
        .evaluator
        .eval_value(&Expr::term("Grow").build(), &env)
        .expect("eval Grow")
}

#[test]
fn reaction_type_reifies_chrysalis_rule_to_transmittable_form() {
    let v = grow_rule_value();
    // A STRUCTURAL reaction reference now evaluates to TRANSPARENT DATA
    // (`{_pat:"Rule"}`, Stage 4c — uniform with a process spec, no opaque Foreign);
    // `realize`/`apply` still reify it to the runnable form below. (A COMPUTED
    // reaction — guard / computed reactum / rate — keeps the `ChrysalisRule`
    // Foreign carrier, which `realize` also reifies.)
    assert_eq!(
        v.as_map().and_then(|m| m.get("_pat")).and_then(|x| x.as_str()),
        Some("Rule"),
        "reaction ref → transparent reaction DATA: {v:?}"
    );

    // `realize` at the `reaction` type reifies it to the transmittable form —
    // the exact value the BRS reads as a rule and that crosses bridges.
    let types = TypeRegistry::new();
    let reified = ReactionType.realize(&types, &Schema::Any, &v);
    assert!(
        matches!(&reified, Value::Foreign(f) if f.type_name == FOREIGN_REACTION),
        "realize reifies to Foreign(FOREIGN_REACTION): {reified:?}"
    );

    // Idempotent: realizing the already-transmittable form is a no-op.
    let again = ReactionType.realize(&types, &Schema::Any, &reified);
    assert!(
        matches!(&again, Value::Foreign(f) if f.type_name == FOREIGN_REACTION),
        "realize is idempotent on the transmittable form: {again:?}"
    );

    // `apply` STORES the reified reaction (overwrite), it does NOT fire (that is
    // the `bigraph` type) — so a `:: reaction` slot holds the rule for the BRS.
    let stored = ReactionType.apply(&types, &Schema::Any, &Value::None, &v);
    assert!(
        matches!(&stored, Value::Foreign(f) if f.type_name == FOREIGN_REACTION),
        "apply stores the reified reaction: {stored:?}"
    );
}
