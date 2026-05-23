//! The full process-contract demo, end to end from the `.ys` file: parse →
//! enforce contracts → compile with the native integrator library + methods →
//! run → read the MSE / figure, and confirm the workflow writes its own files.
//!
//! This test lives in **spatio-flux**, not chrysalis: it wires the ys *language*
//! to spatio-flux's *native* process library, and chrysalis must not depend on
//! spatio-flux (spatio-flux will be rewritten in ys, so the dependency goes
//! spatio-flux → chrysalis). See memory `feedback_ys_layering`.

use std::path::PathBuf;
use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::MethodRegistry;

use chrysalis::compile::{compile_with_modules, ModuleRegistry};
use chrysalis::parse::parse_program;
use spatio_flux::from_config::build_registry;
use spatio_flux::processes::mass_action;

const SRC: &str = include_str!("../../chrysalis/ys/integrator-comparison.ys");

/// The native value-methods the ys-native bodies dispatch to: `TimeSeries`
/// (`species_mse`/`overlay`/`csv`), `Figure` (`svg`), `Map` (`csv`), and the
/// `Integrator` `integrate` the function-bodied processes call.
fn ts_methods() -> MethodRegistry {
    let mut m = MethodRegistry::new();
    mass_action::register_methods(&mut m);
    m
}

/// The native modules the `.ys` imports: `RunProcess` from `core`, `rk4`/`euler`
/// from `integrators`, `CRN` from `chem`, `Path` from `io`.
fn modules() -> ModuleRegistry {
    ModuleRegistry::new()
        .process("core", "RunProcess")
        .object("integrators", "rk4", mass_action::integrator("rk4"))
        .object("integrators", "euler", mass_action::integrator("euler"))
        .type_(
            "chem",
            "CRN",
            "{species: list[string], reactions: list[{reactants: map[float], products: map[float], k: float}]}",
        )
        .type_("io", "Path", "string")
}

/// Run the workflow with its `Output` step pointed at a fresh temp dir, so the
/// test never writes into the source tree. Returns the finished engine + dir.
fn run_workflow(tag: &str) -> (Engine, PathBuf) {
    let out = std::env::temp_dir().join(format!("prism-integrator-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    // Retarget the composite's `out` param (defaulted in the .ys) at the temp dir.
    let src = SRC.replace(
        "IntegratorComparison[]",
        &format!("IntegratorComparison[out: '{}']", out.display()),
    );
    let prog = parse_program(&src).expect("parse");
    let result =
        compile_with_modules(&prog, build_registry(), ts_methods(), modules()).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(2.0);
    (engine, out)
}

#[test]
fn compiles_with_native_imports_and_methods() {
    let prog = parse_program(SRC).expect("integrator-comparison.ys should parse");
    let result = compile_with_modules(&prog, build_registry(), ts_methods(), modules());
    assert!(
        result.is_ok(),
        "should compile: contracts enforced, imported rk4/euler/RunProcess/CRN/Path \
         resolve to native, methods injected — got {:?}",
        result.err()
    );
}

#[test]
fn runs_and_compares_the_two_integrators() {
    let (engine, out) = run_workflow("compare");
    let mse_a = engine
        .state()
        .get_path(&["mse".into(), "A".into()])
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("no mse.A in final state: {:?}", engine.state()));
    assert!(
        mse_a > 0.0 && mse_a < 1e-1,
        "method-induced MSE on A should be small but nonzero, got {mse_a}"
    );
    let _ = std::fs::remove_dir_all(&out);
}

/// The `Plot` step emits a `figure` — `a.overlay(b, …)` dispatches to the native
/// `TimeSeries::overlay`, rendering the two trajectories to an SVG `Figure`.
#[test]
fn produces_an_overlay_figure() {
    let (engine, out) = run_workflow("figure");
    let svg = engine
        .state()
        .get_path(&["figure".into(), "svg".into()])
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| panic!("no figure.svg in final state: {:?}", engine.state()));
    assert!(svg.starts_with("<svg"), "figure should be an SVG document");
    assert!(
        svg.contains("Rk4 vs ForwardEuler"),
        "overlay should carry the title passed from the .ys (a.overlay(b, '…'))"
    );
    let _ = std::fs::remove_dir_all(&out);
}

/// The `Output` step makes the workflow self-outputting: its body calls the
/// native effectful writers (`a.csv(path / a.name)`, `figure.svg(…)`), so running
/// the `.ys` produces files with no Rust harness doing IO.
#[test]
fn workflow_writes_its_own_artifacts() {
    let (_engine, out) = run_workflow("output");
    for file in ["Rk4.csv", "ForwardEuler.csv", "mse.csv", "overlay.svg"] {
        let path = out.join(file);
        assert!(
            path.exists(),
            "the Output step should have written {}",
            path.display()
        );
    }
    // The trajectory CSV has a header + a row per step (runtime/timestep = 25 + 1).
    let rk4 = std::fs::read_to_string(out.join("Rk4.csv")).expect("read Rk4.csv");
    assert!(rk4.starts_with("time,"), "trajectory CSV should have a time header");
    assert!(rk4.lines().count() > 10, "trajectory CSV should have many rows");
    let _ = std::fs::remove_dir_all(&out);
}
