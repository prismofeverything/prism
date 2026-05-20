//! Nuclear shuttle — multi-compartment concentration sensing, as a
//! hand-built chrysalis AST.
//!
//! Surface form lives at
//! [`crates/chrysalis/ys/nuclear-shuttle.ys`](../../ys/nuclear-shuttle.ys).
//! This is the motivating example for units *contexts* (cross-dimension
//! conversion): a transcription factor synthesised in the cytoplasm
//! shuttles into a 10×-smaller nucleus, where a sensor fires on
//! *concentration*. The sensor names no volume — the enclosing
//! compartment's `concentration` context supplies it.
//!
//! This module exercises the AST's ability to *represent* unit
//! declarations, contexts, the `using` clause, dimensioned `Quantity`
//! schemas, place-graph paths (`cytoplasm.tf`), and nested compartments.
//! It does not yet *run* with context semantics — that needs the
//! check-and-erase pass + a runtime unit/context resolver (the
//! prism-schema additions). Quantity schemas lower to `Float` today
//! (units erased), so the structure compiles and the place graph builds;
//! correct count↔concentration behaviour arrives with the resolver.
//!
//! The `.ys` aliases (`Mass = Quantity[...]`, etc.) are inlined here via
//! the `*_t()` helpers; type aliases are a parser-era convenience.

use indexmap::IndexMap;

use crate::ast::{
    Block, CompositeDef, ContextDef, ContextRule, ContextUse, Def, Dimension, Expr, Interface,
    Param, PlacePath, PortDecl, ProcessDef, Program, Ratio, SchemaExpr, TermArg, UnitDef, UnitExpr,
};

// ── Dimensioned scalar types (the .ys `Count = Quantity[...]` aliases) ──

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
fn synth_t() -> SchemaExpr {
    SchemaExpr::quantity(
        UnitExpr::named("molecule").div(UnitExpr::named("s")),
        false,
        false,
    )
}
fn clear_t() -> SchemaExpr {
    SchemaExpr::quantity(UnitExpr::named("fL").div(UnitExpr::named("s")), false, false)
}
fn time_t() -> SchemaExpr {
    SchemaExpr::quantity(UnitExpr::named("s"), false, false)
}

/// Build the complete nuclear-shuttle chrysalis program.
pub fn program() -> Program {
    let mut p = Program::new();
    p.push(Def::Unit(molecule_unit()));
    p.push(Def::Unit(um_unit()));
    p.push(Def::Unit(fl_unit()));
    p.push(Def::Context(concentration_context()));
    p.push(Def::Process(synthesize_def()));
    p.push(Def::Process(sense_def()));
    p.push(Def::Process(transport_def()));
    p.push(Def::Composite(cytoplasm_def()));
    p.push(Def::Composite(nucleus_def()));
    p.push(Def::Composite(cell_def()));
    p.push(Def::Binding {
        name: "main".into(),
        value: Expr::term("Cell").arg_named("id", Expr::string("0")).build(),
    });
    p
}

// ── unit declarations ───────────────────────────────────────────────

fn molecule_unit() -> UnitDef {
    UnitDef {
        name: "molecule".into(),
        dimension: Dimension::of(&[("substance", 1)]),
        // mol / 6.02214076e23  (Avogadro) — a unit, not a context.
        definition: UnitExpr::named("mol").div(UnitExpr::scalar(6.02214076e23)),
        affine_offset: None,
    }
}

fn um_unit() -> UnitDef {
    UnitDef {
        name: "um".into(),
        dimension: Dimension::of(&[("length", 1)]),
        definition: UnitExpr::scalar(1e-6).mul(UnitExpr::named("m")),
        affine_offset: None,
    }
}

fn fl_unit() -> UnitDef {
    UnitDef {
        name: "fL".into(),
        dimension: Dimension::of(&[("length", 3)]),
        // 1 femtolitre = 1 cubic micron.
        definition: UnitExpr::named("um").pow(Ratio::int(3)),
        affine_offset: None,
    }
}

// ── concentration context ───────────────────────────────────────────

fn concentration_context() -> ContextDef {
    // context concentration (volume: Volume) (
    //   [substance] <-> [substance]/[length]^3 : value / volume
    // )
    ContextDef {
        name: "concentration".into(),
        params: vec![Param::required("volume", volume_t())],
        rules: vec![ContextRule {
            from: Dimension::of(&[("substance", 1)]),
            to: Dimension::of(&[("substance", 1), ("length", -3)]),
            bidirectional: true,
            transform: Expr::div(Expr::var("value"), Expr::var("volume")),
        }],
    }
}

// ── process Synthesize ──────────────────────────────────────────────

fn synthesize_def() -> ProcessDef {
    // process Synthesize[rate: Synth = 50.0]
    //   ~{interval: Time = 0.1} ->{tf: Count}
    // ( {tf: rate * interval} )
    let interface = Interface::new()
        .with_input(
            "interval",
            PortDecl::with_default(time_t(), Expr::float(0.1)),
        )
        .with_output("tf", PortDecl::required(count_t()));

    let body = Expr::Record(IndexMap::from_iter([(
        "tf".into(),
        Expr::mul(Expr::var("rate"), Expr::var("interval")),
    )]));

    ProcessDef {
        name: "Synthesize".into(),
        params: vec![Param::with_default("rate", synth_t(), Expr::float(50.0))],
        interface,
        body,
    }
}

// ── process Sense (ambient concentration context) ───────────────────

fn sense_def() -> ProcessDef {
    // process Sense[k_on: Conc = 0.5]
    //   ~{tf: Count} ->{active: Bool}
    // ( {active: tf > k_on} )    -- `tf` (Count) coerced to Conc by the
    //                               enclosing compartment's context.
    let interface = Interface::new()
        .with_input("tf", PortDecl::required(count_t()))
        .with_output("active", PortDecl::required(SchemaExpr::Bool));

    let body = Expr::Record(IndexMap::from_iter([(
        "active".into(),
        Expr::gt(Expr::var("tf"), Expr::var("k_on")),
    )]));

    ProcessDef {
        name: "Sense".into(),
        params: vec![Param::with_default("k_on", conc_t(), Expr::float(0.5))],
        interface,
        body,
    }
}

// ── process Transport (explicit cross-compartment conversion) ───────

fn transport_def() -> ProcessDef {
    let interface = Interface::new()
        .with_input("cyt_tf", PortDecl::required(count_t()))
        .with_input("cyt_vol", PortDecl::required(volume_t()))
        .with_input("nuc_tf", PortDecl::required(count_t()))
        .with_input("nuc_vol", PortDecl::required(volume_t()))
        .with_input(
            "interval",
            PortDecl::with_default(time_t(), Expr::float(0.1)),
        )
        .with_output("cyt_tf", PortDecl::required(count_t()))
        .with_output("nuc_tf", PortDecl::required(count_t()));

    // cyt_conc = cyt_tf / cyt_vol |
    // nuc_conc = nuc_tf / nuc_vol |
    // flux = perm * (cyt_conc - nuc_conc) * interval |
    // {cyt_tf: -flux, nuc_tf: flux}
    let body = Expr::Block(Block::from_parts(
        vec![
            (
                "cyt_conc".into(),
                Expr::div(Expr::var("cyt_tf"), Expr::var("cyt_vol")),
            ),
            (
                "nuc_conc".into(),
                Expr::div(Expr::var("nuc_tf"), Expr::var("nuc_vol")),
            ),
            (
                "flux".into(),
                Expr::mul(
                    Expr::mul(
                        Expr::var("perm"),
                        Expr::sub(Expr::var("cyt_conc"), Expr::var("nuc_conc")),
                    ),
                    Expr::var("interval"),
                ),
            ),
        ],
        Expr::Record(IndexMap::from_iter([
            ("cyt_tf".into(), Expr::neg(Expr::var("flux"))),
            ("nuc_tf".into(), Expr::var("flux")),
        ])),
    ));

    ProcessDef {
        name: "Transport".into(),
        params: vec![Param::with_default("perm", clear_t(), Expr::float(5.0))],
        interface,
        body,
    }
}

// ── composite Cytoplasm (scopes a concentration context) ────────────

fn cytoplasm_def() -> CompositeDef {
    let params = vec![
        Param::with_default("volume", volume_t(), Expr::float(1000.0)),
        Param::with_default("tf", count_t(), Expr::float(0.0)),
        Param::with_default("synth", synth_t(), Expr::float(50.0)),
    ];

    let interface = Interface::new()
        .with_output("tf", PortDecl::required(count_t()))
        .with_output("volume", PortDecl::required(volume_t()));

    let using = vec![ContextUse {
        name: "concentration".into(),
        args: vec![TermArg::named(
            "volume",
            Expr::Path(PlacePath::here().dot("volume")),
        )],
    }];

    let body = Expr::parallel(vec![
        Expr::entry("volume", Expr::var("volume")),
        Expr::entry("tf", Expr::var("tf")),
        Expr::entry(
            "synthesize",
            Expr::term("Synthesize")
                .arg_named("rate", Expr::var("synth"))
                .output("tf", Expr::var("tf"))
                .build(),
        ),
    ]);

    CompositeDef {
        name: "Cytoplasm".into(),
        params,
        interface,
        using,
        body,
    }
}

// ── composite Nucleus (scopes a concentration context) ──────────────

fn nucleus_def() -> CompositeDef {
    let params = vec![
        Param::with_default("volume", volume_t(), Expr::float(100.0)),
        Param::with_default("tf", count_t(), Expr::float(0.0)),
        Param::with_default("k_on", conc_t(), Expr::float(0.5)),
    ];

    let interface = Interface::new()
        .with_output("tf", PortDecl::required(count_t()))
        .with_output("volume", PortDecl::required(volume_t()))
        .with_output("active", PortDecl::required(SchemaExpr::Bool));

    let using = vec![ContextUse {
        name: "concentration".into(),
        args: vec![TermArg::named(
            "volume",
            Expr::Path(PlacePath::here().dot("volume")),
        )],
    }];

    let body = Expr::parallel(vec![
        Expr::entry("volume", Expr::var("volume")),
        Expr::entry("tf", Expr::var("tf")),
        Expr::entry("active", Expr::bool(false)),
        Expr::entry(
            "sense",
            Expr::term("Sense")
                .arg_named("k_on", Expr::var("k_on"))
                .input("tf", Expr::var("tf"))
                .output("active", Expr::var("active"))
                .build(),
        ),
    ]);

    CompositeDef {
        name: "Nucleus".into(),
        params,
        interface,
        using,
        body,
    }
}

// ── composite Cell (two compartments + transport across them) ───────

fn cell_def() -> CompositeDef {
    let params = vec![
        Param::required("id", SchemaExpr::String),
        Param::with_default("cyt_vol", volume_t(), Expr::float(1000.0)),
        Param::with_default("nuc_vol", volume_t(), Expr::float(100.0)),
    ];

    let interface = Interface::new()
        .with_output("cytoplasm", PortDecl::required(SchemaExpr::custom("Cytoplasm")))
        .with_output("nucleus", PortDecl::required(SchemaExpr::custom("Nucleus")));

    // Cross-compartment wiring targets — nested place-graph paths.
    let cyt_tf = || Expr::Path(PlacePath::local("cytoplasm").dot("tf"));
    let cyt_vol = || Expr::Path(PlacePath::local("cytoplasm").dot("volume"));
    let nuc_tf = || Expr::Path(PlacePath::local("nucleus").dot("tf"));
    let nuc_vol = || Expr::Path(PlacePath::local("nucleus").dot("volume"));

    let body = Expr::parallel(vec![
        Expr::entry(
            "cytoplasm",
            Expr::term("Cytoplasm")
                .arg_named("volume", Expr::var("cyt_vol"))
                .build(),
        ),
        Expr::entry(
            "nucleus",
            Expr::term("Nucleus")
                .arg_named("volume", Expr::var("nuc_vol"))
                .build(),
        ),
        Expr::entry(
            "transport",
            Expr::term("Transport")
                .arg_named("perm", Expr::float(5.0))
                .input("cyt_tf", cyt_tf())
                .input("cyt_vol", cyt_vol())
                .input("nuc_tf", nuc_tf())
                .input("nuc_vol", nuc_vol())
                .output("cyt_tf", cyt_tf())
                .output("nuc_tf", nuc_tf())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_builds() {
        let prog = program();
        // unit + context declarations are looked up by name too
        assert!(prog.lookup("molecule").is_some());
        assert!(prog.lookup("fL").is_some());
        assert!(prog.lookup("concentration").is_some());
        for name in [
            "Synthesize", "Sense", "Transport", "Cytoplasm", "Nucleus", "Cell", "main",
        ] {
            assert!(prog.lookup(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn compartments_scope_a_concentration_context() {
        let prog = program();
        for name in ["Cytoplasm", "Nucleus"] {
            let c = match prog.lookup(name) {
                Some(Def::Composite(c)) => c,
                _ => panic!("{name} not a composite"),
            };
            assert_eq!(c.using.len(), 1, "{name} should scope one context");
            assert_eq!(c.using[0].name, "concentration");
        }
    }
}
