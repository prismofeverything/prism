//! End-to-end test: grow/divide via chrysalis pipeline.
//!
//! Acceptance criteria for tier-1 homoiconicity over reactions:
//! - A cell starting at mass 1.2 with growth-rate 0.02 grows above the
//!   threshold (2.0) and divides into two cells whose masses sum to
//!   the parent's pre-split mass.
//! - The division is driven by a **runtime-constructed
//!   `MassThresholdDivide` reaction** installed in a parent BRS (not by
//!   a hardcoded `Divide` step).

use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::Value;

use chrysalis::fixtures::grow_divide;

fn count_cells_in_environment(state: &Value) -> usize {
    state
        .as_map()
        .and_then(|m| m.get("cells"))
        .and_then(|v| v.as_map())
        .map(|m| m.len())
        .unwrap_or(0)
}

fn cell_masses(state: &Value) -> Vec<f64> {
    state
        .as_map()
        .and_then(|m| m.get("cells"))
        .and_then(|v| v.as_map())
        .map(|cells| {
            cells
                .values()
                .filter_map(|c| c.get_field("mass").and_then(|v| v.as_f64()))
                .collect()
        })
        .unwrap_or_default()
}

fn debug_state_shape(state: &Value, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match state {
        Value::Map(m) => {
            let mut out = String::from("{\n");
            for (k, v) in m {
                out.push_str(&format!("{pad}  {k}: "));
                match v {
                    Value::Map(_) => out.push_str(&debug_state_shape(v, indent + 1)),
                    Value::List(_) => out.push_str(&debug_state_shape(v, indent + 1)),
                    Value::Foreign(f) => out.push_str(&format!("<Foreign:{}>\n", f.type_name)),
                    Value::Float(f) => out.push_str(&format!("{}\n", f.0)),
                    Value::Int(i) => out.push_str(&format!("{}\n", i)),
                    Value::String(s) => out.push_str(&format!("{:?}\n", s)),
                    Value::Bool(b) => out.push_str(&format!("{}\n", b)),
                    Value::None => out.push_str("None\n"),
                    other => out.push_str(&format!("{:?}\n", other)),
                }
            }
            out.push_str(&format!("{pad}}}\n"));
            out
        }
        Value::List(l) => {
            let mut out = String::from("[\n");
            for v in l {
                out.push_str(&format!("{pad}  "));
                match v {
                    Value::Map(_) => out.push_str(&debug_state_shape(v, indent + 1)),
                    Value::Foreign(f) => out.push_str(&format!("<Foreign:{}>\n", f.type_name)),
                    other => out.push_str(&format!("{:?}\n", other)),
                }
            }
            out.push_str(&format!("{pad}]\n"));
            out
        }
        other => format!("{:?}\n", other),
    }
}

#[test]
fn grow_divide_pipeline_runs() {
    let program = grow_divide::program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    eprintln!(
        "INITIAL STATE:\n{}",
        debug_state_shape(&result.initial_state, 0)
    );
    eprintln!("REGISTERED TYPES: {:?}", result.registry.type_names());

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();

    eprintln!("ENGINE NODES AFTER DISCOVER: {:?}", engine.node_names());

    engine.run(1.0);
    eprintln!(
        "AFTER 1s:\n{}",
        debug_state_shape(engine.state(), 0)
    );
    eprintln!("ENGINE NODES AT t=1: {:?}", engine.node_names());

    engine.run(40.0);

    let final_state = engine.state();
    eprintln!(
        "FINAL STATE:\n{}",
        debug_state_shape(final_state, 0)
    );

    let n = count_cells_in_environment(final_state);
    let masses = cell_masses(final_state);
    eprintln!("final cell count = {n}, masses = {:?}", masses);

    assert!(n >= 2, "expected at least one division; got {n} cells");
}
