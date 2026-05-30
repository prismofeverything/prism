//! The `Qubits` type — quantum register state as a first-class Custom type.
//!
//! A `Qubits` value is a `map[float]` from basis bitstring (`'00'`, `'101'`, …)
//! to amplitude, tagged `_type: "Qubits"` so value-method dispatch
//! (`state.cnot(0, 1)`, `a.tensor(b)`, `if s.separable(1) then …`) resolves
//! against this type. The quantum operations are the type's ALGEBRA — methods
//! on `Qubits`, not free functions floating in a `meta` namespace
//! (`project_schema_dispatch` / `feedback_schema_algebra`).
//!
//! `apply = overwrite`: a quantum state is a new amplitude vector — it REPLACES,
//! never additively accumulates. The type carrying its own composition law is
//! what makes a republished snapshot stable across ticks (no doubling) without
//! any per-slot `overwrite[…]` annotation — the type knows how it composes.
//!
//! Gates are N-qubit-general over the bitstring keys (qubit `q` = character at
//! index `q`), replacing the hand-written 2-qubit-specific `amp_XX` bodies the
//! demos used to carry. See `docs/quantum-bigraphs.md`.

use indexmap::IndexMap;
use prism_schema::registry::{DivideContext, TypeMethods, TypeRegistry};
use prism_schema::{Key, MethodError, MethodRegistry, MethodResult, Schema, Value};

const TYPE_NAME: &str = "Qubits";
/// The result type of `Qubits.factorize(k)`: a separability verdict + the two
/// factor registers. A real named type, not an anonymous `{separable, a, b}`
/// record repeated at every use site.
const FACTORIZATION: &str = "Factorization";

// ── Representation helpers ───────────────────────────────────────────────

/// Amplitude entries of a `Qubits` value: `(bitstring, amplitude)` for every
/// non-sentinel key (`_type` and any `_`-prefixed metadata are skipped).
fn amps(v: &Value) -> Vec<(String, f64)> {
    match v.as_map() {
        Some(m) => m
            .iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, val)| (k.to_string(), val.as_f64().unwrap_or(0.0)))
            .collect(),
        None => Vec::new(),
    }
}

/// Build a `Qubits` value from `(bitstring, amplitude)` pairs, summing
/// duplicate keys (gates that map several basis states onto one) and stamping
/// `_type: "Qubits"` so the result is itself dispatchable + chainable.
fn build(pairs: impl IntoIterator<Item = (String, f64)>) -> Value {
    let mut acc: IndexMap<Key, f64> = IndexMap::new();
    for (k, a) in pairs {
        *acc.entry(Key::from(k.as_str())).or_insert(0.0) += a;
    }
    let mut out: IndexMap<Key, Value> = IndexMap::new();
    out.insert(Key::from("_type"), Value::String(TYPE_NAME.into()));
    for (k, a) in acc {
        out.insert(k, Value::float(a));
    }
    Value::Map(out)
}

/// The bare bitstring-amplitude map (sentinels like `_type` removed) — what the
/// raw bitstring ops (`prelude::tensor`/`factorize`) iterate over. Without this
/// strip, `_type` would be concatenated into joint keys / break uniform length.
fn bare(v: &Value) -> Value {
    match v.as_map() {
        Some(m) => Value::Map(
            m.iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(k, val)| (k.clone(), val.clone()))
                .collect(),
        ),
        None => v.clone(),
    }
}

/// Ensure a bitstring-amplitude map carries the `Qubits` tag (idempotent).
fn stamp(v: &Value) -> Value {
    match v.as_map() {
        Some(m) => {
            let mut out = m.clone();
            out.insert(Key::from("_type"), Value::String(TYPE_NAME.into()));
            Value::Map(out)
        }
        None => v.clone(),
    }
}

fn bad(method: &str, message: &str) -> MethodError {
    MethodError::BadArgs {
        type_name: TYPE_NAME.into(),
        method: method.into(),
        message: message.into(),
    }
}

/// Read a qubit index argument (`q`/`control`/`target`) as a `usize`.
fn qubit_index(args: &[Value], i: usize, method: &str) -> Result<usize, MethodError> {
    let n = args
        .get(i)
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .ok_or_else(|| bad(method, "expected a numeric qubit index"))?;
    if n < 0 {
        return Err(bad(method, "qubit index must be >= 0"));
    }
    Ok(n as usize)
}

fn bit(key: &str, q: usize) -> Option<char> {
    key.chars().nth(q)
}

/// Return `key` with the character at position `q` set to `b`.
fn with_bit(key: &str, q: usize, b: char) -> String {
    key.chars()
        .enumerate()
        .map(|(i, c)| if i == q { b } else { c })
        .collect()
}

// ── Gates (N-qubit-general over bitstring keys) ──────────────────────────

/// CNOT(control, target): flip `target` where `control` is set.
fn cnot(state: &Value, c: usize, t: usize) -> Value {
    build(amps(state).into_iter().map(|(k, a)| {
        let k2 = if bit(&k, c) == Some('1') {
            let flipped = if bit(&k, t) == Some('1') { '0' } else { '1' };
            with_bit(&k, t, flipped)
        } else {
            k
        };
        (k2, a)
    }))
}

/// Pauli-X (bit flip) on qubit `q`.
fn pauli_x(state: &Value, q: usize) -> Value {
    build(amps(state).into_iter().map(|(k, a)| {
        let flipped = if bit(&k, q) == Some('1') { '0' } else { '1' };
        (with_bit(&k, q, flipped), a)
    }))
}

/// Pauli-Z (phase flip) on qubit `q`: negate amplitudes where `q` is set.
fn pauli_z(state: &Value, q: usize) -> Value {
    build(amps(state).into_iter().map(|(k, a)| {
        let signed = if bit(&k, q) == Some('1') { -a } else { a };
        (k, signed)
    }))
}

/// Hadamard on qubit `q`: |0⟩→(|0⟩+|1⟩)/√2, |1⟩→(|0⟩−|1⟩)/√2.
fn hadamard(state: &Value, q: usize) -> Value {
    let inv_sqrt2 = 1.0 / std::f64::consts::SQRT_2;
    let mut pairs: Vec<(String, f64)> = Vec::new();
    for (k, a) in amps(state) {
        let k0 = with_bit(&k, q, '0');
        let k1 = with_bit(&k, q, '1');
        let was_one = bit(&k, q) == Some('1');
        pairs.push((k0, a * inv_sqrt2));
        pairs.push((k1, if was_one { -a } else { a } * inv_sqrt2));
    }
    build(pairs)
}

// ── Method registrations ─────────────────────────────────────────────────

/// Register the `Qubits` value-methods. These ARE the type's algebra; the
/// surface form is `state.method(args)` (dispatched via the `_type` tag).
pub fn register_quantum_methods(m: &mut MethodRegistry) {
    // tensor(other) — join two separable registers (the reverse of divide).
    m.register(TYPE_NAME, "tensor", |recv, args| -> MethodResult {
        let other = args.first().ok_or_else(|| bad("tensor", "expected tensor(other)"))?;
        Ok(stamp(&crate::prelude::tensor(&bare(recv), &bare(other))?))
    });
    // factorize(k) — try to split at bit `k` into two separable registers.
    // Returns `{separable, a: Qubits, b: Qubits}` (a/b are themselves Qubits).
    m.register(TYPE_NAME, "factorize", |recv, args| -> MethodResult {
        let k = args.first().ok_or_else(|| bad("factorize", "expected factorize(k)"))?;
        let result = crate::prelude::factorize(&bare(recv), k)?;
        // Tag the result as a `Factorization` and re-tag the factor sub-states
        // as `Qubits` so `s.factorize(1).a.cnot(…)` chains.
        Ok(match result.as_map() {
            Some(rm) => {
                let mut out = rm.clone();
                out.insert(Key::from("_type"), Value::String(FACTORIZATION.into()));
                if let Some(a) = rm.get("a") {
                    out.insert(Key::from("a"), stamp(a));
                }
                if let Some(b) = rm.get("b") {
                    out.insert(Key::from("b"), stamp(b));
                }
                Value::Map(out)
            }
            None => result,
        })
    });
    // separable(k) — the boolean query (factorize(k).separable).
    m.register(TYPE_NAME, "separable", |recv, args| -> MethodResult {
        let k = args.first().ok_or_else(|| bad("separable", "expected separable(k)"))?;
        let result = crate::prelude::factorize(&bare(recv), k)?;
        Ok(result.get_field("separable").cloned().unwrap_or(Value::Bool(false)))
    });
    // cnot(control, target)
    m.register(TYPE_NAME, "cnot", |recv, args| -> MethodResult {
        Ok(cnot(recv, qubit_index(args, 0, "cnot")?, qubit_index(args, 1, "cnot")?))
    });
    // hadamard(q)
    m.register(TYPE_NAME, "hadamard", |recv, args| -> MethodResult {
        Ok(hadamard(recv, qubit_index(args, 0, "hadamard")?))
    });
    // pauli_x(q) / pauli_z(q)
    m.register(TYPE_NAME, "pauli_x", |recv, args| -> MethodResult {
        Ok(pauli_x(recv, qubit_index(args, 0, "pauli_x")?))
    });
    m.register(TYPE_NAME, "pauli_z", |recv, args| -> MethodResult {
        Ok(pauli_z(recv, qubit_index(args, 0, "pauli_z")?))
    });
    // measure(seed) — Born-rule sample; returns the observed basis-state key
    // (a String), deterministic given the seed.
    m.register(TYPE_NAME, "measure", |recv, args| -> MethodResult {
        let seed = args.first().ok_or_else(|| bad("measure", "expected measure(seed)"))?;
        let mut dist: IndexMap<Key, Value> = IndexMap::new();
        for (k, a) in amps(recv) {
            dist.insert(Key::from(k.as_str()), Value::float(a * a));
        }
        crate::prelude::sample(&Value::Map(dist), seed)
    });
}

// ── The Qubits type (Custom, apply = overwrite) ──────────────────────────

/// `TypeMethods` for `Qubits`: a quantum state REPLACES on update (overwrite),
/// `realize` stamps the `_type` tag so values read from state/the wire dispatch
/// methods + apply correctly, and `divide` shares the state (placeholder until
/// the schema-driven factorize-divide of #39).
#[derive(Debug)]
struct QubitsTypeMethods;

impl TypeMethods for QubitsTypeMethods {
    fn default(&self, _r: &TypeRegistry, _s: &Schema) -> Value {
        build(std::iter::empty())
    }
    fn apply(&self, _r: &TypeRegistry, _s: &Schema, _state: &Value, update: &Value) -> Value {
        // Overwrite: a new amplitude vector replaces the old one (quantum state
        // is not additive). Keep the tag so the result stays a Qubits.
        stamp(update)
    }
    fn divide(&self, _r: &TypeRegistry, _s: &Schema, state: &Value, ctx: &DivideContext) -> Vec<Value> {
        // Placeholder: replicate the state to each daughter. The principled
        // form is factorize-driven (#39 tensor_by_schema dual) — only a
        // separable register may truly divide.
        vec![stamp(state); ctx.n_daughters.max(1)]
    }
    fn serialize(&self, _r: &TypeRegistry, _s: &Schema, state: &Value) -> Value {
        state.clone()
    }
    fn realize(&self, _r: &TypeRegistry, _s: &Schema, encoded: &Value) -> Value {
        stamp(encoded)
    }
    fn check(&self, _r: &TypeRegistry, _s: &Schema, state: &Value) -> bool {
        state.as_map().is_some_and(|m| {
            m.iter()
                .all(|(k, v)| k.starts_with('_') || v.as_f64().is_some())
        })
    }
}

/// Register the `Qubits` Custom type in the Core's [`TypeRegistry`] so a
/// `:: Qubits` slot (lowered to `Custom{Qubits}`) resolves to overwrite-apply +
/// the methods above. Representation is `map[float]`.
pub fn register_quantum_type(types: &mut TypeRegistry) {
    types.register_full(
        TYPE_NAME.to_string(),
        Schema::Map { value: Box::new(Schema::float()) },
        None,
        Some(std::sync::Arc::new(QubitsTypeMethods)),
        Vec::new(),
    );
    // `Factorization` — the named result of `Qubits.factorize(k)`:
    // `{separable: bool, a: Qubits, b: Qubits}`. A plain record (no custom
    // algebra): every field is replace-y (bool / overwrite-apply Qubits), so
    // the schema's Tree apply suffices.
    let qubits = Schema::Custom {
        name: TYPE_NAME.to_string(),
        parameters: IndexMap::new(),
    };
    types.register_full(
        FACTORIZATION.to_string(),
        Schema::Tree {
            branches: [
                (Key::from("separable"), Schema::bool()),
                (Key::from("a"), qubits.clone()),
                (Key::from("b"), qubits),
            ]
            .into_iter()
            .collect(),
        },
        None,
        None,
        Vec::new(),
    );
}
