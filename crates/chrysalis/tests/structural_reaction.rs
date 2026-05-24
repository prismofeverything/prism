//! End-to-end: a STRUCTURAL chrysalis reaction fired by prism's BRS.
//!
//! Unlike grow/divide (a *computed* reactum — `?cell.divide(?cid)`), this
//! is a Pattern→Pattern rewrite: `Compartment(substrate: ERK | rest) =>
//! Compartment(substrate: pERK | rest)`. It exercises the structural-reactum
//! path (`Reactum::Structural` → prism's native `instantiate`/`fire_rule_at`,
//! with the `rest` site carried through by identity instantiation) running on
//! prism's `BigraphicalReactiveSystem`. This is the capability MAPK (#2)
//! needs, isolated from link-var lowering. There is no chrysalis-side BRS.

use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::Value;

use chrysalis::ast::{
    CompositeDef, Def, Expr, Interface, Param, PortDecl, Program, ReactionDef, SchemaExpr,
    StringLit,
};

/// `Compartment(substrate: <sub> | rest: ?rest)` as a reaction-side term.
fn compartment(sub_control: &str) -> Expr {
    Expr::term("Compartment")
        .body(Expr::parallel(vec![
            Expr::entry("substrate", Expr::term(sub_control).build()),
            Expr::entry("rest", Expr::site("?rest")),
        ]))
        .build()
}

fn program() -> Program {
    let mut p = Program::new();

    // reaction Phos ( {comp: Compartment(substrate: ERK | rest: ?rest)}
    //                 => {comp: Compartment(substrate: pERK | rest: ?rest)} )
    p.push(Def::Reaction(ReactionDef {
        name: "Phos".into(),
        params: vec![],
        redex: Expr::parallel(vec![Expr::entry("comp", compartment("ERK"))]),
        reactum: Expr::parallel(vec![Expr::entry("comp", compartment("pERK"))]),
        guard: None,
        rate: None,
    }));

    // composite Lab[compartments: Map] ~{} ->{compartments}
    // ( compartments: compartments |
    //   brs: BRS[rules: [Phos]] ~{state: compartments} ->{state: compartments} )
    p.push(Def::Composite(CompositeDef {
        name: "Lab".into(),
        params: vec![Param::required(
            "compartments",
            SchemaExpr::map_of(SchemaExpr::Any),
        )],
        interface: Interface::new().with_output(
            "compartments",
            PortDecl::required(SchemaExpr::map_of(SchemaExpr::Any)),
        ),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("compartments", Expr::var("compartments")),
            Expr::entry(
                "brs",
                Expr::term("BRS")
                    .arg_named("rules", Expr::List(vec![Expr::term("Phos").build()]))
                    .input("state", Expr::var("compartments"))
                    .output("state", Expr::var("compartments"))
                    .build(),
            ),
        ]),
    }));

    // main = Lab[compartments: {cyto: Compartment(mek: MEK | erk1: ERK | erk2: ERK)}]
    let cyto = Expr::term("Compartment")
        .body(Expr::parallel(vec![
            Expr::entry("mek", Expr::term("MEK").build()),
            Expr::entry("erk1", Expr::term("ERK").build()),
            Expr::entry("erk2", Expr::term("ERK").build()),
        ]))
        .build();
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: Expr::term("Lab")
            .arg_named(
                "compartments",
                Expr::Map(vec![(StringLit::plain("cyto"), cyto)]),
            )
            .build(),
    });

    p
}

/// Count children of `compartments.cyto` whose `_type` equals `ty`.
fn count_type(state: &Value, ty: &str) -> usize {
    state
        .as_map()
        .and_then(|m| m.get("compartments"))
        .and_then(|v| v.as_map())
        .and_then(|m| m.get("cyto"))
        .and_then(|v| v.as_map())
        .map(|m| {
            m.iter()
                .filter(|(k, v)| {
                    !k.starts_with('_') && v.get_field("_type").and_then(|t| t.as_str()) == Some(ty)
                })
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn structural_reaction_phosphorylates_through_prism_brs() {
    let program = program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    // Sanity: two free ERKs, one MEK, before running.
    assert_eq!(
        count_type(&result.initial_state, "ERK"),
        2,
        "two ERKs initially"
    );
    assert_eq!(count_type(&result.initial_state, "MEK"), 1);
    assert_eq!(count_type(&result.initial_state, "pERK"), 0);

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();

    // Deterministic BRS fires one match per tick; a few ticks convert both
    // ERKs. The structural rewrite preserves the rest (MEK + the other ERK).
    engine.run(5.0);

    let final_state = engine.state();
    assert_eq!(
        count_type(final_state, "pERK"),
        2,
        "both ERKs structurally rewritten to pERK"
    );
    assert_eq!(count_type(final_state, "ERK"), 0, "no free ERK remains");
    assert_eq!(
        count_type(final_state, "MEK"),
        1,
        "the MEK bystander is carried through the rewrite (rest-capture)"
    );
}

// ── Link-graph bond: free `!` ports → a shared `~bond` (MAPK's phospho) ──

/// `K ~{out: <link>}` — a control with one link port `out`. `link` is
/// `Expr::Unbound` (`!`) or `Expr::LinkVar` (`~bond`).
fn ion(control: &str, link: Expr) -> Expr {
    Expr::term(control).input("out", link).build()
}

/// `Compartment(enzyme: MEK ~{out} | substrate: <sub> ~{out} | rest: ?rest)`.
fn complex(sub: &str, link: fn() -> Expr) -> Expr {
    Expr::term("Compartment")
        .body(Expr::parallel(vec![
            Expr::entry("enzyme", ion("MEK", link())),
            Expr::entry("substrate", ion(sub, link())),
            Expr::entry("rest", Expr::site("?rest")),
        ]))
        .build()
}

fn phospho_program() -> Program {
    let mut p = Program::new();

    // reaction Phosphorylate (
    //   {comp: Compartment(MEK ~{out: !} | ERK ~{out: !} | rest: ?rest)}
    //   => {comp: Compartment(MEK ~{out: ~bond} | pERK ~{out: ~bond} | rest: ?rest)} )
    p.push(Def::Reaction(ReactionDef {
        name: "Phosphorylate".into(),
        params: vec![],
        redex: Expr::parallel(vec![Expr::entry("comp", complex("ERK", || Expr::Unbound))]),
        reactum: Expr::parallel(vec![Expr::entry(
            "comp",
            complex("pERK", || Expr::LinkVar("bond".into())),
        )]),
        guard: None,
        rate: None,
    }));

    p.push(Def::Composite(CompositeDef {
        name: "Lab".into(),
        params: vec![Param::required(
            "compartments",
            SchemaExpr::map_of(SchemaExpr::Any),
        )],
        interface: Interface::new().with_output(
            "compartments",
            PortDecl::required(SchemaExpr::map_of(SchemaExpr::Any)),
        ),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("compartments", Expr::var("compartments")),
            Expr::entry(
                "brs",
                Expr::term("BRS")
                    .arg_named(
                        "rules",
                        Expr::List(vec![Expr::term("Phosphorylate").build()]),
                    )
                    .input("state", Expr::var("compartments"))
                    .output("state", Expr::var("compartments"))
                    .build(),
            ),
        ]),
    }));

    // cyto holds a free MEK, a free ERK (the substrate), and a bystander ERK.
    let cyto = Expr::term("Compartment")
        .body(Expr::parallel(vec![
            Expr::entry("mek", Expr::term("MEK").build()),
            Expr::entry("erk1", Expr::term("ERK").build()),
            Expr::entry("erk2", Expr::term("ERK").build()),
        ]))
        .build();
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: Expr::term("Lab")
            .arg_named(
                "compartments",
                Expr::Map(vec![(StringLit::plain("cyto"), cyto)]),
            )
            .build(),
    });

    p
}

/// The `inputs.out` wire of the (single) node in cyto whose `_type` is `ty`.
fn link_of(state: &Value, ty: &str) -> Option<Value> {
    let cyto = state.as_map()?.get("compartments")?.as_map()?.get("cyto")?;
    cyto.as_map()?.iter().find_map(|(k, v)| {
        if k.starts_with('_') {
            return None;
        }
        if v.get_field("_type").and_then(|t| t.as_str()) == Some(ty) {
            v.get_field("inputs")
                .and_then(|i| i.get_field("out"))
                .cloned()
        } else {
            None
        }
    })
}

#[test]
fn link_bond_forms_through_prism_brs() {
    let program = phospho_program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    // Free MEK + free ERK have no `inputs.out` link initially.
    assert!(
        link_of(&result.initial_state, "MEK").is_none(),
        "MEK starts free"
    );

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();

    // One firing bonds one MEK·ERK pair (the redex requires BOTH free, so it
    // can't re-fire on the now-bonded pair — at most one firing regardless).
    engine.run(5.0);
    let final_state = engine.state();

    assert_eq!(count_type(final_state, "pERK"), 1, "one ERK phosphorylated");
    let mek_link = link_of(final_state, "MEK").expect("MEK now has an out link");
    let perk_link = link_of(final_state, "pERK").expect("pERK now has an out link");
    assert!(
        matches!(&mek_link, Value::List(l) if !l.is_empty()),
        "MEK's out port is wired to a freshly-minted edge, got {mek_link:?}"
    );
    assert_eq!(
        mek_link, perk_link,
        "MEK and pERK share the SAME `~bond` edge (one undirected link)"
    );
    // The bystander ERK is untouched (rest-carry) and still free.
    assert_eq!(count_type(final_state, "ERK"), 1, "bystander ERK remains");
}
