//! The agreement demo across ENGINES: native RK4 + COPASI + Tellurium on one CRN,
//! all through the unified `RunProcess[proc: X]` path. COPASI/Tellurium run in a
//! process-server over the rest bridge; the native runs locally — identical
//! surface. They agree because all realize DeterministicMassAction.

use std::path::PathBuf;

use chrysalis::compile::compile_with_modules;
use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_methods, std_modules, std_registry};
use prism_schema::Value;

fn ys() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys/agreement-engines.ys")
}

#[test]
fn compiles_the_engine_agreement_demo() {
    let prog = parse_file(&ys()).expect("parse_file agreement-engines.ys");
    let result = compile_with_modules(&prog, std_registry(), std_methods(), std_modules());
    assert!(
        result.is_ok(),
        "should compile (rest-addressed COPASI/Tellurium + native RK4, uniform RunProcess): {:?}",
        result.err()
    );
}

fn a_column(state: &Value, engine: &str) -> Vec<f64> {
    state
        .get_path(&[engine.into(), "columns".into(), "A".into()])
        .and_then(|v| v.as_list())
        .map(|l| l.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

#[test]
#[ignore = "integration: needs process-server/serve.sh on :8765"]
fn engines_agree_over_the_unified_run_process_path() {
    let prog = parse_file(&ys()).expect("parse_file");
    let state = chrysalis::runner::run(&prog, std_registry(), std_methods(), std_modules(), 2.0)
        .expect("run the engine-agreement demo (is serve.sh on :8765?)");

    let rk4 = a_column(&state, "rk4");
    let copasi = a_column(&state, "copasi");
    let tellurium = a_column(&state, "tellurium");
    assert!(
        !rk4.is_empty() && rk4.len() == copasi.len() && copasi.len() == tellurium.len(),
        "three equal-length trajectories: rk4={} copasi={} tellurium={}",
        rk4.len(),
        copasi.len(),
        tellurium.len()
    );
    // A decays 100 → ~3 over t=5 (k=0.7); the three engines agree along the way.
    assert!((rk4[0] - 100.0).abs() < 1e-9, "A starts at 100");
    assert!(*rk4.last().unwrap() < 5.0, "A decays to ~3, got {}", rk4.last().unwrap());
    for i in 0..rk4.len() {
        assert!(
            (copasi[i] - tellurium[i]).abs() < 1e-2,
            "COPASI ≈ Tellurium @ {i}: {} vs {}",
            copasi[i],
            tellurium[i]
        );
        assert!(
            (copasi[i] - rk4[i]).abs() < 0.5,
            "COPASI ≈ RK4 @ {i}: {} vs {}",
            copasi[i],
            rk4[i]
        );
    }
}
