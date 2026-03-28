//! Generate the spatio-flux HTML report.
//!
//! Usage:
//!   cargo run --bin report                              # standard scale
//!   cargo run --bin report -- debug                     # fast debug (3 timesteps)
//!   cargo run --bin report -- max                       # 10x longer
//!   cargo run --bin report -- debug br_particles_kinetics  # debug all, standard for this one
//!   cargo run --bin report -- debug comets_diffusion br_particles_dfba  # multiple focus sims

use spatio_flux::report::ReportScale;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let scale = match args.first().map(|s| s.as_str()) {
        Some("debug" | "d") => ReportScale::Debug,
        Some("max" | "m") => ReportScale::Max,
        _ => ReportScale::Standard,
    };

    // Any args after the scale name are focus simulations to run at standard scale
    let focus: Vec<String> = if matches!(scale, ReportScale::Debug | ReportScale::Max) {
        args.iter().skip(1).cloned().collect()
    } else {
        vec![]
    };

    let registry = std::sync::Arc::new(spatio_flux::build_registry());

    let fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");
    let output_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/report");

    println!("Scale:    {}", scale.label());
    if !focus.is_empty() {
        println!("Focus:    {:?} (standard scale)", focus);
    }
    println!("Fixtures: {fixture_dir}");
    println!("Output:   {output_dir}");

    spatio_flux::report::generate_report_focused(
        fixture_dir, output_dir, registry, scale, &focus,
    )
    .unwrap_or_else(|e| eprintln!("Error: {e}"));
}
