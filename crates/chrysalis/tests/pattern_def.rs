//! `pattern Name[params] (body)` — a named redex FRAGMENT. A use `Name[args]` in
//! pattern position substitutes the args for the params and splices the body in,
//! so a shared fragment (e.g. MAPK's compartment) is written once, not inlined
//! per reaction. Correctness = "expansion equals inlining": the lowered redex
//! `Pattern` of a reaction using the fragment must equal that of the hand-inlined
//! reaction. Splicing has NO unquote sigil — a parallel arg spliced into a
//! parallel context flattens by the `|` associativity law.

use std::sync::Arc;

use chrysalis::ast::Def;
use chrysalis::eval::Evaluator;
use chrysalis::parse::parse_program;
use chrysalis::prelude::std_methods;
use chrysalis::runtime::rule::RuleBindings;
use indexmap::IndexMap;
use prism_schema::reaction::Pattern;

/// Parse `src`, lower the named reaction's redex to a prism `Pattern`.
fn redex_pattern(src: &str, reaction: &str) -> Pattern {
    let prog = Arc::new(parse_program(src).expect("parse"));
    let ev = Evaluator::new(prog.clone(), Arc::new(std_methods()));
    let redex = match prog.lookup(reaction) {
        Some(Def::Reaction(r)) => r.redex.clone(),
        other => panic!("expected reaction `{reaction}`, got {other:?}"),
    };
    let mut bindings = RuleBindings::new();
    ev.eval_pattern_top(&redex, &IndexMap::new(), &mut bindings)
        .expect("lower redex")
}

#[test]
fn pattern_expansion_equals_the_inlined_form() {
    let src = "\
pattern Holds[item] (
  Holder (contents: item | bystanders: ?rest)
)

reaction WithPattern (
  ( slot: Holds[?g] )
  =>
  ( slot: Holds[?g] )
)

reaction Inlined (
  ( slot: Holder (contents: ?g | bystanders: ?rest) )
  =>
  ( slot: Holder (contents: ?g | bystanders: ?rest) )
)
";
    assert_eq!(
        redex_pattern(src, "WithPattern"),
        redex_pattern(src, "Inlined"),
        "a pattern reference must lower to the same redex as the inlined fragment"
    );
}

#[test]
fn pattern_splices_a_parallel_argument() {
    // The arg `(a: MEK | b: ERK)` is a PARALLEL; spliced into the body's
    // `(contents | rest: ?r)` it flattens (associativity) so a/b/rest are
    // siblings — identical to writing them inline.
    let src = "\
pattern Wrap[contents] (
  Cell (contents | rest: ?r)
)

reaction Spliced (
  ( c: Wrap[(a: MEK | b: ERK)] )
  =>
  ( c: Wrap[(a: MEK | b: ERK)] )
)

reaction Flat (
  ( c: Cell (a: MEK | b: ERK | rest: ?r) )
  =>
  ( c: Cell (a: MEK | b: ERK | rest: ?r) )
)
";
    assert_eq!(
        redex_pattern(src, "Spliced"),
        redex_pattern(src, "Flat"),
        "a parallel pattern arg must splice (flatten) into the body"
    );
}

#[test]
fn pattern_def_round_trips_through_unparse() {
    let src = "pattern Holds[item] (\n  Holder (contents: item | rest: ?r)\n)\n";
    let prog = parse_program(src).expect("parse");
    let text = chrysalis::unparse::unparse(&prog);
    assert!(
        text.contains("pattern Holds[item]"),
        "unparse must emit the pattern definer; got:\n{text}"
    );
    let prog2 = parse_program(&text).expect("reparse unparsed pattern");
    assert!(
        matches!(prog2.lookup("Holds"), Some(Def::Pattern(_))),
        "pattern must survive parse → unparse → parse"
    );
}
