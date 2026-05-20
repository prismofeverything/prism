//! Grow/divide example as a hand-built chrysalis AST.
//!
//! Surface form lives at
//! [`crates/chrysalis/ys/grow-divide-unbounded.ys`](../../ys/grow-divide-unbounded.ys).
//! This module is the parser-free hand-built equivalent until task #12
//! (the parser) lands. NOTE: the `.ys` file is the *target* surface and
//! now uses units + a `.divide()` method call; this module still encodes
//! the original literal-split form (`mass / 2` in the reactum), which is
//! what the runtime executes today. They reconverge when method dispatch
//! + units land and the e2e test loads the `.ys`.
//!
//! Surface form (from `docs/chrysalis-design.md`):
//!
//! ```text
//! process Grow[rate: Float = 0.2]
//!   ~{mass: Float, interval: Float = 0.1}
//!   ->{mass: Float}
//! (
//!   delta = mass * rate * interval |
//!   {mass: delta}
//! )
//!
//! composite Cell[id: String, mass: Float = 1.0, growth_rate: Float = 0.02]
//!   ~{}
//!   ->{mass}
//! (
//!   mass: mass |
//!   Grow[rate: growth_rate] ~{mass: mass} ->{mass: mass}
//! )
//!
//! reaction MassThresholdDivide[threshold: Float = 2.0] (
//!   ?cid : Cell[mass: ?m] where ?m > threshold
//!   =>
//!   { '{?cid}_0' : Cell[mass: ?m / 2],
//!     '{?cid}_1' : Cell[mass: ?m / 2] }
//! )
//!
//! composite Environment[cells: Map[Cell], threshold: Float = 2.0]
//!   ~{}
//!   ->{cells}
//! (
//!   cells: cells |
//!   BRS[rules: [MassThresholdDivide[threshold: threshold]]]
//!     ~{state: cells} ->{state: cells}
//! )
//!
//! main = Environment[cells: {'0': Cell[id: '0', mass: 1.2]}]
//! ```
//!
//! This module builds the same AST directly via the AST constructor
//! helpers in [`crate::ast`].

use indexmap::IndexMap;

use crate::ast::{
    Block, CompositeDef, Def, Expr, Interface, Param, PortBindings, PortDecl, Program,
    ReactionDef, SchemaExpr, StringLit, StringSeg, TermArg,
};

/// Build the complete grow/divide chrysalis program.
pub fn program() -> Program {
    let mut p = Program::new();
    p.push(Def::Process(grow_def()));
    p.push(Def::Composite(cell_def()));
    p.push(Def::Reaction(mass_threshold_divide_def()));
    p.push(Def::Composite(environment_def()));
    p.push(Def::Binding {
        name: "main".into(),
        value: main_expr(),
    });
    p
}

// ── process Grow ────────────────────────────────────────────────────

fn grow_def() -> crate::ast::ProcessDef {
    let params = vec![Param::with_default(
        "rate",
        SchemaExpr::Float,
        Expr::float(0.2),
    )];

    let interface = Interface::new()
        .with_input("mass", PortDecl::required(SchemaExpr::Float))
        .with_input(
            "interval",
            PortDecl::with_default(SchemaExpr::Float, Expr::float(0.1)),
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
        Expr::Record(IndexMap::from_iter([(
            "mass".into(),
            Expr::var("delta"),
        )])),
    ));

    crate::ast::ProcessDef {
        name: "Grow".into(),
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
    ];

    let interface = Interface::new().with_output("mass", PortDecl::required(SchemaExpr::Float));

    // Body:
    //   mass: mass |
    //   Grow[rate: growth_rate] ~{mass: mass} ->{mass: mass}
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
    ]);

    CompositeDef {
        name: "Cell".into(),
        params,
        interface,
        body,
    }
}

// ── reaction MassThresholdDivide ────────────────────────────────────

fn mass_threshold_divide_def() -> ReactionDef {
    let params = vec![Param::with_default(
        "threshold",
        SchemaExpr::Float,
        Expr::float(2.0),
    )];

    // Redex: `?cid : Cell[mass: ?m]`
    let redex = Expr::site_typed(
        "?cid",
        Expr::term("Cell")
            .arg_named("mass", Expr::site("?m"))
            .build(),
    );

    // Guard: `?m > threshold`
    let guard = Some(Expr::gt(Expr::var("?m"), Expr::var("threshold")));

    // Reactum: { '{?cid}_0' : Cell[mass: ?m / 2],
    //           '{?cid}_1' : Cell[mass: ?m / 2] }
    let half = || Expr::div(Expr::var("?m"), Expr::float(2.0));
    let daughter = || {
        Expr::term("Cell")
            .arg_named("id", Expr::var("?cid"))
            .arg_named("mass", half())
            .build()
    };
    let key_with_suffix = |suffix: &str| StringLit {
        segments: vec![
            StringSeg::Expr(Expr::var("?cid")),
            StringSeg::Lit(suffix.into()),
        ],
    };
    let reactum = Expr::Map(vec![
        (key_with_suffix("_0"), daughter()),
        (key_with_suffix("_1"), daughter()),
    ]);

    ReactionDef {
        name: "MassThresholdDivide".into(),
        params,
        redex,
        reactum,
        guard,
        rate: None,
    }
}

// ── composite Environment ───────────────────────────────────────────

fn environment_def() -> CompositeDef {
    let params = vec![
        Param::required("cells", SchemaExpr::map_of(SchemaExpr::custom("Cell"))),
        Param::with_default("threshold", SchemaExpr::Float, Expr::float(2.0)),
    ];

    let interface = Interface::new().with_output(
        "cells",
        PortDecl::required(SchemaExpr::map_of(SchemaExpr::custom("Cell"))),
    );

    // Body:
    //   cells: cells |
    //   BRS[rules: [MassThresholdDivide[threshold: threshold]]]
    //     ~{state: cells} ->{state: cells}
    let body = Expr::parallel(vec![
        Expr::entry("cells", Expr::var("cells")),
        Expr::entry(
            "brs",
            Expr::term("BRS")
                .arg_named(
                    "rules",
                    Expr::List(vec![Expr::term("MassThresholdDivide")
                        .arg_named("threshold", Expr::var("threshold"))
                        .build()]),
                )
                .input("state", Expr::var("cells"))
                .output("state", Expr::var("cells"))
                .build(),
        ),
    ]);

    CompositeDef {
        name: "Environment".into(),
        params,
        interface,
        body,
    }
}

// ── main ────────────────────────────────────────────────────────────

fn main_expr() -> Expr {
    // Environment[cells: {'0': Cell[id: '0', mass: 1.2]}]
    Expr::term("Environment")
        .arg_named(
            "cells",
            Expr::Map(vec![(
                StringLit::plain("0"),
                Expr::term("Cell")
                    .arg_named("id", Expr::string("0"))
                    .arg_named("mass", Expr::float(1.2))
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
        assert!(prog.lookup("Cell").is_some());
        assert!(prog.lookup("MassThresholdDivide").is_some());
        assert!(prog.lookup("Environment").is_some());
        assert!(prog.lookup("main").is_some());
    }
}
