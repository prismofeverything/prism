//! Schema definitions for typed hierarchical state.
//!
//! Schemas describe the structure and types of the state tree.
//! They mirror bigraph-schema's type system but leverage Rust's
//! type system for compile-time safety where possible, with
//! runtime flexibility for dynamic composition.

use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::Value;

/// A schema describing the type of a value in the state tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "_type")]
pub enum Schema {
    /// No type constraint
    Any,

    /// Atomic types
    Bool {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
    },
    Integer {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
    },
    Float {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },
    String {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    /// A list with typed elements
    List {
        element: Box<Schema>,
    },

    /// A map with string keys and typed values
    Map {
        value: Box<Schema>,
    },

    /// A tree with named, individually-typed branches
    Tree {
        branches: IndexMap<String, Schema>,
    },

    /// Optional value
    Maybe {
        inner: Box<Schema>,
    },

    /// Enum with allowed values
    Enum {
        values: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    /// Delta type — updates are additive rather than replacing.
    /// (Note: Float and Integer are ALSO additive by default.
    ///  Delta is kept for backward compatibility.)
    Delta {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },

    /// Overwrite wrapper — updates REPLACE the current value.
    /// Use this for types where replacement semantics are needed
    /// (e.g., positions, absolute state). Wraps any inner schema.
    Overwrite {
        inner: Box<Schema>,
    },

    /// Multidimensional array — element-wise additive apply.
    /// Like Python bigraph-schema's `array` type. Updates are added
    /// element-wise when shapes match.
    Array {
        /// Shape dimensions (e.g., [10, 10] for a 10x10 grid)
        shape: Vec<usize>,
        /// Element type
        element: Box<Schema>,
    },

    /// Tuple — fixed-length, element-wise typed apply.
    /// Like Python bigraph-schema's `tuple` type. Each element
    /// is applied using its own type's semantics.
    Tuple {
        elements: Vec<Schema>,
    },
}

impl Schema {
    // ── Convenience constructors ──

    pub fn float() -> Self {
        Self::Float { default: None }
    }

    pub fn float_default(v: f64) -> Self {
        Self::Float { default: Some(v) }
    }

    pub fn integer() -> Self {
        Self::Integer { default: None }
    }

    pub fn bool() -> Self {
        Self::Bool { default: None }
    }

    pub fn string() -> Self {
        Self::String { default: None }
    }

    pub fn delta() -> Self {
        Self::Delta { default: Some(0.0) }
    }

    pub fn list(element: Schema) -> Self {
        Self::List {
            element: Box::new(element),
        }
    }

    pub fn map(value: Schema) -> Self {
        Self::Map {
            value: Box::new(value),
        }
    }

    pub fn overwrite(inner: Schema) -> Self {
        Self::Overwrite {
            inner: Box::new(inner),
        }
    }

    /// Overwrite float — for positions, absolute values.
    pub fn set_float() -> Self {
        Self::overwrite(Self::float())
    }

    /// Array type — element-wise additive.
    pub fn array(shape: Vec<usize>, element: Schema) -> Self {
        Self::Array { shape, element: Box::new(element) }
    }

    /// Tuple type — element-wise typed apply.
    pub fn tuple(elements: Vec<Schema>) -> Self {
        Self::Tuple { elements }
    }

    pub fn maybe(inner: Schema) -> Self {
        Self::Maybe {
            inner: Box::new(inner),
        }
    }

    pub fn tree(
        branches: impl IntoIterator<Item = (impl Into<String>, Schema)>,
    ) -> Self {
        Self::Tree {
            branches: branches
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
        }
    }

    /// Generate a default value for this schema.
    pub fn default_value(&self) -> Value {
        match self {
            Self::Any => Value::None,
            Self::Bool { default } => Value::Bool(default.unwrap_or(false)),
            Self::Integer { default } => Value::Int(default.unwrap_or(0)),
            Self::Float { default } => Value::float(default.unwrap_or(0.0)),
            Self::String { default } => {
                Value::String(default.clone().unwrap_or_default())
            }
            Self::List { .. } => Value::List(vec![]),
            Self::Map { .. } => Value::map(),
            Self::Tree { branches } => Value::tree(
                branches
                    .iter()
                    .map(|(k, s)| (k.clone(), s.default_value())),
            ),
            Self::Maybe { .. } => Value::None,
            Self::Enum { default, values } => {
                Value::String(default.clone().unwrap_or_else(|| {
                    values.first().cloned().unwrap_or_default()
                }))
            }
            Self::Delta { default } => Value::float(default.unwrap_or(0.0)),
            Self::Overwrite { inner } => inner.default_value(),
            Self::Array { shape, element } => {
                // Build nested list matching shape dimensions
                fn build_array(dims: &[usize], elem: &Schema) -> Value {
                    if dims.is_empty() {
                        return elem.default_value();
                    }
                    let inner = if dims.len() == 1 {
                        (0..dims[0]).map(|_| elem.default_value()).collect()
                    } else {
                        (0..dims[0]).map(|_| build_array(&dims[1..], elem)).collect()
                    };
                    Value::List(inner)
                }
                build_array(shape, element)
            }
            Self::Tuple { elements } => {
                Value::List(elements.iter().map(|s| s.default_value()).collect())
            }
        }
    }

    /// Check if a value conforms to this schema.
    pub fn check(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Any, _) => true,
            (Self::Bool { .. }, Value::Bool(_)) => true,
            (Self::Integer { .. }, Value::Int(_)) => true,
            (Self::Float { .. }, Value::Float(_)) => true,
            (Self::Float { .. }, Value::Int(_)) => true, // int is valid as float
            (Self::Delta { .. }, Value::Float(_)) => true,
            (Self::Delta { .. }, Value::Int(_)) => true,
            (Self::Overwrite { inner }, v) => inner.check(v),
            (Self::String { .. }, Value::String(_)) => true,
            (Self::Enum { values, .. }, Value::String(s)) => values.contains(s),
            (Self::Maybe { .. }, Value::None) => true,
            (Self::Maybe { inner }, v) => inner.check(v),
            (Self::List { element }, Value::List(items)) => {
                items.iter().all(|item| element.check(item))
            }
            (Self::Map { value: val_schema }, Value::Map(map)) => {
                map.values().all(|v| val_schema.check(v))
            }
            (Self::Tree { branches }, Value::Map(map)) => branches
                .iter()
                .all(|(k, s)| map.get(k).is_some_and(|v| s.check(v))),
            (Self::Array { element, .. }, Value::List(items)) => {
                items.iter().all(|item| match item {
                    Value::List(row) => row.iter().all(|v| element.check(v)),
                    _ => element.check(item),
                })
            }
            (Self::Tuple { elements }, Value::List(items)) => {
                elements.len() == items.len()
                    && elements.iter().zip(items.iter()).all(|(s, v)| s.check(v))
            }
            _ => false,
        }
    }

    /// Walk the schema tree to find the sub-schema at a given path.
    /// For example, path ["fields", "glucose"] in Tree{fields: Map(Array(Float))}
    /// returns Array(Float).
    pub fn schema_at_path(&self, path: &[String]) -> &Schema {
        if path.is_empty() {
            return self;
        }
        match self {
            Self::Tree { branches } => {
                if let Some(child) = branches.get(&path[0]) {
                    child.schema_at_path(&path[1..])
                } else {
                    &Schema::Any
                }
            }
            Self::Map { value } => value.schema_at_path(&path[1..]),
            Self::Array { element, .. } => element.schema_at_path(&path[1..]),
            _ => self, // Leaf schema applies to everything below
        }
    }

    /// Merge two schemas, preferring the more specific one.
    pub fn resolve(&self, other: &Schema) -> Schema {
        match (self, other) {
            (Self::Any, other) => other.clone(),
            (this, Self::Any) => this.clone(),
            (Self::Tree { branches: a }, Self::Tree { branches: b }) => {
                let mut merged = a.clone();
                for (k, v) in b {
                    merged
                        .entry(k.clone())
                        .and_modify(|existing| *existing = existing.resolve(v))
                        .or_insert_with(|| v.clone());
                }
                Self::Tree { branches: merged }
            }
            // Default: prefer `other` (the more recent definition)
            (_, other) => other.clone(),
        }
    }

    /// Apply an update to a current value using type-dispatched semantics.
    ///
    /// Default behavior by type:
    /// - `Float`, `Integer`, `Delta` → **additive** (current + update). Commutative.
    /// - `Overwrite` → **replacement** (returns update directly).
    /// - `Bool`, `String`, `Enum` → **replacement** (non-numeric, can't add).
    /// - `List` → **replacement** (lists replace wholesale).
    /// - `Map` → **recursive merge** (each key applies independently).
    /// - `Tree` → **recursive merge** with per-branch schemas.
    /// - `Any` → **inferred**: additive for numbers, merge for maps, replace otherwise.
    pub fn apply_update(&self, current: &Value, update: &Value) -> Value {
        match self {
            // Numeric types: additive (delta) by default
            Self::Float { .. } | Self::Delta { .. } => {
                let base = current.as_f64().unwrap_or(0.0);
                let delta = update.as_f64().unwrap_or(0.0);
                Value::float(base + delta)
            }
            Self::Integer { .. } => {
                let base = current.as_i64().unwrap_or(0);
                let delta = update.as_i64().unwrap_or(0);
                Value::Int(base + delta)
            }

            // Overwrite: always replace
            Self::Overwrite { .. } => update.clone(),

            // Non-numeric atoms: replace
            Self::Bool { .. } | Self::String { .. } | Self::Enum { .. } => {
                update.clone()
            }

            // Tree: recursive merge with per-branch schemas
            Self::Tree { branches } => {
                if let (Value::Map(cur), Value::Map(upd)) = (current, update) {
                    let mut result = cur.clone();
                    apply_add_remove(&mut result, upd);
                    for (k, v) in upd {
                        if k == "_add" || k == "_remove" {
                            continue;
                        }
                        let schema = branches.get(k).unwrap_or(&Schema::Any);
                        let existing = cur.get(k).unwrap_or(&Value::None);
                        result.insert(k.clone(), schema.apply_update(existing, v));
                    }
                    Value::Map(result)
                } else {
                    update.clone()
                }
            }

            // Map: recursive merge with _add/_remove support
            Self::Map { value: val_schema } => {
                if let (Value::Map(cur), Value::Map(upd)) = (current, update) {
                    let mut result = cur.clone();
                    apply_add_remove(&mut result, upd);
                    for (k, v) in upd {
                        if k == "_add" || k == "_remove" {
                            continue;
                        }
                        let existing = cur.get(k).unwrap_or(&Value::None);
                        result.insert(k.clone(), val_schema.apply_update(existing, v));
                    }
                    Value::Map(result)
                } else {
                    update.clone()
                }
            }

            // Lists and Maybe: replace
            Self::List { .. } | Self::Maybe { .. } => update.clone(),

            // Array: element-wise additive apply through all dimensions.
            // For array[ny|nx, float], recursively applies through nested lists
            // until reaching the leaf element type.
            Self::Array { shape, element } => {
                match (current, update) {
                    (Value::List(cur), Value::List(upd)) if cur.len() == upd.len() => {
                        // If there are remaining shape dimensions, recurse as sub-arrays
                        let sub_schema = if shape.len() > 1 {
                            Schema::Array {
                                shape: shape[1..].to_vec(),
                                element: element.clone(),
                            }
                        } else {
                            // Last dimension: use element type directly
                            *element.clone()
                        };
                        Value::List(
                            cur.iter().zip(upd.iter())
                                .map(|(c, u)| sub_schema.apply_update(c, u))
                                .collect()
                        )
                    }
                    _ => update.clone(),
                }
            }

            // Tuple: element-wise typed apply.
            Self::Tuple { elements } => {
                match (current, update) {
                    (Value::List(cur), Value::List(upd)) if cur.len() == upd.len() => {
                        Value::List(
                            cur.iter().zip(upd.iter()).enumerate()
                                .map(|(i, (c, u))| {
                                    let schema = elements.get(i).unwrap_or(&Schema::Any);
                                    schema.apply_update(c, u)
                                })
                                .collect()
                        )
                    }
                    _ => update.clone(),
                }
            }

            // Any: infer behavior from the value types
            Self::Any => {
                match (current, update) {
                    // Both numeric → additive
                    (Value::Float(_) | Value::Int(_), Value::Float(_) | Value::Int(_)) => {
                        let base = current.as_f64().unwrap_or(0.0);
                        let delta = update.as_f64().unwrap_or(0.0);
                        Value::float(base + delta)
                    }
                    // Both maps → recursive merge with _add/_remove
                    (Value::Map(cur), Value::Map(upd)) => {
                        let mut result = cur.clone();
                        apply_add_remove(&mut result, upd);
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" {
                                continue;
                            }
                            let existing = cur.get(k).unwrap_or(&Value::None);
                            result.insert(k.clone(), Schema::Any.apply_update(existing, v));
                        }
                        Value::Map(result)
                    }
                    // Both lists → replace (use Array schema for element-wise additive)
                    (Value::List(_), Value::List(_)) => update.clone(),
                    // Otherwise → replace
                    _ => update.clone(),
                }
            }
        }
    }
}

impl fmt::Display for Schema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "any"),
            Self::Bool { .. } => write!(f, "bool"),
            Self::Integer { .. } => write!(f, "integer"),
            Self::Float { .. } => write!(f, "float"),
            Self::String { .. } => write!(f, "string"),
            Self::Delta { .. } => write!(f, "delta"),
            Self::Overwrite { inner } => write!(f, "overwrite[{inner}]"),
            Self::List { element } => write!(f, "list[{element}]"),
            Self::Map { value } => write!(f, "map[{value}]"),
            Self::Maybe { inner } => write!(f, "maybe[{inner}]"),
            Self::Enum { values, .. } => write!(f, "enum[{}]", values.join(",")),
            Self::Tree { branches } => {
                write!(f, "tree{{")?;
                for (i, (k, v)) in branches.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Self::Array { shape, element } => {
                let dims: Vec<String> = shape.iter().map(|d| d.to_string()).collect();
                write!(f, "array[{}|{}]", dims.join("|"), element)
            }
            Self::Tuple { elements } => {
                write!(f, "tuple[")?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 { write!(f, ",")?; }
                    write!(f, "{e}")?;
                }
                write!(f, "]")
            }
        }
    }
}

/// Parse a simple schema string into a Schema.
/// Supports: "float", "integer", "bool", "string", "delta", "any"
/// For complex schemas, use the Schema constructors directly.
/// Process `_remove` and `_add` special keys in a map update.
///
/// - `_remove`: a list of keys to delete from the map.
/// - `_add`: a map of new entries to insert (absolute values, not deltas).
///
/// These are core process-bigraph operations for structural state changes
/// like particle division, boundary spawning, and process composition.
fn apply_add_remove(result: &mut crate::value::StateMap, update: &crate::value::StateMap) {
    // _remove: delete listed keys
    if let Some(Value::List(keys)) = update.get("_remove") {
        for key in keys {
            if let Some(k) = key.as_str() {
                result.swap_remove(k);
            }
        }
    }

    // _add: insert new entries (absolute values, not deltas)
    if let Some(Value::Map(adds)) = update.get("_add") {
        for (k, v) in adds {
            result.insert(k.clone(), v.clone());
        }
    }
}

pub fn parse_schema(s: &str) -> Schema {
    match s.trim() {
        "any" => Schema::Any,
        "bool" | "boolean" => Schema::bool(),
        "int" | "integer" => Schema::integer(),
        "float" => Schema::float(),
        "string" => Schema::string(),
        "delta" => Schema::delta(),
        "set_float" => Schema::set_float(),
        _ => Schema::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_value() {
        let schema = Schema::tree([
            ("mass", Schema::float_default(1.0)),
            ("alive", Schema::bool()),
        ]);

        let val = schema.default_value();
        let map = val.as_map().unwrap();
        assert_eq!(map["mass"].as_f64(), Some(1.0));
        assert_eq!(map["alive"].as_bool(), Some(false));
    }

    #[test]
    fn test_delta_apply() {
        let schema = Schema::delta();
        let current = Value::float(10.0);
        let update = Value::float(3.0);
        let result = schema.apply_update(&current, &update);
        assert_eq!(result.as_f64(), Some(13.0));
    }

    #[test]
    fn test_check() {
        let schema = Schema::tree([
            ("x", Schema::float()),
            ("name", Schema::string()),
        ]);
        let good = Value::tree([
            ("x", Value::float(1.0)),
            ("name", Value::from("test")),
        ]);
        assert!(schema.check(&good));

        let bad = Value::tree([
            ("x", Value::from("not a float")),
            ("name", Value::from("test")),
        ]);
        assert!(!schema.check(&bad));
    }
}
