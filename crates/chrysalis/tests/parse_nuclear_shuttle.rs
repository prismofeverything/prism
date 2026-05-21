//! The `.ys` parser, contexts: parse the nuclear-shuttle model from a REAL
//! file — `unit`/`context` declarations, `Quantity` + compound-dimension
//! types, `using` clauses, unary minus, `@.field`/`instance.field` paths —
//! then compile (units + contexts resolve, dimensional check) and run it.

use std::sync::Arc;

use chrysalis::ast::Def;
use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use prism_bigraph::Engine;

const NUCLEAR_YS: &str = include_str!("../ys/nuclear-shuttle.ys");

#[test]
fn parses_nuclear_shuttle() {
    let program = parse_program(NUCLEAR_YS).expect("parse nuclear-shuttle.ys");

    // The parser produced the context + composites that scope it via `using`.
    assert!(
        program.defs.iter().any(|d| matches!(d, Def::Context(c) if c.name == "concentration")),
        "parsed the `context concentration (...)` declaration"
    );
    let nucleus = program.defs.iter().find_map(|d| match d {
        Def::Composite(c) if c.name == "Nucleus" => Some(c),
        _ => None,
    });
    let nucleus = nucleus.expect("Nucleus composite");
    assert_eq!(nucleus.using.len(), 1, "Nucleus scopes one context");
    assert_eq!(nucleus.using[0].name, "concentration", "...the concentration context");
}

#[test]
fn nuclear_shuttle_compiles_and_runs() {
    let program = parse_program(NUCLEAR_YS).expect("parse nuclear-shuttle.ys");

    // The substantive result: compile RESOLVES the units + the cross-dimension
    // `concentration` context and runs the dimensional check (Count↔Conc for
    // `Sense`). This is the novel, hard part of the model — it succeeds.
    let result = compile(&program).expect("compile nuclear-shuttle (units + contexts)");

    // The compartment structure built correctly: Cell inlines to a parallel
    // (a List of one-key entries) whose `cytoplasm` / `nucleus` slots are nested
    // `Composite` specs (each carrying its own Synthesize/Sense process).
    let list = result.initial_state.as_list().expect("root parallel list");
    let find = |key: &str| list.iter().find_map(|e| e.get_field(key));
    let cytoplasm = find("cytoplasm").expect("cytoplasm compartment");
    assert_eq!(
        cytoplasm.get_field("address").and_then(|v| v.as_str()),
        Some("local:Composite"),
        "cytoplasm is a nested composite"
    );
    assert!(find("nucleus").is_some(), "nucleus compartment present");

    // It builds a real engine over the two nested compartments and runs to
    // completion.
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(60.0);
    // NOTE: observing the *intracellular* TF dynamics over the run reaches into
    // a nested composite's live state (the `cytoplasm.tf` path Transport reads);
    // surfacing that is a nested-composite execution concern, separate from the
    // parser/units/context work proven here.
}
