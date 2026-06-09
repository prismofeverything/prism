//! Demo 3 — plastic Kuramoto: the network learns its own wiring. Each oscillator
//! has a Hebbian-updated coupling `weight` tracking its coherence with the mean
//! field, so the synchronized core self-strengthens and drifting outliers
//! decouple — the dynamics reshape the soft topology (categorical-core §9, the
//! M/R-closure payoff). With MODERATE coupling the core (ω near the mean) entrains
//! and the outliers (ω far) drift; plasticity must then learn that structure:
//! core weight ↑, outlier weight ↓.

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

/// Each oscillator's learned `weight`, by id — read at its real path inside the
/// child composite (`config.state.weight`).
fn weights(state: &Value) -> std::collections::HashMap<String, f64> {
    let mut out = std::collections::HashMap::new();
    if let Some(oscs) = state.get_field("oscillators").and_then(|v| v.as_map()) {
        for (id, v) in oscs.iter().filter(|(k, _)| !k.starts_with('_')) {
            let w = v
                .get_field("weight")
                .and_then(Value::as_f64)
                .or_else(|| {
                    v.get_field("config")
                        .and_then(|c| c.get_field("state"))
                        .and_then(|s| s.get_field("weight"))
                        .and_then(Value::as_f64)
                });
            if let Some(w) = w {
                out.insert(id.to_string(), w);
            }
        }
    }
    out
}

#[test]
fn probe_weight_path() {
    let state = run(3.0, 100.0);
    eprintln!(
        "TOP KEYS: {:?}",
        state.as_map().map(|m| m.keys().map(|k| format!("{k:?}")).collect::<Vec<_>>())
    );
    if let Some(oscs) = state.get_field("oscillators").and_then(|v| v.as_map()) {
        if let Some((id, first)) = oscs.iter().find(|(k, _)| !k.starts_with('_')) {
            eprintln!("OSC '{id}' = {first:#?}");
        }
    }
}

#[test]
#[ignore]
fn plasticity_learns_the_coupling_structure() {
    // ω by id: 0:0.2 1:0.5 2:0.9 3:1.0 4:1.0 5:1.1 6:1.5 7:1.8
    // CORE (near the mean) = 2,3,4,5 ; OUTLIERS (far) = 0,1,6,7.
    let state = run(3.0, 200.0);
    let w = weights(&state);
    eprintln!("learned weights: {:?}", {
        let mut v: Vec<_> = w.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    });

    let mean = |ids: &[&str]| -> f64 {
        ids.iter().filter_map(|i| w.get(*i)).sum::<f64>() / ids.len() as f64
    };
    let core = mean(&["2", "3", "4", "5"]);
    let outlier = mean(&["0", "1", "6", "7"]);
    eprintln!("core weight = {core:.3}   outlier weight = {outlier:.3}");

    assert_eq!(w.len(), 8, "read all 8 oscillator weights");
    // The network LEARNED its structure: it strengthened the entrained core and
    // decoupled the drifting outliers (the dynamics rewrote the soft topology).
    assert!(
        core > outlier + 0.2,
        "plasticity should differentiate: core={core:.3} vs outlier={outlier:.3}"
    );
}
