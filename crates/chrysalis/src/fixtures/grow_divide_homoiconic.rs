//! Homoiconic grow/divide — tier-1 benchmark #1, Step 2.
//!
//! Division is a FIRST-CLASS external reaction (the research point of the
//! benchmark), in contrast to the internal-division form in
//! [`super::grow_divide`]. The enabling idea (the container layout):
//!
//! Each cell is a CONTAINER node, not a bare subengine:
//! ```text
//!   cells.'0' = { _type: Cell, mass: <exported>, body: <Cell subengine> }
//! ```
//! - `mass` lives in the CONTAINER (the matchable source of truth). The
//!   subengine `body` reads it in (input bridge) and writes it back (output
//!   bridge), so the subengine is mass-stateless and `mass` never collides
//!   across cells (each has its own container).
//! - A parent `BRS` matches `?c : Cell[mass:?m]` — binding `?m` to the
//!   exported sibling, `?c`/`?cid` to the container — and replaces the
//!   matched cell with daughters. No peering inside the subengine.
//!
//! ## Status
//!
//! **Step 2a (this fixture):** the reactum LITERALLY splits (`?m / 2`),
//! proving the external reaction matches + fires end-to-end against real
//! subengine cells. **Step 2b (next):** replace the literal split with
//! `?c.divide()` dispatched to `type_divide` (already in prism-schema), so
//! the rule stops hard-coding the split. See docs/chrysalis-design.md.

use indexmap::IndexMap;

use crate::ast::{
    Block, CompositeDef, Def, Expr, Interface, Param, PortDecl, Program, ReactionDef, SchemaExpr,
    StringLit,
};

/// Build the homoiconic grow/divide program.
pub fn program() -> Program {
    let mut p = Program::new();
    p.push(Def::Process(grow_def()));
    p.push(Def::Composite(cell_def()));
    p.push(Def::Reaction(divide_def()));
    p.push(Def::Composite(environment_def()));
    p.push(Def::Binding {
        name: "main".into(),
        value: main_expr(),
    });
    p
}

// ── helpers ─────────────────────────────────────────────────────────

/// A matchable cell container: `{ _type: Cell, mass: <m>, body: <Cell
/// subengine reading/writing the container's mass> }`. `m` is evaluated
/// for both the container's source-of-truth `mass` and the subengine's
/// initial inner slot.
fn container(m: Expr) -> Expr {
    let mut fields: IndexMap<String, Expr> = IndexMap::new();
    fields.insert("_type".into(), Expr::string("Cell"));
    fields.insert("mass".into(), m.clone());
    fields.insert(
        "body".into(),
        Expr::term("Cell")
            .arg_named("mass", m)
            // `mass` (a bare Var) lowers to the wire `["mass"]` — the
            // container's mass sibling. Read in, written back out.
            .input("mass", Expr::var("mass"))
            .output("mass", Expr::var("mass"))
            .build(),
    );
    Expr::Record(fields)
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

// ── composite Cell (the subengine `body`) ───────────────────────────

fn cell_def() -> CompositeDef {
    let params = vec![
        Param::with_default("mass", SchemaExpr::Float, Expr::float(1.0)),
        Param::with_default("growth_rate", SchemaExpr::Float, Expr::float(0.02)),
    ];

    // Reads its mass from the enclosing container and writes the grown
    // value back: `~{mass} ->{mass}`. `mass` is the EXTENSIVE `Mass`
    // quantity, so the cell's instance schema types it as `Delta` — which is
    // what makes `.divide()` halve it (schema-driven, not hard-coded).
    let mass_t = || SchemaExpr::quantity(crate::ast::UnitExpr::named("kg"), true, false);
    let interface = Interface::new()
        .with_input("mass", PortDecl::required(mass_t()))
        .with_output("mass", PortDecl::required(mass_t()));

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
        using: vec![],
        body,
    }
}

// ── reaction Divide ─────────────────────────────────────────────────

fn divide_def() -> ReactionDef {
    let params = vec![Param::with_default(
        "threshold",
        SchemaExpr::Float,
        Expr::float(2.0),
    )];

    // Redex: `?cid : ?cell::Cell` — the entry key binds to `?cid`, and the
    // matched cell VALUE binds to `?cell` (a nested typed site = as-pattern;
    // `?cell` is captured by `Pattern::Bind`). No `?m` needed — the guard
    // reads the cell directly.
    let redex = Expr::site_typed(
        "?cid",
        Expr::site_typed("?cell", Expr::term("Cell").build()),
    );

    // Guard: `?cell.mass > threshold` — field access on the bound cell.
    let guard = Some(Expr::gt(
        Expr::Path(crate::ast::PlacePath::local("?cell").dot("mass")),
        Expr::var("threshold"),
    ));

    // Reactum: `?cell.divide(?cid)` — the cell divides ITSELF (the
    // type-relative, schema-driven `divide` method): extensive mass halves,
    // the rest shared, daughters keyed `?cid_0`/`?cid_1` from the passed id.
    // The BRS removes the matched `?cid` and `_add`s the daughters.
    let reactum = Expr::method(Expr::var("?cell"), "divide", vec![Expr::var("?cid")]);

    ReactionDef {
        name: "Divide".into(),
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

    let body = Expr::parallel(vec![
        Expr::entry("cells", Expr::var("cells")),
        Expr::entry(
            "brs",
            Expr::term("BRS")
                .arg_named(
                    "rules",
                    Expr::List(vec![Expr::term("Divide")
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
        using: vec![],
        body,
    }
}

// ── main ────────────────────────────────────────────────────────────

fn main_expr() -> Expr {
    Expr::term("Environment")
        .arg_named(
            "cells",
            Expr::Map(vec![(StringLit::plain("0"), container(Expr::float(1.2)))]),
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
        assert!(prog.lookup("Divide").is_some());
        assert!(prog.lookup("Environment").is_some());
        assert!(prog.lookup("main").is_some());
    }
}
