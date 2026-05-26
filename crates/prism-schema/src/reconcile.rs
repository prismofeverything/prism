//! Reconcile multiple updates targeting the same state path into a
//! single update before [`Schema::apply_update`] runs.
//!
//! When multiple steps in one timestep produce updates aimed at the
//! same location, applying them sequentially does not always commute.
//! `reconcile` combines them up-front according to the schema's
//! semantics, so the apply path consumes one normalized update per
//! site.
//!
//! Relationship:
//!
//! ```text
//! apply(schema, state, reconcile(schema, [u1, u2, u3]))
//!   ==
//! apply(schema, apply(schema, apply(schema, state, u1), u2), u3)
//! ```
//!
//! …for commutative types. For non-commutative types (e.g. `Overwrite`,
//! structural `_add`/`_remove` deltas on `List`/`Map`), reconcile
//! provides the correct batching that a sequential apply could not.
//!
//! Ported from `bigraph_schema.methods.reconcile`. Currently covers the
//! types prism has; the few upstream types we haven't ported yet
//! (`Atom`, `Set`, `Frame`, `Quantity`, …) fall back to the generic
//! "last non-None wins" rule.

use indexmap::IndexMap;

use crate::registry::TypeRegistry;
use crate::schema::Schema;
use crate::value::{Key, StateMap, Value};

/// Combine a batch of updates targeting one path into a single
/// reconciled update.
///
/// Returns `None` when the batch's net effect is a no-op (e.g. zero
/// numeric delta, all-empty structural sentinels).
pub fn reconcile(schema: &Schema, updates: &[Value]) -> Option<Value> {
    reconcile_with(None, schema, updates)
}

/// [`reconcile`] consulting a [`TypeRegistry`] so a `Schema::Custom` slot
/// reconciles by its REPRESENTATION (delegating structurally) instead of
/// opaquely last-wins — and nested Custom slots inside `Map`/`Tree`/… delegate
/// too. The registry-aware entry the engine uses; mirrors `apply_with`, giving
/// full Custom→representation delegation (the update monoid is the same whether
/// you `apply` deltas one-by-one or `reconcile` then `apply`).
pub fn reconcile_with(
    registry: Option<&TypeRegistry>,
    schema: &Schema,
    updates: &[Value],
) -> Option<Value> {
    match schema {
        // Schemas that carry no state — updates are ignored.
        Schema::Site { .. }
        | Schema::InnerName { .. }
        | Schema::OuterName { .. }
        | Schema::Interface { .. } => None,

        // Const: immutable, all updates dropped.
        Schema::Const { .. } => None,

        // Overwrite / Quote: last non-None update wins.
        Schema::Overwrite { .. } | Schema::Quote { .. } => last_non_none(updates),

        // Numeric additive types: sum the deltas.
        Schema::Float { .. } | Schema::Delta { .. } => {
            let mut total = 0.0;
            let mut saw = false;
            for u in updates {
                if let Some(v) = u.as_f64() {
                    total += v;
                    saw = true;
                }
            }
            if saw && total.abs() > f64::EPSILON {
                Some(Value::float(total))
            } else {
                None
            }
        }
        Schema::Integer { .. } => {
            let mut total: i64 = 0;
            let mut saw = false;
            for u in updates {
                if let Some(v) = u.as_i64() {
                    total += v;
                    saw = true;
                }
            }
            if saw && total != 0 {
                Some(Value::Int(total))
            } else {
                None
            }
        }

        // Last-wins types.
        Schema::Bool { .. } | Schema::String { .. } | Schema::Enum { .. } => {
            last_non_none(updates)
        }

        // Optional: delegate to inner, treating Value::None as absent.
        Schema::Maybe { inner } => {
            let filtered: Vec<Value> = updates
                .iter()
                .filter(|u| !matches!(u, Value::None))
                .cloned()
                .collect();
            reconcile_with(registry, inner, &filtered)
        }

        // Map: group updates by key, recurse per key. Carve out
        // structural sentinels (`_add`/`_remove`) — they're handled
        // holistically at this level.
        Schema::Map { value } => reconcile_map(registry, value, updates),

        // Tree: per-branch reconcile using each branch's schema. Mixed
        // batches (some dict-shaped, some scalar) resolve to
        // last-non-dict-wins, matching the apply path.
        Schema::Tree { branches } => reconcile_tree(registry, branches, updates),

        // List: structural _add/_remove batching.
        Schema::List { .. } => reconcile_list(updates),

        // Tuple: element-wise reconcile per position.
        Schema::Tuple { elements } => reconcile_tuple(registry, elements, updates),

        // Array: element-wise sum of the deltas (representation-agnostic over
        // flat / nested lists) — coherent with the additive `Array` apply, so
        // `apply(v, reconcile([d…])) == foldl(apply, v, [d…])` (law #2).
        Schema::Array { .. } => reconcile_array(updates),

        // RecursiveTree: infer mode from update shapes. All non-dict →
        // leaf reconcile; any dict → tree-node reconcile.
        Schema::RecursiveTree { leaf } => reconcile_recursive_tree(registry, schema, leaf, updates),

        // Custom: delegate to the type's REPRESENTATION (so e.g. a graph's
        // `_add`/`_remove` deltas collate structurally instead of last-wins).
        // Without a registry/representation, fall back to last-wins.
        Schema::Custom { name, .. } => match registry.and_then(|r| r.schema(name)) {
            Some(repr) => reconcile_with(registry, repr, updates),
            None => last_non_none(updates),
        },

        // Any Link-kind NODE (Link / StepLink / ProcessLink / CompositeLink):
        // a node carries a data face (`mass: Delta`, …) plus spec keys. Concurrent
        // writes to DIFFERENT fields of one node in a tick — e.g. a grow `mass`
        // delta AND a `divide` marker — MUST merge, not last-wins. Reconcile the
        // data face like a Tree (additive `Delta`, etc.) and union the rest,
        // matching `apply`/`divide`'s `node_data_branches` routing. Plain last-wins
        // dropped the mass delta in any tick that also set another field — the
        // grow/divide mass-conservation leak. For nodes with no self-exported
        // face, `node_data_branches` is empty → reconcile_tree collapses to the
        // per-key collation the spec map expects.
        Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. } => {
            reconcile_tree(registry, &schema.node_data_branches(), updates)
        }

        // Bridge + Any: opaque — last non-None wins.
        Schema::Bridge { .. } | Schema::Any => last_non_none(updates),
    }
}

fn last_non_none(updates: &[Value]) -> Option<Value> {
    for u in updates.iter().rev() {
        if !matches!(u, Value::None) {
            return Some(u.clone());
        }
    }
    None
}

/// Collate a batch of map-shaped updates into **one coherent** update: union
/// every `_add`, union every `_remove` (or `all`), last-wins `_divide`, and
/// per-key reconcile of the remaining value updates (each via `schema_at`).
///
/// This is the shared core of every map-like reconciler. Concurrent structural
/// updates from different writers in one tick MUST merge into a single
/// `{_add, _remove, _divide, …per-key}` — collating, not clobbering — so a
/// later writer's `_add` doesn't drop an earlier writer's `_remove`, etc.
fn reconcile_keyed<'s>(
    registry: Option<&TypeRegistry>,
    updates: &[Value],
    schema_at: impl Fn(&Key) -> &'s Schema,
) -> Option<Value> {
    let mut adds: StateMap = IndexMap::new();
    let mut removes: Vec<Key> = Vec::new();
    let mut remove_all = false;
    let mut divide: Option<Value> = None;
    let mut grouped: IndexMap<Key, Vec<Value>> = IndexMap::new();

    for update in updates {
        let Value::Map(map) = update else {
            // Non-map update at a map-like slot is unusual — skip it.
            continue;
        };
        for (k, v) in map {
            match k.as_str() {
                "_add" => {
                    if let Value::Map(add_map) = v {
                        for (ak, av) in add_map {
                            adds.insert(ak.clone(), av.clone());
                        }
                    }
                }
                "_remove" => match v {
                    Value::List(list) => {
                        for item in list {
                            if let Value::String(s) = item {
                                if s == "all" {
                                    remove_all = true;
                                } else if !remove_all {
                                    let key = Key::from(s.as_str());
                                    if !removes.contains(&key) {
                                        removes.push(key);
                                    }
                                }
                            }
                        }
                    }
                    Value::String(s) if s == "all" => remove_all = true,
                    _ => {}
                },
                // `_divide` is a singleton structural directive — last non-None
                // wins (a second within one tick is redundant/contradictory).
                "_divide" => {
                    if !matches!(v, Value::None) {
                        divide = Some(v.clone());
                    }
                }
                _ => {
                    grouped.entry(k.clone()).or_default().push(v.clone());
                }
            }
        }
    }

    // Recurse per key (nested structural sentinels collate at their level too).
    let mut value_updates: StateMap = IndexMap::new();
    for (key, sub_updates) in grouped {
        let reconciled = if sub_updates.len() == 1 {
            Some(sub_updates[0].clone())
        } else {
            reconcile_with(registry, schema_at(&key), &sub_updates)
        };
        if let Some(v) = reconciled {
            value_updates.insert(key, v);
        }
    }

    // A key removed this tick voids any concurrent value-update to it
    // ("remove wins") — otherwise applying the modification would re-create the
    // removed store. (Equivalent to the sequential order "modify, then remove".)
    // Snapshot the removed set before `removes` is consumed below.
    let removed_set: std::collections::HashSet<Key> = removes.iter().cloned().collect();

    let mut result: StateMap = IndexMap::new();
    if !adds.is_empty() {
        result.insert("_add".into(), Value::Map(adds));
    }
    if remove_all {
        result.insert("_remove".into(), Value::String("all".into()));
    } else if !removes.is_empty() {
        result.insert(
            "_remove".into(),
            Value::List(removes.into_iter().map(|k| Value::String(k.to_string())).collect()),
        );
    }
    if let Some(d) = divide {
        result.insert("_divide".into(), d);
    }
    for (k, v) in value_updates {
        if remove_all || removed_set.contains(&k) {
            continue;
        }
        result.insert(k, v);
    }
    if result.is_empty() {
        None
    } else {
        Some(Value::Map(result))
    }
}

/// Map: every key uses the uniform value schema.
fn reconcile_map(
    registry: Option<&TypeRegistry>,
    value_schema: &Schema,
    updates: &[Value],
) -> Option<Value> {
    reconcile_keyed(registry, updates, |_| value_schema)
}

fn reconcile_tree(
    registry: Option<&TypeRegistry>,
    branches: &IndexMap<Key, Schema>,
    updates: &[Value],
) -> Option<Value> {
    // Filter non-None.
    let non_none: Vec<&Value> = updates.iter().filter(|u| !matches!(u, Value::None)).collect();
    if non_none.is_empty() {
        return None;
    }
    let any_map = non_none.iter().any(|u| matches!(u, Value::Map(_)));
    if !any_map {
        // All non-map — leaf-mode; let the last non-None win (no per-branch
        // schema applies to a leaf at this position).
        return last_non_none(updates);
    }
    if non_none.iter().any(|u| !matches!(u, Value::Map(_))) {
        // Mixed: a whole-node overwrite wins (matches the apply outcome).
        for u in updates.iter().rev() {
            if !matches!(u, Value::None | Value::Map(_)) {
                return Some(u.clone());
            }
        }
    }
    // Tree-node mode: collate structural sentinels + per-branch reconcile.
    reconcile_keyed(registry, updates, |k| branches.get(k).unwrap_or(&Schema::Any))
}

fn reconcile_list(updates: &[Value]) -> Option<Value> {
    let mut adds: Vec<Value> = Vec::new();
    let mut remove_indexes: Vec<Value> = Vec::new();
    let mut remove_all = false;
    let mut has_structural = false;
    let mut plain_concat: Vec<Value> = Vec::new();
    let mut any_plain = false;

    for update in updates {
        match update {
            Value::None => continue,
            Value::Map(map) => {
                if let Some(Value::Map(add_map)) = map.get("_add") {
                    // _add as map of items keyed by index/string — flatten values.
                    has_structural = true;
                    for v in add_map.values() {
                        adds.push(v.clone());
                    }
                }
                if let Some(Value::List(add_list)) = map.get("_add") {
                    has_structural = true;
                    adds.extend(add_list.iter().cloned());
                }
                if let Some(rm) = map.get("_remove") {
                    has_structural = true;
                    match rm {
                        Value::String(s) if s == "all" => remove_all = true,
                        Value::List(list) => {
                            if !remove_all {
                                for item in list {
                                    if !remove_indexes.contains(item) {
                                        remove_indexes.push(item.clone());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Value::List(list) => {
                any_plain = true;
                plain_concat.extend(list.iter().cloned());
            }
            _ => {}
        }
    }

    if !has_structural && any_plain {
        // Fast path: concatenation.
        return Some(Value::List(plain_concat));
    }
    if has_structural {
        adds.extend(plain_concat);
        let mut result: StateMap = IndexMap::new();
        if remove_all {
            result.insert("_remove".into(), Value::String("all".into()));
        } else if !remove_indexes.is_empty() {
            result.insert("_remove".into(), Value::List(remove_indexes));
        }
        if !adds.is_empty() {
            result.insert("_add".into(), Value::List(adds));
        }
        if !result.is_empty() {
            return Some(Value::Map(result));
        }
    }
    None
}

fn reconcile_tuple(
    registry: Option<&TypeRegistry>,
    elements: &[Schema],
    updates: &[Value],
) -> Option<Value> {
    let non_none: Vec<&Value> = updates.iter().filter(|u| !matches!(u, Value::None)).collect();
    if non_none.is_empty() {
        return None;
    }
    if non_none.len() == 1 {
        return Some(non_none[0].clone());
    }

    let n = elements.len();
    let mut result: Vec<Value> = vec![Value::None; n];
    let mut has_any = false;

    for i in 0..n {
        let contributions: Vec<Value> = non_none
            .iter()
            .filter_map(|u| match u {
                Value::List(list) => list.get(i).filter(|v| !matches!(v, Value::None)).cloned(),
                _ => None,
            })
            .collect();
        if contributions.is_empty() {
            continue;
        }
        result[i] = if contributions.len() == 1 {
            contributions[0].clone()
        } else {
            reconcile_with(registry, &elements[i], &contributions).unwrap_or(Value::None)
        };
        has_any = true;
    }

    if has_any {
        Some(Value::List(result))
    } else {
        None
    }
}

/// Element-wise sum of array deltas (flat or nested), the additive batching.
fn reconcile_array(updates: &[Value]) -> Option<Value> {
    let mut acc: Option<Value> = None;
    for u in updates {
        if matches!(u, Value::None) {
            continue;
        }
        acc = Some(match acc {
            None => u.clone(),
            Some(a) => array_add(&a, u),
        });
    }
    acc
}

/// Add two array deltas element-wise, recursing through nested lists and
/// summing numeric leaves (`Int + Int → Int`, otherwise `Float`).
fn array_add(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::List(la), Value::List(lb)) if la.len() == lb.len() => {
            Value::List(la.iter().zip(lb.iter()).map(|(x, y)| array_add(x, y)).collect())
        }
        (Value::Int(x), Value::Int(y)) => Value::Int(x + y),
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => Value::float(x + y),
            _ => b.clone(),
        },
    }
}

fn reconcile_recursive_tree(
    registry: Option<&TypeRegistry>,
    schema: &Schema,
    leaf: &Schema,
    updates: &[Value],
) -> Option<Value> {
    let non_none: Vec<&Value> = updates.iter().filter(|u| !matches!(u, Value::None)).collect();
    if non_none.is_empty() {
        return None;
    }
    let any_map = non_none.iter().any(|u| matches!(u, Value::Map(_)));
    if !any_map {
        // All leaves → reconcile at the leaf sort.
        return reconcile_with(registry, leaf, updates);
    }
    if non_none.iter().any(|u| !matches!(u, Value::Map(_))) {
        // Mixed — a whole-node overwrite (non-map) wins.
        for u in updates.iter().rev() {
            if !matches!(u, Value::None | Value::Map(_)) {
                return Some(u.clone());
            }
        }
    }
    // Tree-node mode: collate structural sentinels; children reconcile with
    // the same recursive schema (they may be leaves or nested trees).
    reconcile_keyed(registry, updates, |_| schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_sums() {
        let s = Schema::float();
        let updates = vec![Value::float(1.0), Value::float(2.5), Value::float(-0.5)];
        let r = reconcile(&s, &updates);
        assert_eq!(r.and_then(|v| v.as_f64()), Some(3.0));
    }

    #[test]
    fn integer_sums() {
        let s = Schema::integer();
        let updates = vec![Value::Int(3), Value::Int(-1), Value::Int(2)];
        let r = reconcile(&s, &updates);
        assert_eq!(r.and_then(|v| v.as_i64()), Some(4));
    }

    #[test]
    fn float_zero_returns_none() {
        let s = Schema::float();
        let updates = vec![Value::float(1.0), Value::float(-1.0)];
        assert!(reconcile(&s, &updates).is_none());
    }

    #[test]
    fn overwrite_last_wins() {
        let s = Schema::overwrite(Schema::float());
        let updates = vec![Value::float(1.0), Value::float(2.0), Value::None];
        assert_eq!(
            reconcile(&s, &updates).and_then(|v| v.as_f64()),
            Some(2.0)
        );
    }

    #[test]
    fn const_ignores_updates() {
        let s = Schema::const_of(Schema::float());
        let updates = vec![Value::float(1.0), Value::float(2.0)];
        assert!(reconcile(&s, &updates).is_none());
    }

    #[test]
    fn site_ignores_updates() {
        let s = Schema::site();
        let updates = vec![Value::float(1.0), Value::String("x".into())];
        assert!(reconcile(&s, &updates).is_none());
    }

    #[test]
    fn map_unions_adds_and_removes() {
        let s = Schema::map(Schema::float());
        let u1 = Value::Map(IndexMap::from_iter([
            ("_add".into(), Value::Map(IndexMap::from_iter([
                ("a".into(), Value::float(1.0)),
            ]))),
        ]));
        let u2 = Value::Map(IndexMap::from_iter([
            ("_remove".into(), Value::List(vec![Value::String("b".into())])),
        ]));
        let r = reconcile(&s, &[u1, u2]).unwrap();
        let Value::Map(map) = &r else {
            panic!("expected Map, got {r:?}");
        };
        assert!(map.contains_key("_add"));
        assert!(map.contains_key("_remove"));
    }

    #[test]
    fn map_recurses_per_key() {
        // Two updates to the same key with float-valued entries get summed.
        let s = Schema::map(Schema::float());
        let u1 = Value::Map(IndexMap::from_iter([
            ("a".into(), Value::float(1.0)),
        ]));
        let u2 = Value::Map(IndexMap::from_iter([
            ("a".into(), Value::float(2.0)),
        ]));
        let r = reconcile(&s, &[u1, u2]).unwrap();
        let Value::Map(map) = &r else { panic!() };
        assert_eq!(map.get("a").and_then(|v| v.as_f64()), Some(3.0));
    }

    #[test]
    fn tree_per_branch() {
        let mut branches = IndexMap::new();
        branches.insert("x".into(), Schema::float());
        branches.insert("flag".into(), Schema::bool());
        let s = Schema::Tree { branches };
        let u1 = Value::Map(IndexMap::from_iter([
            ("x".into(), Value::float(0.5)),
        ]));
        let u2 = Value::Map(IndexMap::from_iter([
            ("x".into(), Value::float(0.5)),
            ("flag".into(), Value::Bool(true)),
        ]));
        let r = reconcile(&s, &[u1, u2]).unwrap();
        let Value::Map(map) = &r else { panic!() };
        assert_eq!(map.get("x").and_then(|v| v.as_f64()), Some(1.0));
        assert_eq!(map.get("flag").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn empty_updates_returns_none() {
        let s = Schema::float();
        assert!(reconcile(&s, &[]).is_none());
    }

    #[test]
    fn tree_collates_structural_sentinels_and_branches() {
        // A Tree slot receiving, in one tick, an `_add` from one writer, a
        // `_remove` from another, and a per-branch value delta — all must
        // collate into ONE coherent update, not clobber each other.
        let s = Schema::Tree {
            branches: IndexMap::from([("x".into(), Schema::float())]),
        };
        let add = Value::Map(IndexMap::from_iter([(
            "_add".into(),
            Value::Map(IndexMap::from_iter([("k".into(), Value::float(1.0))])),
        )]));
        let remove = Value::Map(IndexMap::from_iter([(
            "_remove".into(),
            Value::List(vec![Value::String("old".into())]),
        )]));
        let bump = Value::Map(IndexMap::from_iter([("x".into(), Value::float(0.5))]));
        let bump2 = Value::Map(IndexMap::from_iter([("x".into(), Value::float(0.5))]));
        let r = reconcile(&s, &[add, remove, bump, bump2]).unwrap();
        let Value::Map(map) = &r else { panic!("expected Map, got {r:?}") };
        assert!(map.contains_key("_add"), "_add survived");
        assert!(map.contains_key("_remove"), "_remove survived");
        assert_eq!(map.get("x").and_then(|v| v.as_f64()), Some(1.0), "branch delta summed");
    }

    #[test]
    fn custom_delegates_to_representation() {
        // A `Custom` slot must reconcile by its REPRESENTATION — otherwise two
        // writers in one tick collapse to last-wins (a lost update).
        use crate::registry::TypeRegistry;
        let mut reg = TypeRegistry::new();
        reg.register("Counter", Schema::map(Schema::float()), None);
        let counter = Schema::Custom { name: "Counter".into(), parameters: Default::default() };
        let u1 = Value::Map(IndexMap::from_iter([("a".into(), Value::float(1.0))]));
        let u2 = Value::Map(IndexMap::from_iter([("a".into(), Value::float(2.0))]));

        // No registry → opaque last-wins (the old behavior; u1 is lost).
        let opaque = reconcile(&counter, &[u1.clone(), u2.clone()]).unwrap();
        assert_eq!(opaque.get_field("a").and_then(|v| v.as_f64()), Some(2.0), "no registry → last-wins");

        // With registry → delegates to Map(Float), summing both writers.
        let merged = reconcile_with(Some(&reg), &counter, &[u1, u2]).unwrap();
        assert_eq!(merged.get_field("a").and_then(|v| v.as_f64()), Some(3.0), "registry → summed");
    }

    #[test]
    fn custom_delegation_recurses_through_map() {
        // A Custom NESTED inside a Map also delegates (the registry threads all
        // the way down the recursion).
        use crate::registry::TypeRegistry;
        let mut reg = TypeRegistry::new();
        reg.register("Counter", Schema::map(Schema::float()), None);
        let outer = Schema::map(Schema::Custom { name: "Counter".into(), parameters: Default::default() });
        // Two updates to the same outer key `c`, each a Counter delta.
        let u1 = Value::Map(IndexMap::from_iter([(
            "c".into(),
            Value::Map(IndexMap::from_iter([("x".into(), Value::float(1.0))])),
        )]));
        let u2 = Value::Map(IndexMap::from_iter([(
            "c".into(),
            Value::Map(IndexMap::from_iter([("x".into(), Value::float(4.0))])),
        )]));
        let r = reconcile_with(Some(&reg), &outer, &[u1, u2]).unwrap();
        let cx = r.get_field("c").and_then(|c| c.get_field("x")).and_then(|v| v.as_f64());
        assert_eq!(cx, Some(5.0), "nested Counter summed via delegation");
    }

    #[test]
    fn map_carries_divide_sentinel() {
        // `_divide` is a structural directive that MUST survive reconcile —
        // otherwise division silently no-ops when batched with other updates.
        let s = Schema::map(Schema::float());
        let divide = Value::Map(IndexMap::from_iter([(
            "_divide".into(),
            Value::Map(IndexMap::from_iter([("mother".into(), Value::String("0".into()))])),
        )]));
        let bump = Value::Map(IndexMap::from_iter([("0".into(), Value::float(2.0))]));
        let r = reconcile(&s, &[bump, divide]).unwrap();
        let Value::Map(map) = &r else { panic!() };
        assert!(map.contains_key("_divide"), "_divide directive survives batching");
    }

    #[test]
    fn list_batches_structural_add_remove() {
        let s = Schema::List { element: Box::new(Schema::float()) };
        let add = Value::Map(IndexMap::from_iter([(
            "_add".into(),
            Value::List(vec![Value::float(1.0)]),
        )]));
        let remove = Value::Map(IndexMap::from_iter([(
            "_remove".into(),
            Value::List(vec![Value::String("0".into())]),
        )]));
        let r = reconcile(&s, &[add, remove]).unwrap();
        let Value::Map(map) = &r else { panic!("expected structural Map, got {r:?}") };
        assert!(map.contains_key("_add") && map.contains_key("_remove"), "both survive");
    }

    #[test]
    fn list_concatenates_plain() {
        let s = Schema::List { element: Box::new(Schema::float()) };
        let r = reconcile(&s, &[Value::List(vec![Value::float(1.0)]), Value::List(vec![Value::float(2.0)])])
            .unwrap();
        assert_eq!(r.as_list().map(<[_]>::len), Some(2), "plain lists concatenate");
    }

    #[test]
    fn array_sums_elementwise() {
        let s = Schema::Array { shape: vec![3], element: Box::new(Schema::float()) };
        let u1 = Value::List(vec![Value::float(1.0), Value::float(0.0), Value::float(2.0)]);
        let u2 = Value::List(vec![Value::float(0.5), Value::float(1.0), Value::float(0.0)]);
        let r = reconcile(&s, &[u1, u2]).unwrap();
        let Value::List(l) = &r else { panic!() };
        assert_eq!(l[0].as_f64(), Some(1.5));
        assert_eq!(l[1].as_f64(), Some(1.0));
        assert_eq!(l[2].as_f64(), Some(2.0));
    }

    #[test]
    fn maybe_filters_none_then_delegates_to_inner() {
        let s = Schema::maybe(Schema::float());
        // None is dropped; the inner Float sums the rest.
        let r = reconcile(&s, &[Value::None, Value::float(1.0), Value::float(2.0)]).unwrap();
        assert_eq!(r.as_f64(), Some(3.0));
    }
}
