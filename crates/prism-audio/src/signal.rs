//! The `Signal` value and type — a block of audio/CV samples on a wire, and a
//! per-frame mix bus.
//!
//! A `Signal` block is stored as a `Value::List` of samples (the carrier of
//! `Schema::Array { shape: [block], element: Float }`). But an audio bus is
//! *not* the same temporal object as a standing `Array` field, and the type
//! system is where that distinction belongs:
//!
//! | type | reconcile (within a tick) | apply (across ticks) | use |
//! |------|---------------------------|----------------------|-----|
//! | `Array` | additive sum | **additive** (accumulates) | a standing spatial / concentration field |
//! | `Signal` | additive sum | **replace** (this frame) | an audio / CV mix bus |
//!
//! So `Signal` is registered as a `Custom` type whose **representation is
//! `Array[Float]`** — which makes `reconcile` delegate to `Array`'s additive
//! sum, so several module outputs patched to one bus path *mix* at the BSP
//! barrier (shared with `Array`, not duplicated) — but whose **`apply` method
//! is overridden to replace**, so the bus holds *this* frame's mix and never
//! accumulates. `Array` is left untouched for the field callers. (The
//! precedent is `Qubits`, a `Custom` type that "owns its apply".)
//!
//! The PCM / Arrow codecs (`serialize`/`realize`) land at the device / network
//! boundary (A3/A8); for now they are the identity over the `List` carrier.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_schema::registry::{DivideContext, TypeMethods, TypeRegistry};
use prism_schema::{Schema, Value};

/// The registered name of the `Signal` type.
pub const SIGNAL: &str = "Signal";

/// The schema of a one-channel block of `block` samples — the `Array`
/// **representation** behind the `Signal` type (additive reconcile). Use this
/// directly only where additive-*accumulating* apply is wanted; for an audio
/// bus use [`signal_type`].
pub fn signal_schema(block: usize) -> Schema {
    Schema::array(vec![block], Schema::float())
}

/// The nominal `Signal` type — a per-frame audio/CV mix bus (additive
/// reconcile via its `Array` representation, replace apply via [`SignalMethods`]).
/// This is the schema to give a signal slot or port.
pub fn signal_type() -> Schema {
    Schema::Custom {
        name: SIGNAL.to_string(),
        parameters: IndexMap::new(),
    }
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

/// `TypeMethods` for `Signal`: the only override from its `Array`
/// representation is `apply` — **replace**, so a bus holds the reconciled mix
/// of *this* frame, not a growing accumulator. Mixing itself happens in
/// `reconcile` (additive, via the representation) before `apply` runs.
#[derive(Debug)]
struct SignalMethods;

impl TypeMethods for SignalMethods {
    fn default(&self, _r: &TypeRegistry, _s: &Schema) -> Value {
        // Explicit seeding supplies the correct block length; the registered
        // `default` (see `register_signal`) is the real silence value.
        Value::List(Vec::new())
    }

    /// Replace: the bus is *this* frame's reconciled mix, never accumulated.
    fn apply(&self, _r: &TypeRegistry, _s: &Schema, _state: &Value, update: &Value) -> Value {
        update.clone()
    }

    fn divide(
        &self,
        _r: &TypeRegistry,
        _s: &Schema,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value> {
        // A split voice shares the signal's shape; signals don't partition.
        vec![state.clone(); ctx.n_daughters.max(1)]
    }

    fn serialize(&self, _r: &TypeRegistry, _s: &Schema, state: &Value) -> Value {
        state.clone() // already List<Float>; PCM/Arrow codec lands at the boundary
    }

    fn realize(&self, _r: &TypeRegistry, _s: &Schema, encoded: &Value) -> Value {
        encoded.clone()
    }
}

/// Register the `Signal` type (representation = `Array[block]`, replace apply)
/// into a registry, so a `Custom("Signal")` slot mixes additively and holds
/// the current frame.
pub fn register_signal(reg: &mut TypeRegistry, block: usize) {
    reg.register_full(
        SIGNAL,
        signal_schema(block),
        Some(silence(block)),
        Some(Arc::new(SignalMethods)),
        Vec::new(),
    );
}

/// A fresh `TypeRegistry` carrying just the `Signal` type — enough to drive an
/// audio engine so that `Custom("Signal")` slots resolve.
pub fn signal_registry(block: usize) -> Arc<TypeRegistry> {
    let mut reg = TypeRegistry::new();
    register_signal(&mut reg, block);
    Arc::new(reg)
}
