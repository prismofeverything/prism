//! Test suite reproducing spatio-flux simulation scenarios.
//!
//! Each test builds a composite document, runs the simulation,
//! saves JSON outputs, and verifies basic invariants.

use spatio_flux::examples;
use spatio_flux::runner::run_document;

const OUT_DIR: &str = "out";

fn run_example(name: &str) {
    let (doc, registry, duration, emit_interval) = examples::get_example(name)
        .unwrap_or_else(|| panic!("unknown example: {name}"));

    let results = run_document(
        &doc,
        &registry,
        name,
        duration,
        emit_interval,
        OUT_DIR,
    )
    .unwrap_or_else(|e| panic!("{name} failed: {e}"));

    // Basic invariants
    assert!(
        results.times.len() > 1,
        "{name}: should have multiple time points"
    );
    assert!(
        results.states.len() == results.times.len(),
        "{name}: states and times should match"
    );
    assert!(
        *results.times.last().unwrap() > 0.0,
        "{name}: simulation should advance time"
    );

    // Verify JSON was saved
    let json_path = format!("{OUT_DIR}/{name}.json");
    assert!(
        std::path::Path::new(&json_path).exists(),
        "{name}: JSON document should be saved"
    );

    // Verify document round-trips
    let loaded = prism_bigraph::Document::load(&json_path).unwrap();
    assert_eq!(
        loaded.processes.len(),
        doc.processes.len(),
        "{name}: process count should match after reload"
    );

    println!(
        "{name}: OK — {} steps, t={:.1}s, {} processes",
        results.times.len() - 1,
        results.times.last().unwrap(),
        doc.processes.len(),
    );
}

#[test]
fn test_monod_kinetics() {
    run_example("monod_kinetics");
}

#[test]
fn test_diffusion_process() {
    run_example("diffusion_process");
}

#[test]
fn test_brownian_particles() {
    run_example("brownian_particles");
}

#[test]
fn test_br_particles_kinetics() {
    run_example("br_particles_kinetics");
}

#[test]
fn test_comets_diffusion() {
    run_example("comets_diffusion");
}

#[test]
fn test_comets_br_particles_kinetics() {
    run_example("comets_br_particles_kinetics");
}

/// Verify that monod kinetics produces biomass growth
#[test]
fn test_monod_kinetics_growth() {
    let (doc, registry, _, _) = examples::get_example("monod_kinetics").unwrap();
    let results = run_document(&doc, &registry, "monod_kinetics_growth", 10.0, 1.0, OUT_DIR).unwrap();

    let initial_biomass = results.states[0]
        .get_path(&["biomass".into()])
        .and_then(|v| v.as_f64())
        .unwrap();
    let final_biomass = results.final_state().unwrap()
        .get_path(&["biomass".into()])
        .and_then(|v| v.as_f64())
        .unwrap();

    assert!(
        final_biomass > initial_biomass,
        "biomass should grow: {initial_biomass} -> {final_biomass}"
    );
}

/// Verify that diffusion smooths a gradient
#[test]
fn test_diffusion_smoothing() {
    let (doc, registry, _, _) = examples::get_example("diffusion_process").unwrap();
    let results = run_document(&doc, &registry, "diffusion_smoothing", 10.0, 1.0, OUT_DIR).unwrap();

    let initial_field: Vec<f64> = results.states[0]
        .get_path(&["fields".into(), "glucose".into()])
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_f64())
        .collect();

    let final_field: Vec<f64> = results.final_state().unwrap()
        .get_path(&["fields".into(), "glucose".into()])
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_f64())
        .collect();

    // Variance should decrease (field is being smoothed)
    let initial_var = variance(&initial_field);
    let final_var = variance(&final_field);
    assert!(
        final_var < initial_var,
        "diffusion should reduce variance: {initial_var} -> {final_var}"
    );
}

fn variance(data: &[f64]) -> f64 {
    let mean = data.iter().sum::<f64>() / data.len() as f64;
    data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64
}
