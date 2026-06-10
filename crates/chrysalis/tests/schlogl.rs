//! Schlögl — the bistable showcase across engines. COPASI (over rest) from two
//! ICs lands in two basins (X→1 low, X→3 high); native RK4 agrees on the high
//! basin. The contract `Compare` reports both: a LARGE gap between the basins
//! (bistability) and a ~0 gap between engines (agreement) — both legal because
//! both sides are DeterministicMassAction.

use std::path::PathBuf;

use chrysalis::compile::compile_with_core;
use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_core, std_modules};
use prism_schema::Value;

fn ys() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys/schlogl-engines.ys")
}

#[test]
fn compiles_the_schlogl_demo() {
    let prog = parse_file(&ys()).expect("parse_file schlogl-engines.ys");
    let result = compile_with_core(&prog, std_core(), std_modules());
    assert!(result.is_ok(), "should compile: {:?}", result.err());
}

fn mse_x(state: &Value, key: &str) -> f64 {
    state
        .get_path(&[key.into(), "X".into()])
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN)
}

#[test]
#[ignore = "integration: needs process-server/serve.sh on :8765"]
fn schlogl_is_bistable_and_engines_agree_per_basin() {
    let prog = parse_file(&ys()).expect("parse_file");
    let state = chrysalis::runner::run(&prog, std_core(), std_modules(), 2.0)
        .expect("run the Schlögl demo (is serve.sh on :8765?)");

    let gap = mse_x(&state, "bistable_gap");
    let agree = mse_x(&state, "copasi_vs_rk4");
    assert!(gap.is_finite() && agree.is_finite(), "MSEs present: gap={gap} agree={agree}");
    // The two basins (X≈1 vs X≈3) genuinely diverge — bistability.
    assert!(gap > 1.0, "bistable basins should diverge, gap mse.X = {gap}");
    // COPASI and native RK4 land on the SAME high basin — agreement.
    assert!(agree < 0.1, "COPASI ≈ RK4 on the high basin, mse.X = {agree}");
    // And the contrast is stark: bistability ≫ engine disagreement.
    assert!(gap > 10.0 * agree.max(1e-6), "gap ({gap}) ≫ engine disagreement ({agree})");
}
