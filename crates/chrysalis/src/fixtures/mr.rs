//! Rosen (M,R) closure as a hand-built chrysalis AST — tier-1 benchmark #3.
//!
//! The homoiconic point: reactions match on **shared blueprint** between two
//! co-located ions, and mechanism *lineage* flows through data — a new F's
//! blueprint is the one its reactants shared. The cycle:
//!
//! - **MakeB**  `F | A      => F | B`            (F catalyses A → B)
//! - **MakePhi** `F | B  (same blueprint) => F | Phi`   (B becomes φ; F persists)
//! - **MakeF**  `Phi | B (same blueprint) => F`         (φ + B regenerate F)
//!
//! Each reactant pair is an **anonymous parallel** (`?f::F | ?b::B`) matched
//! as a sub-multiset of the soup by prism's matcher; the shared blueprint is
//! a `where ?x.blueprint == ?y.blueprint` guard (as-patterns capture the two
//! whole ions under distinct names, so their `blueprint` fields don't
//! collide). Reactums *construct* the products from the captured blueprint —
//! the rule never hard-codes a recipe, so lineage is carried structurally.
//!
//! This is the focused realisation that exercises the tier-1 property
//! (reactions-as-values matching on shared data + lineage); the design doc's
//! richer form splits F into a `process` and φ into a `step`.

use indexmap::IndexMap;

use prism_schema::Value;

use crate::ast::{BinOp, Def, Expr, PlacePath, Program, ReactionDef};

// ── reaction-side helpers ────────────────────────────────────────────

/// An as-pattern reactant: `?name::Control` — binds the whole matched ion
/// to `?name` (distinct from any field site).
fn reactant(name: &str, control: &str) -> Expr {
    Expr::site_typed(name, Expr::term(control).build())
}

/// `?name.blueprint` — the blueprint field of a captured ion.
fn blueprint_of(name: &str) -> Expr {
    Expr::Path(PlacePath::local(name).dot("blueprint"))
}

/// `?a.blueprint == ?b.blueprint` — the shared-blueprint guard.
fn same_blueprint(a: &str, b: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(blueprint_of(a)),
        rhs: Box::new(blueprint_of(b)),
    }
}

/// A constructed product ion `Control[blueprint: <bp expr>, extra…]`.
fn product(control: &str, bp: Expr, extra: Vec<(&str, Expr)>) -> Expr {
    let mut t = Expr::term(control).arg_named("blueprint", bp);
    for (k, v) in extra {
        t = t.arg_named(k, v);
    }
    t.build()
}

fn rxn(name: &str, redex: Expr, guard: Option<Expr>, reactum: Expr, rate: f64) -> ReactionDef {
    ReactionDef {
        name: name.into(),
        params: vec![],
        redex,
        reactum,
        guard,
        rate: Some(Expr::float(rate)),
    }
}

// ── the three reactions ──────────────────────────────────────────────

/// `F | A => F | B[blueprint: F.blueprint, source: A]` — F catalyses an A
/// into a B carrying F's blueprint (lineage seeded into the product).
fn make_b() -> ReactionDef {
    rxn(
        "MakeB",
        Expr::parallel(vec![reactant("?f", "F"), reactant("?a", "A")]),
        None,
        Expr::parallel(vec![
            product("F", blueprint_of("?f"), vec![]),
            product("B", blueprint_of("?f"), vec![("source", Expr::var("?a"))]),
        ]),
        1.0,
    )
}

/// `F | B (same blueprint) => F | Phi` — a B and a same-lineage F make φ.
fn make_phi() -> ReactionDef {
    rxn(
        "MakePhi",
        Expr::parallel(vec![reactant("?f", "F"), reactant("?b", "B")]),
        Some(same_blueprint("?f", "?b")),
        Expr::parallel(vec![
            product("F", blueprint_of("?f"), vec![]),
            product("Phi", blueprint_of("?f"), vec![]),
        ]),
        0.3,
    )
}

/// `Phi | B (same blueprint) => F` — φ and a same-lineage B regenerate F.
fn make_f() -> ReactionDef {
    rxn(
        "MakeF",
        Expr::parallel(vec![reactant("?p", "Phi"), reactant("?b", "B")]),
        Some(same_blueprint("?p", "?b")),
        product("F", blueprint_of("?p"), vec![]),
        0.4,
    )
}

/// All three reactions, in cycle order.
pub fn all_reactions() -> Vec<ReactionDef> {
    vec![make_b(), make_phi(), make_f()]
}

/// The reaction names, in order.
pub fn rule_names() -> Vec<String> {
    all_reactions().into_iter().map(|r| r.name).collect()
}

// ── soup ions (for tests / initial state) ────────────────────────────

fn vstr(s: &str) -> Value {
    Value::String(s.to_string())
}

/// `{_type: F, blueprint: bp}`.
pub fn f_ion(bp: &str) -> Value {
    Value::tree([("_type", vstr("F")), ("blueprint", vstr(bp))])
}

/// `{_type: B, blueprint: bp}`.
pub fn b_ion(bp: &str) -> Value {
    Value::tree([("_type", vstr("B")), ("blueprint", vstr(bp))])
}

/// `{_type: Phi, blueprint: bp}`.
pub fn phi_ion(bp: &str) -> Value {
    Value::tree([("_type", vstr("Phi")), ("blueprint", vstr(bp))])
}

/// `{_type: A}` — raw material.
pub fn a_ion() -> Value {
    Value::tree([("_type", vstr("A"))])
}

/// Build a soup `Value::Map` from `(key, ion)` entries.
pub fn soup(entries: Vec<(&str, Value)>) -> Value {
    Value::tree(entries)
}

// ── program (reactions only; trivial main so it compiles) ─────────────

/// A program carrying the three reactions, for `compile` to register them
/// and hand back an evaluator. `main` is trivial — the soup is supplied by
/// tests directly to a `BigraphicalReactiveSystem`.
pub fn program() -> Program {
    let mut p = Program::new();
    for r in all_reactions() {
        p.push(Def::Reaction(r));
    }
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: Expr::Record(IndexMap::new()),
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_builds_with_three_reactions() {
        let p = program();
        for r in ["MakeB", "MakePhi", "MakeF"] {
            assert!(p.lookup(r).is_some(), "missing {r}");
        }
    }
}
