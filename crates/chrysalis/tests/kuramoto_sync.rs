//! Demo 1 — Kuramoto phase oscillators synchronize on the prism substrate.
//!
//! Manifold's first proof (docs/categorical-core.md §9, the M5 adaptive-network
//! domain): an adaptive/dynamical network is a *prism theory*, not new engine.
//! `kuramoto.ys` reuses, unchanged, the grow-divide-glucose mechanisms — a
//! shared additive `link` pool (the mean field, θ↔mass / field↔glucose), the
//! engine's additive fold AS the reduction, the BSP tick AS manifold's two-phase
//! update.
//!
//! 8 oscillators start in phase (R=1) with spread natural frequencies. With
//! strong coupling (k=4) they entrain and the order parameter R = |Σe^{iθ}|/N
//! stays high; with no coupling (k=0) they drift apart and R decays. Coupling
//! produces order — on the same engine that grows a cell colony.

use prism_bigraph::Engine;
use prism_schema::Value;

const KURAMOTO: &str = include_str!("../ys/kuramoto.ys");

/// Run the tile (8 oscillators, the `.ys` defaults) at coupling `k` for
/// `duration` and return (R, the mean-field pool [sx, sy]).
fn order_parameter(k: f64, duration: f64) -> (f64, Vec<f64>) {
    let src = format!("{KURAMOTO}\nTile[k: {k:?}]\n");
    let program = chrysalis::parse::parse_program(&src).expect("parse");
    // Compile with the std method bundle so `.at`/`.cos`/`.sin` (prism-std math)
    // reach the Core — bare `compile()` uses an empty MethodRegistry.
    let result = chrysalis::compile::compile_with_core(&program, chrysalis::prelude::std_core(), chrysalis::compile::ModuleRegistry::new())
    .expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(duration);

    let pool: Vec<f64> = engine
        .state()
        .get_field("field")
        .and_then(|v| v.as_list())
        .map(|xs| xs.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    let sx = pool.first().copied().unwrap_or(0.0);
    let sy = pool.get(1).copied().unwrap_or(0.0);
    ((sx * sx + sy * sy).sqrt() / 8.0, pool)
}

#[test]
fn coupling_entrains_frequency_spread_oscillators() {
    let (r_strong, p_strong) = order_parameter(4.0, 200.0);
    let (r_none, p_none) = order_parameter(0.0, 200.0);
    eprintln!("k=4: R={r_strong:.3} pool={p_strong:?}");
    eprintln!("k=0: R={r_none:.3} pool={p_none:?}");

    // Strong coupling entrains the frequency spread → phase-lock (R near 1).
    assert!(r_strong > 0.9, "k=4 should stay phase-locked: R={r_strong:.3} (want > 0.9)");
    // No coupling: spread natural frequencies dephase → low coherence.
    assert!(r_none < 0.45, "k=0 should drift apart: R={r_none:.3} (want < 0.45)");
    // The headline: coupling produces order, on the same engine that grows a colony.
    assert!(
        r_strong - r_none > 0.4,
        "coupling must raise coherence: locked={r_strong:.3} vs free={r_none:.3}"
    );
}
