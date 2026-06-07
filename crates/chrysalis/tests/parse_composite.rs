//! The `.ys` parser, M2: parse a `process` + `composite` from a REAL file
//! (`ys/bump.ys`) — exercising `~{}->{}` interfaces, term calls, `|`
//! parallel-of-entries, and `[params]` — then compile and RUN it on the
//! engine, observing the composite's bridged output evolve.


use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_bigraph::Engine;

const BUMP_YS: &str = include_str!("../ys/bump.ys");

#[test]
fn parses_process_and_composite_from_ys_and_runs() {
    let program = parse_program(BUMP_YS).expect("parse ys/bump.ys");

    // The parsed program has the expected declarations.
    use chrysalis::ast::Def;
    assert!(
        program
            .defs
            .iter()
            .any(|d| matches!(d, Def::Process(p) if p.name == "Bump"))
    );
    assert!(
        program
            .defs
            .iter()
            .any(|d| matches!(d, Def::Composite(c) if c.name == "Leaf"))
    );

    let result = compile(&program).expect("compile parsed bump.ys");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(3.0);

    // The `Bump` process (wired inside the `Leaf` composite via the parsed
    // term call) grew `v`, surfaced through the composite's bridge.
    let v = engine
        .state()
        .get_field("v")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    assert!(
        v > 0.0,
        "Bump grew v through the parsed composite (got {v})"
    );
}
