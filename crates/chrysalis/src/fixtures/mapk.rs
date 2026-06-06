//! MAPK signalling as a hand-built chrysalis AST — tier-1 benchmark #2.
//!
//! A faithful port of `crates/prism-mapk/src/{rules,state}.rs`: seven
//! structural reaction rules over a nested-compartment cell, exercising
//! pattern composition, link variables (`~bond`), free ports (`!`), deep
//! nesting (Cytoplasm > Nucleus / ERLumen), and rest-carry. Every rule is
//! a STRUCTURAL `Reactum` — it lowers to a `prism_schema::Pattern` reactum
//! fired by prism's native `instantiate`/`fire_rule_at` on prism's
//! `BigraphicalReactiveSystem`. There is no chrysalis BRS.
//!
//! Link encoding: a bond is one undirected edge. We write link ports with
//! `~{out: …}` (the in/out split is a *sort*, both matchable); `!` is a free
//! port (no link), `~bond` a shared link variable. chrysalis lowers an
//! all-`!` side to `Absent` (matches a node with no link) and a bound side
//! to `{out: LinkVar}` (the matcher binds `bond` by value across the two
//! ions, so the same edge is enforced). See `eval::link_side`.

use prism_schema::Value;

use crate::ast::{
    CompositeDef, Def, Expr, Interface, Param, PatternDef, PortDecl, Program, ReactionDef,
    SchemaExpr,
};

// ── pattern-context ion helpers ──────────────────────────────────────

/// A free link port: `K ~{out: !}` → `sort(K, {inputs: Absent})` (matches
/// a node with no `out` link at all).
fn free(control: &str) -> Expr {
    Expr::term(control).input("out", Expr::Unbound).build()
}

/// A free, named ion: `K[name: ?n] ~{out: !}`.
fn free_named(control: &str) -> Expr {
    Expr::term(control)
        .arg_named("name", Expr::site("?n"))
        .input("out", Expr::Unbound)
        .build()
}

/// A bonded link port: `K ~{out: ~bond}` → `sort(K, {inputs: {out:
/// LinkVar(bond)}})`. The shared `bond` is one undirected edge.
fn bonded(control: &str) -> Expr {
    Expr::term(control)
        .input("out", Expr::LinkVar("bond".into()))
        .build()
}

/// A bonded, named ion: `K[name: ?n] ~{out: ~bond}`.
fn bonded_named(control: &str) -> Expr {
    Expr::term(control)
        .arg_named("name", Expr::site("?n"))
        .input("out", Expr::LinkVar("bond".into()))
        .build()
}

/// A bare ion with no link constraint: `K` → `sort(K, {})`. As a reactum
/// ion this produces a free node (no link field).
fn plain(control: &str) -> Expr {
    Expr::term(control).build()
}

/// A named ion with no link constraint: `K[name: ?n]` → `sort(K, {name:
/// Site})`. Carries `name` through by identity instantiation.
fn named(control: &str) -> Expr {
    Expr::term(control)
        .arg_named("name", Expr::site("?n"))
        .build()
}

/// `Compartment(child: … | child: …)` from keyed children.
fn comp(children: Vec<(&str, Expr)>) -> Expr {
    Expr::term("Compartment")
        .body(Expr::parallel(
            children
                .into_iter()
                .map(|(k, v)| Expr::entry(k, v))
                .collect(),
        ))
        .build()
}

/// `{key: term}` — the single-keyed outer map a redex/reactum matches at.
fn wrap(key: &str, term: Expr) -> Expr {
    Expr::parallel(vec![Expr::entry(key, term)])
}

fn rxn(name: &str, redex: Expr, reactum: Expr, rate: f64) -> ReactionDef {
    ReactionDef {
        name: name.into(),
        params: vec![],
        redex,
        reactum,
        guard: None,
        rate: Some(Expr::float(rate)),
    }
}

// ── shared redex/reactum FRAGMENTS as `pattern`s (the dedup) ──────────
// Five named fragments factor the seven rules' compartment shapes, so each
// shape is written ONCE and referenced, not inlined per reaction:
//   Catalysis  — a flat compartment: enzyme | substrate | bystanders
//   CytoOuter  — substrate in the cytoplasm, a plain (kindless) inner comp
//   CytoInner  — substrate in a plain (kindless) inner compartment
//   NucCyto    — substrate in the cytoplasm, a Nucleus inner compartment
//   NucInner   — substrate in the Nucleus
// Pattern expansion (`eval_pattern_term`) substitutes the args + splices.

fn pat(name: &str, params: &[&str], body: Expr) -> Def {
    Def::Pattern(PatternDef {
        name: name.into(),
        params: params
            .iter()
            .map(|p| Param::required(*p, SchemaExpr::Any))
            .collect(),
        body,
    })
}

/// A pattern reference `Name[param: arg, …]` (named args bind by param name).
fn use_pat(name: &str, args: Vec<(&str, Expr)>) -> Expr {
    let mut tb = Expr::term(name);
    for (k, v) in args {
        tb = tb.arg_named(k, v);
    }
    tb.build()
}

/// The five shared fragments, pushed into the program ahead of the rules.
fn patterns() -> Vec<Def> {
    let inner_rest = || ("inner_rest", Expr::site("?inner_rest"));
    let outer_rest = || ("outer_rest", Expr::site("?outer_rest"));
    let cyto = || ("kind", plain("Cytoplasm"));
    let nuc = || ("kind", plain("Nucleus"));
    vec![
        pat(
            "Catalysis",
            &["enzyme", "substrate"],
            comp(vec![
                ("enzyme", Expr::var("enzyme")),
                ("substrate", Expr::var("substrate")),
                ("bystanders", Expr::site("?rest")),
            ]),
        ),
        pat(
            "CytoOuter",
            &["substrate"],
            comp(vec![
                cyto(),
                ("substrate", Expr::var("substrate")),
                ("inner", comp(vec![inner_rest()])),
                outer_rest(),
            ]),
        ),
        pat(
            "CytoInner",
            &["substrate"],
            comp(vec![
                cyto(),
                (
                    "inner",
                    comp(vec![("substrate", Expr::var("substrate")), inner_rest()]),
                ),
                outer_rest(),
            ]),
        ),
        pat(
            "NucCyto",
            &["substrate"],
            comp(vec![
                cyto(),
                ("substrate", Expr::var("substrate")),
                ("inner", comp(vec![nuc(), inner_rest()])),
                outer_rest(),
            ]),
        ),
        pat(
            "NucInner",
            &["substrate"],
            comp(vec![
                cyto(),
                (
                    "inner",
                    comp(vec![nuc(), ("substrate", Expr::var("substrate")), inner_rest()]),
                ),
                outer_rest(),
            ]),
        ),
    ]
}

// ── the seven rules (mirroring prism-mapk/src/rules.rs) ───────────────

/// Free MEK + free ERK in a compartment form the MEK·pERK complex (a
/// shared bond), phosphorylating ERK → pERK.
fn phosphorylate() -> ReactionDef {
    rxn(
        "phosphorylate",
        wrap(
            "compartment",
            use_pat(
                "Catalysis",
                vec![("enzyme", free("MEK")), ("substrate", free_named("ERK"))],
            ),
        ),
        wrap(
            "compartment",
            use_pat(
                "Catalysis",
                vec![("enzyme", bonded("MEK")), ("substrate", bonded_named("pERK"))],
            ),
        ),
        2.0,
    )
}

/// The MEK·pERK complex dissociates back to free MEK + free pERK.
fn dissociate() -> ReactionDef {
    rxn(
        "dissociate",
        wrap(
            "compartment",
            use_pat(
                "Catalysis",
                vec![("enzyme", bonded("MEK")), ("substrate", bonded_named("pERK"))],
            ),
        ),
        wrap(
            "compartment",
            use_pat(
                "Catalysis",
                vec![("enzyme", plain("MEK")), ("substrate", named("pERK"))],
            ),
        ),
        0.5,
    )
}

/// Nuclear pERK is dephosphorylated back to ERK.
fn dephosphorylate() -> ReactionDef {
    rxn(
        "dephosphorylate",
        wrap("outer", use_pat("NucInner", vec![("substrate", free_named("pERK"))])),
        wrap("outer", use_pat("NucInner", vec![("substrate", named("ERK"))])),
        0.4,
    )
}

/// Free ERK descends from cytoplasm into a child compartment.
fn translocate_erk_in() -> ReactionDef {
    rxn(
        "translocate_erk_in",
        wrap("outer", use_pat("CytoOuter", vec![("substrate", named("ERK"))])),
        wrap("outer", use_pat("CytoInner", vec![("substrate", named("ERK"))])),
        1.0,
    )
}

/// Free ERK ascends from a child compartment back into cytoplasm.
fn translocate_erk_out() -> ReactionDef {
    rxn(
        "translocate_erk_out",
        wrap("outer", use_pat("CytoInner", vec![("substrate", named("ERK"))])),
        wrap("outer", use_pat("CytoOuter", vec![("substrate", named("ERK"))])),
        1.0,
    )
}

/// Active nuclear import of free phospho-ERK (fast).
fn translocate_perk_in() -> ReactionDef {
    rxn(
        "translocate_perk_in",
        wrap("outer", use_pat("NucCyto", vec![("substrate", free_named("pERK"))])),
        wrap("outer", use_pat("NucInner", vec![("substrate", named("pERK"))])),
        2.0,
    )
}

/// Slow nuclear export / leak of free phospho-ERK.
fn translocate_perk_out() -> ReactionDef {
    rxn(
        "translocate_perk_out",
        wrap("outer", use_pat("NucInner", vec![("substrate", free_named("pERK"))])),
        wrap("outer", use_pat("NucCyto", vec![("substrate", named("pERK"))])),
        0.1,
    )
}

/// All seven rules, in the same order as `prism_mapk::mapk_rules`.
pub fn all_reactions() -> Vec<ReactionDef> {
    vec![
        phosphorylate(),
        dissociate(),
        dephosphorylate(),
        translocate_erk_in(),
        translocate_erk_out(),
        translocate_perk_in(),
        translocate_perk_out(),
    ]
}

/// The names of the seven rules, in order.
pub fn rule_names() -> Vec<String> {
    all_reactions().into_iter().map(|r| r.name).collect()
}

// ── initial state (mirroring prism-mapk/src/state.rs) ─────────────────

fn vstr(s: &str) -> Value {
    Value::String(s.to_string())
}

fn erk(name: &str) -> Value {
    Value::tree([("_type", vstr("ERK")), ("name", vstr(name))])
}

fn kind_node(k: &str) -> Value {
    Value::tree([("_type", vstr(k))])
}

/// The nested-compartment cell: MEK + ERK/pERK in the cytoplasm, an empty
/// nucleus, and an ER lumen holding one ERK. Three ERK + one pERK = 4 total.
pub fn initial_state() -> Value {
    Value::tree([
        ("_type", vstr("Cell")),
        (
            "cytoplasm",
            Value::tree([
                ("_type", vstr("Compartment")),
                ("kind", kind_node("Cytoplasm")),
                ("mek", kind_node("MEK")),
                (
                    "erk0",
                    Value::tree([("_type", vstr("pERK")), ("name", vstr("erk0"))]),
                ),
                ("erk1", erk("erk1")),
                ("erk2", erk("erk2")),
                (
                    "nucleus",
                    Value::tree([
                        ("_type", vstr("Compartment")),
                        ("kind", kind_node("Nucleus")),
                    ]),
                ),
                (
                    "er_lumen",
                    Value::tree([
                        ("_type", vstr("Compartment")),
                        ("kind", kind_node("ERLumen")),
                        ("erk3", erk("erk3")),
                    ]),
                ),
            ]),
        ),
    ])
}

// ── full program (compile → engine path) ─────────────────────────────

/// `composite Mapk[cell] ~{} ->{cell}` holding the cell state plus a BRS of
/// the seven rules wired to it, in Gillespie mode (seed 7).
fn mapk_composite() -> CompositeDef {
    CompositeDef {
        name: "Mapk".into(),
        params: vec![Param::required("cell", SchemaExpr::map_of(SchemaExpr::Any))],
        interface: Interface::new().with_output(
            "cell",
            PortDecl::required(SchemaExpr::map_of(SchemaExpr::Any)),
        ),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("cell", Expr::var("cell")),
            Expr::entry(
                "brs",
                Expr::term("BRS")
                    .arg_named(
                        "rules",
                        Expr::List(rule_names().iter().map(|n| Expr::term(n).build()).collect()),
                    )
                    .arg_named("mode", Expr::string("gillespie"))
                    .arg_named("seed", Expr::int(7))
                    .arg_named("max_per_tick", Expr::int(10_000))
                    .input("state", Expr::var("cell"))
                    .output("state", Expr::var("cell"))
                    .build(),
            ),
        ]),
    }
}

/// `main = Mapk[cell: <the nested cell, built in chrysalis value syntax>]`.
fn main_expr() -> Expr {
    let erk_v = |name: &str| {
        Expr::term("ERK")
            .arg_named("name", Expr::string(name))
            .build()
    };
    let kind_v = |k: &str| Expr::term(k).build();
    let cell = Expr::term("Cell")
        .body(Expr::parallel(vec![Expr::entry(
            "cytoplasm",
            Expr::term("Compartment")
                .body(Expr::parallel(vec![
                    Expr::entry("kind", kind_v("Cytoplasm")),
                    Expr::entry("mek", kind_v("MEK")),
                    Expr::entry(
                        "erk0",
                        Expr::term("pERK")
                            .arg_named("name", Expr::string("erk0"))
                            .build(),
                    ),
                    Expr::entry("erk1", erk_v("erk1")),
                    Expr::entry("erk2", erk_v("erk2")),
                    Expr::entry(
                        "nucleus",
                        Expr::term("Compartment")
                            .body(Expr::parallel(vec![Expr::entry("kind", kind_v("Nucleus"))]))
                            .build(),
                    ),
                    Expr::entry(
                        "er_lumen",
                        Expr::term("Compartment")
                            .body(Expr::parallel(vec![
                                Expr::entry("kind", kind_v("ERLumen")),
                                Expr::entry("erk3", erk_v("erk3")),
                            ]))
                            .build(),
                    ),
                ]))
                .build(),
        )]))
        .build();
    Expr::term("Mapk").arg_named("cell", cell).build()
}

/// Build the complete MAPK chrysalis program (reactions + composite + main).
pub fn program() -> Program {
    let mut p = Program::new();
    for d in patterns() {
        p.push(d);
    }
    for r in all_reactions() {
        p.push(Def::Reaction(r));
    }
    p.push(Def::Composite(mapk_composite()));
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: main_expr(),
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_builds_with_seven_rules() {
        let p = program();
        for r in rule_names() {
            assert!(p.lookup(&r).is_some(), "missing rule {r}");
        }
        assert!(p.lookup("Mapk").is_some());
        assert!(p.lookup("main").is_some());
    }

    #[test]
    fn initial_state_has_four_erk_perk() {
        fn count(v: &Value, c: &mut usize) {
            if let Some(it) = v.iter_fields() {
                for (k, child) in it {
                    if k.starts_with('_') {
                        continue;
                    }
                    if matches!(
                        child.get_field("_type").and_then(|t| t.as_str()),
                        Some("ERK") | Some("pERK")
                    ) {
                        *c += 1;
                    }
                    count(child, c);
                }
            }
        }
        let mut c = 0;
        count(&initial_state(), &mut c);
        assert_eq!(c, 4, "3 ERK + 1 pERK");
    }
}
