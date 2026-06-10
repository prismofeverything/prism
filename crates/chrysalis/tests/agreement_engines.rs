//! The agreement demo across ENGINES: native RK4 + COPASI + Tellurium on one CRN,
//! all through the unified `RunProcess[proc: X]` path. COPASI/Tellurium run in a
//! process-server over the rest bridge; the native runs locally — identical
//! surface. They agree because all realize DeterministicMassAction.

use std::path::PathBuf;

use chrysalis::compile::compile_with_core;
use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_core, std_modules};
use prism_schema::Value;

fn ys() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys/agreement-engines.ys")
}

#[test]
fn compiles_the_engine_agreement_demo() {
    let prog = parse_file(&ys()).expect("parse_file agreement-engines.ys");
    let result = compile_with_core(&prog, std_core(), std_modules());
    assert!(
        result.is_ok(),
        "should compile (rest-addressed COPASI/Tellurium + native RK4, uniform RunProcess): {:?}",
        result.err()
    );
}

fn mse_a(state: &Value, key: &str) -> f64 {
    state
        .get_path(&[key.into(), "A".into()])
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN)
}

#[test]
#[ignore = "integration: needs process-server/serve.sh on :8765"]
fn engines_agree_over_the_unified_run_process_path() {
    let prog = parse_file(&ys()).expect("parse_file");
    let state = chrysalis::runner::run(&prog, std_core(), std_modules(), 2.0)
        .expect("run the engine-agreement demo (is serve.sh on :8765?)");

    // The contract-checked comparison: each Compare demanded DeterministicMassAction
    // (forwarded from the proc — native OR rest — through RunProcess), and the
    // per-species MSE is ~0: native RK4, COPASI, and Tellurium agree on one CRN.
    let cr = mse_a(&state, "copasi_vs_rk4");
    let tr = mse_a(&state, "tellurium_vs_rk4");
    let ct = mse_a(&state, "copasi_vs_tellurium");
    assert!(
        cr.is_finite() && tr.is_finite() && ct.is_finite(),
        "all three MSEs present: copasi_vs_rk4={cr} tellurium_vs_rk4={tr} copasi_vs_tellurium={ct}"
    );
    assert!(ct < 1e-2, "COPASI ≈ Tellurium (both CVODE), mse.A = {ct}");
    assert!(cr < 1.0, "COPASI ≈ RK4, mse.A = {cr}");
    assert!(tr < 1.0, "Tellurium ≈ RK4, mse.A = {tr}");
}
