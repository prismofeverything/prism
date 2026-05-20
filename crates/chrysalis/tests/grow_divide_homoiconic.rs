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
fn divide_method_is_type_relative_and_schema_driven() {
    // `.divide()` dispatched on the cell's TYPE (`_type: Cell`), derives the
    // cell's instance schema from the program (mass = extensive → Delta),
    // and runs the schema-driven split: mass halves, id reissued, the rest
    // shared. No literal `mass / 2`, no sentinel — divide is type-relative.
    let program = gd::program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    let cell = Value::tree([
        ("_type", Value::String("Cell".to_string())),
        ("mass", Value::float(2.0)),
        ("body", Value::map()),
    ]);
    // `?cell.divide(?cid)` — the id is passed in (here "0"); the cell stores none.
    let daughters = result
        .methods
        .dispatch(&cell, "divide", &[Value::String("0".to_string())])
        .expect("divide dispatch");
    let m = daughters.as_map().expect("daughters are a map");
    assert_eq!(m.len(), 2, "two daughters");
    for k in ["0_0", "0_1"] {
        let d = m.get(k).unwrap_or_else(|| panic!("daughter {k} present"));
        assert_eq!(
            d.get_field("mass").and_then(|v| v.as_f64()),
            Some(1.0),
            "{k}: extensive mass halved by the schema"
        );
        assert_eq!(
            d.get_field("_type").and_then(|v| v.as_str()),
            Some("Cell"),
            "{k}: _type shared"
        );
    }
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
    let keys = cells_keys(final_state);
    // Real division: the mother `0` is gone, replaced by daughters — not
    // budding (which would leave `0` alongside new cells).
    assert!(
        n >= 2,
        "expected the external `?cell.divide(?cid)` reaction to split the cell; got {n} (keys {keys:?})"
    );
    assert!(
        !keys.iter().any(|k| k == "0"),
        "mother `0` should be replaced by its daughters (true division), got keys {keys:?}"
    );
    assert!(
        keys.iter().all(|k| k.starts_with("0_")),
        "all surviving cells should descend from `0` (keys {keys:?})"
    );
}
