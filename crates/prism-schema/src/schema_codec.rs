//! Lossless `Schema` ↔ `Value` codec.
//!
//! A declared schema (with apply-critical types inference can't recover —
//! `Array`, `Delta`, `StepLink`, …) must sometimes travel *inside* a `Value`:
//! e.g. a composite spec's `config` carries its inner schema so the nested
//! subengine is **informed by the declared types** rather than re-inferring
//! them (the bare-`infer` floor loses `Array` → `List`, breaking additive
//! fields through the bridge). Both `Schema` and `Value` are serde types, so we
//! transcode through their canonical serde form (`Schema` is tagged `_type`;
//! the resulting `Value` is a `Map` tree with a `_type` key per node).

use crate::{Schema, Value};

/// The canonical `Value` representation of a `Schema` (inverse:
/// [`value_to_schema`]). Used to embed a declared schema in a config `Value`.
pub fn schema_to_value(schema: &Schema) -> Value {
    serde_json::to_value(schema)
        .ok()
        .and_then(|j| serde_json::from_value::<Value>(j).ok())
        .unwrap_or(Value::None)
}

/// Reconstruct a `Schema` from its `Value` representation
/// (inverse of [`schema_to_value`]); `None` if `value` isn't a schema encoding.
pub fn value_to_schema(value: &Value) -> Option<Schema> {
    serde_json::to_value(value)
        .ok()
        .and_then(|j| serde_json::from_value::<Schema>(j).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn roundtrips(s: Schema) {
        let v = schema_to_value(&s);
        let back = value_to_schema(&v).expect("value_to_schema");
        assert_eq!(back, s, "schema round-trip via Value\n value={v:?}");
    }

    fn array_3x3() -> Schema {
        Schema::Array { shape: vec![3, 3], element: Box::new(Schema::float()) }
    }

    #[test]
    fn codec_round_trips_apply_critical_schemas() {
        roundtrips(Schema::float());
        roundtrips(Schema::Any);
        roundtrips(Schema::List { element: Box::new(Schema::float()) });
        roundtrips(array_3x3());
        roundtrips(Schema::map(array_3x3()));
        roundtrips(Schema::Tree {
            branches: IndexMap::from([("fields".into(), Schema::map(array_3x3()))]),
        });
        roundtrips(Schema::step_link(
            IndexMap::from([("snapshots".into(), Schema::Any)]),
            IndexMap::from([("report".into(), Schema::Any)]),
        ));
    }

    #[test]
    fn codec_round_trips_the_type_vocabulary() {
        // The schema sorts a user `type Name = …` can carry — the Axis-A
        // definition-as-data path (`homoiconic-unification.md`): chrysalis's
        // `EntityDef` serializes a `type`'s representation through THIS codec, so
        // a typed definition survives `quote ↔ reify`. The serde transcode is
        // total over `Schema` by construction; this makes the guarantee
        // EXECUTABLE for the vocabulary `type` actually emits (incl. `Custom`, the
        // common named-type case, and the algebraic + link sorts).
        roundtrips(Schema::bool());
        roundtrips(Schema::integer());
        roundtrips(Schema::string());
        roundtrips(Schema::delta());
        roundtrips(Schema::maybe(Schema::float()));
        roundtrips(Schema::overwrite(Schema::float()));
        roundtrips(Schema::tuple(vec![Schema::float(), Schema::string()]));
        roundtrips(Schema::recursive_tree(Schema::float()));
        roundtrips(Schema::Enum {
            values: vec!["a".into(), "b".into()],
            default: Some("a".into()),
        });
        roundtrips(Schema::Const { inner: Box::new(Schema::float()) });
        roundtrips(Schema::Quote { inner: Box::new(Schema::float()) });
        roundtrips(Schema::Custom { name: "Counter".into(), parameters: Default::default() });
        // A `type` can name a node kind (composite/process/link).
        roundtrips(Schema::link(IndexMap::new(), IndexMap::new()));
        roundtrips(Schema::process_link(IndexMap::new(), IndexMap::new()));
        roundtrips(Schema::composite_link(IndexMap::new(), IndexMap::new(), Schema::Any));
    }
}
