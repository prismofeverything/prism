//! The `.ys` parser, M3: parse the tier-1 grow/divide benchmark from a REAL
//! file (`ys/grow_divide.ys`) — exercising Block process bodies, `if/then/else`,
//! `replace … with`, string interpolation (`'{id}_0'`), `@` (self), and term
//! calls — then compile and RUN it: a cell grows past threshold and the inner
//! Divide step writes two daughters up to the parent `cells` map.


use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_bigraph::Engine;
use prism_schema::Value;

const GROW_DIVIDE_YS: &str = include_str!("../ys/grow_divide.ys");

/// Count cells in `environment.cells` (the proven division observable —
/// daughters appear there via the passthrough bridge).
fn count_cells(state: &Value) -> usize {
    state
        .get_field("cells")
        .and_then(|m| m.as_map())
        .map(|m| m.keys().filter(|k| !k.starts_with('_')).count())
        .unwrap_or(0)
}

#[test]
fn parses_grow_divide_ys_and_divides() {
    let program = parse_program(GROW_DIVIDE_YS).expect("parse ys/grow_divide.ys");
    let result = compile(&program).expect("compile parsed grow/divide");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();

    assert_eq!(count_cells(engine.state()), 1, "starts with one cell");

    // Grow rate 0.02 → mass *= 1.02/tick; mass 1.2 crosses threshold 2.0 well
    // within 50 ticks → at least one division.
    engine.run(50.0);

    let n = count_cells(engine.state());
    assert!(
        n >= 2,
        "expected at least one division from the parsed .ys; got {n} cells"
    );
}
