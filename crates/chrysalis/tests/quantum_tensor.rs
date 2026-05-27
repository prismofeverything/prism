//! `meta::tensor` regression tests (Q3 of #36).
//!
//! `tensor(a, b)` is the inverse of `divide`: takes two SEPARABLE
//! quantum-state maps (bitstring → amplitude) and combines them into
//! one joint-state map by cross product + amplitude multiplication.
//! See `docs/quantum-bigraphs.md` §III/VII.
//!
//! These tests verify the math directly on Rust `Value`s — no parser /
//! engine in the picture. The `.ys`-level demo is `quantum-tensor.ys`.
//!
//! Key invariants:
//!   • |a| * |b| outcomes (cross-product cardinality)
//!   • amp_joint(ka||kb) = amp_a(ka) * amp_b(kb)
//!   • norm preserved: sum |amp|² is multiplicative
//!   • tensoring with |0⟩ embeds (zero-amplitude branches stay zero)

use indexmap::IndexMap;
use prism_schema::{Key, Value};

fn state(pairs: &[(&str, f64)]) -> Value {
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    for (k, v) in pairs {
        m.insert(Key::from(*k), Value::float(*v));
    }
    Value::Map(m)
}

fn amp(v: &Value, key: &str) -> f64 {
    v.as_map().unwrap().get(&Key::from(key)).unwrap().as_f64().unwrap()
}

const SQRT_HALF: f64 = 0.7071067811865476;

#[test]
fn tensor_two_plus_states() {
    // |+⟩ ⊗ |+⟩ = (|00⟩ + |01⟩ + |10⟩ + |11⟩) / 2.
    let plus = state(&[("0", SQRT_HALF), ("1", SQRT_HALF)]);
    let joint = chrysalis::prelude::tensor(&plus, &plus).expect("tensor");
    let m = joint.as_map().unwrap();
    assert_eq!(m.len(), 4, "two single-qubit states → 4-key joint state");
    for k in &["00", "01", "10", "11"] {
        let a = amp(&joint, k);
        assert!((a - 0.5).abs() < 1e-9, "|+⟩⊗|+⟩ amplitude at {k} should be 0.5, got {a}");
    }
}

#[test]
fn tensor_plus_zero_is_separable_embedding() {
    // |+⟩ ⊗ |0⟩ = (|00⟩ + |10⟩) / √2. Branches with second-qubit=1 stay zero.
    let plus = state(&[("0", SQRT_HALF), ("1", SQRT_HALF)]);
    let zero = state(&[("0", 1.0), ("1", 0.0)]);
    let joint = chrysalis::prelude::tensor(&plus, &zero).expect("tensor");
    assert!((amp(&joint, "00") - SQRT_HALF).abs() < 1e-9);
    assert!((amp(&joint, "01") - 0.0).abs() < 1e-9);
    assert!((amp(&joint, "10") - SQRT_HALF).abs() < 1e-9);
    assert!((amp(&joint, "11") - 0.0).abs() < 1e-9);
}

#[test]
fn tensor_preserves_norm() {
    // |ψ|² · |φ|² = |ψ⊗φ|². If each input has norm 1, so does the tensor.
    let a = state(&[("0", 0.6), ("1", 0.8)]);  // 0.36 + 0.64 = 1.0
    let b = state(&[("0", SQRT_HALF), ("1", SQRT_HALF)]); // 0.5 + 0.5 = 1.0
    let joint = chrysalis::prelude::tensor(&a, &b).expect("tensor");
    let total_prob: f64 = joint
        .as_map()
        .unwrap()
        .values()
        .map(|v| v.as_f64().unwrap().powi(2))
        .sum();
    assert!((total_prob - 1.0).abs() < 1e-9, "norm preserved; got {total_prob}");
}

#[test]
fn tensor_cross_product_cardinality() {
    // tensor of m-key and n-key maps → m·n keys (the Cartesian product).
    let two_key = state(&[("0", SQRT_HALF), ("1", SQRT_HALF)]);
    let four_key = state(&[("00", 0.5), ("01", 0.5), ("10", 0.5), ("11", 0.5)]);
    let joint = chrysalis::prelude::tensor(&two_key, &four_key).expect("tensor");
    assert_eq!(joint.as_map().unwrap().len(), 8, "2 × 4 = 8-key joint state");
    // spot-check one joint amplitude: amp('0' || '01') = SQRT_HALF * 0.5
    assert!((amp(&joint, "001") - SQRT_HALF * 0.5).abs() < 1e-9);
}
