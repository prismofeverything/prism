//! Generate the spatio-flux HTML report.
//!
//! Usage:
//!   cargo run --bin report                              # assemble report from cached results
//!   cargo run --bin report -- run                       # run ALL sims then assemble
//!   cargo run --bin report -- run monod_kinetics        # run one sim then assemble
//!   cargo run --bin report -- run ecoli_core_dfba br_particles_kinetics  # run specific sims

use spatio_flux::report::ReportScale;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let registry = Arc::new(spatio_flux::build_registry());

    let fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");
    let output_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/report");

    if args.first().map(|s| s.as_str()) == Some("run") {
        let sims: Vec<String> = args.iter().skip(1).cloned().collect();
        if sims.is_empty() {
            println!("Running ALL simulations...");
            spatio_flux::report::run_all_sims(fixture_dir, output_dir, &registry)
                .unwrap_or_else(|e| eprintln!("Error: {e}"));
        } else {
            for sim in &sims {
                println!("Running {sim}...");
                match spatio_flux::report::run_single_sim(sim, fixture_dir, output_dir, &registry) {
                    Ok(ms) => println!("  {sim}: {ms}ms"),
                    Err(e) => eprintln!("  {sim}: ERROR {e}"),
                }
            }
        }
    }

    println!("Assembling report...");
    spatio_flux::report::assemble_report(fixture_dir, output_dir)
        .unwrap_or_else(|e| eprintln!("Error: {e}"));
}
