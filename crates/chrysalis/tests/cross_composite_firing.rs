//! #43 firing — a cross-composite LINK-GRAPH reaction FIRES end-to-end through
//! the engine (the surface → compile → `BigraphicalReactiveSystem` → engine path).
//!
//! The matcher (`cross_composite_link_redex.rs`) proves the redex COUPLES two
//! sealed composites on a shared link. This proves it FIRES: two `Cell`
//! composites coupled on a shared `link e` are replaced (faithful BRS: redex →
//! reactum) by a `bond` recording the link `~e`. The link-graph form needs NO
//! unfurl — it matches the composites' published ports, so it runs through the
//! EXISTING `BRS`, not the place-graph `CrossCompositeReactor`.
//!
//! The reactum is a STRUCTURAL map `{ bond: Bond[link: ~e] }`: structural so the
//! bound link `~e` resolves through prism's `instantiate` (a computed reactum
//! can't yet read a `~link` as a value), and a map so the List redex's fire
//! emits a well-formed `_add`. Coupling that MODIFIES the composites in place
//! (diffusion — `?west.balance(~e)`) is the next refinement; it needs list-bound
//! sites to carry their matched key (today only a top-level `?c :: Cell` does).

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

const COUPLE: &str = r#"
composite Cell[mass0 :: Float = 1.0]
  ~{edge :: any}
  ->{mass :: Float @ mass}
( mass: mass0 )

reaction Couple (
  ?west :: Cell ~{edge: ~e} | ?east :: Cell ~{edge: ~e}
  => { bond: Bond[link: ~e] }
)

composite Environment
  ->{cells :: map[Cell] @ cells}
(
  link e :: any = 'e0' |
  cells: {
    'a': Cell[mass0: 1.0] ~{edge: ~e} ->{mass: %.mass},
    'b': Cell[mass0: 1.0] ~{edge: ~e} ->{mass: %.mass}
  } |
  rxn: BRS[rules: [Couple]] ~{state: cells} ->{state: cells}
)

Environment[]
"#;

#[test]
fn cross_composite_link_reaction_fires_and_produces_a_bond() {
    let s = run(COUPLE, 2.0);
    let cells = s
        .get_field("cells")
        .and_then(|v| v.as_map())
        .expect("cells map present");
    // The coupling FIRED: the two cells (coupled on `~e`) were replaced by a
    // `bond` recording the shared link.
    let bond = cells
        .get("bond")
        .unwrap_or_else(|| panic!("expected a `bond` from the cross-composite reaction; cells={cells:?}"));
    assert_eq!(
        bond.get_field("_type").and_then(|v| v.as_str()),
        Some("Bond"),
        "the produced entity is a Bond: {bond:?}"
    );
    // Faithful BRS: the two coupled Cells were consumed (replaced by the reactum).
    let remaining_cells = cells
        .iter()
        .filter(|(k, v)| !k.starts_with('_') && v.get_field("_type").and_then(|t| t.as_str()) == Some("Cell"))
        .count();
    assert_eq!(remaining_cells, 0, "both coupled cells consumed by the reaction; cells={cells:?}");
}
