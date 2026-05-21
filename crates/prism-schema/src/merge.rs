//! `merge` — combine two *values* (not a value and an update) under a sort.
//!
//! Where [`apply`](crate::algebra::apply) acts an *update* on a value, `merge`
//! folds two complete values together: per-key union for maps/trees,
//! concatenation for lists, "non-empty wins (update first)" for atoms,
//! replacement for `Overwrite`, preservation for `Const`. Used at composition
//! boundaries (e.g. bridging an external input value into inner state).
//!
//! Faithful port of `bigraph_schema/methods/merge.py` for prism's sorts.

use crate::schema::Schema;
use crate::value::{StateMap, Value};

/// Combine `current` and `update` into a single value of sort `schema`.
pub fn merge(schema: &Schema, current: &Value, update: &Value) -> Value {
    match schema {
        // Immutable: current is preserved.
        Schema::Const { .. } => current.clone(),

        // Overwrite: update wins unless it is absent.
        Schema::Overwrite { .. } => {
            if matches!(update, Value::None) { current.clone() } else { update.clone() }
        }

        // Optional: absent side yields; else merge through the inner sort.
        Schema::Maybe { inner } => match (current, update) {
            (_, Value::None) => current.clone(),
            (Value::None, _) => update.clone(),
            _ => merge(inner, current, update),
        },

        // Quote: opaque — last (update) wins unless absent.
        Schema::Quote { .. } => {
            if matches!(update, Value::None) { current.clone() } else { update.clone() }
        }

        // Stateless bigraph sorts carry nothing.
        Schema::Site { .. }
        | Schema::InnerName { .. }
        | Schema::OuterName { .. }
        | Schema::Interface { .. } => Value::None,

        // Atoms: non-empty update wins, else non-empty current.
        Schema::Float { .. }
        | Schema::Delta { .. }
        | Schema::Integer { .. }
        | Schema::Bool { .. }
        | Schema::String { .. }
        | Schema::Enum { .. } => non_empty_wins(current, update),

        // List: concatenation (absent side yields).
        Schema::List { .. } => match (current, update) {
            (Value::None, _) => update.clone(),
            (_, Value::None) => current.clone(),
            (Value::List(c), Value::List(u)) => {
                let mut out = c.clone();
                out.extend(u.iter().cloned());
                Value::List(out)
            }
            _ => update.clone(),
        },

        // Array: update wins unless absent (no element-wise merge — that is
        // apply's additive job; merge combines two complete arrays).
        Schema::Array { .. } => {
            if matches!(update, Value::None) { current.clone() } else { update.clone() }
        }

        // Tuple: element-wise merge.
        Schema::Tuple { elements } => match (current, update) {
            (Value::None, _) => update.clone(),
            (_, Value::None) => current.clone(),
            (Value::List(c), Value::List(u)) => Value::List(
                elements
                    .iter()
                    .enumerate()
                    .map(|(i, es)| {
                        merge(es, c.get(i).unwrap_or(&Value::None), u.get(i).unwrap_or(&Value::None))
                    })
                    .collect(),
            ),
            _ => update.clone(),
        },

        // Map: union of keys, per-key merge on the value sort.
        Schema::Map { value } => merge_keyed(current, update, |_, c, u| merge(value, c, u)),

        // Tree: union of keys, per-branch merge (unknown keys via Any).
        Schema::Tree { branches } => {
            merge_keyed(current, update, |k, c, u| merge(branches.get(k).unwrap_or(&Schema::Any), c, u))
        }

        // RecursiveTree: leaf-merge when both leaves, else per-key recurse.
        Schema::RecursiveTree { leaf } => {
            let is_leaf = |v: &Value| !v.is_map_like();
            if is_leaf(current) && is_leaf(update) {
                merge(leaf, current, update)
            } else {
                merge_keyed(current, update, |_, c, u| merge(schema, c, u))
            }
        }

        // Opaque / typed-node sorts: non-empty update wins.
        Schema::Any
        | Schema::Custom { .. }
        | Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. }
        | Schema::Bridge { .. } => non_empty_wins(current, update),
    }
}

fn non_empty_wins(current: &Value, update: &Value) -> Value {
    if !matches!(update, Value::None) {
        update.clone()
    } else {
        current.clone()
    }
}

/// Union two map-like values key-by-key; shared keys are merged via `merge_at`.
fn merge_keyed(
    current: &Value,
    update: &Value,
    mut merge_at: impl FnMut(&crate::value::Key, &Value, &Value) -> Value,
) -> Value {
    match (current.as_map(), update.as_map()) {
        (Some(c), Some(u)) => {
            let mut out: StateMap = c.clone();
            for (k, uv) in u {
                match c.get(k) {
                    Some(cv) => {
                        out.insert(k.clone(), merge_at(k, cv, uv));
                    }
                    None => {
                        out.insert(k.clone(), uv.clone());
                    }
                }
            }
            Value::Map(out)
        }
        (None, Some(_)) => update.clone(),
        (Some(_), None) => current.clone(),
        _ => non_empty_wins(current, update),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn atom_update_wins() {
        assert_eq!(merge(&Schema::float(), &Value::float(1.0), &Value::float(2.0)), Value::float(2.0));
        // Absent update → current preserved.
        assert_eq!(merge(&Schema::float(), &Value::float(1.0), &Value::None), Value::float(1.0));
    }

    #[test]
    fn list_concatenates() {
        let s = Schema::list(Schema::float());
        let a = Value::List(vec![Value::float(1.0)]);
        let b = Value::List(vec![Value::float(2.0)]);
        assert_eq!(merge(&s, &a, &b), Value::List(vec![Value::float(1.0), Value::float(2.0)]));
    }

    #[test]
    fn map_unions_and_merges_shared() {
        let s = Schema::map(Schema::float());
        let a = Value::Map(IndexMap::from([("x".into(), Value::float(1.0))]));
        let b = Value::Map(IndexMap::from([("x".into(), Value::float(9.0)), ("y".into(), Value::float(2.0))]));
        let Value::Map(m) = merge(&s, &a, &b) else { panic!() };
        assert_eq!(m.get("x").and_then(|v| v.as_f64()), Some(9.0)); // update wins on shared
        assert_eq!(m.get("y").and_then(|v| v.as_f64()), Some(2.0));
    }

    #[test]
    fn const_preserves_current() {
        let s = Schema::const_of(Schema::float());
        assert_eq!(merge(&s, &Value::float(1.0), &Value::float(2.0)), Value::float(1.0));
    }
}
