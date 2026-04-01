use std::sync::Arc;

fn main() {
    let fixture = include_str!("../../fixtures/newtonian_particles.json");
    let registry = Arc::new(spatio_flux::build_registry());
    let (mut engine, _) = spatio_flux::vivarium_loader::load_vivarium(fixture, registry).unwrap();

    let pid = "p_b4437a";
    let mut prev_vy: f64 = 0.0;

    // Use tick() to step one event at a time
    for i in 0..300 {
        let time = match engine.tick() {
            Some(t) => t,
            None => break,
        };

        let vel = engine.state()
            .get_path(&["particles".into(), pid.into(), "velocity".into()])
            .and_then(|v| v.as_list())
            .map(|l| l.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0))
            .unwrap_or(0.0);

        // Report when velocity changes significantly
        if (vel - prev_vy).abs() > 0.001 || vel == 0.0 && prev_vy != 0.0 {
            eprintln!("tick={i} t={time:.3} vy={vel:.4} (was {prev_vy:.4})");
        }
        prev_vy = vel;

        if time > 2.5 { break; }
    }
}
