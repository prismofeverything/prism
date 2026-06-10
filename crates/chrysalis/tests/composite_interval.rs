//! #70 — a composite's `interval` is its OUTER scheduling rate, exactly like a
//! process's: a value that lands in the engine front and can be wired.
//!
//! THE INVARIANT (the law this pins): the composite's interval is *outer
//! bookkeeping only* — the inner dynamics are INDEPENDENT of how often the outer
//! engine ticks the composite. Running the outer composite once for 60 units or
//! 60 times for 1 unit yields IDENTICAL inner results, because inner time
//! accumulates and the inner processes tick at their OWN intervals. A composite
//! is a time-transparent boundary; "larger outer step" never changes "what the
//! interior computed", only how coarsely we observe/sync it.

use prism_bigraph::Engine;
use prism_schema::Value;

// Inc adds its own timestep to `count` each tick, so `count` tracks elapsed
// inner time. Counter wraps it and declares its OWN (outer) interval. Harness
// holds Counter as a child whose interval we vary.
const PROG: &str = r#"
process Inc[] ~{count :: Float, interval :: Float = 1.0} ->{count :: Float} (
  {count: interval}
)

composite Counter[interval :: Float = 1.0] ->{count :: Float @ count} (
  count: 0.0 |
  inc: Inc[] ~{count: count} ->{count: count}
)

composite Harness[child_dt :: Float = 1.0] ->{count :: Float @ count} (
  count: 0.0 |
  c: Counter[interval: child_dt] ->{count: count}
)
"#;

/// Run the harness (whose child Counter ticks at `child_dt`) for `duration` and
/// read the accumulated inner count forwarded out.
fn count_after(child_dt: f64, duration: f64) -> f64 {
    let src = format!("{PROG}\nHarness[child_dt: {child_dt:?}]\n");
    let program = chrysalis::parse::parse_program(&src).expect("parse");
    let result = chrysalis::compile::compile_with_core(&program, chrysalis::prelude::std_core(), chrysalis::compile::ModuleRegistry::new())
    .expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine");
    engine.discover_all_processes();
    engine.run(duration);
    engine
        .state()
        .get_field("count")
        .and_then(Value::as_f64)
        .unwrap_or(f64::NAN)
}

#[test]
fn composite_interval_does_not_change_inner_dynamics() {
    // Same total time, three OUTER cadences for the same composite.
    let fine = count_after(1.0, 60.0);
    let mid = count_after(10.0, 60.0);
    let coarse = count_after(60.0, 60.0);
    eprintln!("inner count:  child_dt=1 -> {fine}   child_dt=10 -> {mid}   child_dt=60 -> {coarse}");

    // The dynamics ran (not trivially zero) and tracked elapsed inner time.
    assert!((fine - 60.0).abs() < 1e-6, "child_dt=1: count={fine}, want 60");
    // THE INVARIANT: the composite's interval is outer bookkeeping — it must NOT
    // change what the interior computed.
    assert!(
        (fine - mid).abs() < 1e-6 && (fine - coarse).abs() < 1e-6,
        "composite interval changed inner dynamics: 1->{fine}  10->{mid}  60->{coarse}"
    );
}
