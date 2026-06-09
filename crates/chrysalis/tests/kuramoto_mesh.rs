//! Demo 2 — Kuramoto over a CRDT-safe `mesh` link (local slice).
//!
//! Demo 1's coupling, re-expressed so it distributes: the mean field is a
//! PER-SOURCE `map[id -> array[[2]]] mesh` (each oscillator owns its key), which
//! is key-union = join-semilattice = exactly what `mesh_safety` blesses. So the
//! same coupling is CRDT-safe and will replicate across peers unchanged. Here we
//! prove the local slice: oscillators couple through the mesh map and phase-lock
//! (strong k) vs drift (k=0) — identical physics to Demo 1, mesh-safe carrier.

use prism_bigraph::Engine;
use prism_schema::Value;

const KURAMOTO_MESH: &str = include_str!("../ys/kuramoto-mesh.ys");

/// Run the mesh tile at coupling `k` for `duration`; return R = |Σ field| / N.
fn order_parameter(k: f64, duration: f64) -> f64 {
    let src = format!("{KURAMOTO_MESH}\nTileMesh[k: {k:?}]\n");
    let program = chrysalis::parse::parse_program(&src).expect("parse");
    let result = chrysalis::compile::compile_with_methods(
        &program,
        chrysalis::prelude::std_registry(),
        chrysalis::prelude::std_methods(),
    )
    .expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine");
    engine.discover_all_processes();
    engine.run(duration);

    // The mesh field is a map[id -> [cos,sin]]; sum the vectors → [sx, sy].
    let (mut sx, mut sy) = (0.0, 0.0);
    if let Some(m) = engine.state().get_field("field").and_then(|v| v.as_map()) {
        for (key, v) in m.iter() {
            if key.starts_with('_') {
                continue;
            }
            if let Some(arr) = v.as_list() {
                sx += arr.first().and_then(Value::as_f64).unwrap_or(0.0);
                sy += arr.get(1).and_then(Value::as_f64).unwrap_or(0.0);
            }
        }
    }
    (sx * sx + sy * sy).sqrt() / 8.0
}

#[test]
fn oscillators_phase_lock_over_a_mesh_link() {
    let r_strong = order_parameter(4.0, 200.0);
    let r_none = order_parameter(0.0, 200.0);
    eprintln!("mesh Kuramoto:  R(k=4) = {r_strong:.3}   R(k=0) = {r_none:.3}");

    // Strong coupling entrains across the (CRDT-safe) mesh map → phase-lock.
    assert!(r_strong > 0.9, "k=4 should phase-lock over the mesh link: R={r_strong:.3}");
    // No coupling: spread frequencies dephase.
    assert!(r_none < 0.45, "k=0 should drift: R={r_none:.3}");
}
