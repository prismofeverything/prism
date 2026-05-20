//! End-to-end: homoiconic grow/divide (tier-1 #2, Step 2a).
//!
//! Division is driven by a FIRST-CLASS external `Divide` reaction installed
//! in a parent `BRS` — not an internal step. Each cell is a container
//! `{_type: Cell, mass: <exported>, body: <subengine>}`; the BRS matches
//! the exported mass and replaces the matched cell with two half-mass
//! daughters. Observed by the cell COUNT in `cells`.

use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::Value;

use chrysalis::fixtures::grow_divide_homoiconic as gd;

fn count_cells(state: &Value) -> usize {
    state
        .as_map()
        .and_then(|m| m.get("cells"))
        .and_then(|v| v.as_map())
        .map(|m| {
            m.iter()
                .filter(|(k, v)| !k.starts_with('_') && v.as_map().is_some())
                .count()
        })
        .unwrap_or(0)
}

fn cells_keys(state: &Value) -> Vec<String> {
    state
        .as_map()
        .and_then(|m| m.get("cells"))
        .and_then(|v| v.as_map())
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn homoiconic_grow_divide_runs() {
    let program = gd::program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();

    // mass 1.2, rate 0.02 → crosses threshold 2.0 around tick 26.
    engine.run(50.0);

    let final_state = engine.state();
    let n = count_cells(final_state);
    assert!(
        n >= 2,
        "expected the external Divide reaction to split the cell; got {n} cells (keys {:?})",
        cells_keys(final_state)
    );
}
