//! A pure one-shot Step DAG (no Processes) fires in dependency order when
//! discovered — the capability arbitrarily large workflows need, and the root
//! issue behind the earlier Process workaround (#7,
//! `reference_ys_workflow_dag_scheduling`). Step `B` consumes Step `A`'s output;
//! both are ys-compiled (address-based) specs instantiated by
//! `discover_all_processes`, so this pins that the dependency-ordered firing
//! runs on discovery, not only at construction.

use std::sync::Arc;

use prism_bigraph::Engine;

use chrysalis::compile::compile;
use chrysalis::parse::parse_program;

const SRC: &str = r#"
step A ->{x: float} ( {x: 42.0} )
step B ~{x: float} ->{y: float} ( {y: x + 1.0} )

composite W ->{y: float} (
  a: A ->{x: x} |
  b: B ~{x: x} ->{y: y}
)

W[]
"#;

#[test]
fn one_shot_step_dag_fires_in_dependency_order() {
    let prog = parse_program(SRC).expect("parse");
    let result = compile(&prog).expect("compile");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(1.0);

    // B ran after A: y = A.x + 1 = 43. Before #7, B fired before A (or not at
    // all) and saw nothing.
    let y = engine
        .state()
        .get_path(&["y".into()])
        .and_then(|v| v.as_f64());
    assert_eq!(
        y,
        Some(43.0),
        "B must fire after A and see its output (42 → 43); final state: {:?}",
        engine.state()
    );
}
