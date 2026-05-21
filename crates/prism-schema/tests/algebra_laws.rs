//! Executable axioms for the schema algebra (`docs/schema-algebra.md`).
//!
//! Each law is a `proptest` over generated `(schema, value)` pairs. These are
//! the laws an implementation must satisfy to be a faithful *model* of the
//! theory — so this file doubles as a **cross-implementation conformance
//! suite**: port the ops to Python / a GPU / a distributed backend, run these
//! laws, green ⇒ faithful.
//!
//! The generators (`arb_schema`, `arb_value`, …) produce *conforming* typed
//! values by construction. Floats are drawn from a small integer range so all
//! arithmetic (apply/reconcile sums) is exact and the laws stay crisp.

use indexmap::IndexMap;
use proptest::prelude::*;

use prism_schema::algebra;
use prism_schema::schema::Schema;
use prism_schema::value::{Key, Value};

// ════════════════════════════════════════════════════════════════════════
// Generators
// ════════════════════════════════════════════════════════════════════════

/// A random well-formed schema, depth-bounded. Array elements are fixed to
/// `float` so value generation for arrays stays simple (and additive).
fn arb_schema() -> impl Strategy<Value = Schema> {
    let leaf = prop_oneof![
        Just(Schema::float()),
        Just(Schema::Float { default: Some(0.0) }),
        Just(Schema::integer()),
        Just(Schema::bool()),
        Just(Schema::string()),
        Just(Schema::Delta { default: None }),
        Just(Schema::Enum { values: vec!["x".into(), "y".into()], default: None }),
    ];
    leaf.prop_recursive(3, 24, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(Schema::overwrite),
            inner.clone().prop_map(Schema::maybe),
            inner.clone().prop_map(Schema::list),
            inner.clone().prop_map(Schema::map),
            prop::collection::vec(("[a-c]", inner.clone()), 1..3)
                .prop_map(|brs| Schema::tree(brs.into_iter().map(|(k, s)| (Key::from(k.as_str()), s)))),
            Just(Schema::Array { shape: vec![2, 2], element: Box::new(Schema::float()) }),
        ]
    })
}

/// A value conforming to `schema`.
fn arb_value(schema: &Schema) -> BoxedStrategy<Value> {
    match schema {
        Schema::Float { .. } | Schema::Delta { .. } => {
            (-100i64..100).prop_map(|i| Value::float(i as f64)).boxed()
        }
        Schema::Integer { .. } => (-100i64..100).prop_map(Value::Int).boxed(),
        Schema::Bool { .. } => any::<bool>().prop_map(Value::Bool).boxed(),
        Schema::String { .. } => "[a-c]{1,3}".prop_map(Value::String).boxed(),
        Schema::Enum { values, .. } => {
            let vs = values.clone();
            (0..vs.len()).prop_map(move |i| Value::String(vs[i].clone())).boxed()
        }
        Schema::Overwrite { inner } => arb_value(inner),
        Schema::Maybe { inner } => {
            let some = arb_value(inner);
            prop_oneof![Just(Value::None), some].boxed()
        }
        Schema::List { element } => {
            prop::collection::vec(arb_value(element), 0..3).prop_map(Value::List).boxed()
        }
        Schema::Map { value } => {
            prop::collection::vec(("[a-c]{1,2}", arb_value(value)), 0..3)
                .prop_map(|kvs| {
                    Value::Map(kvs.into_iter().map(|(k, v)| (Key::from(k.as_str()), v)).collect())
                })
                .boxed()
        }
        Schema::Tree { branches } => {
            let strategies: Vec<(Key, BoxedStrategy<Value>)> =
                branches.iter().map(|(k, s)| (k.clone(), arb_value(s))).collect();
            combine_fields(strategies).prop_map(Value::Map).boxed()
        }
        Schema::Array { shape, element } => arb_array(shape, element),
        Schema::Tuple { elements } => {
            let strategies: Vec<(Key, BoxedStrategy<Value>)> = elements
                .iter()
                .enumerate()
                .map(|(i, s)| (Key::from(i.to_string()), arb_value(s)))
                .collect();
            combine_fields(strategies)
                .prop_map(|m| Value::List(m.into_values().collect()))
                .boxed()
        }
        _ => Just(Value::None).boxed(),
    }
}

/// Fold a vec of per-key strategies into one strategy producing an ordered map
/// (proptest has no built-in for a heterogeneous vec of strategies).
fn combine_fields(
    items: Vec<(Key, BoxedStrategy<Value>)>,
) -> BoxedStrategy<IndexMap<Key, Value>> {
    let mut acc: BoxedStrategy<IndexMap<Key, Value>> = Just(IndexMap::new()).boxed();
    for (k, strat) in items {
        acc = (acc, strat)
            .prop_map(move |(mut m, v)| {
                m.insert(k.clone(), v);
                m
            })
            .boxed();
    }
    acc
}

/// Nested lists of small-int floats matching `shape`.
fn arb_array(shape: &[usize], element: &Schema) -> BoxedStrategy<Value> {
    if shape.is_empty() {
        return arb_value(element);
    }
    let head = shape[0];
    let rest = shape[1..].to_vec();
    let element = element.clone();
    Just(())
        .prop_flat_map(move |_| {
            let cells: Vec<BoxedStrategy<Value>> =
                (0..head).map(|_| arb_array(&rest, &element)).collect();
            cells
        })
        .prop_map(Value::List)
        .boxed()
}

/// The identity element of the update monoid for a sort: applying it leaves
/// the value unchanged. Additive numeric → 0; containers that walk keys →
/// empty; element-wise-additive arrays → zeros; replace-types → the value
/// itself.
fn identity_update(schema: &Schema, current: &Value) -> Value {
    match schema {
        Schema::Float { .. } | Schema::Delta { .. } => Value::float(0.0),
        Schema::Integer { .. } => Value::Int(0),
        Schema::Map { .. } | Schema::Tree { .. } | Schema::RecursiveTree { .. } => Value::map(),
        Schema::Array { shape, element } => zeros_array(shape, element),
        Schema::Tuple { elements } => {
            let cur = current.as_list().unwrap_or(&[]);
            Value::List(
                elements
                    .iter()
                    .enumerate()
                    .map(|(i, s)| identity_update(s, cur.get(i).unwrap_or(&Value::None)))
                    .collect(),
            )
        }
        Schema::Maybe { inner } => match current {
            Value::None => Value::None,
            other => identity_update(inner, other),
        },
        // Bool / String / Enum / List / Overwrite: apply replaces, so the
        // value itself is its own identity update.
        _ => current.clone(),
    }
}

fn zeros_array(shape: &[usize], element: &Schema) -> Value {
    if shape.is_empty() {
        return identity_update(element, &Value::float(0.0));
    }
    Value::List((0..shape[0]).map(|_| zeros_array(&shape[1..], element)).collect())
}

// ── schema comparison helpers ──────────────────────────────────────────

/// Normalize a schema for "up to default / branch order" comparison: clear
/// every `default`, sort `Tree` branches by key. Used by the
/// commutativity/associativity laws (resolve is commutative *up to default*).
fn normalize(schema: &Schema) -> Schema {
    use Schema::*;
    match schema {
        Float { .. } => Float { default: None },
        Integer { .. } => Integer { default: None },
        Delta { .. } => Delta { default: None },
        Bool { .. } => Bool { default: None },
        String { .. } => String { default: None },
        Enum { values, .. } => {
            let mut v = values.clone();
            v.sort();
            Enum { values: v, default: None }
        }
        Overwrite { inner } => Overwrite { inner: Box::new(normalize(inner)) },
        Maybe { inner } => Maybe { inner: Box::new(normalize(inner)) },
        Const { inner } => Const { inner: Box::new(normalize(inner)) },
        Quote { inner } => Quote { inner: Box::new(normalize(inner)) },
        List { element } => List { element: Box::new(normalize(element)) },
        Map { value } => Map { value: Box::new(normalize(value)) },
        Array { shape, element } => Array { shape: shape.clone(), element: Box::new(normalize(element)) },
        RecursiveTree { leaf } => RecursiveTree { leaf: Box::new(normalize(leaf)) },
        Tuple { elements } => Tuple { elements: elements.iter().map(normalize).collect() },
        Tree { branches } => {
            let mut keys: Vec<&Key> = branches.keys().collect();
            keys.sort();
            Tree {
                branches: keys.into_iter().map(|k| (k.clone(), normalize(&branches[k]))).collect(),
            }
        }
        other => other.clone(),
    }
}

fn schema_eq_mod(a: &Schema, b: &Schema) -> bool {
    normalize(a) == normalize(b)
}

// ── compatible-schema generators (for the semilattice laws) ─────────────
//
// `resolve` is a semilattice only on *compatible* sorts; on incompatible
// sorts (e.g. `Float` vs `List`) it deterministically takes the update side
// (faithful to upstream-minus-raise), which is not commutative. So the
// commutativity / associativity laws range over triples of the *same kind*.

/// Three schemas of the same base kind, recursively. Each leaf group keeps
/// the three sides within one resolve-compatible family.
fn arb_compatible_triple() -> impl Strategy<Value = (Schema, Schema, Schema)> {
    // Float/Integer are mutually resolve-compatible; so are Float/Delta. Keep
    // Integer and Delta apart (no resolve arm pairs them).
    let fi = || prop_oneof![
        Just(Schema::float()),
        Just(Schema::Float { default: Some(0.0) }),
        Just(Schema::integer()),
        Just(Schema::Integer { default: Some(0) }),
    ];
    let fd = || prop_oneof![
        Just(Schema::float()),
        Just(Schema::Delta { default: None }),
        Just(Schema::Delta { default: Some(0.0) }),
    ];
    let scalars = prop_oneof![
        (fi(), fi(), fi()),
        (fd(), fd(), fd()),
        (Just(Schema::bool()), Just(Schema::bool()), Just(Schema::bool())),
        (Just(Schema::string()), Just(Schema::string()), Just(Schema::string())),
        (
            Just(Schema::Enum { values: vec!["x".into(), "y".into()], default: None }),
            Just(Schema::Enum { values: vec!["y".into(), "z".into()], default: None }),
            Just(Schema::Enum { values: vec!["x".into(), "z".into()], default: None }),
        ),
    ];
    scalars.prop_recursive(3, 30, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|(a, b, c)| (Schema::overwrite(a), Schema::overwrite(b), Schema::overwrite(c))),
            inner.clone().prop_map(|(a, b, c)| (Schema::maybe(a), Schema::maybe(b), Schema::maybe(c))),
            inner.clone().prop_map(|(a, b, c)| (Schema::list(a), Schema::list(b), Schema::list(c))),
            inner.clone().prop_map(|(a, b, c)| (Schema::map(a), Schema::map(b), Schema::map(c))),
            inner.clone().prop_map(|(a, b, c)| (
                Schema::tree([("k", a)]),
                Schema::tree([("k", b)]),
                Schema::tree([("k", c)]),
            )),
        ]
    })
}

/// Structural value equality with exact float compare (floats are integer-
/// valued by construction, so this is safe) and **order-insensitive** map
/// comparison — `diff`+`apply` legitimately rebuild a map's keys in a different
/// order (current's order + appended additions), which is the same value.
fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Map(ma), Value::Map(mb)) => {
            ma.len() == mb.len()
                && ma.iter().all(|(k, va)| mb.get(k).is_some_and(|vb| value_eq(va, vb)))
        }
        (Value::List(la), Value::List(lb)) => {
            la.len() == lb.len() && la.iter().zip(lb).all(|(x, y)| value_eq(x, y))
        }
        _ => a == b,
    }
}

/// Commutative (additive) sorts for the reconcile-coherence law. For these,
/// batching via `reconcile` must equal sequential `apply` (law #2). Non-
/// commutative sorts (`List` concat, structural maps) are excluded — there
/// reconcile is the *defined* batching, not equal to sequential apply.
fn arb_additive() -> impl Strategy<Value = Schema> {
    prop_oneof![
        Just(Schema::float()),
        Just(Schema::integer()),
        Just(Schema::delta()),
        Just(Schema::map(Schema::float())),
        Just(Schema::tree([("a", Schema::float()), ("b", Schema::integer())])),
        Just(Schema::Array { shape: vec![3], element: Box::new(Schema::float()) }),
    ]
}

// ════════════════════════════════════════════════════════════════════════
// Laws
// ════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, cases: 400, ..ProptestConfig::default() })]

    // Law 1 — Apply identity. `apply(s, v, identity(s)) ≡ v`.
    #[test]
    fn law_apply_identity((schema, value) in arb_schema().prop_flat_map(|s| {
        let v = arb_value(&s); (Just(s), v)
    })) {
        let id = identity_update(&schema, &value);
        let out = algebra::apply(&schema, &value, &id);
        prop_assert!(value_eq(&out, &value), "apply(s,v,id) != v\n s={schema:?}\n v={value:?}\n id={id:?}\n out={out:?}");
    }

    // Law 2 — Reconcile coherence (commutative sorts).
    // `apply(s, v, reconcile(s, [u…])) ≡ foldl(apply, v, [u…])`.
    #[test]
    fn law_reconcile_coherence((schema, value, updates) in arb_additive().prop_flat_map(|s| {
        let v = arb_value(&s);
        let us = prop::collection::vec(arb_value(&s), 0..4);
        (Just(s), v, us)
    })) {
        let folded = updates.iter().fold(value.clone(), |acc, u| algebra::apply(&schema, &acc, u));
        match algebra::reconcile(&schema, &updates) {
            Some(u) => {
                let batched = algebra::apply(&schema, &value, &u);
                prop_assert!(value_eq(&batched, &folded), "reconcile incoherent\n s={schema:?}\n v={value:?}\n updates={updates:?}\n batched={batched:?}\n folded={folded:?}");
            }
            None => prop_assert!(value_eq(&folded, &value), "reconcile None but fold changed value\n s={schema:?}\n v={value:?}\n updates={updates:?}\n folded={folded:?}"),
        }
    }

    // Law 7 — Diff/apply inverse. `apply(s, a, diff(s, a, b)) ≡ b`.
    #[test]
    fn law_diff_apply_inverse((schema, a, b) in arb_schema().prop_flat_map(|s| {
        let av = arb_value(&s); let bv = arb_value(&s); (Just(s), av, bv)
    })) {
        match algebra::diff(&schema, &a, &b) {
            Some(u) => {
                let out = algebra::apply(&schema, &a, &u);
                prop_assert!(value_eq(&out, &b), "apply(a, diff(a,b)) != b\n s={schema:?}\n a={a:?}\n b={b:?}\n u={u:?}\n out={out:?}");
            }
            None => prop_assert!(value_eq(&a, &b), "diff returned None but a != b\n s={schema:?}\n a={a:?}\n b={b:?}"),
        }
    }

    // Law 6 — Check preservation. `check(s, default(s))`.
    #[test]
    fn law_check_default(schema in arb_schema()) {
        let d = algebra::default(&schema);
        prop_assert!(algebra::check(&schema, &d), "check(s, default(s)) failed\n s={schema:?}\n default={d:?}");
    }

    // Law 6 — Apply stays in the sort. `check(s, apply(s, v, u))`.
    #[test]
    fn law_apply_preserves_sort((schema, value, update) in arb_schema().prop_flat_map(|s| {
        let v = arb_value(&s); let u = arb_value(&s); (Just(s), v, u)
    })) {
        let out = algebra::apply(&schema, &value, &update);
        // Maybe can legitimately become None (delete); skip that case.
        if matches!(schema, Schema::Maybe { .. }) && matches!(out, Value::None) { return Ok(()); }
        prop_assert!(algebra::check(&schema, &out), "apply left the sort\n s={schema:?}\n v={value:?}\n u={update:?}\n out={out:?}");
    }

    // Law 8 — Codec round-trip. `deserialize(s, serialize(s, v)) ≡ v`.
    #[test]
    fn law_codec_round_trip((schema, value) in arb_schema().prop_flat_map(|s| {
        let v = arb_value(&s); (Just(s), v)
    })) {
        let enc = algebra::serialize(&schema, &value);
        let dec = algebra::deserialize(&schema, &enc);
        prop_assert!(value_eq(&dec, &value), "round-trip changed value\n s={schema:?}\n v={value:?}\n enc={enc:?}\n dec={dec:?}");
    }

    // Law 4 — Resolve idempotent. `resolve(s, s) ≡ s`.
    #[test]
    fn law_resolve_idempotent(schema in arb_schema()) {
        let r = algebra::resolve(&schema, &schema);
        prop_assert!(schema_eq_mod(&r, &schema), "resolve(s,s) != s\n s={schema:?}\n r={r:?}");
    }

    // Law 4 — Resolve `Any`-identity. `resolve(Any, s) = s = resolve(s, Any)`.
    #[test]
    fn law_resolve_any_identity(schema in arb_schema()) {
        prop_assert_eq!(algebra::resolve(&Schema::Any, &schema), schema.clone());
        prop_assert_eq!(algebra::resolve(&schema, &Schema::Any), schema.clone());
    }

    // Law 4 — Resolve commutative *up to default / branch order* (on
    // compatible sorts; incompatible sorts deterministically take the update).
    #[test]
    fn law_resolve_commutative((a, b, _c) in arb_compatible_triple()) {
        let ab = algebra::resolve(&a, &b);
        let ba = algebra::resolve(&b, &a);
        prop_assert!(schema_eq_mod(&ab, &ba), "resolve not commutative (mod default)\n a={a:?}\n b={b:?}\n ab={ab:?}\n ba={ba:?}");
    }

    // Law 4 — Resolve associative (mod default / branch order) on compatible
    // sorts. `resolve(resolve(a,b),c) ≡ resolve(a,resolve(b,c))`.
    #[test]
    fn law_resolve_associative((a, b, c) in arb_compatible_triple()) {
        let left = algebra::resolve(&algebra::resolve(&a, &b), &c);
        let right = algebra::resolve(&a, &algebra::resolve(&b, &c));
        prop_assert!(schema_eq_mod(&left, &right), "resolve not associative (mod default)\n a={a:?}\n b={b:?}\n c={c:?}\n left={left:?}\n right={right:?}");
    }

    // Law 5 — Promote ≤ resolve. On a leaf pair, `promote ≡ resolve`; over a
    // tree, promote restricted to sparse's branches agrees with resolve there.
    #[test]
    fn law_promote_agrees_with_resolve_on_sparse_paths(lib in arb_schema(), sparse in arb_schema()) {
        let p = algebra::promote(&lib, &sparse);
        match (&sparse, &p) {
            // Tree sparse: every branch promote produced must equal resolve's
            // on that branch (the paths sparse touches).
            (Schema::Tree { branches: sp }, Schema::Tree { branches: pr }) => {
                let r = algebra::resolve(&lib, &sparse);
                if let Schema::Tree { branches: rb } = &r {
                    for k in sp.keys() {
                        if let (Some(pv), Some(rv)) = (pr.get(k), rb.get(k)) {
                            prop_assert!(schema_eq_mod(pv, rv), "promote disagrees with resolve at {k}\n p={pv:?}\n r={rv:?}");
                        }
                    }
                }
            }
            // Leaf pair: promote is exactly resolve.
            _ => {
                let r = algebra::resolve(&lib, &sparse);
                prop_assert!(schema_eq_mod(&p, &r), "promote != resolve on leaf pair\n lib={lib:?}\n sparse={sparse:?}\n p={p:?}\n r={r:?}");
            }
        }
    }

    // Generalize (meet) — idempotent and `Any`-identity.
    #[test]
    fn law_generalize_idempotent(schema in arb_schema()) {
        let g = algebra::generalize(&schema, &schema);
        // generalize forgets modifiers; idempotency holds modulo that + default.
        let expect = strip_all_modifiers(&schema);
        prop_assert!(schema_eq_mod(&g, &expect), "generalize(s,s) != s (mod modifiers)\n s={schema:?}\n g={g:?}");
    }

    #[test]
    fn law_generalize_any_identity(schema in arb_schema()) {
        prop_assert_eq!(algebra::generalize(&Schema::Any, &schema), schema.clone());
        prop_assert_eq!(algebra::generalize(&schema, &Schema::Any), schema.clone());
    }
}

/// Recursively strip update-modifier wrappers (for the generalize idempotency
/// expectation — `generalize` forgets them).
fn strip_all_modifiers(schema: &Schema) -> Schema {
    use Schema::*;
    match schema {
        Overwrite { inner } | Maybe { inner } | Const { inner } | Quote { inner } => {
            strip_all_modifiers(inner)
        }
        List { element } => List { element: Box::new(strip_all_modifiers(element)) },
        Map { value } => Map { value: Box::new(strip_all_modifiers(value)) },
        Array { shape, element } => {
            Array { shape: shape.clone(), element: Box::new(strip_all_modifiers(element)) }
        }
        RecursiveTree { leaf } => RecursiveTree { leaf: Box::new(strip_all_modifiers(leaf)) },
        Tuple { elements } => Tuple { elements: elements.iter().map(strip_all_modifiers).collect() },
        Tree { branches } => Tree {
            branches: branches.iter().map(|(k, s)| (k.clone(), strip_all_modifiers(s))).collect(),
        },
        other => other.clone(),
    }
}
