//! Debug spatioflux_reference_demo: dump per-second particle count, total mass,
//! and glucose at center to compare with Python.

use std::sync::Arc;

fn main() {
    let fixture = include_str!("../../fixtures/spatioflux_reference_demo.json");
    let registry = Arc::new(spatio_flux::build_registry());
    let (mut engine, _) = spatio_flux::vivarium_loader::load_vivarium(fixture, registry).unwrap();

    println!("time,n_particles,total_mass,glucose_center");
    for step in 0..120 {
        engine.run(1.0);
        let state = engine.state();

        let particles = state.as_map().and_then(|m| m.get("particles")).and_then(|v| v.as_map());
        let n = particles.map(|p| p.len()).unwrap_or(0);
        let total_mass: f64 = particles.map(|p| {
            p.values().filter_map(|v| {
                v.as_map().and_then(|m| m.get("mass")).and_then(|v| v.as_f64())
            }).sum()
        }).unwrap_or(0.0);

        // Glucose at center (row 5, col 5 of 10x10 grid)
        let glucose = state.as_map()
            .and_then(|m| m.get("fields"))
            .and_then(|v| v.as_map())
            .and_then(|m| m.get("glucose"))
            .and_then(|v| v.as_list())
            .and_then(|rows| rows.get(5))
            .and_then(|row| row.as_list())
            .and_then(|cols| cols.get(5))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if step % 5 == 0 || step < 5 {
            println!("{},{n},{total_mass:.4},{glucose:.4}", step + 1);
            // Dump first particle's exchange and local
            if let Some(p) = particles.and_then(|p| p.values().next()) {
                let ex = p.as_map().and_then(|m| m.get("exchange"));
                let local = p.as_map().and_then(|m| m.get("local"));
                eprintln!("  t={}: exchange={ex:?} local={local:?}", step + 1);
            }
        }
    }
}
