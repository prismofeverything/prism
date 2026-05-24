//! Compositional invocation as a TRACE (decision #24, output→trace): `invoke_trace`
//! samples the entry composite's `->{outputs}` every `sample_dt` into a delta-log
//! `Trace[T]`, which serializes to the Arrow wire and round-trips. Batch `invoke`
//! is exactly this trace's final frame.

use std::collections::BTreeMap;

use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::invoke_trace;
use prism_schema::Value;

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn field(frame: &Value, key: &str) -> Option<f64> {
    frame.get_field(key).and_then(|v| v.as_f64())
}

// A pure (time-invariant) composite: result = value * factor.
const SCALE: &str = "\
composite Scale[factor :: Float = 1.0] ~{value :: Float @ amount} ->{result :: Float @ amount} (
  amount: value * factor
)
";

// An evolving composite: a Tick process adds 1.0 to `n` every tick (interval 1.0).
const COUNTER: &str = "\
process Tick ~{n :: Float} ->{n :: Float} ( {n: 1.0} )

composite Counter[start :: Float = 0.0] ->{n :: Float @ n} (
  n: start |
  Tick ~{n: n} ->{n: n}
)
";

#[test]
fn constant_composite_trace_round_trips_through_arrow() {
    let prog = chrysalis::parse::parse_program(SCALE).expect("parse");
    // duration 3, sample_dt 1 ⇒ frames at t = 0,1,2,3 (4 frames), all identical.
    let trace = invoke_trace(
        &prog,
        std_registry(),
        std_methods(),
        std_modules(),
        &args(&[("value", "10.0"), ("factor", "3.0")]),
        3.0,
        1.0,
    )
    .expect("invoke_trace");

    assert_eq!(prism_trace::len(&trace), 4, "one frame per sample, incl. t=0");

    // The Arrow wire round-trips the trace exactly.
    let bytes = prism_trace::serialize_trace(&trace).expect("serialize");
    let back = prism_trace::deserialize_trace(&bytes).expect("deserialize");
    assert_eq!(back, trace, "deserialize(serialize(trace)) ≡ trace");

    // No dynamics ⇒ every frame is {result: 30}.
    for f in prism_trace::frames(&trace) {
        assert_eq!(field(&f, "result"), Some(30.0), "constant trace; got {f:?}");
    }
}

#[test]
fn evolving_composite_trace_captures_dynamics() {
    let prog = chrysalis::parse::parse_program(COUNTER).expect("parse");
    // duration 5, sample_dt 1 ⇒ 6 frames; n grows by 1 each tick from start = 0.
    let trace = invoke_trace(
        &prog,
        std_registry(),
        std_methods(),
        std_modules(),
        &args(&[("start", "0.0")]),
        5.0,
        1.0,
    )
    .expect("invoke_trace");

    let ns: Vec<f64> =
        prism_trace::frames(&trace).iter().map(|f| field(f, "n").unwrap_or(f64::NAN)).collect();
    assert_eq!(ns, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0], "the trace captures per-tick growth");

    // …and that captured evolution round-trips on the Arrow wire.
    let back = prism_trace::deserialize_trace(&prism_trace::serialize_trace(&trace).unwrap()).unwrap();
    assert_eq!(back, trace);
}
