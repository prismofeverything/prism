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
use prism_std::complex;

const TYPE_NAME: &str = "Qubits";
/// The result type of `Qubits.factorize(k)`: a separability verdict + the two
/// factor registers. A real named type, not an anonymous `{separable, a, b}`
/// record repeated at every use site.
const FACTORIZATION: &str = "Factorization";

// ── Representation helpers ───────────────────────────────────────────────

/// Amplitude entries of a `Qubits` value: `(bitstring, amplitude)` for every
/// non-sentinel key (`_type` and any `_`-prefixed metadata are skipped). The
/// amplitude is a `complex` value — a bare `float` (`re + 0i`) for real circuits,
/// or a `[re, im]` array once a gate makes it genuinely complex.
fn amps(v: &Value) -> Vec<(String, Value)> {
    match v.as_map() {
        Some(m) => m
            .iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, val)| (k.to_string(), val.clone()))
            .collect(),
        None => Vec::new(),
    }
}

/// Build a `Qubits` value from `(bitstring, amplitude)` pairs, summing duplicate
/// keys via ℂ addition (gates that map several basis states onto one) and stamping
/// `_type: "Qubits"`. Each amplitude is normalized through [`complex::of`], so an
/// exactly-real result collapses to a bare `float` — keeping real-amplitude
/// circuits byte-identical to the pre-complex form.
fn build(pairs: impl IntoIterator<Item = (String, Value)>) -> Value {
    let mut acc: IndexMap<Key, Value> = IndexMap::new();
    for (k, a) in pairs {
        let key = Key::from(k.as_str());
        let summed = match acc.get(&key) {
            Some(prev) => complex::add(prev, &a),
            None => a,
        };
        acc.insert(key, summed);
    }
    let mut out: IndexMap<Key, Value> = IndexMap::new();
    out.insert(Key::from("_type"), Value::String(TYPE_NAME.into()));
    for (k, a) in acc {
        let (re, im) = complex::parts(&a);
        out.insert(k, complex::of(re, im));
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

// ── Gates ────────────────────────────────────────────────────────────────
//
// Single-qubit gates are ONE operation — `apply_1q(state, q, U)` with a 2×2
// complex matrix `U`. That is the whole canonical single-qubit set (X/Y/Z/H/S/T/
// phase/Rx/Ry/Rz) as DATA, not N hand-written amplitude loops, and the seam a
// circuit/gate library composes over. CNOT stays a controlled key-permutation.
//
// Amplitudes are complex (`prism_std::complex`): a bare `float` reads as `re+0i`,
// so the real gates (X/Z/H/CNOT) emit byte-identical real output via `complex::of`'s
// real-collapse, while S/T/Y/phase/Rx/Rz produce genuine `[re, im]` amplitudes.

/// CNOT(control, target): flip `target` where `control` is set. Carries each
/// (complex) amplitude unchanged to the permuted basis key.
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

/// Apply a single-qubit 2×2 complex unitary `u = [[u00,u01],[u10,u11]]` to qubit
/// `q`: a basis amplitude on `q=b` spreads to `q=0` (×`u[0][b]`) and `q=1`
/// (×`u[1][b]`), summed by `build`. Every single-qubit gate is this + a matrix.
fn apply_1q(state: &Value, q: usize, u: &[[Value; 2]; 2]) -> Value {
    build(amps(state).into_iter().flat_map(|(k, a)| {
        let b = if bit(&k, q) == Some('1') { 1 } else { 0 };
        let k0 = with_bit(&k, q, '0');
        let k1 = with_bit(&k, q, '1');
        [
            (k0, complex::mul(&u[0][b], &a)),
            (k1, complex::mul(&u[1][b], &a)),
        ]
    }))
}

// The canonical single-qubit matrices — the gate library's data.
fn m_x() -> [[Value; 2]; 2] {
    [[complex::zero(), complex::one()], [complex::one(), complex::zero()]]
}
fn m_y() -> [[Value; 2]; 2] {
    // [[0, -i], [i, 0]]
    [
        [complex::zero(), complex::neg(&complex::i())],
        [complex::i(), complex::zero()],
    ]
}
fn m_z() -> [[Value; 2]; 2] {
    [[complex::one(), complex::zero()], [complex::zero(), complex::of(-1.0, 0.0)]]
}
fn m_h() -> [[Value; 2]; 2] {
    let s = 1.0 / std::f64::consts::SQRT_2;
    [
        [complex::of(s, 0.0), complex::of(s, 0.0)],
        [complex::of(s, 0.0), complex::of(-s, 0.0)],
    ]
}
fn m_s() -> [[Value; 2]; 2] {
    // phase π/2 on |1⟩: diag(1, i)
    [[complex::one(), complex::zero()], [complex::zero(), complex::i()]]
}
fn m_t() -> [[Value; 2]; 2] {
    // phase π/4 on |1⟩: diag(1, e^{iπ/4})
    [
        [complex::one(), complex::zero()],
        [complex::zero(), complex::from_phase(std::f64::consts::FRAC_PI_4)],
    ]
}
fn m_phase(theta: f64) -> [[Value; 2]; 2] {
    // diag(1, e^{iθ})
    [
        [complex::one(), complex::zero()],
        [complex::zero(), complex::from_phase(theta)],
    ]
}
fn m_rx(theta: f64) -> [[Value; 2]; 2] {
    // [[cos, -i sin], [-i sin, cos]] (half-angle)
    let (c, s) = ((theta / 2.0).cos(), (theta / 2.0).sin());
    let nisin = complex::of(0.0, -s);
    [[complex::of(c, 0.0), nisin.clone()], [nisin, complex::of(c, 0.0)]]
}
fn m_ry(theta: f64) -> [[Value; 2]; 2] {
    // [[cos, -sin], [sin, cos]] (half-angle) — the REAL rotation
    let (c, s) = ((theta / 2.0).cos(), (theta / 2.0).sin());
    [
        [complex::of(c, 0.0), complex::of(-s, 0.0)],
        [complex::of(s, 0.0), complex::of(c, 0.0)],
    ]
}
fn m_rz(theta: f64) -> [[Value; 2]; 2] {
    // diag(e^{-iθ/2}, e^{+iθ/2})
    [
        [complex::from_phase(-theta / 2.0), complex::zero()],
        [complex::zero(), complex::from_phase(theta / 2.0)],
    ]
}

// ── Method registrations ─────────────────────────────────────────────────

/// Register a single-qubit gate as `state.<name>(q)` — `apply_1q` with the gate's
/// matrix. The matrix is rebuilt per call (cheap: ≤4 small complex literals).
fn reg_1q(m: &mut MethodRegistry, name: &'static str, mat: fn() -> [[Value; 2]; 2]) {
    m.register(TYPE_NAME, name, move |recv, args| -> MethodResult {
        Ok(apply_1q(recv, qubit_index(args, 0, name)?, &mat()))
    });
}

/// Register a parametrized single-qubit gate as `state.<name>(q, theta)`.
fn reg_1q_param(m: &mut MethodRegistry, name: &'static str, mat: fn(f64) -> [[Value; 2]; 2]) {
    m.register(TYPE_NAME, name, move |recv, args| -> MethodResult {
        let q = qubit_index(args, 0, name)?;
        let theta = args
            .get(1)
            .and_then(|v| v.as_f64())
            .ok_or_else(|| bad(name, "expected (qubit, theta)"))?;
        Ok(apply_1q(recv, q, &mat(theta)))
    });
}

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
    // cnot(control, target) — the controlled key-permutation.
    m.register(TYPE_NAME, "cnot", |recv, args| -> MethodResult {
        Ok(cnot(recv, qubit_index(args, 0, "cnot")?, qubit_index(args, 1, "cnot")?))
    });
    // The canonical single-qubit gate set — each the descriptive name plus a short
    // alias, dispatched through `apply_1q` + the matrix.
    reg_1q(m, "hadamard", m_h);
    reg_1q(m, "h", m_h);
    reg_1q(m, "pauli_x", m_x);
    reg_1q(m, "x", m_x);
    reg_1q(m, "pauli_y", m_y);
    reg_1q(m, "y", m_y);
    reg_1q(m, "pauli_z", m_z);
    reg_1q(m, "z", m_z);
    reg_1q(m, "s", m_s);
    reg_1q(m, "t", m_t);
    // Parametrized phase / rotation gates — `state.<name>(qubit, theta)`.
    reg_1q_param(m, "phase", m_phase);
    reg_1q_param(m, "rx", m_rx);
    reg_1q_param(m, "ry", m_ry);
    reg_1q_param(m, "rz", m_rz);
    // measure(seed) — Born-rule sample over |amp|²; returns the observed
    // basis-state key (a String), deterministic given the seed.
    m.register(TYPE_NAME, "measure", |recv, args| -> MethodResult {
        let seed = args.first().ok_or_else(|| bad("measure", "expected measure(seed)"))?;
        let mut dist: IndexMap<Key, Value> = IndexMap::new();
        for (k, a) in amps(recv) {
            dist.insert(Key::from(k.as_str()), Value::float(complex::abs2(&a)));
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
    fn tensor(&self, _r: &TypeRegistry, _s: &Schema, a: &Value, b: &Value) -> Value {
        // Quantum tensor: cross-product over basis bitstrings, amplitudes
        // multiplied. The dual of divide for `Qubits` — slice 2 of the merge
        // protocol made type-specific (#39 + #36 Q3). The default
        // schema-driven tensor would per-key share intensive floats; the
        // quantum tensor combines two SEPARATE registers into ONE joint
        // register over the cartesian product of basis states.
        match crate::prelude::tensor(&bare(a), &bare(b)) {
            Ok(joint) => stamp(&joint),
            Err(_) => stamp(a),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-qubit basis ket `|bits⟩` as a `Qubits` value.
    fn ket(bits: &str) -> Value {
        build([(bits.to_string(), complex::one())])
    }

    /// `(re, im)` of the amplitude on basis key `key`.
    fn amp(state: &Value, key: &str) -> (f64, f64) {
        let v = state
            .as_map()
            .and_then(|m| m.get(key))
            .cloned()
            .unwrap_or(Value::None);
        complex::parts(&v)
    }

    #[test]
    fn s_gate_phases_one_by_i() {
        // S|1⟩ = i|1⟩, S|0⟩ = |0⟩.
        assert_eq!(amp(&apply_1q(&ket("1"), 0, &m_s()), "1"), (0.0, 1.0));
        assert_eq!(amp(&apply_1q(&ket("0"), 0, &m_s()), "0"), (1.0, 0.0));
    }

    #[test]
    fn t_gate_phases_one_by_eighth_turn() {
        // T|1⟩ = e^{iπ/4}|1⟩.
        let (re, im) = amp(&apply_1q(&ket("1"), 0, &m_t()), "1");
        assert!((re - std::f64::consts::FRAC_PI_4.cos()).abs() < 1e-12);
        assert!((im - std::f64::consts::FRAC_PI_4.sin()).abs() < 1e-12);
    }

    #[test]
    fn y_gate_maps_zero_to_i_one() {
        // Y|0⟩ = i|1⟩, Y|1⟩ = -i|0⟩.
        assert_eq!(amp(&apply_1q(&ket("0"), 0, &m_y()), "1"), (0.0, 1.0));
        assert_eq!(amp(&apply_1q(&ket("1"), 0, &m_y()), "0"), (0.0, -1.0));
    }

    #[test]
    fn s_squared_is_z() {
        // S·S = Z: S²|1⟩ = -|1⟩.
        let ss = apply_1q(&apply_1q(&ket("1"), 0, &m_s()), 0, &m_s());
        assert_eq!(amp(&ss, "1"), (-1.0, 0.0));
    }

    #[test]
    fn real_gates_stay_byte_identical_real() {
        // H|0⟩ = (|0⟩+|1⟩)/√2 — both amplitudes are bare real floats, NOT [re, 0].
        let out = apply_1q(&ket("0"), 0, &m_h());
        let s = 1.0 / std::f64::consts::SQRT_2;
        assert_eq!(amp(&out, "0"), (s, 0.0));
        assert_eq!(amp(&out, "1"), (s, 0.0));
        let v0 = out.as_map().unwrap().get("0").unwrap();
        assert!(v0.as_f64().is_some(), "a real amplitude must stay a bare float");
    }

    #[test]
    fn measure_weights_use_abs2_and_preserve_norm() {
        // |+⟩ = H|0⟩: Σ|amp|² = 1 (a complex-free check that abs2 normalizes).
        let plus = apply_1q(&ket("0"), 0, &m_h());
        let total: f64 = amps(&plus).iter().map(|(_, a)| complex::abs2(a)).sum();
        assert!((total - 1.0).abs() < 1e-12);
    }
}
