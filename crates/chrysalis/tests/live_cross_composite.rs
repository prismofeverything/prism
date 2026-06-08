//! #43 — the cross-composite reactor over LIVE (running, EVOLVING) composites,
//! reached through the BRIDGE (not `config.state`).
//!
//! A composite holds its evolving state in a private sub-engine; the parent
//! tree's `config.state` is the INIT seed and stays put (mutating it would just
//! re-initialise the node — `discover_processes` re-adds on a config change). So
//! the reactor must NOT read `config.state`. It reads the **face** — what the
//! composite PUBLISHES through its output bridge, live each tick — exactly the
//! same surface every protocol uses, and exactly what the bridge is FOR.
//!
//! Here two `Cell` composites GROW (an internal `Grow` process raises `mass`,
//! published to the face via `%.mass`). A cross-composite reaction's guard reads
//! BOTH cells' live face masses — `?mw + ?me > threshold` — and only fires after
//! growth pushes the combined mass past the threshold. The reactor reaches both
//! running composites' evolving interiors through the bridge; `config` is never
//! touched.
//!
//! (The single-composite live case is `reaction_divide.rs`: `?c :: Cell[mass:
//! ?m] where ?m > t => ?c.divide()` fires on the GROWN mass. This is its
//! cross-composite sibling — a condition over two live faces at once.)

use prism_bigraph::Engine;
use prism_schema::Value;

fn run(src: &str, time: f64) -> Value {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(time);
    engine.state().clone()
}

/// Cells GROW (`Grow` raises the inner `mass`, the bridge publishes it on the
/// cell's own node via `%.mass` — the live face). Two cells couple on `link e`.
/// The reaction's guard reads BOTH live face masses; it fires only once their
/// combined mass crosses `threshold` (i.e. after several ticks of growth).
const LIVE_COUPLE: &str = r#"
process Grow[rate :: Float = 0.5]
  ~{mass :: Float, interval :: Float = 1.0}
  ->{mass :: Float}
( delta = mass * rate * interval | {mass: delta} )

composite Cell[mass0 :: Float = 1.0, rate :: Float = 0.5]
  ~{edge :: any, mass :: Float @ mass = mass0}
  ->{mass :: Float @ mass}
( mass: mass0 | grow: Grow[rate: rate] ~{mass: mass} ->{mass: mass} )

reaction Bond[threshold :: Float = 3.5] (
  ?west :: Cell[mass: ?mw] ~{edge: ~e} | ?east :: Cell[mass: ?me] ~{edge: ~e}
  where ?mw + ?me > threshold
  => { bond: Bond[link: ~e] }
)

composite Environment[threshold :: Float = 3.5]
  ->{cells :: map[Cell] @ cells}
(
  link e :: any = 'e0' |
  cells: {
    'a': Cell[mass0: 1.0] ~{edge: ~e, mass: %.mass} ->{mass: %.mass},
    'b': Cell[mass0: 1.0] ~{edge: ~e, mass: %.mass} ->{mass: %.mass}
  } |
  rxn: BRS[rules: [Bond[threshold: threshold]]] ~{state: cells} ->{state: cells}
)

Environment[]
"#;

fn has_bond(state: &Value) -> bool {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .and_then(|m| m.get("bond"))
        .and_then(|b| b.get_field("_type"))
        .and_then(|t| t.as_str())
        == Some("Bond")
}

fn cell_keys(state: &Value) -> Vec<String> {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| {
            m.iter()
                .filter(|(k, v)| !k.starts_with('_') && v.get_field("_type").and_then(|t| t.as_str()) == Some("Cell"))
                .map(|(k, _)| k.to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_cross_composite_guard_reads_the_live_evolving_faces() {
    // Combined mass starts at 2.0 and grows ×1.5/tick. The reactor reads the
    // PRE-tick face (BSP snapshot): t1→2.0, t2→3.0, t3→4.5. Threshold 3.5.

    // Early: combined mass is still below the threshold → no coupling. The two
    // cells are present and growing.
    let early = run(LIVE_COUPLE, 2.0);
    assert!(!has_bond(&early), "no bond before the live masses cross the threshold: {early:?}");
    assert_eq!(cell_keys(&early).len(), 2, "both cells still running");

    // Later: growth has pushed the combined LIVE mass past the threshold, so the
    // cross-composite reaction — reading both running composites' faces — fires.
    let late = run(LIVE_COUPLE, 4.0);
    assert!(
        has_bond(&late),
        "the cross-composite reaction fired once growth raised the combined LIVE face mass past \
         the threshold — the reactor reached both running composites' evolving interiors through \
         the BRIDGE (not config.state): {late:?}"
    );
}
