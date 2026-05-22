//! The full process-contract demo, end to end from the `.ys` file: parse →
//! enforce contracts → compile with the native integrator library + `TimeSeries`
//! methods → run → read the MSE.
//!
//! This test lives in **spatio-flux**, not chrysalis: it wires the ys *language*
//! to spatio-flux's *native* process library, and chrysalis must not depend on
//! spatio-flux (spatio-flux will be rewritten in ys, so the dependency goes
//! spatio-flux → chrysalis). See memory `feedback_ys_layering`.

use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::MethodRegistry;

use chrysalis::compile::compile_with_methods;
use chrysalis::parse::parse_program;
use spatio_flux::from_config::build_registry;
use spatio_flux::processes::mass_action;

const SRC: &str = include_str!("../../chrysalis/ys/integrator-comparison.ys");

/// A method registry carrying the native `TimeSeries` value-methods the
/// ys-native `Compare` body dispatches to (`a.species_mse(b)`, `a.overlay(b)`).
fn ts_methods() -> MethodRegistry {
    let mut m = MethodRegistry::new();
    mass_action::register_methods(&mut m);
    m
}

#[test]
fn compiles_with_native_processes_and_methods() {
    let prog = parse_program(SRC).expect("integrator-comparison.ys should parse");
    let result = compile_with_methods(&prog, build_registry(), ts_methods());
    assert!(
        result.is_ok(),
        "should compile: contracts enforced, `extern Rk4`/`ForwardEuler` resolve to \
         native factories, TimeSeries methods injected — got {:?}",
        result.err()
    );
}

#[test]
fn runs_and_compares_the_two_integrators() {
    let prog = parse_program(SRC).expect("parse");
    let result = compile_with_methods(&prog, build_registry(), ts_methods()).expect("compile");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(2.0);

    let mse_a = engine
        .state()
        .get_path(&["mse".into(), "A".into()])
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("no mse.A in final state: {:?}", engine.state()));
    assert!(
        mse_a > 0.0 && mse_a < 1e-1,
        "method-induced MSE on A should be small but nonzero, got {mse_a}"
    );
}
