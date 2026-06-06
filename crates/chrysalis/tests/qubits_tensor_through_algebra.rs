//! Regression for #39: `tensor_by_schema(Custom{Qubits}, …)` dispatches
//! through the registry's `TypeMethods::tensor` to the quantum cross-product
//! tensor (`prelude::tensor`), instead of falling back to the schema-driven
//! per-key share-left.
//!
//! This is the SCHEMA-ALGEBRA HOME for `meta::tensor` — once `Qubits` declares
//! its tensor on `TypeMethods`, the algebra recognises it and any caller
//! invoking `tensor_by_schema` on a `Custom{Qubits}` slot gets the quantum
//! semantics for free. The merge-protocol slice 2 / docs/quantum-bigraphs.md
//! §VII identification of tensor as the dual of divide, made schema-driven.

use indexmap::IndexMap;
use prism_schema::{tensor_by_schema, Key, Schema, TypeRegistry, Value};

fn qubits_schema() -> Schema {
    Schema::Custom {
        name: "Qubits".to_string(),
        parameters: IndexMap::new(),
    }
}

fn make_state(pairs: &[(&str, f64)]) -> Value {
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(Key::from("_type"), Value::String("Qubits".into()));
    for (k, v) in pairs {
        m.insert(Key::from(*k), Value::float(*v));
    }
    Value::Map(m)
}

fn amp(v: &Value, key: &str) -> f64 {
    v.as_map()
        .and_then(|m| m.get(&Key::from(key)))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

const SQRT_HALF: f64 = std::f64::consts::FRAC_1_SQRT_2;

#[test]
fn algebra_tensor_dispatches_to_qubits_quantum_tensor() {
    // Register Qubits in a fresh registry — same path as compile.rs does
    // for the real Core. The algebra walks `Schema::Custom { name }` →
    // `registry.type_tensor(name, a, b)` → `TypeMethods::tensor` → the
    // quantum cross-product.
    let mut types = TypeRegistry::new();
    chrysalis::quantum::register_quantum_type(&mut types);

    let plus = make_state(&[("0", SQRT_HALF), ("1", SQRT_HALF)]);
    let joint = tensor_by_schema(&qubits_schema(), &plus, &plus, &types);

    let m = joint.as_map().expect("joint is a map");
    // The QUANTUM dispatch produces |++⟩ = (|00⟩+|01⟩+|10⟩+|11⟩)/2 — four
    // basis keys, each at amplitude 0.5. The default schema-driven
    // (share-left or per-key share) would have only produced 2 single-bit
    // keys with amplitude 1/√2. The cardinality alone distinguishes them.
    assert!(
        m.contains_key("00") && m.contains_key("11"),
        "expected cross-product keys 00/11, got: {:?}",
        m.keys().collect::<Vec<_>>()
    );
    let expected_amp = 0.5;
    for k in ["00", "01", "10", "11"] {
        assert!(
            (amp(&joint, k) - expected_amp).abs() < 1e-9,
            "amp[{}] = {} (expected {})",
            k,
            amp(&joint, k),
            expected_amp
        );
    }
    // The `_type` tag survives the dispatch — the result is itself a Qubits.
    assert_eq!(
        joint.as_map().and_then(|m| m.get("_type")).and_then(|v| v.as_str()),
        Some("Qubits"),
        "tensor result should be tagged Qubits (chainable)"
    );
}
