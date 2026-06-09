//! One-instantiate one-door guard — `simplify` / Stage 3 of
//! `docs/homoiconic-unification.md` (invariant #1: *exactly one path turns an entity
//! into its runnable form*). The generalization of `closure_guard` + #45 to the
//! homoiconic surface.
//!
//! ## What this pins (the Stage-2a achievement)
//! Stage 2a extracted **`build_rule`** (`eval.rs`) as the ONE reaction-lowering core:
//! the three reaction surfaces all funnel through it —
//! - the **definer** (`reaction X (redex => reactum)` → `build_reaction_value`),
//! - the **anonymous** `=>`-in-value (`eval_rule_expr`),
//! - the **data / quoted** form (`compile_reaction_value`, now just `eval ∘ quote`
//!   after Stage 1b dissolved its bespoke re-parse).
//!
//! This guard asserts the **homoiconic identity for reactions**: a reaction authored
//! via the *definer* and the *same* reaction authored as *quoted data* lower to the
//! SAME redex + reactum. If a second lowering path ever forks (the "second door"), the
//! two diverge and this fires — a regression made LOUD, not silent (`generative-core.md`
//! §3). It's a behavioral/structural property guard (robust to refactoring), distinct
//! from `lang`'s consumer tests (which prove each surface *works*; this proves they
//! *agree*).
//!
//! ## Scope (honest)
//! 2a unified the reaction *lowering*. The FULL invariant #1 — every capitalized
//! constructor (`Process[…]`/`Reaction[…]`/`Composite[…]`) routing through the unified
//! path as `quote` of its definer — completes with **Stage 2b** (capitalized
//! constructors + parser work, checkpointed with `unify`/human). When 2b lands, EXTEND
//! this file with the constructor↔definer convergence (`Reaction[…]` ≡ `reaction …`).
//! The node-spec family is already DRY (one `build_spec_value`; `build_protocol_outer`
//! delegates) per `lang`'s 2a finding, so the remaining gap is purely the constructor
//! surface.

use chrysalis::ast::{Def, Expr, PortBindings};
use prism_schema::reaction::ReactionRule;
use prism_schema::{Value, FOREIGN_REACTION};

#[test]
fn definer_and_quote_eval_reactions_share_one_lowering_core() {
    // A structural reaction (no guard / computed reactum / params) so both surfaces
    // reify to the transmittable structural form we can compare as data.
    let src = "reaction Convert ( (?a :: A) => (B) )";
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let ev = &result.evaluator;
    let env = indexmap::IndexMap::new();

    // A `Foreign(FOREIGN_REACTION, ReactionRule)` Value → its structural data form
    // (`{_pat:"Rule", label, redex, reactum, …}`).
    let reified = |which: &str, v: &Value| -> Value {
        let Value::Foreign(f) = v else {
            panic!("{which}: expected a Foreign reaction, got {v:?}");
        };
        assert_eq!(f.type_name, FOREIGN_REACTION, "{which}: expected the transmittable reaction form");
        f.downcast_ref::<ReactionRule>()
            .expect("a prism ReactionRule")
            .to_data_value()
            .expect("a structural reaction has a data form")
    };

    // DEFINER surface: `Convert[]` → `build_reaction_value` → `Foreign(FOREIGN_RULE)`;
    // reify via `compile_reaction_value` (now `eval ∘ quote`) → transmittable form.
    let definer_term = Expr::Term {
        control: "Convert".into(),
        args: vec![],
        ports: PortBindings::default(),
        body: None,
    };
    let definer_val = ev.eval_value(&definer_term, &env).expect("eval `Convert[]`");
    let definer_reified = ev
        .compile_reaction_value(&definer_val, &env)
        .expect("reify the definer-built reaction");

    // QUOTE→EVAL surface: quote the SAME reaction body, run it through the GENERAL eval
    // (Stage 1b: `eval_value(Expr::Rule)` routes to the one lowering core — no
    // `compile_reaction` special case).
    let rd = program
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Reaction(r) => Some(r),
            _ => None,
        })
        .expect("the reaction def");
    let quoted = Expr::Rule {
        redex: Box::new(rd.redex.clone()),
        reactum: Box::new(rd.reactum.clone()),
    }
    .to_value();
    let quote_val = ev
        .eval_value(&Expr::from_value(&quoted).expect("from_value"), &env)
        .expect("eval the quoted reaction");

    // CONVERGENCE: both surfaces lower through the ONE core (`build_rule`) to the SAME
    // redex + reactum. The `label` legitimately differs (a named definer vs an anonymous
    // assembled rule), so compare the LOWERING, not the label.
    let d = reified("definer", &definer_reified);
    let q = reified("quote→eval", &quote_val);
    assert_eq!(
        d.get_field("redex"),
        q.get_field("redex"),
        "definer vs quote→eval REDEX differs — a second reaction-lowering path has appeared:\n  \
         definer = {:?}\n  quote   = {:?}",
        d.get_field("redex"),
        q.get_field("redex"),
    );
    assert_eq!(
        d.get_field("reactum"),
        q.get_field("reactum"),
        "definer vs quote→eval REACTUM differs — a second reaction-lowering path has appeared:\n  \
         definer = {:?}\n  quote   = {:?}",
        d.get_field("reactum"),
        q.get_field("reactum"),
    );
}
