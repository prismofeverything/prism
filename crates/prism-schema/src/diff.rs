//! `diff` — the minimal update that transforms `state_a` into `state_b`.
//!
//! The inverse of [`apply`](crate::algebra::apply):
//! `apply(s, a, diff(s, a, b)) ≡ b` (law #7). `None` means "no change"
//! (`a` already equals `b`), mirroring upstream's `None` return.
//!
//! Faithful port of `bigraph_schema/methods/diff.py`, mapping upstream sorts
//! to prism's: upstream `Number` → `Float`/`Integer`/`Delta` (the delta is
//! `b - a`, so the additive apply lands on `b`); `List` replaces; `Map` emits
//! per-key diffs plus `_add`/`_remove`; `dict`/`Node` → prism `Tree`
//! (per-branch); `Tree[leaf]` → prism `RecursiveTree`.

use indexmap::IndexMap;

use crate::registry::TypeRegistry;
use crate::schema::Schema;
use crate::value::{Key, StateMap, Value};

/// The update taking `a` to `b` under sort `schema`, or `None` if `a == b`.
pub fn diff(schema: &Schema, a: &Value, b: &Value) -> Option<Value> {
    diff_with(None, schema, a, b)
}

/// [`diff`] consulting a [`TypeRegistry`] so a `Schema::Custom` slot diffs by
/// its REPRESENTATION (a structural delta) instead of opaquely replacing — and
/// nested Custom slots delegate too. The registry-aware entry the engine /
/// composite bridge uses, so `apply_with(reg, a, diff_with(reg, a, b)) == b`
/// holds for Custom-typed slots (law #7).
pub fn diff_with(
    registry: Option<&TypeRegistry>,
    schema: &Schema,
    a: &Value,
    b: &Value,
) -> Option<Value> {
    match schema {
        // Immutable / stateless sorts never produce an update.
        Schema::Const { .. }
        | Schema::Site { .. }
        | Schema::InnerName { .. }
        | Schema::OuterName { .. }
        | Schema::Interface { .. } => None,

        // Wrappers: Overwrite replaces (the update is `b`); the others diff
        // through their inner sort.
        Schema::Overwrite { .. } => {
            if a == b { None } else { Some(b.clone()) }
        }
        Schema::Quote { .. } => {
            if a == b { None } else { Some(b.clone()) }
        }
        Schema::Maybe { inner } => match (a, b) {
            (Value::None, Value::None) => None,
            (Value::None, _) => Some(b.clone()),
            // Deletion: apply(Maybe, a, None) returns None — so the update *is*
            // `None`, which we must emit as `Some(None)` (a present "set to
            // absent"), not as "no change".
            (_, Value::None) => Some(Value::None),
            _ => diff_with(registry, inner, a, b),
        },

        // Numeric: the delta is `b - a` (additive apply lands on `b`).
        Schema::Float { .. } | Schema::Delta { .. } => {
            let (x, y) = (a.as_f64(), b.as_f64());
            match (x, y) {
                (Some(x), Some(y)) if x != y => Some(Value::float(y - x)),
                _ if a == b => None,
                _ => Some(b.clone()),
            }
        }
        Schema::Integer { .. } => match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) if x != y => Some(Value::Int(y - x)),
            _ if a == b => None,
            _ => Some(b.clone()),
        },

        // Replacing leaves.
        Schema::Bool { .. } | Schema::String { .. } | Schema::Enum { .. } | Schema::List { .. } => {
            if a == b { None } else { Some(b.clone()) }
        }

        // Array: element-wise delta (additive apply reconstructs `b`).
        Schema::Array { element, .. } => {
            if a == b {
                None
            } else {
                match (a, b) {
                    (Value::List(_), Value::List(_)) => Some(array_diff(registry, element, a, b)),
                    _ => Some(b.clone()),
                }
            }
        }

        // Tuple: per-position diff (None positions become no-op deltas).
        Schema::Tuple { elements } => {
            if a == b {
                return None;
            }
            let (Value::List(la), Value::List(lb)) = (a, b) else {
                return Some(b.clone());
            };
            let mut out: Vec<Value> = Vec::with_capacity(elements.len());
            let mut any = false;
            for (i, es) in elements.iter().enumerate() {
                let (ai, bi) = (la.get(i).unwrap_or(&Value::None), lb.get(i).unwrap_or(&Value::None));
                match diff_with(registry, es, ai, bi) {
                    Some(d) => {
                        any = true;
                        out.push(d);
                    }
                    None => out.push(identity_for(es, ai)),
                }
            }
            if any { Some(Value::List(out)) } else { None }
        }

        // Map: per-key diff over shared keys, `_add` for new keys, `_remove`
        // for dropped keys.
        Schema::Map { value } => diff_keyed(a, b, |_, av, bv| diff_with(registry, value, av, bv)),

        // Tree: per-branch diff using each branch's schema (unknown keys via Any).
        Schema::Tree { branches } => diff_keyed(a, b, |k, av, bv| {
            diff_with(registry, branches.get(k).unwrap_or(&Schema::Any), av, bv)
        }),

        // RecursiveTree: leaf when both sides are leaves, else recurse per key.
        Schema::RecursiveTree { leaf } => {
            let is_leaf = |v: &Value| !v.is_map_like();
            if is_leaf(a) && is_leaf(b) {
                return diff_with(registry, leaf, a, b);
            }
            diff_keyed(a, b, |_, av, bv| diff_with(registry, schema, av, bv))
        }

        // Custom: diff by the type's REPRESENTATION (a structural delta), so a
        // bridged Custom slot sends the delta — not a wholesale replace. No
        // registry/representation → replace if changed.
        Schema::Custom { name, .. } => match registry.and_then(|r| r.schema(name)) {
            Some(repr) => diff_with(registry, repr, a, b),
            None => {
                if a == b {
                    None
                } else {
                    Some(b.clone())
                }
            }
        },

        // Link-kind NODE: per-key diff over `node_data_branches` (additive
        // deltas for the self-exported face) with spec keys flowing through
        // `Any` — matching `apply`/`reconcile`/`divide`'s routing so the law
        // `apply(a, diff(a,b)) ≡ b` holds for nodes that carry a data face.
        Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. } => {
            let branches = schema.node_data_branches();
            diff_keyed(a, b, |k, av, bv| {
                diff_with(registry, branches.get(k).unwrap_or(&Schema::Any), av, bv)
            })
        }

        // Opaque sorts: replace if changed.
        Schema::Any | Schema::Bridge { .. } => {
            if a == b { None } else { Some(b.clone()) }
        }
    }
}

/// Element-wise diff of two nested numeric lists of identical shape. An
/// unchanged cell yields the **additive identity** (0), not its value — apply
/// is element-wise additive, so 0 leaves the cell unchanged (returning `b`
/// would double it).
fn array_diff(registry: Option<&TypeRegistry>, element: &Schema, a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::List(la), Value::List(lb)) if la.len() == lb.len() => Value::List(
            la.iter().zip(lb.iter()).map(|(x, y)| array_diff(registry, element, x, y)).collect(),
        ),
        _ => diff_with(registry, element, a, b).unwrap_or_else(|| match element {
            Schema::Integer { .. } => Value::Int(0),
            _ => Value::float(0.0),
        }),
    }
}

/// The no-op update for `schema` at value `current` (used to fill un-changed
/// tuple positions so the positional update stays well-shaped).
fn identity_for(schema: &Schema, current: &Value) -> Value {
    match schema {
        Schema::Float { .. } | Schema::Delta { .. } => Value::float(0.0),
        Schema::Integer { .. } => Value::Int(0),
        _ => current.clone(),
    }
}

/// Shared map/tree diff: per-key diff over shared keys, `_add` for keys only in
/// `b`, `_remove` for keys only in `a`.
fn diff_keyed(
    a: &Value,
    b: &Value,
    mut diff_at: impl FnMut(&Key, &Value, &Value) -> Option<Value>,
) -> Option<Value> {
    // `to_map` normalizes both `Map` and the compiled `Struct` representation,
    // so diffing a struct-backed slot produces a per-key delta instead of a
    // wholesale replace.
    let (Some(am), Some(bm)) = (a.to_map(), b.to_map()) else {
        return if a == b { None } else { Some(b.clone()) };
    };

    let mut result: StateMap = IndexMap::new();
    let mut adds: StateMap = IndexMap::new();
    for (k, bv) in &bm {
        match am.get(k) {
            Some(av) => {
                if let Some(d) = diff_at(k, av, bv) {
                    result.insert(k.clone(), d);
                }
            }
            None => {
                adds.insert(k.clone(), bv.clone());
            }
        }
    }
    let removed: Vec<Value> =
        am.keys().filter(|k| !bm.contains_key(*k)).map(|k| Value::String(k.to_string())).collect();

    if !adds.is_empty() {
        result.insert(Key::from("_add"), Value::Map(adds));
    }
    if !removed.is_empty() {
        result.insert(Key::from("_remove"), Value::List(removed));
    }
    if result.is_empty() { None } else { Some(Value::Map(result)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra;

    fn round_trips(schema: &Schema, a: &Value, b: &Value) {
        match diff(schema, a, b) {
            Some(u) => assert_eq!(&algebra::apply_with(None, schema, a, &u), b, "apply(a, diff(a,b)) != b"),
            None => assert_eq!(a, b, "diff returned None but a != b"),
        }
    }

    #[test]
    fn numeric_delta() {
        round_trips(&Schema::float(), &Value::float(3.0), &Value::float(8.0));
        round_trips(&Schema::integer(), &Value::Int(5), &Value::Int(2));
        round_trips(&Schema::delta(), &Value::float(1.0), &Value::float(1.0));
    }

    #[test]
    fn map_add_remove_change() {
        let s = Schema::map(Schema::float());
        let a = Value::Map(IndexMap::from([("x".into(), Value::float(1.0)), ("drop".into(), Value::float(9.0))]));
        let b = Value::Map(IndexMap::from([("x".into(), Value::float(4.0)), ("new".into(), Value::float(2.0))]));
        round_trips(&s, &a, &b);
    }

    #[test]
    fn tree_per_branch() {
        let s = Schema::tree([("m", Schema::float()), ("flag", Schema::bool())]);
        let a = Value::tree([("m", Value::float(1.0)), ("flag", Value::Bool(false))]);
        let b = Value::tree([("m", Value::float(3.0)), ("flag", Value::Bool(true))]);
        round_trips(&s, &a, &b);
    }

    #[test]
    fn array_elementwise() {
        let s = Schema::Array { shape: vec![2, 2], element: Box::new(Schema::float()) };
        let a = Value::List(vec![
            Value::List(vec![Value::float(1.0), Value::float(2.0)]),
            Value::List(vec![Value::float(3.0), Value::float(4.0)]),
        ]);
        let b = Value::List(vec![
            Value::List(vec![Value::float(1.5), Value::float(2.0)]),
            Value::List(vec![Value::float(0.0), Value::float(4.0)]),
        ]);
        round_trips(&s, &a, &b);
    }

    #[test]
    fn maybe_deletion() {
        let s = Schema::maybe(Schema::float());
        round_trips(&s, &Value::float(5.0), &Value::None);
        round_trips(&s, &Value::None, &Value::float(5.0));
        round_trips(&s, &Value::None, &Value::None);
    }

    #[test]
    fn custom_diffs_by_representation() {
        // A bridged `Custom` slot must diff by its REPRESENTATION (a structural
        // delta) — otherwise the bridge sends the whole value (a replace), which
        // for additive representations double-counts or clobbers.
        use crate::registry::TypeRegistry;
        let mut reg = TypeRegistry::new();
        reg.register("Counter", Schema::map(Schema::float()), None);
        let counter = Schema::Custom { name: "Counter".into(), parameters: Default::default() };
        let a = Value::Map(IndexMap::from_iter([("x".into(), Value::float(1.0))]));
        let b = Value::Map(IndexMap::from_iter([("x".into(), Value::float(4.0))]));

        // No registry → opaque replace (sends the whole `b`).
        let opaque = diff(&counter, &a, &b).unwrap();
        assert_eq!(opaque.get_field("x").and_then(|v| v.as_f64()), Some(4.0), "no registry → replace");

        // With registry → a structural delta (+3).
        let d = diff_with(Some(&reg), &counter, &a, &b).unwrap();
        assert_eq!(d.get_field("x").and_then(|v| v.as_f64()), Some(3.0), "registry → delta (+3)");

        // The inverse law holds through the registry:
        // apply_with(reg, a, diff_with(reg, a, b)) == b.
        let back = crate::algebra::apply_with(Some(&reg), &counter, &a, &d);
        assert_eq!(back, b, "apply_with(a, diff_with(a, b)) == b for a Custom slot");
    }
}
