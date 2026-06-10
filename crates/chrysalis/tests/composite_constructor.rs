//! `Composite[…]` — the rich-kind VALUE constructor (synth A6 Phase 2: "the
//! synth writes synths"). A reactum / expression builds a composite MODULE
//! *inline* as data; it lowers to the same instance-spec envelope a DEFINED
//! `composite` produces, so the node rung (`instantiate`) brings it to life and
//! runs it. The surface cash-out of the homoiconic face: build + install a
//! module value live (#61).

use std::collections::BTreeMap;

use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_core, std_modules};
use chrysalis::runner::invoke;
use prism_schema::Value;

fn invoke_out(src: &str, duration: f64) -> Value {
    let prog = parse_program(src).expect("parse");
    invoke(
        &prog,
        std_core(),
        std_modules(),
        &BTreeMap::new(),
        duration,
    )
    .expect("invoke")
}

/// Build a composite module inline (`Composite[state, bridge]`) holding an inner
/// `Tick` writing to inner `n`, bridge port `out` → inner `n`, outer output
/// `out` → sibling `bus`; then `instantiate` it and read the bridged result.
const STACK_DEMO: &str = "\
process Tick ~{n :: Float} ->{n :: Float} (
  {n: 1.0}
)
composite StackDemo ~{} ->{ result :: Float } (
  result: instantiate({
    stack: Composite[
      state: { n: 0.0, tick: Tick ~{n: n} ->{n: n} },
      bridge: { inputs: {}, outputs: { out: ['n'] } }
    ] ~{} ->{ out: bus },
    bus: 0.0
  }, 5.0).bus
)
";

#[test]
fn composite_constructor_builds_an_instantiable_module() {
    // The `Composite[…]` constructor produced a real subengine spec; `instantiate`
    // brought it to life; the inner `Tick` ran 5 ticks and its inner `n` was
    // bridged out (`out` → `bus`) → 5.0. The synth-writes-synths pattern, on the
    // surface, riding the node rung.
    let out = invoke_out(STACK_DEMO, 0.0);
    assert_eq!(
        out.get_field("result").and_then(|v| v.as_f64()),
        Some(5.0),
        "Composite[…] built a module, instantiate ran it, the inner Tick bridged 5.0 out: {out:?}"
    );
}

/// A probe reading the constructor's envelope shape directly.
const PROBE: &str = "\
composite Probe ~{} ->{ kind :: String, addr :: String } (
  kind: Composite[state: {x: 1.0}]._type |
  addr: Composite[state: {x: 1.0}].address
)
";

#[test]
fn composite_constructor_emits_the_composite_envelope() {
    // The bracket lowers to `{_type:"composite", address:"local:Composite", …}` —
    // the same envelope shape a defined composite / a hand-built spec uses, so
    // discovery + `Composite::from_config` recognise it.
    let out = invoke_out(PROBE, 0.0);
    assert_eq!(
        out.get_field("kind").and_then(|v| v.as_str()),
        Some("composite"),
        "Composite[…]._type is the composite kind hint: {out:?}"
    );
    assert_eq!(
        out.get_field("addr").and_then(|v| v.as_str()),
        Some("local:Composite"),
        "Composite[…].address targets the generic Composite factory: {out:?}"
    );
}
