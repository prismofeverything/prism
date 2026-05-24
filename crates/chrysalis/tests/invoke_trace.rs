//! Compositional invocation as a TRACE (decision #24, output→trace): `invoke_trace`
//! samples the entry composite's `->{outputs}` every `sample_dt` into a delta-log
//! `Trace[T]`, which serializes to the Arrow wire and round-trips. Batch `invoke`
//! is exactly this trace's final frame.

use std::collections::BTreeMap;

use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::{invoke_driven, invoke_trace};
use prism_schema::{Schema, Value};

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
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

    assert_eq!(
        prism_trace::len(&trace),
        4,
        "one frame per sample, incl. t=0"
    );

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

    let ns: Vec<f64> = prism_trace::frames(&trace)
        .iter()
        .map(|f| field(f, "n").unwrap_or(f64::NAN))
        .collect();
    assert_eq!(
        ns,
        vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        "the trace captures per-tick growth"
    );

    // …and that captured evolution round-trips on the Arrow wire.
    let back =
        prism_trace::deserialize_trace(&prism_trace::serialize_trace(&trace).unwrap()).unwrap();
    assert_eq!(back, trace);
}

// B: its driven input `n` initializes inner `v`, which the output `out` reads —
// so each injected frame shows straight through (a pure echo, no process needed).
const ECHO: &str = "\
composite Echo ~{n :: Float @ v} ->{out :: Float @ v} (
  v: n
)
";

#[test]
fn pipe_round_trip_drives_b_from_a_over_arrow() {
    // A: produce an output trace n = [0,1,2,3,4,5].
    let a = chrysalis::parse::parse_program(COUNTER).expect("parse A");
    let a_trace = invoke_trace(
        &a,
        std_registry(),
        std_methods(),
        std_modules(),
        &args(&[("start", "0.0")]),
        5.0,
        1.0,
    )
    .expect("A trace");

    // Cross the wire: serialize A's output, deserialize as B's input (the pipe).
    let bytes = prism_trace::serialize_trace(&a_trace).expect("serialize");
    let b_input = prism_trace::deserialize_trace(&bytes).expect("deserialize");

    // B: driven by A's trace, echoes n→out each tick.
    let b = chrysalis::parse::parse_program(ECHO).expect("parse B");
    let b_trace = invoke_driven(
        &b,
        std_registry(),
        std_methods(),
        std_modules(),
        &args(&[]),
        &b_input,
    )
    .expect("B driven");

    let outs: Vec<f64> = prism_trace::frames(&b_trace)
        .iter()
        .map(|f| field(f, "out").unwrap_or(f64::NAN))
        .collect();
    assert_eq!(
        outs,
        vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        "B's output follows A's trace, driven per tick across the Arrow wire"
    );
}

#[test]
fn mismatched_input_stream_is_rejected_at_connect() {
    // A trace of bare Floats cannot drive a composite wanting ~{n :: Float}.
    let bad = prism_trace::trace_of("bad", &Schema::float(), vec![(0.0, Value::float(1.0))]);
    let b = chrysalis::parse::parse_program(ECHO).expect("parse");
    let err = invoke_driven(
        &b,
        std_registry(),
        std_methods(),
        std_modules(),
        &args(&[]),
        &bad,
    )
    .expect_err("a non-refining input stream should be rejected");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("refine") || msg.contains("input"),
        "clear connect-time error; got: {err}"
    );
}

// An accumulator: `Bump` adds 1.0 to `total` every interval (1.0), so `total`
// tracks cumulative *sim time* — making it sensitive to the per-step dt.
const ACC: &str = "\
process Bump ~{total :: Float} ->{total :: Float} ( {total: 1.0} )

composite Acc ~{tick :: Float @ marker} ->{total :: Float @ total} (
  total: 0.0 |
  marker: 0.0 |
  Bump ~{total: total} ->{total: total}
)
";

#[test]
fn driven_advances_by_irregular_frame_times() {
    // An input trace with IRREGULAR times (gaps 2, then 3). Because the consumer
    // advances by each frame's actual time-delta, `total` = cumulative sim time:
    // [0, 0+2, 2+3] = [0, 2, 5] — NOT [0,1,2] as a fixed dt=1 would give.
    let elem = Schema::Tree {
        branches: [(prism_schema::Key::from("tick"), Schema::float())]
            .into_iter()
            .collect(),
    };
    let rec = |t: f64| Value::tree([("tick", Value::float(t))]);
    let input = prism_trace::trace_of(
        "drive",
        &elem,
        vec![(0.0, rec(0.0)), (2.0, rec(0.0)), (5.0, rec(0.0))],
    );

    let acc = chrysalis::parse::parse_program(ACC).expect("parse acc");
    let out = invoke_driven(
        &acc,
        std_registry(),
        std_methods(),
        std_modules(),
        &args(&[]),
        &input,
    )
    .expect("driven");

    let totals: Vec<f64> = prism_trace::frames(&out)
        .iter()
        .map(|f| {
            f.get_field("total")
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN)
        })
        .collect();
    assert_eq!(
        totals,
        vec![0.0, 2.0, 5.0],
        "the consumer advances by each frame's real dt"
    );
}
