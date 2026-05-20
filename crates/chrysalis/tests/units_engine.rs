//! End-to-end: a units *context coercion* runs unit-correct in the live
//! engine, with the factor supplied by an enclosing composite's `using`.
//!
//! `Report` derives `excess = n - k_on`, where `n` is a Count and `k_on`
//! a Conc. Inside a `Cell` that scopes `concentration(volume: @.volume)`,
//! compile-time erasure rewrites the body to `n - k_on * volume` and
//! surfaces `volume` as an input port; the engine wires it to the cell's
//! volume slot and runs bare `f64`.
//!
//! Discriminator: n=40 in a 100 fL cell, threshold 0.5/fL ⇒
//!   excess = 40 - 0.5*100 = -10   (volume coercion ran)
//! Without the coercion it would be 40 - 0.5 = +39.5. Sign separates the
//! two regardless of how many ticks accumulate.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::Engine;

use chrysalis::ast::{
    CompositeDef, ContextDef, ContextRule, ContextUse, Def, Dimension, Expr, Interface, Param,
    PlacePath, PortDecl, ProcessDef, Program, Ratio, SchemaExpr, TermArg, UnitDef, UnitExpr,
};

fn count_t() -> SchemaExpr {
    SchemaExpr::quantity(UnitExpr::named("molecule"), true, false)
}
fn volume_t() -> SchemaExpr {
    SchemaExpr::quantity(UnitExpr::named("fL"), true, false)
}
fn conc_t() -> SchemaExpr {
    SchemaExpr::quantity(
        UnitExpr::named("molecule").div(UnitExpr::named("fL")),
        false,
        false,
    )
}

/// Push the shared prelude: units (um, fL, molecule), the `concentration`
/// context, and a `Report` process whose body coerces a Count against a
/// Conc threshold (`excess = n - k_on`).
fn push_prelude(p: &mut Program) {
    p.push(Def::Unit(UnitDef {
        name: "um".into(),
        dimension: Dimension::of(&[("length", 1)]),
        definition: UnitExpr::scalar(1e-6).mul(UnitExpr::named("m")),
        affine_offset: None,
    }));
    p.push(Def::Unit(UnitDef {
        name: "fL".into(),
        dimension: Dimension::of(&[("length", 3)]),
        definition: UnitExpr::named("um").pow(Ratio::int(3)),
        affine_offset: None,
    }));
    p.push(Def::Unit(UnitDef {
        name: "molecule".into(),
        dimension: Dimension::of(&[("substance", 1)]),
        definition: UnitExpr::named("mol").div(UnitExpr::scalar(6.02214076e23)),
        affine_offset: None,
    }));
    p.push(Def::Context(ContextDef {
        name: "concentration".into(),
        params: vec![Param::required("volume", volume_t())],
        rules: vec![ContextRule {
            from: Dimension::of(&[("substance", 1)]),
            to: Dimension::of(&[("substance", 1), ("length", -3)]),
            bidirectional: true,
            transform: Expr::div(Expr::var("value"), Expr::var("volume")),
        }],
    }));
    p.push(Def::Process(ProcessDef {
        name: "Report".into(),
        params: vec![Param::with_default("k_on", conc_t(), Expr::float(0.5))],
        interface: Interface::new()
            .with_input("n", PortDecl::required(count_t()))
            .with_output("excess", PortDecl::required(count_t())),
        body: Expr::Record(IndexMap::from_iter([(
            "excess".into(),
            Expr::sub(Expr::var("n"), Expr::var("k_on")),
        )])),
    }));
}

/// Build the program. `vol_slot` is the name of the cell's volume slot;
/// the `concentration` context's parameter is always `volume`, so when
/// `vol_slot != "volume"` this exercises `using`-path injection (the
/// factor name differs from the compartment slot).
fn program(n0: f64, vol_slot: &str) -> Program {
    let mut p = Program::new();
    push_prelude(&mut p);

    // composite Cell[volume: Volume = 100, n: Count = n0]
    //   using concentration(volume: @.volume)
    //   ~{} ->{n, volume, excess}
    // ( volume: volume | n: n | excess: 0.0 |
    //   Report[k_on: 0.5] ~{n: n} ->{excess: excess} )
    p.push(Def::Composite(CompositeDef {
        name: "Cell".into(),
        params: vec![
            Param::with_default(vol_slot, volume_t(), Expr::float(100.0)),
            Param::with_default("n", count_t(), Expr::float(n0)),
        ],
        interface: Interface::new()
            .with_output("n", PortDecl::required(count_t()))
            .with_output(vol_slot, PortDecl::required(volume_t()))
            .with_output("excess", PortDecl::required(count_t())),
        using: vec![ContextUse {
            // context param is `volume`; bound to the (possibly
            // differently-named) compartment slot.
            name: "concentration".into(),
            args: vec![TermArg::named(
                "volume",
                Expr::Path(PlacePath::here().dot(vol_slot)),
            )],
        }],
        body: Expr::parallel(vec![
            Expr::entry(vol_slot, Expr::var(vol_slot)),
            Expr::entry("n", Expr::var("n")),
            Expr::entry("excess", Expr::float(0.0)),
            Expr::entry(
                "report",
                Expr::term("Report")
                    .arg_named("k_on", Expr::float(0.5))
                    .input("n", Expr::var("n"))
                    .output("excess", Expr::var("excess"))
                    .build(),
            ),
        ]),
    }));

    p.push(Def::Binding {
        name: "main".into(),
        value: Expr::term("Cell").build(),
    });
    p
}

/// Compile, run in the engine, return the accumulated `excess` slot.
fn run_excess(n0: f64, vol_slot: &str) -> f64 {
    let prog = program(n0, vol_slot);
    let result = chrysalis::compile::compile(&prog).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(3.0);
    engine
        .state()
        .get_path(&["excess".into()])
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("no `excess` slot in final state: {:?}", engine.state()))
}

#[test]
fn context_coercion_runs_unit_correct_in_engine() {
    // Below threshold: 40 molecules in 100 fL = 0.4 < 0.5/fL.
    // excess = n - k_on*volume = 40 - 50 = -10 (per tick), i.e. NEGATIVE.
    // If units were ignored it would be 40 - 0.5 = +39.5 (positive).
    let below = run_excess(40.0, "volume");
    assert!(
        below < 0.0,
        "below-threshold excess must be negative if the volume coercion ran \
         (got {below}; ~+39.5 would mean units were ignored)"
    );

    // Above threshold: 60 molecules in 100 fL = 0.6 > 0.5/fL ⇒ positive.
    let above = run_excess(60.0, "volume");
    assert!(
        above > 0.0,
        "above-threshold excess should be positive (got {above})"
    );
}

#[test]
fn using_injection_handles_factor_name_differing_from_slot() {
    // The compartment slot is `cell_vol`, but the context parameter is
    // `volume`. Default same-name wiring (`volume` → `["volume"]`) would
    // find nothing; only the `using`-path injection
    // (`volume: @.cell_vol`) connects the factor — so a working result
    // proves the injection fired.
    let below = run_excess(40.0, "cell_vol");
    assert!(
        below < 0.0,
        "with slot `cell_vol`, the volume coercion must still run via \
         using-path injection (got {below})"
    );
    let above = run_excess(60.0, "cell_vol");
    assert!(above > 0.0, "above threshold should be positive (got {above})");
}

// ── Cross-boundary: a context activated by an ANCESTOR composite,
//    consumed by a process inside a child composite. ──

fn tissue_program(n0: f64) -> Program {
    let mut p = Program::new();
    push_prelude(&mut p);

    // composite Cell[n: Count] ~{} ->{n, excess}  (does NOT activate
    // the context). Report inside needs `volume` from the ancestor.
    p.push(Def::Composite(CompositeDef {
        name: "Cell".into(),
        params: vec![Param::with_default("n", count_t(), Expr::float(0.0))],
        interface: Interface::new()
            .with_output("n", PortDecl::required(count_t()))
            .with_output("excess", PortDecl::required(count_t())),
        using: vec![],
        body: Expr::parallel(vec![
            Expr::entry("n", Expr::var("n")),
            Expr::entry("excess", Expr::float(0.0)),
            Expr::entry(
                "report",
                Expr::term("Report")
                    .arg_named("k_on", Expr::float(0.5))
                    .input("n", Expr::var("n"))
                    .output("excess", Expr::var("excess"))
                    .build(),
            ),
        ]),
    }));

    // composite Tissue[volume: Volume = 100] using concentration(volume: @.volume)
    //   ~{} ->{volume, cell}  ( volume: volume | cell: Cell[n: n0] )
    p.push(Def::Composite(CompositeDef {
        name: "Tissue".into(),
        params: vec![Param::with_default("volume", volume_t(), Expr::float(100.0))],
        interface: Interface::new()
            .with_output("volume", PortDecl::required(volume_t()))
            .with_output("cell", PortDecl::required(SchemaExpr::custom("Cell"))),
        using: vec![ContextUse {
            name: "concentration".into(),
            args: vec![TermArg::named(
                "volume",
                Expr::Path(PlacePath::here().dot("volume")),
            )],
        }],
        body: Expr::parallel(vec![
            Expr::entry("volume", Expr::var("volume")),
            Expr::entry(
                "cell",
                Expr::term("Cell").arg_named("n", Expr::float(n0)).build(),
            ),
        ]),
    }));

    p.push(Def::Binding {
        name: "main".into(),
        value: Expr::term("Tissue").build(),
    });
    p
}

fn run_tissue_excess(n0: f64) -> f64 {
    let prog = tissue_program(n0);
    let result = chrysalis::compile::compile(&prog).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(3.0);
    // `Cell` exposes `excess` as an output, so it BRIDGES to its container
    // (the root) — the "updates out" half of the bridge. We observe it
    // there, the same legitimate bridged-output path the within-composite
    // tests use; reaching into `cell.config.state` would be illegitimate.
    engine
        .state()
        .get_path(&["excess".into()])
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("no excess in final state: {:?}", engine.state()))
}

#[test]
fn context_factor_crosses_composite_boundary() {
    // `Tissue` activates concentration(volume: @.volume); `Report` lives
    // inside the child `Cell` composite and needs that volume — it must be
    // threaded across the Cell sub-engine boundary. Below threshold:
    // 40 molecules / 100 fL = 0.4 < 0.5 ⇒ excess = 40 - 50 = -10 < 0.
    let below = run_tissue_excess(40.0);
    assert!(
        below < 0.0,
        "ancestor-scoped volume must reach Report across the Cell boundary \
         (got {below}; 0.0 means it never arrived, +39.5 that units were ignored)"
    );
    let above = run_tissue_excess(60.0);
    assert!(above > 0.0, "above threshold should be positive (got {above})");
}
