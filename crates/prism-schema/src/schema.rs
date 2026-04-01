//! Schema definitions for typed hierarchical state.
//!
//! Schemas describe the structure and types of the state tree.
//! They mirror bigraph-schema's type system but leverage Rust's
//! type system for compile-time safety where possible, with
//! runtime flexibility for dynamic composition.

use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::{Key, StateMap, Value};

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
        branches: IndexMap<Key, Schema>,
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

    /// Recursive tree — a nested dict where every leaf matches the
    /// leaf schema. Like Python bigraph-schema's `tree[float]`.
    /// Values can be either the leaf type or another nested map.
    RecursiveTree {
        leaf: Box<Schema>,
    },

    /// A link (edge) in the bigraph — represents a process or step.
    /// This is the schema-level declaration that a node is computational,
    /// not just data. The engine uses this to identify and instantiate
    /// processes without scanning state for "address" fields.
    ///
    /// Corresponds to Python bigraph-schema's `Link` type and
    /// process-bigraph's `ProcessLink`/`StepLink`.
    Link {
        /// Schema for input ports: port_name → type
        inputs: IndexMap<Key, Schema>,
        /// Schema for output ports: port_name → type
        outputs: IndexMap<Key, Schema>,
        /// Whether this link has a temporal interval (process) or not (step).
        /// None means unspecified (inferred at instantiation time).
        temporal: Option<bool>,
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

    /// Create a link schema (process or step).
    pub fn link(inputs: IndexMap<Key, Schema>, outputs: IndexMap<Key, Schema>) -> Self {
        Self::Link { inputs, outputs, temporal: None }
    }

    /// Create a process link (temporal — has interval).
    pub fn process(inputs: IndexMap<Key, Schema>, outputs: IndexMap<Key, Schema>) -> Self {
        Self::Link { inputs, outputs, temporal: Some(true) }
    }

    /// Create a step link (non-temporal — fires on state change).
    pub fn step(inputs: IndexMap<Key, Schema>, outputs: IndexMap<Key, Schema>) -> Self {
        Self::Link { inputs, outputs, temporal: Some(false) }
    }

    pub fn recursive_tree(leaf: Schema) -> Self {
        Self::RecursiveTree { leaf: Box::new(leaf) }
    }

    pub fn maybe(inner: Schema) -> Self {
        Self::Maybe {
            inner: Box::new(inner),
        }
    }

    pub fn tree(
        branches: impl IntoIterator<Item = (impl Into<Key>, Schema)>,
    ) -> Self {
        Self::Tree {
            branches: branches
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
        }
    }

    /// Compile a Tree schema into a StructLayout for O(1) field access.
    /// Returns None for non-Tree schemas.
    pub fn compile_layout(&self) -> Option<std::sync::Arc<crate::value::StructLayout>> {
        match self {
            Self::Tree { branches } => {
                let fields: Vec<Key> = branches.keys().cloned().collect();
                Some(crate::value::StructLayout::new(fields))
            }
            _ => None,
        }
    }

    /// Recursively compile a value tree into Struct values wherever
    /// the schema declares a Tree with known branches AND no child
    /// uses Map/dynamic semantics. Container-level trees (with particles,
    /// fields, etc.) stay as Maps since processes iterate their keys.
    pub fn compile_value(&self, value: &Value) -> Value {
        match (self, value) {
            (Self::Tree { branches }, Value::Map(map)) => {
                // Only compile if no branch has Map/RecursiveTree schema
                // (those need dynamic key iteration which Struct doesn't support)
                let has_dynamic = branches.values().any(|s| matches!(s,
                    Schema::Map { .. } | Schema::RecursiveTree { .. }
                ));
                if has_dynamic {
                    // Keep as Map but recursively compile children
                    let compiled: StateMap = map.iter()
                        .map(|(k, v)| {
                            let child_schema = branches.get(k).unwrap_or(&Schema::Any);
                            (k.clone(), child_schema.compile_value(v))
                        })
                        .collect();
                    Value::Map(compiled)
                } else {
                    // Safe to compile to Struct — all children are fixed-structure
                    let layout = self.compile_layout().unwrap();
                    let values: Vec<Value> = layout.fields.iter()
                        .map(|k| {
                            let child_schema = branches.get(k).unwrap_or(&Schema::Any);
                            let child_val = map.get(k).unwrap_or(&Value::None);
                            child_schema.compile_value(child_val)
                        })
                        .collect();
                    Value::Struct { layout, values }
                }
            }
            _ => value.clone(),
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
            Self::RecursiveTree { .. } => Value::map(),
            Self::Link { inputs, .. } => {
                // Default link state: address, default wiring (port→[port])
                let mut state = IndexMap::new();
                state.insert(Key::from("address"), Value::String("local:edge".into()));
                let default_inputs: IndexMap<Key, Value> = inputs.keys()
                    .map(|k| (k.clone(), Value::List(vec![Value::String(k.to_string())])))
                    .collect();
                state.insert(Key::from("inputs"), Value::Map(default_inputs.clone()));
                state.insert(Key::from("outputs"), Value::Map(default_inputs));
                Value::Map(state)
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
            (Self::Link { .. }, Value::Map(map)) => {
                // A realized link must have an "instance" key (after instantiation)
                // An unrealized link has "address" + "inputs" + "outputs"
                map.contains_key("instance") || map.contains_key("address")
            }
            (Self::RecursiveTree { leaf }, v) => {
                // A recursive tree value is either a leaf or a map of recursive trees
                if leaf.check(v) {
                    true
                } else if let Value::Map(map) = v {
                    map.values().all(|child| Self::RecursiveTree { leaf: leaf.clone() }.check(child))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Walk the schema tree to find the sub-schema at a given path.
    /// For example, path ["fields", "glucose"] in Tree{fields: Map(Array(Float))}
    /// returns Array(Float).
    pub fn schema_at_path(&self, path: &[Key]) -> &Schema {
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
            Self::RecursiveTree { .. } => self,
            Self::Link { inputs, outputs, .. } => {
                // Navigate into link's port schemas
                match path[0].as_str() {
                    "inputs" => {
                        if path.len() > 1 {
                            inputs.get(&path[1]).unwrap_or(&Schema::Any)
                                .schema_at_path(&path[2..])
                        } else {
                            &Schema::Any
                        }
                    }
                    "outputs" => {
                        if path.len() > 1 {
                            outputs.get(&path[1]).unwrap_or(&Schema::Any)
                                .schema_at_path(&path[2..])
                        } else {
                            &Schema::Any
                        }
                    }
                    _ => &Schema::Any,
                }
            }
            _ => self, // Leaf schema applies to everything below
        }
    }

    /// Infer a schema from a value, including `_type` annotations.
    ///
    /// This is the Rust equivalent of Python bigraph-schema's `infer` +
    /// `realize` for state with embedded type declarations. When a map
    /// contains `_type`, it's used to determine the schema for that node.
    pub fn infer(value: &Value) -> Schema {
        match value {
            Value::Float(_) => Schema::float(),
            Value::Int(_) => Schema::integer(),
            Value::Bool(_) => Schema::bool(),
            Value::String(s) => {
                // Try to parse as a number → infer float
                if s.parse::<f64>().is_ok() {
                    Schema::float()
                } else {
                    Schema::string()
                }
            }
            Value::List(_) => Schema::List { element: Box::new(Schema::Any) },
            Value::Map(map) => {
                // Check for _type annotation
                if let Some(Value::String(type_str)) = map.get("_type") {
                    let base = crate::type_parser::parse_type_expression(type_str);
                    // For Link types without port info, infer ports from
                    // _inputs/_outputs in the state (if present)
                    if matches!(base, Schema::Link { .. }) {
                        if let Schema::Link { inputs, outputs, temporal } = &base {
                            if inputs.is_empty() && outputs.is_empty() {
                                let inferred_inputs = map.get("_inputs")
                                    .and_then(|v| v.as_map())
                                    .map(|m| m.iter()
                                        .map(|(k, v)| (k.clone(), crate::type_parser::parse_type_expression(
                                            v.as_str().unwrap_or("any"))))
                                        .collect())
                                    .unwrap_or_default();
                                let inferred_outputs = map.get("_outputs")
                                    .and_then(|v| v.as_map())
                                    .map(|m| m.iter()
                                        .map(|(k, v)| (k.clone(), crate::type_parser::parse_type_expression(
                                            v.as_str().unwrap_or("any"))))
                                        .collect())
                                    .unwrap_or_default();
                                return Schema::Link {
                                    inputs: inferred_inputs,
                                    outputs: inferred_outputs,
                                    temporal: *temporal,
                                };
                            }
                        }
                    }
                    return base;
                }
                // Infer as Tree with branches
                let branches: IndexMap<Key, Schema> = map.iter()
                    .filter(|(k, _)| !k.starts_with('_'))
                    .map(|(k, v)| (k.clone(), Schema::infer(v)))
                    .collect();
                if branches.is_empty() {
                    Schema::Any
                } else {
                    Schema::Tree { branches }
                }
            }
            Value::None => Schema::Any,
            _ => Schema::Any,
        }
    }

    /// Infer schema from state and merge with an existing schema.
    /// State values with `_type` annotations override the existing schema.
    /// State values without annotations use their inferred types.
    /// The existing schema provides defaults for keys not in state.
    pub fn infer_and_merge(schema: &Schema, state: &Value) -> Schema {
        match (schema, state) {
            (_, Value::Map(map)) if map.contains_key("_type") => {
                // _type annotation overrides schema
                Schema::infer(state)
            }
            (Self::Tree { branches }, Value::Map(map)) => {
                let mut merged = branches.clone();
                for (k, v) in map {
                    if k.starts_with('_') { continue; }
                    let existing = branches.get(k).unwrap_or(&Schema::Any);
                    merged.insert(k.clone(), Schema::infer_and_merge(existing, v));
                }
                Schema::Tree { branches: merged }
            }
            (Self::Any, _) => Schema::infer(state),
            _ => schema.clone(),
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

            // Tree: recursive merge with per-branch schemas.
            // Handles both Map and Struct current values.
            Self::Tree { branches } => {
                match (current, update) {
                    // Struct current + Map update (common: process delta applied to compiled state)
                    (Value::Struct { layout, values }, Value::Map(upd)) => {
                        let mut new_values = values.clone();
                        // Note: _add/_remove not supported on Struct (fixed fields)
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            if let Some(idx) = layout.index_of(k) {
                                let schema = branches.get(k).unwrap_or(&Schema::Any);
                                let existing = &values[idx];
                                new_values[idx] = schema.apply_update(existing, v);
                            }
                        }
                        Value::Struct { layout: layout.clone(), values: new_values }
                    }
                    // Map current + Map update (original path)
                    (Value::Map(cur), Value::Map(upd)) => {
                        let mut result = cur.clone();
                        apply_add_remove(&mut result, upd);
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            let schema = branches.get(k).unwrap_or(&Schema::Any);
                            let existing = cur.get(k).unwrap_or(&Value::None);
                            result.insert(k.clone(), schema.apply_update(existing, v));
                        }
                        Value::Map(result)
                    }
                    _ => update.clone(),
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

            // Lists: replace
            Self::List { .. } => update.clone(),

            // Maybe: delegate to inner when both non-None, otherwise replace
            Self::Maybe { inner } => {
                match (current, update) {
                    (Value::None, _) => update.clone(),
                    (_, Value::None) => Value::None,
                    _ => inner.apply_update(current, update),
                }
            }

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

            // Link: not a data type — replace entirely if updated
            Self::Link { .. } => update.clone(),

            // RecursiveTree: merge like Map with leaf-type apply
            Self::RecursiveTree { leaf } => {
                match (current, update) {
                    (Value::Map(cur), Value::Map(upd)) => {
                        let mut result = cur.clone();
                        apply_add_remove(&mut result, upd);
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            let existing = cur.get(k).unwrap_or(&Value::None);
                            match (existing, v) {
                                // Both maps: recurse as tree
                                (Value::Map(_), Value::Map(_)) => {
                                    result.insert(k.clone(), self.apply_update(existing, v));
                                }
                                // Both leaves: apply leaf semantics
                                (_, _) if existing.as_map().is_none() && v.as_map().is_none() => {
                                    result.insert(k.clone(), leaf.apply_update(existing, v));
                                }
                                // Type mismatch (map vs leaf): update replaces
                                _ => {
                                    result.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        Value::Map(result)
                    }
                    _ => leaf.apply_update(current, update),
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
                    // Struct current + Map update → update fields in place
                    (Value::Struct { layout, values }, Value::Map(upd)) => {
                        let mut new_values = values.clone();
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            if let Some(idx) = layout.index_of(k) {
                                let existing = &values[idx];
                                new_values[idx] = Schema::Any.apply_update(existing, v);
                            }
                        }
                        Value::Struct { layout: layout.clone(), values: new_values }
                    }
                    // Both lists → replace (use Array schema for element-wise additive)
                    (Value::List(_), Value::List(_)) => update.clone(),
                    // Otherwise → replace
                    _ => update.clone(),
                }
            }
        }
    }

    /// Serialize a typed value to a JSON-compatible representation.
    ///
    /// This is the inverse of `realize`. Numbers, strings, and bools
    /// pass through. Maps and trees recurse. Links encode their
    /// address and port schemas.
    pub fn encode(&self, value: &Value) -> Value {
        match (self, value) {
            // Atoms pass through
            (Self::Float { .. } | Self::Delta { .. }, _) => value.clone(),
            (Self::Integer { .. }, _) => value.clone(),
            (Self::Bool { .. }, _) => value.clone(),
            (Self::String { .. } | Self::Enum { .. }, _) => value.clone(),
            (Self::Any, _) => value.clone(),

            // Overwrite/Maybe: delegate to inner
            (Self::Overwrite { inner }, _) => inner.encode(value),
            (Self::Maybe { .. }, Value::None) => Value::None,
            (Self::Maybe { inner }, _) => inner.encode(value),

            // List: serialize each element
            (Self::List { element }, Value::List(items)) => {
                Value::List(items.iter().map(|v| element.encode(v)).collect())
            }

            // Map: serialize each value
            (Self::Map { value: val_schema }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), val_schema.encode(v)))
                    .collect())
            }

            // Tree: serialize each branch with its schema
            (Self::Tree { branches }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| {
                        let s = branches.get(k).unwrap_or(&Schema::Any);
                        (k.clone(), s.encode(v))
                    })
                    .collect())
            }

            // Tuple: element-wise serialize
            (Self::Tuple { elements }, Value::List(items)) => {
                Value::List(items.iter().enumerate()
                    .map(|(i, v)| {
                        elements.get(i).unwrap_or(&Schema::Any).encode(v)
                    })
                    .collect())
            }

            // Array: pass through (already numeric lists)
            (Self::Array { .. }, _) => value.clone(),

            // RecursiveTree: serialize leaves, recurse maps
            (Self::RecursiveTree { leaf }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), self.encode(v)))
                    .collect())
            }
            (Self::RecursiveTree { leaf }, _) => leaf.encode(value),

            // Link: encode address, port schemas as strings, wiring
            (Self::Link { inputs, outputs, .. }, Value::Map(map)) => {
                let mut encoded = IndexMap::new();
                if let Some(addr) = map.get("address") {
                    encoded.insert(Key::from("address"), addr.clone());
                }
                let inputs_str = render_port_schema(inputs);
                let outputs_str = render_port_schema(outputs);
                encoded.insert(Key::from("_inputs"), Value::String(inputs_str));
                encoded.insert(Key::from("_outputs"), Value::String(outputs_str));
                if let Some(w) = map.get("inputs") {
                    encoded.insert(Key::from("inputs"), w.clone());
                }
                if let Some(w) = map.get("outputs") {
                    encoded.insert(Key::from("outputs"), w.clone());
                }
                if let Some(c) = map.get("config") {
                    encoded.insert(Key::from("config"), c.clone());
                }
                Value::Map(encoded)
            }

            _ => value.clone(),
        }
    }

    /// Realize (decode) an encoded value into a typed representation.
    ///
    /// This is the inverse of `serialize`. Converts string-encoded
    /// numbers, parses JSON strings into structured values, etc.
    pub fn realize(&self, encoded: &Value) -> Value {
        match (self, encoded) {
            // Float: accept string encoding
            (Self::Float { .. } | Self::Delta { .. }, Value::String(s)) => {
                s.parse::<f64>().map(Value::float).unwrap_or(encoded.clone())
            }
            (Self::Float { .. } | Self::Delta { .. }, Value::Int(i)) => {
                Value::float(*i as f64)
            }
            (Self::Float { .. } | Self::Delta { .. }, _) => encoded.clone(),

            // Integer: accept string encoding
            (Self::Integer { .. }, Value::String(s)) => {
                s.parse::<i64>().map(Value::Int).unwrap_or(encoded.clone())
            }
            (Self::Integer { .. }, _) => encoded.clone(),

            // Bool: accept string encoding
            (Self::Bool { .. }, Value::String(s)) => {
                match s.to_lowercase().as_str() {
                    "true" | "1" => Value::Bool(true),
                    "false" | "0" => Value::Bool(false),
                    _ => encoded.clone(),
                }
            }
            (Self::Bool { .. }, _) => encoded.clone(),

            // String/Enum: pass through
            (Self::String { .. } | Self::Enum { .. }, _) => encoded.clone(),

            // Overwrite/Maybe: delegate
            (Self::Overwrite { inner }, _) => inner.realize(encoded),
            (Self::Maybe { .. }, Value::None) => Value::None,
            (Self::Maybe { inner }, _) => inner.realize(encoded),

            // List
            (Self::List { element }, Value::List(items)) => {
                Value::List(items.iter().map(|v| element.realize(v)).collect())
            }

            // Map
            (Self::Map { value: val_schema }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), val_schema.realize(v)))
                    .collect())
            }

            // Tree: realize each branch, fill defaults for missing
            (Self::Tree { branches }, Value::Map(map)) => {
                let mut result: IndexMap<Key, Value> = branches.iter()
                    .map(|(k, s)| (k.clone(), s.default_value()))
                    .collect();
                for (k, v) in map {
                    let s = branches.get(k).unwrap_or(&Schema::Any);
                    result.insert(k.clone(), s.realize(v));
                }
                Value::Map(result)
            }

            // Tuple
            (Self::Tuple { elements }, Value::List(items)) => {
                Value::List(items.iter().enumerate()
                    .map(|(i, v)| {
                        elements.get(i).unwrap_or(&Schema::Any).realize(v)
                    })
                    .collect())
            }
            // Tuple from JSON string
            (Self::Tuple { elements }, Value::String(s)) => {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                    if let Some(arr) = parsed.as_array() {
                        return Value::List(arr.iter().enumerate()
                            .map(|(i, v)| {
                                let schema = elements.get(i).unwrap_or(&Schema::Any);
                                schema.realize(&json_to_value(v))
                            })
                            .collect());
                    }
                }
                encoded.clone()
            }

            // Map from JSON string
            (Self::Map { value: val_schema }, Value::String(s)) => {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                    if let Some(obj) = parsed.as_object() {
                        return Value::Map(obj.iter()
                            .map(|(k, v)| (Key::from(k.as_str()), val_schema.realize(&json_to_value(v))))
                            .collect());
                    }
                }
                encoded.clone()
            }

            // Array/RecursiveTree
            (Self::Array { .. }, _) => encoded.clone(),
            (Self::RecursiveTree { .. }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), self.realize(v)))
                    .collect())
            }
            (Self::RecursiveTree { leaf }, _) => leaf.realize(encoded),

            // Link: preserve as-is (engine handles instantiation)
            (Self::Link { .. }, _) => encoded.clone(),

            // Any/fallback
            (Self::Any, _) => encoded.clone(),
            _ => encoded.clone(),
        }
    }
}

/// Render port schema as a type expression string.
fn render_port_schema(ports: &IndexMap<Key, Schema>) -> String {
    ports.iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// Convert a serde_json::Value to our Value type.
fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::None,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { Value::Int(i) }
            else { Value::float(n.as_f64().unwrap_or(0.0)) }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        serde_json::Value::Object(obj) => {
            Value::Map(obj.iter().map(|(k, v)| (Key::from(k.as_str()), json_to_value(v))).collect())
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
            Self::RecursiveTree { leaf } => write!(f, "tree[{leaf}]"),
            Self::Link { inputs, outputs, temporal } => {
                let prefix = match temporal {
                    Some(true) => "process",
                    Some(false) => "step",
                    None => "link",
                };
                write!(f, "{prefix}[")?;
                for (i, (k, v)) in inputs.iter().enumerate() {
                    if i > 0 { write!(f, "|")?; }
                    write!(f, "{k}:{v}")?;
                }
                write!(f, ",")?;
                for (i, (k, v)) in outputs.iter().enumerate() {
                    if i > 0 { write!(f, "|")?; }
                    write!(f, "{k}:{v}")?;
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
pub fn apply_add_remove(result: &mut crate::value::StateMap, update: &crate::value::StateMap) {
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
