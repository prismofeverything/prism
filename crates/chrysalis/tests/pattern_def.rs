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
fn pattern_constructor_is_a_first_class_matcher() {
    // Stage 2b — the flat-kind parallel to `Reaction[…]`: `Pattern( fragment )`
    // reifies a redex fragment to a first-class `prism Pattern` value
    // (`Foreign(FOREIGN_PATTERN, …)`), lowered through the SAME `eval_pattern`
    // core a reaction redex uses. So `pattern X (frag)` ≡ `def X = Pattern(frag)`
    // (constructor = quote of definer), and the value is a real MATCHER — what a
    // pattern is FOR — the substrate for `count(…)`/`.matches(…)` query builtins
    // and composition into a `Reaction[…]`.
    use chrysalis::ast::{Expr, PortBindings};
    use chrysalis::runtime::rule::FOREIGN_PATTERN;
    use prism_schema::Value;
    use prism_schema::reaction::find_matches;

    let prog = Arc::new(parse_program("def here = 0.0").expect("parse"));
    let ev = Evaluator::new(prog, Arc::new(std_methods()));

    // `Pattern( ?c :: Cell )` — the fragment is the body.
    let frag = Expr::site_typed("?c", Expr::term("Cell").build());
    let ctor = Expr::Term {
        control: "Pattern".into(),
        args: vec![],
        ports: PortBindings::default(),
        body: Some(Box::new(frag.clone())),
    };
    let value = ev.eval_value(&ctor, &IndexMap::new()).expect("eval Pattern(…)");

    // (1) It is a `Foreign(FOREIGN_PATTERN, prism Pattern)`.
    let Value::Foreign(f) = &value else {
        panic!("Pattern(…) is a Foreign pattern value: {value:?}")
    };
    assert_eq!(f.type_name, FOREIGN_PATTERN, "tagged as a pattern value");
    let pattern: Pattern = f.downcast_ref::<Pattern>().expect("a prism Pattern").clone();

    // (2) It lowers IDENTICALLY to the fragment through `eval_pattern_top` — the
    //     SAME core a reaction's REDEX is lowered through (`build_rule`). So
    //     `Pattern(frag)` is `quote` of the definer in the matchable-redex form (a
    //     bare site is wrapped into a container `Map`/`_rest`, not left a free
    //     `Bind` that `find_matches` can't anchor).
    let mut b = RuleBindings::new();
    let direct = ev
        .eval_pattern_top(&frag, &IndexMap::new(), &mut b)
        .expect("lower fragment directly");
    assert_eq!(pattern, direct, "Pattern(frag) ≡ the redex `eval_pattern_top` lowering");

    // (3) It is a real MATCHER: a state holding a Cell is found.
    let state = Value::tree([("c0", Value::tree([("_type", Value::String("Cell".into()))]))]);
    assert!(
        !find_matches(&state, &pattern, None).is_empty(),
        "the constructed pattern matched a Cell"
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
