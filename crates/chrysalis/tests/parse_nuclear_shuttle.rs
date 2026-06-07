//! The `.ys` parser, contexts: parse the nuclear-shuttle model from a REAL
//! file — `unit`/`context` declarations, `Quantity` + compound-dimension
//! types, `using` clauses, unary minus, `@.field` paths — then compile (units
//! + contexts resolve, dimensional check) and run it.
//!
//! Composite encapsulation: a compartment's internal state is PRIVATE; it is
//! observed only through its bridge. Each compartment exposes its TF count via
//! its output port onto a `Cell` slot (`cyt_tf` / `nuc_tf`), and we observe
//! THOSE bridged slots — never compartment internals.


use chrysalis::ast::Def;
use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_bigraph::Engine;
use prism_schema::Value;

const NUCLEAR_YS: &str = include_str!("../ys/nuclear-shuttle.ys");

/// A top-level `Cell` slot value (the named-parallel body is a Tree).
fn slot(state: &Value, key: &str) -> Option<f64> {
    state.get_field(key).and_then(|v| v.as_f64())
}

#[test]
fn parses_nuclear_shuttle() {
    let program = parse_program(NUCLEAR_YS).expect("parse nuclear-shuttle.ys");

    // The parser produced the context + composites that scope it via `using`.
    assert!(
        program
            .defs
            .iter()
            .any(|d| matches!(d, Def::Context(c) if c.name == "concentration")),
        "parsed the `context concentration (...)` declaration"
    );
    let nucleus = program
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Composite(c) if c.name == "Nucleus" => Some(c),
            _ => None,
        })
        .expect("Nucleus composite");
    assert_eq!(nucleus.using.len(), 1, "Nucleus scopes one context");
    assert_eq!(
        nucleus.using[0].name, "concentration",
        "...the concentration context"
    );
}

#[test]
fn nuclear_shuttle_runs_and_exposes_tf_via_bridge() {
    let program = parse_program(NUCLEAR_YS).expect("parse nuclear-shuttle.ys");

    // compile resolves units + the cross-dimension `concentration` context and
    // runs the dimensional check (Count↔Conc for `Sense`) — the novel part.
    let result = compile(&program).expect("compile nuclear-shuttle (units + contexts)");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();

    // The Cell exposes the compartments' TF only through their bridges — these
    // slots are the ONLY window onto compartment state.
    assert_eq!(
        slot(engine.state(), "cyt_tf"),
        Some(0.0),
        "cyt_tf seeded at 0"
    );
    assert_eq!(
        slot(engine.state(), "nuc_tf"),
        Some(0.0),
        "nuc_tf seeded at 0"
    );

    engine.run(60.0);

    // Synthesize ran inside the cytoplasm; its TF surfaced through the cytoplasm
    // bridge onto `cyt_tf` — and Transport (reading only the exposed slots)
    // moved some across to `nuc_tf`. Both observed WITHOUT touching compartment
    // internals: the compartments' own `tf` is private and never read here.
    let cyt_tf = slot(engine.state(), "cyt_tf").expect("cyt_tf present");
    let nuc_tf = slot(engine.state(), "nuc_tf").expect("nuc_tf present");
    assert!(
        cyt_tf > 0.0,
        "cytoplasm TF exposed through its bridge onto cyt_tf (got {cyt_tf})"
    );
    assert!(
        nuc_tf > 0.0,
        "TF transported to the nucleus via the exposed slots (got {nuc_tf})"
    );
}
