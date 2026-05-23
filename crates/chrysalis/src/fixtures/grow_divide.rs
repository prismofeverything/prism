//! Grow/divide example as a hand-built chrysalis AST — **internal division**.
//!
//! Surface form lives at
//! [`crates/chrysalis/ys/grow-divide-unbounded.ys`](../../ys/grow-divide-unbounded.ys).
//! This module is the parser-free hand-built equivalent until task #12
//! (the parser) lands.
//!
//! ## Why internal division
//!
//! A `Cell` is a real encapsulated subengine (`from_config`). Its `mass`
//! lives inside `config.state`, so an *external* BRS reaction at the parent
//! cannot see it to match `Cell[mass > threshold]`. That external form was
//! idealistic (it's the homoiconic Step-2 target; see the `.ys`). The
//! PROVEN pattern — a faithful port of upstream
//! `process_bigraph/processes/growth_division.py`, verified in
//! `crates/prism-bigraph/tests/growth_division.rs` — is **internal**: a
//! `Divide` step *inside* each cell reads the cell's own mass and writes
//! daughters UP to the parent via the bridge.
//!
//! ## Wiring (mirrors growth_division.rs verbatim)
//!
//! - inner `Grow`:   `~{mass: ["mass"]} ->{mass: ["mass"]}`   (sibling slot)
//! - inner `Divide`: `~{trigger: ["mass"]} ->{environment: ["environment"]}`
//! - the cell has **no** inner `environment` slot, so the bridge port
//!   `environment → ["environment"]` is a *passthrough*: the raw
//!   `{_remove, _add}` delta crosses to the parent intact (true division —
//!   the parent cell is removed, two daughters added).
//! - cell top-level output: `->{environment: @}` → `[]`. An empty wire
//!   resolves to the process's CONTAINER (the engine is container-relative),
//!   i.e. the `cells` map the cell lives in. This is exactly
//!   growth_division.rs's `wire(&[])`. (`^` → `[".."]` would pop one level
//!   too high — see docs/chrysalis-design.md.)
//!
//! Surface form (target):
//!
//! ```text
//! process Grow[rate: Float = 0.02]
//!   ~{mass: Float, interval: Float = 1.0}
//!   ->{mass: Float}
//! ( delta = mass * rate * interval | {mass: delta} )
//!
//! step Divide[id: String, threshold: Float = 2.0]
//!   ~{trigger: Float}
//!   ->{environment: Map[Cell]}
//! ( { environment:
//!       if trigger > threshold
//!         then replace id with { '{id}_0': Cell[id: '{id}_0', mass: trigger / 2],
//!                                '{id}_1': Cell[id: '{id}_1', mass: trigger / 2] }
//!         else {} } )
//!
//! composite Cell[id: String, mass: Float = 1.0,
//!                growth_rate: Float = 0.02, threshold: Float = 2.0]
//!   ~{}
//!   ->{environment}
//! (
//!   mass: mass |
//!   Grow[rate: growth_rate] ~{mass: mass} ->{mass: mass} |
//!   Divide[id: id, threshold: threshold] ~{trigger: mass} ->{environment: environment}
//! )
//!
//! composite Environment[cells: Map[Cell]] ~{} ->{cells} ( cells: cells )
//!
//! main = Environment[cells: {'0': Cell[id: '0', mass: 1.2] ->{environment: @}}]
//! ```

use indexmap::IndexMap;

use crate::ast::{
    Block, CompositeDef, Def, Expr, Interface, Param, PlacePath, PortDecl, Program, SchemaExpr,
    StepDef, StringLit, StringSeg,
};

/// Build the complete grow/divide chrysalis program.
pub fn program() -> Program {
    let mut p = Program::new();
    p.push(Def::Process(grow_def()));
    p.push(Def::Step(divide_def()));
    p.push(Def::Composite(cell_def()));
    p.push(Def::Composite(environment_def()));
    p.push(Def::Binding {
        name: "main".into(),
        schema: None,
        value: main_expr(),
    });
    p
}

// ── helpers ─────────────────────────────────────────────────────────

/// `@` — the empty wire `[]`, which resolves to the process's container.
fn here() -> Expr {
    Expr::Path(PlacePath::here())
}

/// `'{id}{suffix}'` — a string template over the in-scope `id` variable.
fn id_template(suffix: &str) -> StringLit {
    StringLit::template(vec![
        StringSeg::Expr(Expr::var("id")),
        StringSeg::Lit(suffix.into()),
    ])
}

/// A daughter cell: `Cell[id: '{id}{suffix}', mass: trigger / 2,
/// threshold: threshold] ->{environment: @}`.
fn daughter(suffix: &str) -> Expr {
    Expr::term("Cell")
        .arg_named("id", Expr::Str(id_template(suffix)))
        .arg_named("mass", Expr::div(Expr::var("trigger"), Expr::float(2.0)))
        .arg_named("threshold", Expr::var("threshold"))
        .output("environment", here())
        .build()
}

// ── process Grow ────────────────────────────────────────────────────

fn grow_def() -> crate::ast::ProcessDef {
    let params = vec![Param::with_default(
        "rate",
        SchemaExpr::Float,
        Expr::float(0.02),
    )];

    let interface = Interface::new()
        .with_input("mass", PortDecl::required(SchemaExpr::Float))
        .with_input(
            "interval",
            PortDecl::with_default(SchemaExpr::Float, Expr::float(1.0)),
        )
        .with_output("mass", PortDecl::required(SchemaExpr::Float));

    // Body:
    //   delta = mass * rate * interval |
    //   {mass: delta}
    let body = Expr::Block(Block::from_parts(
        vec![(
            "delta".into(),
            Expr::mul(
                Expr::mul(Expr::var("mass"), Expr::var("rate")),
                Expr::var("interval"),
            ),
        )],
        Expr::Record(IndexMap::from_iter([("mass".into(), Expr::var("delta"))])),
    ));

    crate::ast::ProcessDef {
        name: "Grow".into(),
        params,
        interface,
        body,
    }
}

// ── step Divide ─────────────────────────────────────────────────────

fn divide_def() -> StepDef {
    let params = vec![
        Param::required("id", SchemaExpr::String),
        Param::with_default("threshold", SchemaExpr::Float, Expr::float(2.0)),
    ];

    let interface = Interface::new()
        .with_input("trigger", PortDecl::required(SchemaExpr::Float))
        .with_output(
            "environment",
            PortDecl::required(SchemaExpr::map_of(SchemaExpr::custom("Cell"))),
        );

    // Body:
    //   { environment:
    //       if trigger > threshold
    //         then replace id with { '{id}_0': Cell[...], '{id}_1': Cell[...] }
    //         else {} }
    let daughters = Expr::Map(vec![
        (id_template("_0"), daughter("_0")),
        (id_template("_1"), daughter("_1")),
    ]);
    let replace = Expr::ReplaceWith {
        id: Box::new(Expr::var("id")),
        with: Box::new(daughters),
    };
    let environment_delta = Expr::If {
        cond: Box::new(Expr::gt(Expr::var("trigger"), Expr::var("threshold"))),
        then_: Box::new(replace),
        // Empty map = a zero delta on `environment` (no division this tick).
        else_: Some(Box::new(Expr::Map(vec![]))),
    };
    let body = Expr::Record(IndexMap::from_iter([(
        "environment".into(),
        environment_delta,
    )]));

    StepDef {
        name: "Divide".into(),
        params,
        interface,
        body,
    }
}

// ── composite Cell ──────────────────────────────────────────────────

fn cell_def() -> CompositeDef {
    let params = vec![
        Param::required("id", SchemaExpr::String),
        Param::with_default("mass", SchemaExpr::Float, Expr::float(1.0)),
        Param::with_default("growth_rate", SchemaExpr::Float, Expr::float(0.02)),
        Param::with_default("threshold", SchemaExpr::Float, Expr::float(2.0)),
    ];

    let interface = Interface::new().with_output(
        "environment",
        PortDecl::required(SchemaExpr::map_of(SchemaExpr::custom("Cell"))),
    );

    // Body (NO inner `environment` slot — that makes the bridge port a
    // passthrough; see module docs):
    //   mass: mass |
    //   grow: Grow[rate: growth_rate] ~{mass: mass} ->{mass: mass} |
    //   divide: Divide[id: id, threshold: threshold]
    //             ~{trigger: mass} ->{environment: environment}
    let body = Expr::parallel(vec![
        Expr::entry("mass", Expr::var("mass")),
        Expr::entry(
            "grow",
            Expr::term("Grow")
                .arg_named("rate", Expr::var("growth_rate"))
                .input("mass", Expr::var("mass"))
                .output("mass", Expr::var("mass"))
                .build(),
        ),
        Expr::entry(
            "divide",
            Expr::term("Divide")
                .arg_named("id", Expr::var("id"))
                .arg_named("threshold", Expr::var("threshold"))
                .input("trigger", Expr::var("mass"))
                .output("environment", Expr::var("environment"))
                .build(),
        ),
    ]);

    CompositeDef {
        name: "Cell".into(),
        params,
        interface,
        using: vec![],
        body,
    }
}

// ── composite Environment ───────────────────────────────────────────

fn environment_def() -> CompositeDef {
    let params = vec![Param::required(
        "cells",
        SchemaExpr::map_of(SchemaExpr::custom("Cell")),
    )];

    let interface = Interface::new().with_output(
        "cells",
        PortDecl::required(SchemaExpr::map_of(SchemaExpr::custom("Cell"))),
    );

    // Body: just hold the cells. Each cell divides itself internally;
    // there is no parent-level BRS in the internal-division model.
    let body = Expr::parallel(vec![Expr::entry("cells", Expr::var("cells"))]);

    CompositeDef {
        name: "Environment".into(),
        params,
        interface,
        using: vec![],
        body,
    }
}

// ── main ────────────────────────────────────────────────────────────

fn main_expr() -> Expr {
    // Environment[cells: {'0': Cell[id: '0', mass: 1.2] ->{environment: @}}]
    Expr::term("Environment")
        .arg_named(
            "cells",
            Expr::Map(vec![(
                StringLit::plain("0"),
                Expr::term("Cell")
                    .arg_named("id", Expr::string("0"))
                    .arg_named("mass", Expr::float(1.2))
                    .output("environment", here())
                    .build(),
            )]),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_builds() {
        let prog = program();
        assert!(prog.lookup("Grow").is_some());
        assert!(prog.lookup("Divide").is_some());
        assert!(prog.lookup("Cell").is_some());
        assert!(prog.lookup("Environment").is_some());
        assert!(prog.lookup("main").is_some());
    }
}
