//! The **trace** — an event-sourced history of one item extended through time.
//!
//! A `Trace[T]` is the delta-log of `docs/delta-traces.md`: an `initial` state
//! (the fold seed) plus per-step `deltas`, where
//! `state(t) = fold(apply, initial, deltas[0..t])`. The element schema `T` is
//! carried alongside the data, so replay and plotting never re-infer it.
//!
//! The homoiconic shape (a `Value::Tree`, so a trace is itself bigraph state):
//!
//! ```text
//! { _type: "Trace", name, element: <schema_to_value(T)>,
//!   initial: <state(0)>, deltas: [<delta>…], times: [<float>…] }
//! ```
//!
//! `times` has one entry per frame (`deltas.len() + 1`). A no-change step is
//! recorded as a `None` delta (a no-op tick), so `deltas` stays 1:1 with steps
//! and the frame/time axes stay aligned. Full frames are *derived* (`frames`),
//! not stored — that is the point of a delta-log.

use prism_schema::{algebra, schema_to_value, value_to_schema, Schema, Value};

/// Build a `Trace[T]` (delta-log) from a sampled `(time, state)` stream under
/// element schema `T`. `initial` is the first sampled state; each subsequent
/// delta is `diff(T, prevₜ, nextₜ₊₁)` — the event advancing the state — with a
/// no-change step stored as `Value::None`.
///
/// Law (the diff/apply law #7 lifted over the stream):
/// `frames(trace_of(name, T, samples)) ≡ [s for (_, s) in samples]`.
pub fn trace_of(
    name: &str,
    element: &Schema,
    samples: impl IntoIterator<Item = (f64, Value)>,
) -> Value {
    let mut times: Vec<Value> = Vec::new();
    let mut deltas: Vec<Value> = Vec::new();
    let mut initial = Value::None;
    let mut prev: Option<Value> = None;

    for (t, state) in samples {
        times.push(Value::float(t));
        match &prev {
            None => initial = state.clone(),
            // `diff` returns `None` when the state is unchanged; record that as a
            // `None` delta so the delta/time axes stay aligned with the frames.
            Some(p) => deltas.push(algebra::diff(element, p, &state).unwrap_or(Value::None)),
        }
        prev = Some(state);
    }

    Value::tree([
        ("_type", Value::from("Trace")),
        ("name", Value::from(name)),
        ("element", schema_to_value(element)),
        ("initial", initial),
        ("deltas", Value::List(deltas)),
        ("times", Value::List(times)),
    ])
}

/// The carried element schema `T` (defaults to `Any` if absent or unreadable).
pub fn element(trace: &Value) -> Schema {
    trace.get_field("element").and_then(value_to_schema).unwrap_or(Schema::Any)
}

/// Number of frames (= sampled states = `deltas` + 1; `0` for an empty trace).
pub fn len(trace: &Value) -> usize {
    trace.get_field("times").and_then(|v| v.as_list()).map_or(0, |l| l.len())
}

/// Whether the trace has no frames.
pub fn is_empty(trace: &Value) -> bool {
    len(trace) == 0
}

/// The sample times, one per frame (length = [`len`]).
pub fn times(trace: &Value) -> Vec<f64> {
    trace
        .get_field("times")
        .and_then(|v| v.as_list())
        .map(|l| l.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

/// All reconstructed frames, oldest first — `scan(apply, initial, deltas)`.
/// A `None` delta is a no-op tick (the frame carries forward unchanged).
pub fn frames(trace: &Value) -> Vec<Value> {
    let n = len(trace);
    if n == 0 {
        return Vec::new();
    }
    let elem = element(trace);
    let mut cur = trace.get_field("initial").cloned().unwrap_or(Value::None);
    let mut out = Vec::with_capacity(n);
    out.push(cur.clone());
    if let Some(deltas) = trace.get_field("deltas").and_then(|v| v.as_list()) {
        for d in deltas {
            if !matches!(d, Value::None) {
                cur = algebra::apply(&elem, &cur, d);
            }
            out.push(cur.clone());
        }
    }
    out
}

/// The state at frame index `t` (0 = `initial`), by folding `deltas[0..t]`.
/// `t` past the end folds all deltas (clamps to the last frame).
pub fn state_at(trace: &Value, t: usize) -> Value {
    let elem = element(trace);
    let mut cur = trace.get_field("initial").cloned().unwrap_or(Value::None);
    if let Some(deltas) = trace.get_field("deltas").and_then(|v| v.as_list()) {
        for d in deltas.iter().take(t) {
            if !matches!(d, Value::None) {
                cur = algebra::apply(&elem, &cur, d);
            }
        }
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Map[Float]` — a state of named scalars (the common kinetics shape).
    fn map_float() -> Schema {
        Schema::Map { value: Box::new(Schema::float()) }
    }

    /// A `Map[Float]` value from `(name, magnitude)` pairs.
    fn mf(pairs: &[(&str, f64)]) -> Value {
        Value::tree(pairs.iter().map(|(k, v)| (*k, Value::float(*v))))
    }

    fn states(samples: &[(f64, Value)]) -> Vec<Value> {
        samples.iter().map(|(_, s)| s.clone()).collect()
    }

    #[test]
    fn frames_replay_the_sampled_states() {
        let elem = map_float();
        let samples = vec![
            (0.0, mf(&[("a", 1.0), ("b", 2.0)])),
            (1.0, mf(&[("a", 1.5), ("b", 2.0)])),
            (2.0, mf(&[("a", 2.0), ("b", 5.0)])),
        ];
        let expected = states(&samples);
        let trace = trace_of("test", &elem, samples);
        assert_eq!(len(&trace), 3);
        assert_eq!(times(&trace), vec![0.0, 1.0, 2.0]);
        // The law: replaying initial + deltas reconstructs every sampled state.
        assert_eq!(frames(&trace), expected);
    }

    #[test]
    fn structural_add_and_remove_replay() {
        let elem = map_float();
        let samples = vec![
            (0.0, mf(&[("a", 1.0)])),
            (1.0, mf(&[("a", 1.0), ("b", 2.0)])), // _add b
            (2.0, mf(&[("a", 1.0)])),             // _remove b
        ];
        let expected = states(&samples);
        let trace = trace_of("structural", &elem, samples);
        // Structural deltas (place-graph events) replay exactly like value deltas.
        assert_eq!(frames(&trace), expected);
    }

    #[test]
    fn no_change_step_is_a_noop_tick() {
        let elem = map_float();
        let samples = vec![
            (0.0, mf(&[("a", 1.0)])),
            (1.0, mf(&[("a", 1.0)])), // identical → a None delta
            (2.0, mf(&[("a", 3.0)])),
        ];
        let expected = states(&samples);
        let trace = trace_of("noop", &elem, samples);
        assert_eq!(len(&trace), 3, "a no-change step is still a frame");
        assert_eq!(frames(&trace), expected);
    }

    #[test]
    fn state_at_folds_to_index() {
        let elem = map_float();
        let trace = trace_of(
            "fold",
            &elem,
            vec![
                (0.0, mf(&[("a", 1.0)])),
                (1.0, mf(&[("a", 2.0)])),
                (2.0, mf(&[("a", 4.0)])),
            ],
        );
        assert_eq!(state_at(&trace, 0), mf(&[("a", 1.0)]));
        assert_eq!(state_at(&trace, 1), mf(&[("a", 2.0)]));
        assert_eq!(state_at(&trace, 2), mf(&[("a", 4.0)]));
    }

    #[test]
    fn carried_element_round_trips() {
        let elem = map_float();
        let trace = trace_of("elem", &elem, vec![(0.0, mf(&[("a", 1.0)]))]);
        assert_eq!(element(&trace), elem, "the element schema T is carried, not inferred");
    }

    #[test]
    fn empty_trace_has_no_frames() {
        let trace = trace_of("empty", &map_float(), Vec::<(f64, Value)>::new());
        assert_eq!(len(&trace), 0);
        assert!(is_empty(&trace));
        assert!(frames(&trace).is_empty());
    }
}
