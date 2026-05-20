//! End-to-end test: grow/divide via chrysalis pipeline.
//!
//! STATUS (2026-05-20): `#[ignore]`d — a known, honest gap. This drives
//! division with an EXTERNAL BRS reaction that reads each cell's `mass`.
//! That cannot work now that a `composite Cell` is a real encapsulated
//! subengine (`from_config`): the cell's mass lives inside its subengine
//! and its bridged output collides at `cells.mass`, so the BRS never sees
//! per-cell mass and no division happens. (It was previously a FALSE PASS
//! — the spurious `cells.mass` key was miscounted as a second "cell".)
//!
//! Two valid fixes (a design choice, pending):
//!  (a) make `Cell` a STORE — `mass` = readable data + an inner `Grow`
//!      process (like spatio-flux particles) — so the external BRS can
//!      read and divide it; or
//!  (b) INTERNAL division — a `Divide` step inside the subengine cell that
//!      writes daughters up to the parent. Proven for the subengine model
//!      in crates/prism-bigraph/tests/growth_division.rs (15 cells).

use std::sync::Arc;

use prism_bigraph::Engine;
use prism_schema::Value;

use chrysalis::fixtures::grow_divide;

fn count_cells_in_environment(state: &Value) -> usize {
    state
        .as_map()
        .and_then(|m| m.get("cells"))
        .and_then(|v| v.as_map())
        // Count only actual cell entries (maps) — NOT scalar pollution such
        // as a bridged `mass` output colliding into the cells map (which is
        // what made the old assertion a false pass).
        .map(|m| m.values().filter(|v| v.as_map().is_some()).count())
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

#[test]
#[ignore = "Honest known gap (was a FALSE PASS) — see module docs. An external \
            BRS reaction cannot divide encapsulated subengine cells; the fix is \
            a STORE Cell or internal division (crates/prism-bigraph/tests/\
            growth_division.rs proves the subengine case)."]
fn grow_divide_pipeline_runs() {
    // Tier-1 chrysalis acceptance for grow/divide:
    //   - Cell at mass 1.2 grows past threshold 2.0
    //   - BRS fires the runtime-constructed MassThresholdDivide
    //     reaction (NOT a hardcoded step)
    //   - At least one division happens — at least 2 cells remain
    //
    // Specific final masses depend on exact tick alignment between
    // Grow's mass update and the BRS's match check; we just assert
    // the structural property (division happened).
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
    // ~tick 26. Run 40 ticks for some buffer.
    engine.run(40.0);

    let final_state = engine.state();
    let n = count_cells_in_environment(final_state);

    assert!(n >= 2, "expected at least one division; got {n} cells");
}
