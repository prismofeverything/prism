//! Demo 3 — plastic Kuramoto: the network learns its own wiring. Each oscillator
//! has a Hebbian-updated coupling `weight` tracking its coherence with the mean
//! field; the synchronized core self-strengthens and drifting outliers decouple,
//! so the dynamics reshape the soft topology (categorical-core §9, the M/R-closure
//! payoff). The learned weights are SURFACED through a shared per-key `weights`
//! link (child inner state is otherwise encapsulated). With moderate coupling the
//! CORE (ω near the mean) entrains and the OUTLIERS (ω far) drift; plasticity must
//! learn that: core weight ↑, outlier weight ↓.

use prism_bigraph::Engine;
use prism_schema::Value;

const PLASTIC: &str = include_str!("../ys/kuramoto-plastic.ys");

fn run(k: f64, duration: f64) -> Value {
    let src = format!("{PLASTIC}\nPlasticTile[k: {k:?}]\n");
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
    engine.state().clone()
}

/// Each oscillator's learned `weight`, by id — surfaced through the shared link.
fn weights(state: &Value) -> std::collections::HashMap<String, f64> {
    state
        .get_field("weights")
        .and_then(|v| v.as_map())
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .filter_map(|(k, v)| v.as_f64().map(|w| (k.to_string(), w)))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn plasticity_learns_the_coupling_structure() {
    // ω by id: 0:0.2 1:0.5 2:0.9 3:1.0 4:1.0 5:1.1 6:1.5 7:1.8
    // CORE (near the mean) = 2,3,4,5 ; OUTLIERS (far) = 0,1,6,7.
    let w = weights(&run(2.0, 400.0));
    let mut sorted: Vec<_> = w.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    eprintln!("learned weights: {sorted:?}");

    let mean = |ids: &[&str]| -> f64 {
        ids.iter().filter_map(|i| w.get(*i)).sum::<f64>() / ids.len() as f64
    };
    let core = mean(&["2", "3", "4", "5"]);
    let outlier = mean(&["0", "1", "6", "7"]);
    eprintln!("core weight = {core:.3}   outlier weight = {outlier:.3}");

    assert_eq!(w.len(), 8, "read all 8 oscillator weights (surfaced)");
    // The network LEARNED its structure: the entrained core strengthened and the
    // outliers weakened — the dynamics rewrote the soft coupling topology. The
    // signature is MONOTONICITY: coupling weight decreases with distance from the
    // mean frequency (id 3 = ω1.0 core > id 1 = ω0.5 > id 0 = ω0.2 outlier).
    assert!(
        core > outlier + 0.05,
        "the entrained core should out-weigh the outliers: core={core:.3} vs outlier={outlier:.3}"
    );
    assert!(
        w["3"] > w["1"] && w["1"] > w["0"],
        "weight should fall monotonically with frequency-distance (learned structure): {:.3} > {:.3} > {:.3}",
        w["3"], w["1"], w["0"]
    );
}
