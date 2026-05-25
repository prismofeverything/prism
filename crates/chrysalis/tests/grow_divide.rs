//! End-to-end test: grow/divide via the chrysalis pipeline (INTERNAL division).
//!
//! A `Cell` is a real encapsulated subengine. It grows internally (inner
//! `Grow`) and divides itself via an inner `Divide` step that writes
//! daughters UP to the parent `cells` map through the bridge — the proven
//! upstream pattern (`crates/prism-bigraph/tests/growth_division.rs`).
//! Division is observed ONLY by the cell COUNT in `cells`, never by reading
//! a cell's encapsulated inner mass.

use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::Value;

use chrysalis::fixtures::grow_divide;

fn count_cells_in_environment(state: &Value) -> usize {
    state
        .as_map()
        .and_then(|m| m.get("cells"))
        .and_then(|v| v.as_map())
        // Count only actual cell entries (subengine spec maps). Sentinel
        // keys (`_add`/`_remove`) and any scalar pollution are excluded.
        .map(|m| {
            m.iter()
                .filter(|(k, v)| !k.starts_with('_') && v.as_map().is_some())
                .count()
        })
        .unwrap_or(0)
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

#[ignore = "Form-1 internal division writes daughters to the cell's CONTAINER via \
            `->{environment: %}` (the 1-up `[]` wire). With `%`=self (protocols-as-types \
            3c), `%` is the own node and the 1-up container has no sigil (`^` is the \
            2-up grandparent, needed by the env pool). Form-1 (internal/`replace`) is \
            superseded by Form-3 (Divider + `_divide`), proven in \
            prism-bigraph/tests/cells_division.rs + chrysalis/tests/grow_divide_stream.rs. \
            Re-enable after migrating this fixture to Form-3 addressed cells \
            (cells-and-division #9 step 5)."]
#[test]
fn grow_divide_pipeline_runs() {
    // Tier-1 chrysalis acceptance for grow/divide:
    //   - Cell at mass 1.2 grows past threshold 2.0 (inner Grow)
    //   - the inner Divide step fires and writes daughters up to `cells`
    //   - at least one division happens → at least 2 cells remain
    let program = grow_divide::program();
    let result = chrysalis::compile::compile(&program).expect("compile");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        Arc::clone(&result.registry),
    )
    .expect("engine init");
    engine.discover_all_processes();

    // Grow rate 0.02 → mass *= 1.02/tick. Crosses threshold 2.0 at
    // ~tick 26. Run 50 ticks so a first division certainly lands.
    engine.run(50.0);

    let final_state = engine.state();
    let n = count_cells_in_environment(final_state);

    assert!(
        n >= 2,
        "expected at least one division; got {n} cells.\nstate:\n{}",
        debug_state_shape(final_state, 0)
    );
}
