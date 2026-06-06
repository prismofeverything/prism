//! `?c :: Cell[mass: ?m] where ?m > t => ?c.divide()` — cell division driven by
//! a first-class BRS **reaction** (vs the Form-3 Divider step). This is the
//! end-to-end proof of the brand/subtype type system (#30 / Cardelli F₁<:):
//!
//!   - the redex matches a composite cell BY ITS BRAND (`_type: "Cell"`, which
//!     the matcher's sort-check sees directly now that a composite node carries
//!     its NAME, unified with how a molecule node carries `_type: "ERK"`);
//!   - `?c` binds the matched NODE as a typed value (a `BindingSource::Node`),
//!     so `?c.divide()` dispatches the cell's schema-driven split;
//!   - `?c.divide()` returns the PRODUCTS and the firing (`reaction_delta`)
//!     consumes the mother + keys the daughters `<mother>_0/_1` — the container
//!     owns the key (same `divide_by_schema(CompositeLink)` the Divider uses).
//!
//! Invariants (memory `feedback_grow_divide_runaway`): division CONSERVES mass
//! and TERMINATES (daughters fall below threshold; no per-tick runaway).

use prism_bigraph::Engine;
use prism_schema::Value;

/// Compile + run an inline `.ys` program and return the final root state.
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

/// `(key, mass)` for every cell in the `cells` map (skipping `_`-sentinels).
fn cells(state: &Value) -> Vec<(String, f64)> {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .filter_map(|(k, v)| {
                    v.get_field("mass").and_then(|x| x.as_f64()).map(|mass| (k.to_string(), mass))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// No growth + a single over-threshold cell: division must CONSERVE mass exactly
/// and TERMINATE (the two half-mass daughters sit at the threshold, `2.0 > 2.0`
/// is false, so they never re-divide).
const NO_GROWTH: &str = r#"
unit pg : [mass] = 1e-12 kg
Mass = Quantity[unit: pg, extensive]

process Grow[rate :: Float = 0.0]
  ~{mass :: Float, interval :: Float = 1.0}
  ->{mass :: Mass}
( delta = mass * rate * interval | {mass: delta} )

composite Cell[mass0 :: Mass = 1.0, rate :: Float = 0.0]
  ~{mass :: Mass @ mass = mass0}
  ->{mass :: Mass @ mass}
( mass: mass0 | grow: Grow[rate: rate] ~{mass: mass} ->{mass: mass} )

reaction Divide[threshold :: Mass = 2.0] (
  ?c :: Cell[mass: ?m] where ?m > threshold => ?c.divide()
)

composite Environment[threshold :: Mass = 2.0]
  ->{cells :: map[Cell] @ cells}
(
  cells: { '0': Cell[mass0: 4.0] ~{mass: %.mass} ->{mass: %.mass} } |
  rxn: BRS[rules: [Divide[threshold: threshold]]] ~{state: cells} ->{state: cells}
)

Environment[]
"#;

#[test]
fn reaction_divide_conserves_mass_and_terminates() {
    let s0 = run(NO_GROWTH, 0.0);
    let c0 = cells(&s0);
    assert_eq!(c0.len(), 1, "one cell at t=0");
    let total0: f64 = c0.iter().map(|(_, m)| m).sum();
    assert!((total0 - 4.0).abs() < 1e-9, "seed mass 4.0, got {total0}");

    // After the division settles: exactly two daughters, each half the mother.
    let s = run(NO_GROWTH, 3.0);
    let c = cells(&s);
    assert_eq!(c.len(), 2, "one over-threshold cell splits into exactly two: {c:?}");
    let total: f64 = c.iter().map(|(_, m)| m).sum();
    assert!((total - 4.0).abs() < 1e-9, "mass conserved across the split: {total} (cells {c:?})");
    for (k, m) in &c {
        assert!((m - 2.0).abs() < 1e-9, "daughter {k} carries half the mother: {m}");
    }
}

/// Growing cells divide REPEATEDLY as biomass accumulates — division happens
/// many times, the population grows, and it stays BOUNDED (no per-tick runaway
/// on a single cell: the engine removes the mother and the daughters are below
/// threshold the tick they're born).
const GROWTH: &str = r#"
unit pg : [mass] = 1e-12 kg
Mass = Quantity[unit: pg, extensive]

process Grow[rate :: Float = 0.5]
  ~{mass :: Float, interval :: Float = 1.0}
  ->{mass :: Mass}
( delta = mass * rate * interval | {mass: delta} )

composite Cell[mass0 :: Mass = 1.0, rate :: Float = 0.5]
  ~{mass :: Mass @ mass = mass0}
  ->{mass :: Mass @ mass}
( mass: mass0 | grow: Grow[rate: rate] ~{mass: mass} ->{mass: mass} )

reaction Divide[threshold :: Mass = 2.0] (
  ?c :: Cell[mass: ?m] where ?m > threshold => ?c.divide()
)

composite Environment[threshold :: Mass = 2.0]
  ->{cells :: map[Cell] @ cells}
(
  cells: { '0': Cell[mass0: 1.2] ~{mass: %.mass} ->{mass: %.mass} } |
  rxn: BRS[rules: [Divide[threshold: threshold]]] ~{state: cells} ->{state: cells}
)

Environment[]
"#;

#[test]
fn reaction_divide_grows_population_bounded() {
    let c1 = cells(&run(GROWTH, 1.0));
    assert_eq!(c1.len(), 1, "no division before mass crosses threshold");

    // After several growth+divide cycles the population has grown past one cell.
    let c = cells(&run(GROWTH, 8.0));
    assert!(c.len() > 1, "division happened: {} cells", c.len());
    // Bounded: a runaway (mother not removed / daughters re-divide same tick)
    // would blow up exponentially every tick. With ~3 doublings in 8 ticks the
    // count is small.
    assert!(c.len() < 64, "bounded, not runaway: {} cells", c.len());
    for (k, m) in &c {
        assert!(*m > 0.0 && *m < 10.0, "cell {k} mass sane (no runaway): {m}");
    }
}
