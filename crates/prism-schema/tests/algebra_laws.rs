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
        // Link-kind nodes — the four bigraph sorts. Each carries a small
        // scalar data face (additive `Delta`/`Integer`) so apply/reconcile/
        // divide actually exercise `node_data_branches`. `inputs` are empty
        // (no port-side state is materialised at the laws' level — ports are
        // wires, not local fields).
        Just(node_link_with_face()),
        Just(node_step_link_with_face()),
        Just(node_process_link_with_face()),
        Just(node_composite_link_with_face()),
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

/// Standard self-exported data face for the law generators: `mass: Delta`
/// (extensive additive) + `count: Integer` (extensive additive). Both are in
/// `node_data_branches`'s additive filter, so apply/reconcile/divide route
/// them through the data-face path and the laws stay crisp.
fn node_face() -> IndexMap<Key, Schema> {
    let mut outputs = IndexMap::new();
    outputs.insert(Key::from("mass"), Schema::Delta { default: None });
    outputs.insert(Key::from("count"), Schema::integer());
    outputs
}

fn node_link_with_face() -> Schema {
    Schema::Link {
        inputs: IndexMap::new(),
        outputs: node_face(),
        temporal: None,
    }
}

fn node_step_link_with_face() -> Schema {
    Schema::StepLink {
        inputs: IndexMap::new(),
        outputs: node_face(),
        priority: 0.0,
    }
}

fn node_process_link_with_face() -> Schema {
    Schema::ProcessLink {
        inputs: IndexMap::new(),
        outputs: node_face(),
        interval: 1.0,
    }
}

fn node_composite_link_with_face() -> Schema {
    Schema::CompositeLink {
        inputs: IndexMap::new(),
        outputs: node_face(),
        interval: 1.0,
        inner_schema: Box::new(Schema::Tree {
            branches: IndexMap::from([(Key::from("mass"), Schema::Delta { default: None })]),
        }),
    }
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
        // Link-kind: a node value is `{address, …spec keys…, …self-exported face…}`.
        // We emit the face fields (drawn from `node_data_branches`, additive) plus
        // a fixed `address` so `check` passes (Link checks for `address` or
        // `instance`). The address is HELD CONSTANT across a triple so the law
        // generators don't race on the non-additive spec key when sequenced —
        // a process spec is set once at instantiation; varying it across
        // updates is unrealistic and would break per-key commutativity.
        Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. } => {
            let face_branches = schema.node_data_branches();
            let strategies: Vec<(Key, BoxedStrategy<Value>)> = face_branches
                .iter()
                .map(|(k, s)| (k.clone(), arb_value(s)))
                .collect();
            combine_fields(strategies)
                .prop_map(|mut m| {
                    m.insert(Key::from("address"), Value::String("local:node".into()));
                    Value::Map(m)
                })
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
        // Link-kind nodes route apply through Tree + `node_data_branches`. An
        // empty-map update touches no keys → current value is preserved.
        Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. } => Value::map(),
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

/// Does `promote` agree with `resolve` on exactly the paths `sparse` touches?
/// `promote` deliberately *restricts* to sparse's branches (it does not union
/// in library-only branches the way `resolve` does), so the agreement must be
/// checked **recursively, branch-by-branch over `sparse`** — comparing whole
/// subtrees is wrong (e.g. lib=`{a:{c}}`, sparse=`{a:{a}}`: `promote[a]={a}`
/// but `resolve[a]={c,a}` — they agree on sparse's path `a.a`, which is all the
/// law claims).
fn agrees_on_sparse_paths(p: &Schema, r: &Schema, sparse: &Schema) -> bool {
    match sparse {
        Schema::Tree { branches: sp } => match (p, r) {
            (Schema::Tree { branches: pb }, Schema::Tree { branches: rb }) => {
                sp.iter().all(|(k, sv)| match (pb.get(k), rb.get(k)) {
                    (Some(pv), Some(rv)) => agrees_on_sparse_paths(pv, rv, sv),
                    // A branch promote/resolve didn't carry isn't a path the
                    // claim covers.
                    _ => true,
                })
            }
            _ => schema_eq_mod(p, r),
        },
        // Leaf path: promote ≡ resolve there.
        _ => schema_eq_mod(p, r),
    }
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
        // Link-kind nodes are additive on their `node_data_branches` data face
        // (`Delta`/`Integer`). For reconcile coherence to hold we feed only
        // face-shaped updates (no varying spec keys) — see `arb_face_update`.
        Just(node_link_with_face()),
        Just(node_step_link_with_face()),
        Just(node_process_link_with_face()),
        Just(node_composite_link_with_face()),
    ]
}

/// Updates suitable for the additive reconcile-coherence law.
///
/// For Link-kind schemas: a `Value::Map` of ONLY the additive face fields (no
/// `address`/spec keys), because those non-additive keys would break per-key
/// commutativity if they varied across updates. For all other schemas: defers
/// to `arb_value`.
fn arb_face_update(schema: &Schema) -> BoxedStrategy<Value> {
    match schema {
        Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. } => {
            let face_branches = schema.node_data_branches();
            let strategies: Vec<(Key, BoxedStrategy<Value>)> = face_branches
                .iter()
                .map(|(k, s)| (k.clone(), arb_value(s)))
                .collect();
            combine_fields(strategies).prop_map(Value::Map).boxed()
        }
        _ => arb_value(schema),
    }
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
        let out = algebra::apply_with(None, &schema, &value, &id);
        prop_assert!(value_eq(&out, &value), "apply(s,v,id) != v\n s={schema:?}\n v={value:?}\n id={id:?}\n out={out:?}");
    }

    // Law 2 — Reconcile coherence (commutative sorts).
    // `apply(s, v, reconcile(s, [u…])) ≡ foldl(apply, v, [u…])`.
    //
    // For Link-kind schemas the base value carries spec keys (`address`) plus
    // the data face, but the *updates* must be face-only (`arb_face_update`)
    // — varying a non-additive spec key across updates would break per-key
    // commutativity (the law tests commutative composition of the face).
    #[test]
    fn law_reconcile_coherence((schema, value, updates) in arb_additive().prop_flat_map(|s| {
        let v = arb_value(&s);
        let us = prop::collection::vec(arb_face_update(&s), 0..4);
        (Just(s), v, us)
    })) {
        let folded = updates.iter().fold(value.clone(), |acc, u| algebra::apply_with(None, &schema, &acc, u));
        match algebra::reconcile(&schema, &updates) {
            Some(u) => {
                let batched = algebra::apply_with(None, &schema, &value, &u);
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
                let out = algebra::apply_with(None, &schema, &a, &u);
                prop_assert!(value_eq(&out, &b), "apply(a, diff(a,b)) != b\n s={schema:?}\n a={a:?}\n b={b:?}\n u={u:?}\n out={out:?}");
            }
            None => prop_assert!(value_eq(&a, &b), "diff returned None but a != b\n s={schema:?}\n a={a:?}\n b={b:?}"),
        }
    }

    // Law 6 — Check preservation. `check(s, default(s))`.
    #[test]
    fn law_check_default(schema in arb_schema()) {
        let d = algebra::default_with(None, &schema);
        prop_assert!(algebra::check_with(None, &schema, &d), "check(s, default(s)) failed\n s={schema:?}\n default={d:?}");
    }

    // Law 6 — Apply stays in the sort. `check(s, apply(s, v, u))`.
    #[test]
    fn law_apply_preserves_sort((schema, value, update) in arb_schema().prop_flat_map(|s| {
        let v = arb_value(&s); let u = arb_value(&s); (Just(s), v, u)
    })) {
        let out = algebra::apply_with(None, &schema, &value, &update);
        // Maybe can legitimately become None (delete); skip that case.
        if matches!(schema, Schema::Maybe { .. }) && matches!(out, Value::None) { return Ok(()); }
        prop_assert!(algebra::check_with(None, &schema, &out), "apply left the sort\n s={schema:?}\n v={value:?}\n u={update:?}\n out={out:?}");
    }

    // Law 8 — Codec round-trip. `deserialize(s, serialize(s, v)) ≡ v`.
    #[test]
    fn law_codec_round_trip((schema, value) in arb_schema().prop_flat_map(|s| {
        let v = arb_value(&s); (Just(s), v)
    })) {
        let enc = algebra::serialize_with(None, &schema, &value);
        let dec = algebra::realize_with(None, &schema, &enc);
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
        // `promote` restricts the join to the paths `sparse` touches, so it
        // must agree with `resolve` on exactly those paths — checked
        // recursively (comparing whole subtrees would wrongly flag the
        // library-only branches `resolve` unions in but `promote` omits).
        let p = algebra::promote(&lib, &sparse);
        let r = algebra::resolve(&lib, &sparse);
        prop_assert!(
            agrees_on_sparse_paths(&p, &r, &sparse),
            "promote disagrees with resolve on a sparse path\n lib={lib:?}\n sparse={sparse:?}\n p={p:?}\n r={r:?}"
        );
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
