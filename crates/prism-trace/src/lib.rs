//! prism-trace — the **delta-log trace**: an event-sourced history of one item
//! extended through time, plus (later) its Arrow-IPC wire codec.
//!
//! See [`docs/delta-traces.md`]. A `Trace[T]` is `initial` + `deltas`, where
//! `state(t) = fold(apply, initial, deltas[0..t])` — the schema algebra's
//! `apply` is the replay operator, `diff` the capture operator. The element
//! schema `T` is **carried** (never re-inferred — memory
//! `feedback_carry_dont_infer_schema`), so replay and plotting dispatch on the
//! real type.
//!
//! This is the currency shared by `Simulate` (capture), `prism_viz::plot`
//! (render), and chrysalis streaming invocation (the pipe wire). Capture and
//! replay are *defined in terms of* the algebra — no hand-rolled delta munging.
//!
//! [`docs/delta-traces.md`]: https://example.invalid/delta-traces

pub mod codec;
pub mod trace;

pub use codec::{deserialize_trace, serialize_trace, CodecError, TraceReader, TraceWriter};
pub use trace::{element, frames, is_empty, len, state_at, times, trace_of};
