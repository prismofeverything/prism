//! Tests for `tensor_by_schema` — the schema-driven dual of
//! `divide_by_schema` (the `fold` / merge direction of
//! `docs/bigraphs-all-the-way-down.md`, slice 1 of `docs/merge-protocol.md`).
//!
//! The key law: for extensive types, **tensor inverts divide** —
//! `tensor(divide(state, 2).0, divide(state, 2).1) = state`. For intensive
//! types, tensor is **idempotent on identical inputs** — `tensor(x, x) = x`.

use indexmap::IndexMap;
use prism_schema::{
    divide_by_schema, tensor_by_schema, DivideContext, Key, Schema, TypeRegistry, Value,
};

fn reg() -> TypeRegistry {
    TypeRegistry::new()
}

#[test]
fn delta_tensor_sums_extensive_floats() {
    let s = Schema::delta();
    let r = reg();
    let out = tensor_by_schema(&s, &Value::float(2.5), &Value::float(1.5), &r);
    assert_eq!(out.as_f64(), Some(4.0));
}

#[test]
fn integer_tensor_sums_extensive_ints() {
    let s = Schema::integer();
    let r = reg();
    let out = tensor_by_schema(&s, &Value::Int(3), &Value::Int(4), &r);
    assert_eq!(out.as_i64(), Some(7));
}

#[test]
fn float_tensor_shares_left_intensive() {
    let s = Schema::float();
    let r = reg();
    // Intensive: tensor shares the LEFT (caller responsible for consistency).
    let out = tensor_by_schema(&s, &Value::float(2.0), &Value::float(2.0), &r);
    assert_eq!(out.as_f64(), Some(2.0), "tensor(x, x) = x");
}

#[test]
fn delta_tensor_inverts_divide_round_trip() {
    // The defining law for extensive types: divide-then-tensor = identity.
    let s = Schema::delta();
    let r = reg();
    let state = Value::float(10.0);
    let ctx = DivideContext::binary();
    let daughters = divide_by_schema(&s, &state, &ctx, &r);
    assert_eq!(daughters.len(), 2);
    let merged = tensor_by_schema(&s, &daughters[0], &daughters[1], &r);
    assert_eq!(merged.as_f64(), Some(10.0), "tensor ∘ divide = id (extensive)");
}

#[test]
fn integer_tensor_inverts_divide_round_trip() {
    let s = Schema::integer();
    let r = reg();
    let state = Value::Int(7);
    let ctx = DivideContext::binary();
    let daughters = divide_by_schema(&s, &state, &ctx, &r);
    let merged = tensor_by_schema(&s, &daughters[0], &daughters[1], &r);
    assert_eq!(merged.as_i64(), Some(7));
}

#[test]
fn map_float_tensor_per_key_shares_intensive() {
    // map[float] — Float is intensive, so per-key tensor shares left.
    let s = Schema::Map {
        value: Box::new(Schema::float()),
    };
    let r = reg();
    let a = Value::Map(IndexMap::from([
        (Key::from("x"), Value::float(1.0)),
        (Key::from("y"), Value::float(2.0)),
    ]));
    let b = Value::Map(IndexMap::from([
        (Key::from("x"), Value::float(1.0)),
        (Key::from("z"), Value::float(3.0)),
    ]));
    let out = tensor_by_schema(&s, &a, &b, &r);
    let m = out.as_map().expect("map");
    assert_eq!(m.get("x").and_then(|v| v.as_f64()), Some(1.0));
    assert_eq!(m.get("y").and_then(|v| v.as_f64()), Some(2.0), "left-only key preserved");
    assert_eq!(m.get("z").and_then(|v| v.as_f64()), Some(3.0), "right-only key preserved");
}

#[test]
fn map_delta_tensor_per_key_sums_extensive() {
    // map[Delta] — Delta is extensive, so per-key tensor sums shared keys.
    let s = Schema::Map {
        value: Box::new(Schema::delta()),
    };
    let r = reg();
    let a = Value::Map(IndexMap::from([
        (Key::from("p"), Value::float(1.0)),
        (Key::from("q"), Value::float(5.0)),
    ]));
    let b = Value::Map(IndexMap::from([
        (Key::from("p"), Value::float(2.0)),
        (Key::from("r"), Value::float(7.0)),
    ]));
    let out = tensor_by_schema(&s, &a, &b, &r);
    let m = out.as_map().expect("map");
    assert_eq!(m.get("p").and_then(|v| v.as_f64()), Some(3.0), "shared key sums");
    assert_eq!(m.get("q").and_then(|v| v.as_f64()), Some(5.0));
    assert_eq!(m.get("r").and_then(|v| v.as_f64()), Some(7.0));
}

#[test]
fn tree_tensor_per_field_by_schema() {
    // A Tree with mixed extensivity: `mass :: Delta`, `rate :: Float`.
    let s = Schema::Tree {
        branches: IndexMap::from([
            (Key::from("mass"), Schema::delta()),
            (Key::from("rate"), Schema::float()),
        ]),
    };
    let r = reg();
    let a = Value::Map(IndexMap::from([
        (Key::from("mass"), Value::float(2.0)),
        (Key::from("rate"), Value::float(0.1)),
    ]));
    let b = Value::Map(IndexMap::from([
        (Key::from("mass"), Value::float(3.0)),
        (Key::from("rate"), Value::float(0.1)),
    ]));
    let out = tensor_by_schema(&s, &a, &b, &r);
    let m = out.as_map().expect("map");
    assert_eq!(m.get("mass").and_then(|v| v.as_f64()), Some(5.0), "mass (Delta) sums");
    assert_eq!(m.get("rate").and_then(|v| v.as_f64()), Some(0.1), "rate (Float) shares");
}

#[test]
fn tree_tensor_inverts_divide_for_mixed_extensivity() {
    // The interesting round-trip: a Tree with both Delta (extensive) and
    // Float (intensive) — divide splits the Delta, shares the Float; tensor
    // sums the Delta, shares the Float; result equals the original.
    let s = Schema::Tree {
        branches: IndexMap::from([
            (Key::from("mass"), Schema::delta()),
            (Key::from("temperature"), Schema::float()),
        ]),
    };
    let r = reg();
    let state = Value::Map(IndexMap::from([
        (Key::from("mass"), Value::float(8.0)),
        (Key::from("temperature"), Value::float(37.0)),
    ]));
    let ctx = DivideContext::binary();
    let daughters = divide_by_schema(&s, &state, &ctx, &r);
    let merged = tensor_by_schema(&s, &daughters[0], &daughters[1], &r);
    let m = merged.as_map().expect("map");
    assert_eq!(m.get("mass").and_then(|v| v.as_f64()), Some(8.0));
    assert_eq!(m.get("temperature").and_then(|v| v.as_f64()), Some(37.0));
}

#[test]
fn list_tensor_elementwise_by_element_schema() {
    let s = Schema::List {
        element: Box::new(Schema::delta()),
    };
    let r = reg();
    let a = Value::List(vec![Value::float(1.0), Value::float(2.0), Value::float(3.0)]);
    let b = Value::List(vec![Value::float(10.0), Value::float(20.0), Value::float(30.0)]);
    let out = tensor_by_schema(&s, &a, &b, &r);
    let list = out.as_list().expect("list");
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].as_f64(), Some(11.0));
    assert_eq!(list[1].as_f64(), Some(22.0));
    assert_eq!(list[2].as_f64(), Some(33.0));
}

#[test]
fn overwrite_wrapper_delegates_to_inner() {
    // `overwrite[Delta]` — wrapper carries no semantic weight at the tensor
    // level; the inner schema decides extensivity.
    let s = Schema::Overwrite {
        inner: Box::new(Schema::delta()),
    };
    let r = reg();
    let out = tensor_by_schema(&s, &Value::float(2.0), &Value::float(3.0), &r);
    assert_eq!(out.as_f64(), Some(5.0));
}

#[test]
fn composite_link_tensor_merges_self_face_shares_spec() {
    // A composite node carries `address`/`config`/`inputs`/`outputs` (the
    // spec — shared) plus exported data slots (the face — tensored). For
    // `CompositeLink` with no inner_schema declaring the face, the
    // node_data_branches is empty and the whole node shares-left, mirroring
    // the divide convention.
    let s = Schema::CompositeLink {
        inputs: IndexMap::new(),
        outputs: IndexMap::new(),
        interval: 1.0,
        inner_schema: Box::new(Schema::Any),
    };
    let r = reg();
    let make = |mass: f64| {
        Value::Map(IndexMap::from([
            (Key::from("_type"), Value::String("composite".into())),
            (Key::from("address"), Value::String("local:Cell".into())),
            (Key::from("mass"), Value::float(mass)),
        ]))
    };
    let a = make(2.0);
    let b = make(3.0);
    let out = tensor_by_schema(&s, &a, &b, &r);
    let m = out.as_map().expect("map");
    // With Any inner_schema, the face has no declared structure — the merge
    // is the left node by share. Concrete face merging arrives when a
    // CompositeLink declares its self-exported port types (the principled
    // way mass would be summed is via `mass :: Delta` in the inner schema).
    assert!(m.contains_key("address"), "spec preserved");
    assert!(m.contains_key("mass"), "face slot present");
}
