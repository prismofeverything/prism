use std::sync::Arc;
use std::time::Instant;
fn main() {
    let fixture = include_str!("../../fixtures/spatioflux_reference_demo.json");
    let registry = Arc::new(spatio_flux::build_registry());
    let (mut engine, _) = spatio_flux::vivarium_loader::load_vivarium(fixture, registry).unwrap();

    let start = Instant::now();
    eprintln!(
        "engine built in {:.1}s, starting ticks...",
        start.elapsed().as_secs_f64()
    );
    // Run 0.1s ticks to see fine-grained progress
    for tick in 0..200 {
        let tick_start = Instant::now();
        engine.tick();
        eprintln!(
            "  engine.tick() took {:.2}s",
            tick_start.elapsed().as_secs_f64()
        );
        let state = engine.state();
        let particles = state
            .as_map()
            .and_then(|m| m.get("particles"))
            .and_then(|v| v.as_map());
        let n = particles.map(|p| p.len()).unwrap_or(0);
        let masses: Vec<f64> = particles
            .map(|p| {
                p.values()
                    .filter_map(|v| {
                        v.as_map()
                            .and_then(|m| m.get("mass"))
                            .and_then(|v| v.as_f64())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let total: f64 = masses.iter().sum();
        let elapsed = start.elapsed().as_secs_f64();
        // Print every tick to see where it stalls
        eprintln!(
            "tick={} t={:.1} n={} mass={:.4} wall={:.1}s",
            tick,
            engine.time(),
            n,
            total,
            elapsed
        );
    }
}
