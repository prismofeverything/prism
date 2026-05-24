//! The process-contract demo, end to end over chrysalis's std prelude (prism-std
//! natives): parse → enforce contracts → compile → run (via `chrysalis::runner`)
//! → read MSE/figure, and confirm the workflow writes its own files.
//!
//! This is a chrysalis demo: the ys *language* + its bundled std library
//! (prism-std). No downstream package (e.g. spatio-flux) is involved.

use std::path::PathBuf;

use chrysalis::compile::compile_with_modules;
use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_methods, std_modules, std_registry};
use prism_schema::Value;

const SRC: &str = include_str!("../ys/integrator-comparison.ys");

/// Run with the `Output` step pointed at a fresh temp dir (no source-tree
/// writes), returning the final state. Uses the shared `chrysalis::runner`.
fn run_workflow(tag: &str) -> (Value, PathBuf) {
    let out = std::env::temp_dir().join(format!("prism-integrator-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let src = SRC.replace(
        "IntegratorComparison[]",
        &format!("IntegratorComparison[out: '{}']", out.display()),
    );
    let prog = parse_program(&src).expect("parse");
    let state = chrysalis::runner::run(&prog, std_registry(), std_methods(), std_modules(), 2.0)
        .expect("run");
    (state, out)
}

#[test]
fn compiles_with_std_natives() {
    let prog = parse_program(SRC).expect("integrator-comparison.ys should parse");
    let result = compile_with_modules(&prog, std_registry(), std_methods(), std_modules());
    assert!(
        result.is_ok(),
        "should compile: contracts enforced, std natives resolve — got {:?}",
        result.err()
    );
}

#[test]
fn runs_and_compares_the_two_integrators() {
    let (state, out) = run_workflow("compare");
    let mse_a = state
        .get_path(&["mse".into(), "A".into()])
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("no mse.A in final state: {state:?}"));
    assert!(
        mse_a > 0.0 && mse_a < 1e-1,
        "method-induced MSE on A should be small but nonzero, got {mse_a}"
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn produces_an_overlay_figure() {
    let (state, out) = run_workflow("figure");
    let svg = state
        .get_path(&["figure".into(), "svg".into()])
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| panic!("no figure.svg in final state: {state:?}"));
    assert!(svg.starts_with("<svg"), "figure should be an SVG document");
    assert!(
        svg.contains("Rk4 vs ForwardEuler"),
        "overlay should carry the title passed from the .ys"
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn workflow_writes_its_own_artifacts() {
    let (_state, out) = run_workflow("output");
    for file in ["Rk4.csv", "ForwardEuler.csv", "mse.csv", "overlay.svg"] {
        assert!(
            out.join(file).exists(),
            "the Output step should have written {}",
            out.join(file).display()
        );
    }
    let _ = std::fs::remove_dir_all(&out);
}
