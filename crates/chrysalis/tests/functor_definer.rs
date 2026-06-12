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
