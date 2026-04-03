//! Benchmark raw FBA solve time across different COBRA models.
//!
//! Measures only LP setup + solve, independent of process/engine overhead.
//! Usage: cargo run --release --bin bench_fba

use std::time::Instant;
use spatio_flux::processes::fba::{CobraModel, FbaSolver, model_path};

fn bench_model(name: &str, n_solves: usize) {
    let path = match model_path(name) {
        Some(p) => p,
        None => { eprintln!("  {name}: model not found"); return; }
    };

    let t_load = Instant::now();
    let model = match CobraModel::from_json_file(&path) {
        Ok(m) => m,
        Err(e) => { eprintln!("  {name}: {e}"); return; }
    };
    let load_ms = t_load.elapsed().as_secs_f64() * 1000.0;

    let n_mets = model.metabolites.len();
    let n_rxns = model.reactions.len();

    let t_build = Instant::now();
    let mut solver = FbaSolver::new(&model);
    let build_us = t_build.elapsed().as_micros();

    // First solve (cold)
    let t0 = Instant::now();
    let sol = solver.solve(&model.lower_bounds, &model.upper_bounds);
    let cold_us = t0.elapsed().as_micros();

    let obj = sol.as_ref().map(|s| s.objective_value).unwrap_or(0.0);

    // Warm solves
    let t1 = Instant::now();
    for _ in 0..n_solves {
        let _ = solver.solve(&model.lower_bounds, &model.upper_bounds);
    }
    let total_us = t1.elapsed().as_micros();
    let avg_us = total_us as f64 / n_solves as f64;

    println!("{name:<15} | {n_mets:>4} mets | {n_rxns:>5} rxns | load {load_ms:>6.1}ms | build {build_us:>5}us | cold {cold_us:>6}us | avg {avg_us:>8.1}us ({n_solves} solves) | obj={obj:.4}");
}

fn main() {
    println!("FBA Benchmark: raw LP solve time (Rust/HiGHS)");
    println!("{}", "-".repeat(130));

    let models = [
        ("e_coli_core", 10000),
        ("iNF517",      5000),
        ("iJN746",      2000),
        ("iCN900",      2000),
        ("iMM904",      1000),
        ("iAF1260",     500),
    ];

    for &(name, n) in &models {
        bench_model(name, n);
    }
}
