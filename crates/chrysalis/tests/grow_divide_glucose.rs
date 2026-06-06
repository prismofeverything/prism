//! Grow / divide on a **shared glucose pool** — the link surface + reaction
//! division, end to end. This is the file `grow-divide-glucose.ys` exercises:
//!
//!   - `link glucose` is the shared pool — a value-bearing HYPEREDGE every cell
//!     attaches to by name (`~{glucose: ~glucose}`). All cells read the same
//!     level and their uptakes accumulate on the one slot, so growth is
//!     RESOURCE-LIMITED and the population SATURATES as glucose runs out.
//!   - Division is a first-class BRS reaction (`?c :: Cell[mass:?m] => ?c.divide()`):
//!     daughters inherit the pool attachment (the divided `CompositeLink` spec)
//!     and half the mass.
//!   - Mass is CONSERVED every tick: the pool's glucose becomes biomass 1:1
//!     (`Δmass = u`, `Δglucose = −u`) and division splits mass, so
//!     `glucose + Σ(cell mass) = const`.

use prism_bigraph::Engine;
use prism_schema::Value;

const SRC: &str = r#"
unit pg   : [mass]      = 1e-12 kg
unit fmol : [substance] = 1e-15 mol
Mass    = Quantity[unit: pg,   extensive]
Glucose = Quantity[unit: fmol, extensive]

process Grow[mu_max :: Float = 0.3, k_half :: Float = 50.0]
  ~{mass :: Float, glucose :: Float, interval :: Float = 1.0}
  ->{mass :: Mass, glucose :: Glucose}
(
  mu  = mu_max * glucose / (k_half + glucose) |
  raw = mu * mass * interval |
  u   = if raw > glucose then glucose else raw |
  {mass: u, glucose: -u}
)

composite Cell[mass0 :: Mass = 1.0]
  ~{glucose :: Float @ glucose = 0.0}
  ->{mass :: Mass @ mass, glucose :: Glucose @ glucose}
(
  mass: mass0 |
  glucose: glucose |
  grow: Grow[] ~{mass: mass, glucose: glucose} ->{mass: mass, glucose: glucose}
)

reaction Divide[threshold :: Mass = 2.0] (
  ?c :: Cell[mass: ?m] where ?m > threshold => ?c.divide()
)

composite Environment[glucose0 :: Float = 1000.0, threshold :: Mass = 2.0]
  ->{glucose :: Float @ glucose, cells :: map[Cell] @ cells}
(
  link glucose :: Glucose = glucose0 |
  cells: {
    '0': Cell[mass0: 1.2] ~{glucose: ~glucose} ->{mass: %.mass, glucose: ~glucose}
  } |
  rxn: BRS[rules: [Divide[threshold: threshold]]] ~{state: cells} ->{state: cells}
)

Environment[]
"#;

fn run(time: f64) -> Value {
    let program = chrysalis::parse::parse_program(SRC).expect("parse");
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

fn pool(state: &Value) -> f64 {
    state.get_field("glucose").and_then(|v| v.as_f64()).expect("glucose pool")
}

fn cell_masses(state: &Value) -> Vec<f64> {
    state
        .get_field("cells")
        .and_then(|v| v.as_map())
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .filter_map(|(_, v)| v.get_field("mass").and_then(|x| x.as_f64()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn shared_pool_limits_growth_and_mass_is_conserved() {
    // Conserved total = initial pool + seed biomass.
    const TOTAL: f64 = 1000.0 + 1.2;

    let s0 = run(0.0);
    assert_eq!(cell_masses(&s0).len(), 1, "one seed cell");
    assert!((pool(&s0) - 1000.0).abs() < 1e-6, "pool seeded at 1000");
    assert!((pool(&s0) + cell_masses(&s0).iter().sum::<f64>() - TOTAL).abs() < 1e-6);

    // Mid-run: the cell has grown + divided (population > 1), the pool has been
    // drawn down, and glucose + Σmass is STILL the conserved total — every tick.
    for t in [5.0, 20.0, 60.0] {
        let s = run(t);
        let masses = cell_masses(&s);
        let total = pool(&s) + masses.iter().sum::<f64>();
        assert!(
            (total - TOTAL).abs() < 1e-3,
            "t={t}: glucose ({}) + Σmass ({}) = {total}, expected {TOTAL}",
            pool(&s),
            masses.iter().sum::<f64>(),
        );
    }

    // The shared pool LIMITS growth: division happened (>1 cell), the pool is
    // depleted (growth drew it down), and the population stays BOUNDED — a
    // runaway (mother not removed / daughters re-divide same tick) would blow up.
    let s = run(60.0);
    let masses = cell_masses(&s);
    assert!(masses.len() > 1, "division happened: {} cells", masses.len());
    assert!(pool(&s) < 1000.0, "the shared pool was drawn down: {}", pool(&s));
    assert!(masses.len() < 2000, "bounded, not runaway: {} cells", masses.len());
}
