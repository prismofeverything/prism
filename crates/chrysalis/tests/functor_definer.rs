//! The `functor` definer (categorical-core §4) — slice 1: parse + AST + round-trip.
//!
//! `functor Name :: Source -> Target ( Gen => construction, … )` is the surface for a
//! first-class **structure-preserving map** — each source generator maps to a construction
//! (a morphism) in the target prop, lifted by functoriality. render is the forcing consumer
//! (`OrganismBoard :: Colony -> Svg`). APPLYING a functor (the functorial lift) is a later
//! slice (lang ⋈ core); this proves the SURFACE parses + round-trips.

use chrysalis::ast::{Def, FunctorDef};
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;

fn the_functor(src: &str) -> FunctorDef {
    let prog = parse_program(src).expect("parses");
    prog.defs
        .iter()
        .find_map(|d| match d {
            Def::Functor(f) => Some(f.clone()),
            _ => None,
        })
        .expect("a functor def")
}

#[test]
fn parses_the_functor_definer() {
    let f = the_functor(
        "functor OrganismBoard :: Colony -> Svg (\n  \
         Cell => Circle[cx: x, cy: y, r: 5.0],\n  \
         Field => Rect[w: w, h: h],\n)\n",
    );
    assert_eq!(f.name.as_str(), "OrganismBoard");
    assert_eq!(f.source.as_str(), "Colony", "the :: source prop");
    assert_eq!(f.target.as_str(), "Svg", "the -> target prop");
    assert_eq!(f.mappings.len(), 2, "two generator => construction mappings");
    assert_eq!(f.mappings[0].0.as_str(), "Cell");
    assert_eq!(f.mappings[1].0.as_str(), "Field");
}

#[test]
fn functor_round_trips_through_unparse() {
    // parse → unparse → parse preserves the functor (the homoiconic round-trip).
    let src = "functor View :: Cells -> Svg (\n  Cell => Circle[r: 1.0],\n)\n";
    let prog = parse_program(src).expect("parses");
    let reparsed = parse_program(&unparse(&prog)).expect("re-parses the unparse");
    let f = reparsed
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Functor(f) => Some(f),
            _ => None,
        })
        .expect("functor survives the round-trip");
    assert_eq!(f.name.as_str(), "View");
    assert_eq!(f.source.as_str(), "Cells");
    assert_eq!(f.target.as_str(), "Svg");
    assert_eq!(f.mappings.len(), 1);
    assert_eq!(f.mappings[0].0.as_str(), "Cell");
}

#[test]
fn functor_is_contextual_not_a_reserved_word() {
    // `functor` is CONTEXTUAL (only `functor <Name>` is the definer): `def functor = …`
    // and a `functor:` field key stay ordinary, so adding the definer broke no `.ys`.
    let prog = parse_program("def functor = 3.0\n").expect("`functor` usable as a name");
    assert!(
        prog.defs
            .iter()
            .any(|d| matches!(d, Def::Binding { name, .. } if name == "functor")),
        "`def functor = …` parses as a binding, not the definer"
    );
}

// ── slice 2: APPLY — the functorial lift, through core's `prism_schema::functor` ──

#[test]
fn apply_functor_lifts_a_relabel_over_a_bigraph() {
    // A RELABEL functor (core proved relabel converges one-pass): `Foo => a Bar node`.
    // `apply_functor(F, state)` builds a `RuleFunctor` from the mappings (each
    // construction → a reactum `Pattern` via `eval_pattern`, then core's `functor_rule`)
    // and calls prism's `apply_functor` — THIN: lang builds + calls, the BRS-to-fixpoint
    // (fire-once / endofunctor-safe) is core's. The Foo node becomes a Bar; `n` is kept.
    use std::sync::Arc;

    use chrysalis::eval::Evaluator;
    use indexmap::IndexMap;
    use prism_schema::MethodRegistry;

    let src = "functor Relabel :: Things -> Things (\n  \
               Foo => {_type: 'Bar'},\n)\n\
               apply_functor(Relabel, {x: {_type: 'Foo', n: 1.0}})\n";
    let prog = parse_program(src).expect("parses the functor + trailing apply_functor");
    let main = prog
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Binding { name, value, .. } if name == "main" => Some(value.clone()),
            _ => None,
        })
        .expect("the trailing `apply_functor(…)` is the `main` value");

    let ev = Evaluator::new(Arc::new(prog), Arc::new(MethodRegistry::new()));
    let result = ev
        .eval_value(&main, &IndexMap::new())
        .expect("apply_functor evaluates through core's lift");

    let s = format!("{result:?}");
    assert!(s.contains("Bar"), "the Foo node was relabelled to Bar: {s}");
    assert!(!s.contains("Foo"), "no Foo survives the lift (fire-once relabel): {s}");
}

