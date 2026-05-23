//! First-class functions: `def name(params) = body` defines a function; `f(args)`
//! calls it (the body evaluates with params bound — the same mechanism as a
//! process/step body, minus the bigraph interface).

use std::sync::Arc;

use chrysalis::compile::compile;
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;
use prism_bigraph::Engine;

const SRC: &str = r#"
def double(x) = x * 2

step Doubler ->{y: float} (
  { y: double(21.0) }
)

composite W ->{y: float} (
  d: Doubler ->{y: y}
)

W[]
"#;

#[test]
fn function_def_and_call_runs() {
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
    let y = engine.state().get_path(&["y".into()]).and_then(|v| v.as_f64());
    assert_eq!(y, Some(42.0), "double(21.0) should be 42; state: {:?}", engine.state());
}

/// First-class: a function passed as an argument to another function.
const HIGHER_ORDER: &str = r#"
def double(x) = x * 2
def apply(f, x) = f(x)

step S ->{y: float} (
  { y: apply(double, 21.0) }
)

composite W ->{y: float} (
  s: S ->{y: y}
)

W[]
"#;

#[test]
fn functions_are_first_class_arguments() {
    let prog = parse_program(HIGHER_ORDER).expect("parse");
    let result = compile(&prog).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(1.0);
    let y = engine.state().get_path(&["y".into()]).and_then(|v| v.as_f64());
    assert_eq!(
        y,
        Some(42.0),
        "apply(double, 21.0) should call the passed function → 42; state: {:?}",
        engine.state()
    );
}

#[test]
fn function_round_trips_through_unparse() {
    let prog = parse_program("def double(x) = x * 2\n{}").expect("parse");
    let text = unparse(&prog);
    assert!(
        text.contains("def double(x) ="),
        "function should unparse with `def name(params) =`, got: {text}"
    );
    // Fixpoint: re-parse + re-unparse is stable.
    let text2 = unparse(&parse_program(&text).expect("reparse"));
    assert_eq!(text, text2, "function def should be a round-trip fixpoint");
}
