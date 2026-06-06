//! First-class `link` — a shared value-bearing **hyperedge** (the bigraph link
//! graph). `link glucose :: Float = 100.0` declares a pool; two `Eat` processes
//! attach via `~{glucose: ~glucose}` and both deplete the ONE pool: each reads
//! the same pre-tick level (BSP snapshot) and its uptake accumulates on the
//! shared slot. The attachment is resolved by NAME up the place graph (engine
//! `resolve_link`) — depth-independent, no `^.glucose` path-counting, and a
//! daughter inheriting the wire would attach wherever it lands.

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

const POOL: &str = r#"
process Eat[k :: Float = 0.1]
  ~{glucose :: Float, interval :: Float = 1.0}
  ->{glucose :: Float}
( u = k * glucose * interval | {glucose: -u} )

composite Environment[]
  ->{glucose :: Float @ glucose}
(
  link glucose :: Float = 100.0 |
  eaters: {
    e0: Eat[] ~{glucose: ~glucose} ->{glucose: ~glucose},
    e1: Eat[] ~{glucose: ~glucose} ->{glucose: ~glucose}
  }
)

Environment[]
"#;

#[test]
fn link_pool_is_one_shared_hyperedge() {
    let g = |s: &Value| s.get_field("glucose").and_then(|v| v.as_f64());

    // t=0: the pool is full.
    assert_eq!(g(&run(POOL, 0.0)), Some(100.0), "pool seeded at 100");

    // t=1: BOTH eaters drew u = 0.1·100 = 10 from the SAME pool (each reads the
    // same pre-tick 100 under the BSP snapshot), and both uptakes accumulate on
    // the one slot → 100 − 20 = 80. A single eater alone would leave 90, so the
    // 80 is the proof that `~glucose` is ONE shared hyperedge, not two slots.
    let g1 = g(&run(POOL, 1.0)).expect("glucose after 1 tick");
    assert!(
        (g1 - 80.0).abs() < 1e-6,
        "both eaters deplete the one shared pool (expected 80): {g1}"
    );

    // It keeps depleting (compounding 0.8×/tick): 80 → 64.
    let g2 = g(&run(POOL, 2.0)).expect("glucose after 2 ticks");
    assert!(
        (g2 - 64.0).abs() < 1e-6,
        "the shared pool keeps depleting (expected 64): {g2}"
    );
}
