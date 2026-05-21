//! The `.ys` parser, units: parse a dimensioned program from a REAL file
//! (`ys/units.ys`) — `unit` declarations, unit expressions, and
//! `Quantity[unit: …, extensive]` schemas — then compile (the dimensional
//! check runs, units erase to floats) and run it.

use std::sync::Arc;

use chrysalis::ast::Def;
use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_bigraph::Engine;

const UNITS_YS: &str = include_str!("../ys/units.ys");

#[test]
fn parses_units_and_runs() {
    let program = parse_program(UNITS_YS).expect("parse ys/units.ys");

    // The parser produced a `unit pg` declaration + a Quantity-typed process.
    assert!(
        program.defs.iter().any(|d| matches!(d, Def::Unit(u) if u.name == "pg")),
        "parsed the `unit pg : [mass] = 1e-12 kg` declaration"
    );

    // compile runs the dimensional check (mass·rate·interval = mass) and erases
    // units to floats.
    let result = compile(&program).expect("compile units.ys (dimensional check + erase)");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();

    let m0 = engine.state().get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
    engine.run(5.0);
    let m1 = engine.state().get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!(m1 > m0, "Grow grew the dimensioned mass (checked + erased): {m0} -> {m1}");
}
