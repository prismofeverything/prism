//! **Stage 4a — the metacircular close** (homoiconic-unification §3/§4). The
//! reflective tower has two rungs; this pins the LOWER one, made callable from
//! `.ys`:
//!
//!   - UPPER — `meta::eval` : `Expr → spec data` (reduce a constructor to its
//!     data form). Already first-class.
//!   - LOWER — `instantiate(state, time)` : `spec data → running` (bring a
//!     spec-bearing state to life and run it). NEW — the surface exposure of
//!     prism's engine `discover_processes`.
//!
//! Together they make the metacircular identity
//! `run(p) = instantiate(surface_eval(quote(p)))` a *callable* loop: a running
//! `.ys` program can build a node SPEC as data and `instantiate` it into a live
//! node — reflection from the surface. The substrate this rides is proven by
//! `crates/prism-bigraph/tests/reaction_creates_process.rs` (a reactum `_add`s a
//! spec and the engine evals it to a live node); here a *program* drives the
//! same eval explicitly.
//!
//! The key reason `instantiate` is an Evaluator builtin (not a core-less `meta::`
//! host fn): it runs against the program's OWN `Core`, so a built spec's
//! `local:Tick` address resolves to *this program's* `process Tick` factory.

use std::collections::BTreeMap;

use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::invoke;
use prism_schema::Value;

/// Baseline: `Tick` (emits +1.0 to `count` each tick) run DIRECTLY as a
/// composite body. After 5 ticks, `count == 5.0`.
const DIRECT: &str = "\
process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)
composite Direct ->{ count :: Float } (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}
)
";

/// The SAME `Tick`, but its node is built as DATA (a `{_type, address, inputs,
/// outputs}` spec — exactly what the `tick:` wiring above lowers to) and brought
/// to life with `instantiate(state, time)` INSIDE the composite body. The inner
/// 5 ticks run eagerly at body-eval time, so the outer run needs no duration.
/// `instantiate` resolves `local:Tick` against this program's own Core.
const VIA_INSTANTIATE: &str = "\
process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)
composite ViaInstantiate ->{ result :: Float } (
  result: instantiate({
    'count': 0.0,
    'tick': {
      '_type': 'process',
      'address': 'local:Tick',
      'inputs': { 'count': ['count'] },
      'outputs': { 'count': ['count'] }
    }
  }, 5.0).count
)
";

/// Compile + invoke `src`'s entry composite for `duration`, returning its output
/// record (the `->{…}` ports, serialized).
fn invoke_out(src: &str, duration: f64) -> Value {
    let prog = parse_program(src).expect("parse");
    invoke(
        &prog,
        std_registry(),
        std_methods(),
        std_modules(),
        &BTreeMap::new(),
        duration,
    )
    .expect("invoke")
}

#[test]
fn instantiate_brings_a_built_spec_to_life() {
    // A `.ys` program builds a process node spec as DATA and `instantiate`s it
    // into a running node — the surface exposure of `discover_processes`. The
    // built `local:Tick` resolves because `instantiate` uses the program's own
    // Core. 5 ticks → the evolved `count` is 5.0.
    let out = invoke_out(VIA_INSTANTIATE, 0.0);
    let result = out.get_field("result").and_then(|v| v.as_f64());
    assert_eq!(
        result,
        Some(5.0),
        "instantiate ran the hand-built Tick spec for 5 ticks (out={out:?})"
    );
}

#[test]
fn instantiate_equals_running_the_spec_directly() {
    // The metacircular identity at the node rung: running `Tick` DIRECTLY as a
    // composite body reaches the same count as building the SAME spec as data and
    // `instantiate`-ing it. `run(spec-as-body) == instantiate(spec-as-data)` —
    // the two surfaces of one node are one running thing.
    let direct = invoke_out(DIRECT, 5.0)
        .get_field("count")
        .and_then(|v| v.as_f64());
    let via = invoke_out(VIA_INSTANTIATE, 0.0)
        .get_field("result")
        .and_then(|v| v.as_f64());
    assert_eq!(direct, Some(5.0), "baseline: Tick run directly reaches 5.0");
    assert_eq!(
        direct, via,
        "instantiate(spec-as-data) == run(spec-as-body): the metacircular node rung"
    );
}

#[test]
fn instantiate_rejects_a_non_spec_value() {
    // `instantiate` expects a spec-bearing STATE (a map). A scalar is a clear
    // error, not a silent no-op — the surface op validates its input.
    const BAD: &str = "\
composite Bad ->{ result :: Float } (
  result: instantiate(3.0, 1.0)
)
";
    let prog = parse_program(BAD).expect("parse");
    let err = invoke(
        &prog,
        std_registry(),
        std_methods(),
        std_modules(),
        &BTreeMap::new(),
        0.0,
    );
    assert!(
        err.is_err(),
        "instantiate(non-map) must error, not silently no-op (got {err:?})"
    );
}
