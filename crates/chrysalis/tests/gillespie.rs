//! The `.ys` MinimalGillespie (`ys/gillespie.ys`) end to end: a Process fires the
//! reaction and a separate Step overwrites the shared `clock` driving the event's
//! `interval`. The engine schedules the event by the **dynamic** τ = 1/(k·A) — so
//! `interval` is an ordinary overridable input, the step drives it, and the
//! timestep varies. Proves dynamic timesteps + `overwrite[Float]` in chrysalis.

use prism_bigraph::Engine;

#[test]
fn ys_gillespie_drives_a_dynamic_timestep() {
    let src = include_str!("../ys/gillespie.ys");
    let program = chrysalis::parse::parse_program(src).expect("parse gillespie.ys");
    let result = chrysalis::compile::compile(&program).expect("compile");

    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine");
    engine.discover_all_processes();

    // Step event-by-event; the gap between consecutive event times IS the interval
    // the engine scheduled by (chosen each tick by the GillespieInterval step).
    let round3 = |x: f64| (x * 1000.0).round() / 1000.0;
    let mut times = Vec::new();
    for _ in 0..5 {
        if engine.tick().is_none() {
            break;
        }
        times.push(engine.time());
    }
    let mut intervals = Vec::new();
    let mut prev = 0.0;
    for &t in &times {
        intervals.push(round3(t - prev));
        prev = t;
    }
    let a = engine
        .state()
        .get_field("a")
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0);
    let b = engine
        .state()
        .get_field("b")
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0);
    eprintln!("dynamic intervals: {intervals:?}; a={a}, b={b}");

    // τ = 1/(k·A) for A = 5,4,3,2,1 — the step's `overwrite` lands and the engine
    // re-reads the live interval each tick (additive would give [0.2,0.45,…]).
    assert_eq!(
        intervals,
        vec![0.2, 0.25, 0.333, 0.5, 1.0],
        "the .ys step drives a dynamic τ = 1/(k·A); got {intervals:?}"
    );
    assert_eq!(a, 0.0, "all five A reacted");
    assert_eq!(b, 5.0, "…into five B");
}
