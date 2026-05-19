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

use crate::schema::Schema;
use crate::value::{Key, StateMap, Value};

/// Combine a batch of updates targeting one path into a single
/// reconciled update.
///
/// Returns `None` when the batch's net effect is a no-op (e.g. zero
/// numeric delta, all-empty structural sentinels).
pub fn reconcile(schema: &Schema, updates: &[Value]) -> Option<Value> {
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
            reconcile(inner, &filtered)
        }

        // Map: group updates by key, recurse per key. Carve out
        // structural sentinels (`_add`/`_remove`) — they're handled
        // holistically at this level.
        Schema::Map { value } => reconcile_map(value, updates),

        // Tree: per-branch reconcile using each branch's schema. Mixed
        // batches (some dict-shaped, some scalar) resolve to
        // last-non-dict-wins, matching the apply path.
        Schema::Tree { branches } => reconcile_tree(branches, updates),

        // List: structural _add/_remove batching.
        Schema::List { .. } => reconcile_list(updates),

        // Tuple: element-wise reconcile per position.
        Schema::Tuple { elements } => reconcile_tuple(elements, updates),

        // Array: simplified — sum element-wise when shapes match, else
        // last-wins. (Upstream's full sparse-dict / sparse-list /
        // ndarray merging is deferred.)
        Schema::Array { .. } => last_non_none(updates),

        // RecursiveTree: infer mode from update shapes. All non-dict →
        // leaf reconcile; any dict → tree-node reconcile.
        Schema::RecursiveTree { leaf } => reconcile_recursive_tree(leaf, updates),

        // Link, Custom, Any, typed link variants, Bridge:
        // opaque — last non-None wins.
        Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. }
        | Schema::Bridge { .. }
        | Schema::Custom { .. }
        | Schema::Any => last_non_none(updates),
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

fn reconcile_map(value_schema: &Schema, updates: &[Value]) -> Option<Value> {
    let mut adds: StateMap = IndexMap::new();
    let mut removes: Vec<Key> = Vec::new();
    let mut remove_all = false;
    let mut grouped: IndexMap<Key, Vec<Value>> = IndexMap::new();

    for update in updates {
        let Value::Map(map) = update else {
            // Non-map update at a Map slot is unusual — skip it.
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
                _ => {
                    grouped.entry(k.clone()).or_default().push(v.clone());
                }
            }
        }
    }

    // Recurse per key.
    let mut value_updates: StateMap = IndexMap::new();
    for (key, sub_updates) in grouped {
        let reconciled = if sub_updates.len() == 1 {
            Some(sub_updates[0].clone())
        } else {
            reconcile(value_schema, &sub_updates)
        };
        if let Some(v) = reconciled {
            value_updates.insert(key, v);
        }
    }

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
    for (k, v) in value_updates {
        result.insert(k, v);
    }
    if result.is_empty() {
        None
    } else {
        Some(Value::Map(result))
    }
}

fn reconcile_tree(branches: &IndexMap<Key, Schema>, updates: &[Value]) -> Option<Value> {
    // Filter non-None.
    let non_none: Vec<&Value> = updates.iter().filter(|u| !matches!(u, Value::None)).collect();
    if non_none.is_empty() {
        return None;
    }
    let any_map = non_none.iter().any(|u| matches!(u, Value::Map(_)));
    let any_non_map = non_none.iter().any(|u| !matches!(u, Value::Map(_)));

    if !any_map {
        // All non-map — leaf-mode; let the last non-None win (no per-branch
        // schema applies to a leaf at this position).
        return last_non_none(updates);
    }

    if any_non_map {
        // Mixed: last non-map overrides.
        for u in updates.iter().rev() {
            if !matches!(u, Value::None | Value::Map(_)) {
                return Some(u.clone());
            }
        }
    }

    // Tree-node mode: per-branch reconcile.
    let mut grouped: IndexMap<Key, Vec<Value>> = IndexMap::new();
    for u in updates {
        if let Value::Map(map) = u {
            for (k, v) in map {
                grouped.entry(k.clone()).or_default().push(v.clone());
            }
        }
    }
    let mut result: StateMap = IndexMap::new();
    for (key, sub_updates) in grouped {
        let branch_schema = branches.get(&key).unwrap_or(&Schema::Any);
        let reconciled = if sub_updates.len() == 1 {
            Some(sub_updates[0].clone())
        } else {
            reconcile(branch_schema, &sub_updates)
        };
        if let Some(v) = reconciled {
            result.insert(key, v);
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(Value::Map(result))
    }
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

fn reconcile_tuple(elements: &[Schema], updates: &[Value]) -> Option<Value> {
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
            reconcile(&elements[i], &contributions).unwrap_or(Value::None)
        };
        has_any = true;
    }

    if has_any {
        Some(Value::List(result))
    } else {
        None
    }
}

fn reconcile_recursive_tree(leaf: &Schema, updates: &[Value]) -> Option<Value> {
    let non_none: Vec<&Value> = updates.iter().filter(|u| !matches!(u, Value::None)).collect();
    if non_none.is_empty() {
        return None;
    }
    let any_map = non_none.iter().any(|u| matches!(u, Value::Map(_)));
    let any_non_map = non_none.iter().any(|u| !matches!(u, Value::Map(_)));

    if !any_map {
        return reconcile(leaf, updates);
    }
    if any_non_map {
        // Mixed — non-map wins.
        for u in updates.iter().rev() {
            if !matches!(u, Value::None | Value::Map(_)) {
                return Some(u.clone());
            }
        }
    }

    // Tree-node mode: collect per-key and recurse using the same recursive
    // tree schema (children may be leaves or nested trees).
    let mut grouped: IndexMap<Key, Vec<Value>> = IndexMap::new();
    for u in updates {
        if let Value::Map(map) = u {
            for (k, v) in map {
                grouped.entry(k.clone()).or_default().push(v.clone());
            }
        }
    }
    let recursive_schema = Schema::RecursiveTree { leaf: Box::new(leaf.clone()) };
    let mut result: StateMap = IndexMap::new();
    for (key, sub_updates) in grouped {
        let reconciled = if sub_updates.len() == 1 {
            Some(sub_updates[0].clone())
        } else {
            reconcile(&recursive_schema, &sub_updates)
        };
        if let Some(v) = reconciled {
            result.insert(key, v);
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(Value::Map(result))
    }
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
}
