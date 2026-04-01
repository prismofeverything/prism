use std::sync::Arc;

fn main() {
    let fixture = include_str!("../../fixtures/newtonian_particles.json");
    let registry = Arc::new(spatio_flux::build_registry());
    let (mut engine, _) = spatio_flux::vivarium_loader::load_vivarium(fixture, registry).unwrap();

    for step in 0..40 {
        engine.run(0.1);
        let state = engine.state();
        let particles = state.as_map().and_then(|m| m.get("particles")).and_then(|v| v.as_map()).unwrap();
        eprintln!("  t={:.1} n_particles={}", (step+1) as f64 * 0.1, particles.len());
        if let Some((pid, p)) = particles.iter().next() {
            let pos = p.as_map().and_then(|m| m.get("position")).and_then(|v| v.as_list());
            let vel = p.as_map().and_then(|m| m.get("velocity")).and_then(|v| v.as_list());
            let mass = p.as_map().and_then(|m| m.get("mass")).and_then(|v| v.as_f64());
            if let (Some(pos), Some(vel)) = (pos, vel) {
                let x = pos[0].as_f64().unwrap_or(0.0);
                let y = pos[1].as_f64().unwrap_or(0.0);
                let vx = vel[0].as_f64().unwrap_or(0.0);
                let vy = vel[1].as_f64().unwrap_or(0.0);
                eprintln!("t={:.1} {pid}: pos=({x:.3},{y:.3}) vel=({vx:.4},{vy:.4}) mass={:.4}",
                    (step+1) as f64 * 0.1, mass.unwrap_or(0.0));
            }
        }
    }
}
