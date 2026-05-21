//! `*` wildcard wires: `agents.*.mass` fans a port across every child of a
//! map. The engine already resolves star paths (`get_star_path`); this checks
//! the parser emits them, they round-trip, and the fan-out reaches a process.

use std::sync::Arc;

use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;
use prism_bigraph::Engine;

const STAR_YS: &str = r#"
process Snapshot ~{masses: any} ->{snap: any} (
  { snap: masses }
)

composite Pop (
  agents: { a1: { mass: 1.0 }, a2: { mass: 2.0 }, a3: { mass: 3.0 } } |
  snap: {} |
  s: Snapshot ~{masses: agents.*.mass} ->{snap: snap}
)

Pop
"#;

#[test]
fn star_wire_parses_roundtrips_and_fans_out() {
    let program = parse_program(STAR_YS).expect("parse the * wire");

    // The unparser preserves `*` segments, and the wire round-trips.
    let text = unparse(&program);
    assert!(text.contains("agents.*.mass"), "unparse preserves the * wire:\n{text}");
    let reparsed = unparse(&parse_program(&text).expect("re-parse"));
    assert_eq!(text, reparsed, "the * wire round-trips");

    // Run: `agents.*.mass` fans `mass` across every agent into `masses`, which
    // Snapshot copies onto `snap`.
    let result = compile(&program).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(1.0);

    let snap = engine.state().get_field("snap").expect("snap slot");
    let m = |k: &str| snap.get_field(k).and_then(|v| v.as_f64());
    assert_eq!(m("a1"), Some(1.0), "fanned a1.mass");
    assert_eq!(m("a2"), Some(2.0), "fanned a2.mass");
    assert_eq!(m("a3"), Some(3.0), "fanned a3.mass");
}
