//! The `Signal` value — a block of audio/CV samples on a wire.
//!
//! For A1 a `Signal` is represented structurally as `Schema::Array { shape:
//! [block], element: Float }`: a fixed-size block of samples (one tick = one
//! block, see [`crate::oscillator`]). The `Array` choice is deliberate and
//! load-bearing — its `apply` / `reconcile` are **element-wise additive**
//! (`prism_schema::reconcile::reconcile_array`), so several module outputs
//! wired into one bus path *sum* at the BSP barrier: **a mix bus falls out of
//! the algebra**, no `Mixer` node required (synthesis-bigraphs.md §III).
//!
//! The named `Signal` `Custom` type and its PCM/Arrow codecs are added when
//! their consumers exist — the chrysalis import surface (A2) and the device /
//! network boundary (A3/A8) — *over this same representation*. So this is the
//! real representation, not a stand-in: naming and codecs are additive.

use prism_schema::{Schema, Value};

/// The schema of a one-channel audio/CV block of `block` samples.
///
/// `Array` ⇒ additive `apply`: two signals landing on one path sum (mixing).
pub fn signal_schema(block: usize) -> Schema {
    Schema::array(vec![block], Schema::float())
}

/// A block of `block` zeros — silence, and the additive identity of a mix bus.
pub fn silence(block: usize) -> Value {
    Value::List(vec![Value::float(0.0); block])
}

/// Build a `Signal` value from f32 samples (the device / network boundary
/// form; prism carries samples as f64 internally).
pub fn signal_from_slice(samples: &[f32]) -> Value {
    Value::List(samples.iter().map(|&s| Value::float(s as f64)).collect())
}

/// Read a `Signal` value back to f32 samples (the boundary form). A non-list
/// value yields an empty block; non-numeric entries read as `0.0`.
pub fn signal_to_vec(signal: &Value) -> Vec<f32> {
    match signal.as_list() {
        Some(items) => items
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect(),
        None => Vec::new(),
    }
}
