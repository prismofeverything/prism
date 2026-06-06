//! Rate-as-expression: `reaction … ( redex => reactum ) rate ( expr )` carries a
//! propensity EXPRESSION (closing over config params + matched bindings), not a
//! nominal constant. The eval side (`eval.rs`: `rate: def.rate.clone()`) and the
//! runtime (`runtime/rule.rs` `to_prism_rule` → `RateFn`) already consumed
//! `ReactionDef.rate`; this slice activated the dormant path by PARSING the
//! clause (the parser previously hardcoded `None`).
//!
//! `rate` is CONTEXTUAL — it stays an ordinary identifier as a config-param
//! name; it's a rate clause only as `rate (` immediately after a reaction body.

use chrysalis::ast::Def;
use chrysalis::parse::parse_program;

/// `(guard_is_some, rate_is_some)` for the named reaction in `src`.
fn guard_rate(src: &str, name: &str) -> (bool, bool) {
    let prog = parse_program(src).expect("parse");
    match prog.lookup(name) {
        Some(Def::Reaction(rd)) => (rd.guard.is_some(), rd.rate.is_some()),
        other => panic!("expected reaction `{name}`, got {other:?}"),
    }
}

#[test]
fn rate_clause_is_parsed_into_the_reaction() {
    let (_, rate) = guard_rate(
        "reaction Convert[k :: float = 0.5] ( (?a :: A) => B ) rate ( k * 2.0 )\n",
        "Convert",
    );
    assert!(rate, "the `rate ( … )` clause should land in ReactionDef.rate");
}

#[test]
fn no_rate_clause_leaves_rate_none() {
    let (_, rate) = guard_rate("reaction Plain[k :: float = 0.5] ( (?a :: A) => B )\n", "Plain");
    assert!(!rate, "no clause ⇒ rate stays None (regression)");
}

#[test]
fn rate_is_contextual_not_a_reserved_keyword() {
    // The wei-qi guard: adding the clause must not break existing programs that
    // use `rate` as an ordinary config-param name.
    parse_program(
        "process Grow[rate :: float = 0.2] ~{mass :: float} ->{mass :: float} \
         ( { mass: mass * rate } )\n",
    )
    .expect("`rate` as a config-param name must still parse");
}

#[test]
fn guard_and_rate_coexist() {
    // `where` guard (between redex and `=>`) and a trailing `rate` clause are
    // independent and compose on one reaction.
    let (guard, rate) = guard_rate(
        "reaction Decay[k :: float = 1.0] ( (?a :: A) where k > 0.0 => B ) rate ( k )\n",
        "Decay",
    );
    assert!(guard, "where-guard parsed");
    assert!(rate, "rate clause parsed");
}

#[test]
fn rate_round_trips_through_unparse() {
    // No silent loss: the unparser must emit the rate clause, and a reparse of
    // the unparsed text must still carry it.
    let src = "reaction Convert[k :: float = 0.5] ( (?a :: A) => B ) rate ( k * 2.0 )\n";
    let prog = parse_program(src).expect("parse");
    let text = chrysalis::unparse::unparse(&prog);
    assert!(
        text.contains("rate ("),
        "unparse must emit the rate clause; got:\n{text}"
    );
    let (_, rate) = guard_rate(&text, "Convert");
    assert!(rate, "rate must survive parse → unparse → parse");
}
