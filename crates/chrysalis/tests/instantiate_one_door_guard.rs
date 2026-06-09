//! One-instantiate one-door guard — `simplify` / Stage 3 of
//! `docs/homoiconic-unification.md` (invariant #1: *exactly one path turns an entity
//! into its runnable form*). The generalization of `closure_guard` + #45 to the
//! homoiconic surface.
//!
//! ## What this pins
//! After Stages 1b + 2a + 2b, the homoiconic flat family lowers through ONE core each,
//! and this guard asserts the surfaces CONVERGE (not just that each works):
//!
//! - **Reactions** (`reaction_surfaces_share_one_lowering_core`) — three surfaces, one
//!   `build_rule` core: definer `reaction X (r => x)` ≡ constructor `Reaction[redex:,
//!   reactum:]` (2b) ≡ quote→eval `eval(quote(r => x))` (1b). Identical redex+reactum.
//! - **Patterns** (`pattern_constructor_shares_the_redex_lowering_core`) — the
//!   `Pattern[ frag ]` first-class matcher (2b) lowers `frag` through the SAME
//!   `eval_pattern_top` core a reaction REDEX uses, so a matcher and a redex over the
//!   same fragment are the identical prism `Pattern`. No separate `Pattern[]` path.
//!
//! A second lowering door forking ⇒ divergence ⇒ this fires — a regression made LOUD,
//! not silent (`generative-core.md` §3). Property guards (robust to refactoring),
//! distinct from the consumer tests that prove each surface *works*.
//!
//! ## Scope notes
//! - The node-spec family is already DRY (`lang`'s 2a finding: one `build_spec_value`,
//!   `build_protocol_outer` delegates), and there is no capitalized `Process[]`/
//!   `Composite[]` constructor yet — so no node divergence to pin today.
//! - A `pattern X` DEFINER is a pattern-context mechanism (spliced into redexes via
//!   `eval_pattern_term`), NOT a value-context entity — there is no `entity.pattern` arm
//!   in `eval_term_value`. So the value-form equivalence we pin is the **constructor**
//!   against the shared redex core. (If `pattern X` gains a value form — `def X =
//!   Pattern(…)` made real — add a definer surface here; see coord task #5.)
//! - Forward-compatible with core's Stage-4c: a reaction may be a `Foreign` OR pure
//!   `_pat="Rule"` data; `reified` accepts both.

use chrysalis::ast::{Def, Expr, PortBindings, TermArg};
use prism_schema::reaction::{Pattern, ReactionRule};
use prism_schema::{Value, FOREIGN_REACTION};

fn term(control: &str, args: Vec<TermArg>) -> Expr {
    Expr::Term { control: control.into(), args, ports: PortBindings::default(), body: None }
}
fn named(name: &str, value: Expr) -> TermArg {
    TermArg::Named { name: name.into(), value }
}

/// A reaction value → its structural data form `{_pat:"Rule", label, redex, reactum}`.
/// Accepts a `Foreign(FOREIGN_REACTION, ReactionRule)` OR (forward-compat with core's
/// Stage-4c reaction-as-DATA) a pure `_pat="Rule"` map.
fn reified(which: &str, v: &Value) -> Value {
    match v {
        Value::Foreign(f) if f.type_name == FOREIGN_REACTION => f
            .downcast_ref::<ReactionRule>()
            .expect("a prism ReactionRule")
            .to_data_value()
            .expect("a structural reaction has a data form"),
        Value::Map(_) if v.get_field("_pat").and_then(|t| t.as_str()) == Some("Rule") => v.clone(),
        other => panic!("{which}: expected a reaction (Foreign or `_pat=Rule` data), got {other:?}"),
    }
}

/// The LOWERING (redex + reactum) of a reaction — the `label` legitimately differs per
/// surface (Convert / Reaction / assembled), so it is excluded from the comparison.
fn lowering(which: &str, v: &Value) -> (Option<Value>, Option<Value>) {
    let d = reified(which, v);
    (d.get_field("redex").cloned(), d.get_field("reactum").cloned())
}

#[test]
fn reaction_surfaces_share_one_lowering_core() {
    // The one reaction `(?a :: A) => (B)`, authored three ways, all through `build_rule`.
    let src = "reaction Convert ( (?a :: A) => (B) )";
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let ev = &result.evaluator;
    let env = indexmap::IndexMap::new();
    let rd = program
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Reaction(r) => Some(r),
            _ => None,
        })
        .expect("the reaction def");

    // (1) DEFINER: `Convert[]` → Foreign(FOREIGN_RULE); reify via `compile_reaction_value`.
    let definer_val = ev.eval_value(&term("Convert", vec![]), &env).expect("eval `Convert[]`");
    let definer = ev
        .compile_reaction_value(&definer_val, &env)
        .expect("reify the definer-built reaction");

    // (2) CONSTRUCTOR (2b): the definer's OWN redex/reactum Exprs as the raw pattern args.
    let constructor = ev
        .eval_value(
            &term(
                "Reaction",
                vec![named("redex", rd.redex.clone()), named("reactum", rd.reactum.clone())],
            ),
            &env,
        )
        .expect("eval `Reaction[redex:, reactum:]`");

    // (3) QUOTE→EVAL (1b): quote the same `redex => reactum`, run the GENERAL eval.
    let quoted = Expr::Rule {
        redex: Box::new(rd.redex.clone()),
        reactum: Box::new(rd.reactum.clone()),
    }
    .to_value();
    let quote = ev
        .eval_value(&Expr::from_value(&quoted).expect("from_value"), &env)
        .expect("eval the quoted reaction");

    let def = lowering("definer", &definer);
    let con = lowering("constructor", &constructor);
    let quo = lowering("quote→eval", &quote);
    assert_eq!(
        def, con,
        "DEFINER vs CONSTRUCTOR lowering differs — a second reaction-lowering door has \
         appeared (Reaction[…] must be `quote` of the definer):\n  definer={def:?}\n  ctor={con:?}"
    );
    assert_eq!(
        def, quo,
        "DEFINER vs QUOTE→EVAL lowering differs — a second reaction-lowering door has \
         appeared (eval∘quote must equal the definer):\n  definer={def:?}\n  quote={quo:?}"
    );
}

#[test]
fn pattern_constructor_shares_the_redex_lowering_core() {
    // `Pattern[ frag ]` (the first-class matcher, 2b) must lower `frag` through the SAME
    // `eval_pattern_top` core a reaction's REDEX uses — so a matcher and a redex over the
    // same fragment are the identical prism `Pattern`. Pins `eval_pattern_top` as the one
    // pattern-lowering door: `Pattern[…]` does not fork a second path.
    let src = "reaction Convert ( (?a :: A) => (B) )";
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let ev = &result.evaluator;
    let env = indexmap::IndexMap::new();
    let rd = program
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Reaction(r) => Some(r),
            _ => None,
        })
        .expect("the reaction def");

    // The reaction's REDEX, as data (via the definer path) — `eval_pattern_top(frag)`
    // inside `build_rule`.
    let definer = ev
        .compile_reaction_value(
            &ev.eval_value(&term("Convert", vec![]), &env).expect("eval `Convert[]`"),
            &env,
        )
        .expect("reify");
    let redex_data = reified("definer", &definer)
        .get_field("redex")
        .cloned()
        .expect("the reaction redex");

    // `Pattern[ <same frag> ]` → `Foreign(FOREIGN_PATTERN, prism Pattern)` → its data form
    // (`eval_pattern_top(frag)` inside `build_pattern_constructor`).
    let pat_val = ev
        .eval_value(&term("Pattern", vec![TermArg::Positional(rd.redex.clone())]), &env)
        .expect("eval `Pattern[ frag ]`");
    let Value::Foreign(f) = &pat_val else {
        panic!("`Pattern[…]` must be a Foreign-wrapped prism Pattern, got {pat_val:?}");
    };
    let pattern_data = f
        .downcast_ref::<Pattern>()
        .expect("`Pattern[…]` wraps a prism `Pattern`")
        .to_value();

    assert_eq!(
        pattern_data, redex_data,
        "`Pattern[ frag ]` and a reaction REDEX over the same fragment must be the \
         identical prism Pattern — both lower through `eval_pattern_top`. A divergence = \
         a second pattern-lowering door.\n  Pattern[] = {pattern_data:?}\n  redex     = {redex_data:?}"
    );
}
