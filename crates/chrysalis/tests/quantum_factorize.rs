//! `meta::factorize` regression — the inverse of `meta::tensor` (Q5 of #36).
//!
//! Detects whether a joint quantum-state map is separable at bit `k`. If
//! so, returns the two factor states such that `tensor(a, b) = joint`.
//! Substrate for auto-divide-on-factorizability: composites can call this
//! after a measurement to check whether to split into sub-composites.

use indexmap::IndexMap;
use prism_schema::{Key, Value};

fn state(pairs: &[(&str, f64)]) -> Value {
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    for (k, v) in pairs {
        m.insert(Key::from(*k), Value::float(*v));
    }
    Value::Map(m)
}

fn amp(map: &IndexMap<Key, Value>, key: &str) -> f64 {
    map.get(&Key::from(key)).unwrap().as_f64().unwrap()
}

fn assert_separable(result: &Value, expected: bool) {
    let m = result.as_map().unwrap();
    let sep = m.get(&Key::from("separable")).unwrap().as_bool().unwrap();
    assert_eq!(sep, expected, "separable mismatch: got {sep}, expected {expected}");
}

const SQRT_HALF: f64 = 0.7071067811865476;

#[test]
fn separable_joint_factors() {
    // |+⟩ ⊗ |+⟩ at split=1 should factor back to two |+⟩ states.
    let joint = state(&[("00", 0.5), ("01", 0.5), ("10", 0.5), ("11", 0.5)]);
    let result = chrysalis::prelude::factorize(&joint, &Value::float(1.0)).expect("factorize");
    assert_separable(&result, true);
    let m = result.as_map().unwrap();
    let a = m.get(&Key::from("a")).unwrap().as_map().unwrap();
    let b = m.get(&Key::from("b")).unwrap().as_map().unwrap();
    // Each should have amplitudes (1/√2, 1/√2) — recovering |+⟩.
    assert!((amp(a, "0") - SQRT_HALF).abs() < 1e-6);
    assert!((amp(a, "1") - SQRT_HALF).abs() < 1e-6);
    assert!((amp(b, "0") - SQRT_HALF).abs() < 1e-6);
    assert!((amp(b, "1") - SQRT_HALF).abs() < 1e-6);
}

#[test]
fn bell_state_is_entangled_not_factorable() {
    // The Bell state (|00⟩ + |11⟩)/√2 is the canonical entangled state.
    // Its joint amplitudes form a rank-2 matrix; no separable factorization.
    let bell = state(&[("00", SQRT_HALF), ("01", 0.0), ("10", 0.0), ("11", SQRT_HALF)]);
    let result = chrysalis::prelude::factorize(&bell, &Value::float(1.0)).expect("factorize");
    assert_separable(&result, false);
}

#[test]
fn plus_zero_factors_to_plus_and_zero() {
    // |+⟩ ⊗ |0⟩ at split=1 should recover (|+⟩, |0⟩).
    let joint = state(&[("00", SQRT_HALF), ("01", 0.0), ("10", SQRT_HALF), ("11", 0.0)]);
    let result = chrysalis::prelude::factorize(&joint, &Value::float(1.0)).expect("factorize");
    assert_separable(&result, true);
    let m = result.as_map().unwrap();
    let a = m.get(&Key::from("a")).unwrap().as_map().unwrap();
    let b = m.get(&Key::from("b")).unwrap().as_map().unwrap();
    assert!((amp(a, "0") - SQRT_HALF).abs() < 1e-6);
    assert!((amp(a, "1") - SQRT_HALF).abs() < 1e-6);
    assert!((amp(b, "0") - 1.0).abs() < 1e-6);
    assert!((amp(b, "1") - 0.0).abs() < 1e-6);
}

#[test]
fn tensor_then_factorize_round_trips() {
    // factorize ∘ tensor = identity on separable states.
    let a_orig = state(&[("0", 0.6), ("1", 0.8)]);
    let b_orig = state(&[("0", SQRT_HALF), ("1", SQRT_HALF)]);
    let joint = chrysalis::prelude::tensor(&a_orig, &b_orig).expect("tensor");
    let result =
        chrysalis::prelude::factorize(&joint, &Value::float(1.0)).expect("factorize");
    assert_separable(&result, true);
    let m = result.as_map().unwrap();
    let a = m.get(&Key::from("a")).unwrap().as_map().unwrap();
    let b = m.get(&Key::from("b")).unwrap().as_map().unwrap();
    assert!((amp(a, "0") - 0.6).abs() < 1e-6, "a[0]: got {}, expected 0.6", amp(a, "0"));
    assert!((amp(a, "1") - 0.8).abs() < 1e-6, "a[1]: got {}, expected 0.8", amp(a, "1"));
    assert!((amp(b, "0") - SQRT_HALF).abs() < 1e-6);
    assert!((amp(b, "1") - SQRT_HALF).abs() < 1e-6);
}

#[test]
fn ghz_state_is_entangled_at_any_split() {
    // The 3-qubit GHZ state (|000⟩ + |111⟩)/√2 is maximally entangled —
    // no split (k=1 or k=2) factors it.
    let ghz = state(&[
        ("000", SQRT_HALF),
        ("001", 0.0),
        ("010", 0.0),
        ("011", 0.0),
        ("100", 0.0),
        ("101", 0.0),
        ("110", 0.0),
        ("111", SQRT_HALF),
    ]);
    let result_k1 =
        chrysalis::prelude::factorize(&ghz, &Value::float(1.0)).expect("factorize k=1");
    assert_separable(&result_k1, false);
    let result_k2 =
        chrysalis::prelude::factorize(&ghz, &Value::float(2.0)).expect("factorize k=2");
    assert_separable(&result_k2, false);
}

#[test]
fn three_qubit_separable_factors_at_split_one() {
    // |+⟩ ⊗ |+⟩ ⊗ |0⟩ → 8-key joint, but factors at split=1 into
    // (|+⟩, |+⟩⊗|0⟩) — both halves valid.
    let plus = state(&[("0", SQRT_HALF), ("1", SQRT_HALF)]);
    let zero = state(&[("0", 1.0), ("1", 0.0)]);
    let pp = chrysalis::prelude::tensor(&plus, &plus).expect("tensor");
    let ppz = chrysalis::prelude::tensor(&pp, &zero).expect("tensor");
    let result =
        chrysalis::prelude::factorize(&ppz, &Value::float(1.0)).expect("factorize");
    assert_separable(&result, true);
    let m = result.as_map().unwrap();
    let a = m.get(&Key::from("a")).unwrap().as_map().unwrap();
    let b = m.get(&Key::from("b")).unwrap().as_map().unwrap();
    // a should be |+⟩ (2 keys: '0', '1' with amp √½)
    assert_eq!(a.len(), 2);
    assert!((amp(a, "0") - SQRT_HALF).abs() < 1e-6);
    // b should be |+⟩⊗|0⟩ (4 keys: '00': √½, '01': 0, '10': √½, '11': 0)
    assert_eq!(b.len(), 4);
    assert!((amp(b, "00") - SQRT_HALF).abs() < 1e-6);
    assert!((amp(b, "10") - SQRT_HALF).abs() < 1e-6);
}
