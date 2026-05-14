//! Core value type for the hierarchical state tree.
//!
//! `Value` is the universal currency of prism state — every node in the
//! hierarchical state tree is a `Value`, and every process update produces
//! and consumes `Value`s through its ports.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use compact_str::CompactString;
use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

/// String key type — strings ≤24 bytes stored inline (no heap allocation).
/// Covers all common keys: "mass", "position", "velocity", "environment",
/// "particles", "fields", "glucose", "acetate", etc.
/// Cloning is a 24-byte memcpy instead of heap alloc + copy + dealloc.
pub type Key = CompactString;

/// A path through the hierarchical state tree.
pub type Path = Vec<Key>;

/// An ordered map preserving insertion order, used for tree nodes.
pub type StateMap = IndexMap<Key, Value>;

/// Compiled struct layout — shared among all instances with the same schema.
/// Fields are accessed by index (O(1)) instead of by hash lookup (O(1) amortized
/// but with allocation/hashing overhead).
#[derive(Clone, Debug)]
pub struct StructLayout {
    /// Field names in order.
    pub fields: Vec<Key>,
    /// Field name → index for O(1) lookup by name.
    pub field_index: HashMap<Key, usize>,
}

impl StructLayout {
    /// Create a layout from an ordered list of field names.
    pub fn new(fields: Vec<Key>) -> Arc<Self> {
        let field_index = fields.iter().enumerate()
            .map(|(i, k)| (k.clone(), i))
            .collect();
        Arc::new(Self { fields, field_index })
    }

    /// Get field index by name.
    #[inline]
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.field_index.get(name).copied()
    }
}

impl PartialEq for StructLayout {
    fn eq(&self, other: &Self) -> bool {
        self.fields == other.fields
    }
}
impl Eq for StructLayout {}

/// Opaque carrier for type-registered "Foreign" values — rich runtime
/// data that doesn't fit into the structural Value variants (numeric,
/// container, etc.). Foreign values participate in the place graph
/// through `TypeMethods` dispatch registered against the carrier's
/// `type_name`.
///
/// **Equality** is pointer identity (`Arc::ptr_eq`) plus type-name
/// match — Foreign values aren't structurally comparable in general,
/// since the underlying types may not impl `Eq`.
///
/// **Serialization** is opaque at the serde-derive boundary; portable
/// form is produced by `TypeMethods::serialize` and consumed by
/// `TypeMethods::realize`.
#[derive(Clone)]
pub struct Foreign {
    /// The registered type name. Drives method dispatch.
    pub type_name: String,
    /// Type-erased opaque payload. Must be `Send + Sync` so engine
    /// state can be passed between processes/threads.
    pub data: Arc<dyn Any + Send + Sync>,
}

impl Foreign {
    pub fn new<T: Any + Send + Sync + 'static>(type_name: impl Into<String>, value: T) -> Self {
        Self {
            type_name: type_name.into(),
            data: Arc::new(value),
        }
    }

    /// Try to downcast the opaque payload to `&T`. Returns `None` if
    /// the concrete type doesn't match.
    pub fn downcast_ref<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.data.downcast_ref::<T>()
    }
}

impl PartialEq for Foreign {
    fn eq(&self, other: &Self) -> bool {
        self.type_name == other.type_name && Arc::ptr_eq(&self.data, &other.data)
    }
}

impl Eq for Foreign {}

impl fmt::Debug for Foreign {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Foreign({})", self.type_name)
    }
}

/// The universal value type for all simulation state.
///
/// Mirrors the bigraph-schema type hierarchy:
/// - Atoms: Bool, Int, Float, String
/// - Containers: List, Map (ordered), Tree (recursive)
/// - Struct: fixed-layout map compiled from schema (O(1) field access)
/// - Foreign: type-erased rich values dispatched via `TypeMethods`
/// - Special: None, Bytes (for serialized blobs like numpy arrays)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    None,
    Bool(bool),
    Int(i64),
    Float(OrderedFloat<f64>),
    String(String),
    List(Vec<Value>),
    Map(StateMap),
    /// Fixed-layout struct — fields accessed by index.
    /// Serializes as a map for JSON compatibility.
    #[serde(skip)]
    Struct {
        #[serde(skip)]
        layout: Arc<StructLayout>,
        values: Vec<Value>,
    },
    /// Opaque rich type — see [`Foreign`].
    #[serde(skip)]
    Foreign(Foreign),
    Bytes(Vec<u8>),
}

impl Value {
    // ── Constructors ──

    pub fn float(v: f64) -> Self {
        Self::Float(OrderedFloat(v))
    }

    pub fn map() -> Self {
        Self::Map(StateMap::new())
    }

    pub fn tree(entries: impl IntoIterator<Item = (impl Into<Key>, Value)>) -> Self {
        Self::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// Create a Struct value from a layout and initial values.
    pub fn make_struct(layout: Arc<StructLayout>, values: Vec<Value>) -> Self {
        Self::Struct { layout, values }
    }

    /// Compile a Map into a Struct using the given layout.
    /// Fields not present in the map get Value::None.
    pub fn compile_struct(map: &StateMap, layout: Arc<StructLayout>) -> Self {
        let values: Vec<Value> = layout.fields.iter()
            .map(|k| map.get(k).cloned().unwrap_or(Value::None))
            .collect();
        Self::Struct { layout, values }
    }

    // ── Accessors ──

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(f.0),
            Self::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&StateMap> {
        match self {
            Self::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_map_mut(&mut self) -> Option<&mut StateMap> {
        // If this is a Struct, promote to Map first so callers can mutate
        if matches!(self, Self::Struct { .. }) {
            self.promote_to_map();
        }
        match self {
            Self::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Convert a Struct to a Map in-place. Called when mutable map access
    /// is needed (e.g., _add/_remove operations on a compiled state).
    fn promote_to_map(&mut self) {
        if let Self::Struct { layout, values } = self {
            let map: StateMap = layout.fields.iter().zip(values.drain(..))
                .map(|(k, v)| (k.clone(), v))
                .collect();
            *self = Self::Map(map);
        }
    }

    /// Get a field from a Struct by name (O(1) index lookup).
    /// Also works on Map as fallback.
    #[inline]
    pub fn get_field(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Struct { layout, values } => {
                layout.index_of(name).and_then(|i| values.get(i))
            }
            Self::Map(m) => m.get(name),
            _ => None,
        }
    }

    /// Set a field on a Struct by name (O(1) index lookup).
    /// Also works on Map as fallback.
    #[inline]
    pub fn set_field(&mut self, name: &str, value: Value) {
        match self {
            Self::Struct { layout, values } => {
                if let Some(i) = layout.index_of(name) {
                    if i < values.len() {
                        values[i] = value;
                    }
                }
            }
            Self::Map(m) => {
                m.insert(Key::from(name), value);
            }
            _ => {}
        }
    }

    /// Convert a Struct to a Map (for serialization or code that needs Map).
    pub fn to_map(&self) -> Option<StateMap> {
        match self {
            Self::Struct { layout, values } => {
                let map: StateMap = layout.fields.iter().zip(values.iter())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Some(map)
            }
            Self::Map(m) => Some(m.clone()),
            _ => None,
        }
    }

    /// Check if this is a map-like value (Map or Struct).
    pub fn is_map_like(&self) -> bool {
        matches!(self, Self::Map(_) | Self::Struct { .. })
    }

    /// Iterate over (key, &value) pairs from Map or Struct.
    /// Returns None for non-map-like values.
    pub fn iter_fields(&self) -> Option<FieldIter<'_>> {
        match self {
            Self::Map(m) => Some(FieldIter::Map(m.iter())),
            Self::Struct { layout, values } => Some(FieldIter::Struct {
                fields: &layout.fields,
                values,
                idx: 0,
            }),
            _ => None,
        }
    }

    /// Number of fields/keys in a Map or Struct. Returns 0 for others.
    pub fn field_count(&self) -> usize {
        match self {
            Self::Map(m) => m.len(),
            Self::Struct { values, .. } => values.len(),
            _ => 0,
        }
    }

    /// Check if a field/key exists in a Map or Struct.
    pub fn contains_field(&self, name: &str) -> bool {
        match self {
            Self::Map(m) => m.contains_key(name),
            Self::Struct { layout, .. } => layout.field_index.contains_key(name),
            _ => false,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(l) => Some(l),
            _ => None,
        }
    }

    // ── Tree navigation ──

    /// Get a value at a path through nested maps and lists.
    /// Numeric string keys are used as list indices when the current
    /// value is a List (e.g., path `["fields", "glucose", "0", "0"]`
    /// indexes into nested lists).
    pub fn get_path(&self, path: &[Key]) -> Option<&Value> {
        let mut current = self;
        for key in path {
            match current {
                Self::Map(map) => current = map.get(key)?,
                Self::Struct { layout, values } => {
                    let idx = layout.index_of(key)?;
                    current = values.get(idx)?;
                }
                Self::List(list) => {
                    let idx: usize = key.parse().ok()?;
                    current = list.get(idx)?;
                }
                _ => return None,
            }
        }
        Some(current)
    }

    /// Get a mutable reference to a value at a path.
    pub fn get_path_mut(&mut self, path: &[Key]) -> Option<&mut Value> {
        let mut current = self;
        for key in path {
            match current {
                Self::Map(map) => current = map.get_mut(key)?,
                Self::Struct { layout, values } => {
                    let idx = layout.index_of(key)?;
                    current = values.get_mut(idx)?;
                }
                Self::List(list) => {
                    let idx: usize = key.parse().ok()?;
                    current = list.get_mut(idx)?;
                }
                _ => return None,
            }
        }
        Some(current)
    }

    /// Set a value at a path, creating intermediate maps as needed.
    /// Numeric string keys index into existing lists (lists are not
    /// auto-created, but existing list elements can be updated).
    pub fn set_path(&mut self, path: &[Key], value: Value) {
        if path.is_empty() {
            *self = value;
            return;
        }

        let mut current = self;
        for key in &path[..path.len() - 1] {
            match current {
                Self::List(list) => {
                    if let Ok(idx) = key.parse::<usize>() {
                        if idx < list.len() {
                            current = &mut list[idx];
                            continue;
                        }
                    }
                    return;
                }
                Self::Struct { .. } => {
                    // Try navigating into existing field by promoting
                    // to Map first (avoids complex borrow issues with
                    // index lookup + mutable access on the same enum).
                    current.promote_to_map();
                    if let Self::Map(map) = current {
                        current = map.entry(key.clone()).or_insert_with(Self::map);
                        continue;
                    }
                    return;
                }
                _ => {
                    if !matches!(current, Self::Map(_)) {
                        *current = Self::map();
                    }
                    let map = current.as_map_mut().unwrap();
                    current = map.entry(key.clone()).or_insert_with(Self::map);
                }
            }
        }

        let last_key = path.last().unwrap();
        match current {
            Self::Struct { layout, values } => {
                if let Some(idx) = layout.index_of(last_key) {
                    if idx < values.len() {
                        values[idx] = value;
                        return;
                    }
                }
                // Field not in layout — promote to Map then insert
                current.promote_to_map();
                if let Self::Map(map) = current {
                    map.insert(last_key.clone(), value);
                }
            }
            Self::Map(map) => {
                map.insert(last_key.clone(), value);
            }
            Self::List(list) => {
                if let Ok(idx) = last_key.parse::<usize>() {
                    if idx < list.len() {
                        list[idx] = value;
                    }
                }
            }
            _ => {}
        }
    }

    /// Deep merge: recursively combine two values.
    /// For maps, merge keys. For atoms, `other` overwrites `self`.
    pub fn merge(&mut self, other: Value) {
        match (self, other) {
            (Self::Map(base), Self::Map(update)) => {
                for (key, val) in update {
                    base.entry(key)
                        .and_modify(|existing| existing.merge(val.clone()))
                        .or_insert(val);
                }
            }
            (this, other) => *this = other,
        }
    }

    /// Returns true if this is Value::None
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns the type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bool(_) => "bool",
            Self::Int(_) => "integer",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::Struct { .. } => "struct",
            Self::Foreign(_) => "foreign",
            Self::Bytes(_) => "bytes",
        }
    }

    /// If this is a `Foreign` value, return the registered type name
    /// it carries; otherwise `None`. Use this when dispatching through
    /// a `TypeRegistry` against the actual registered name (not the
    /// generic "foreign" label).
    pub fn foreign_type_name(&self) -> Option<&str> {
        match self {
            Self::Foreign(f) => Some(&f.type_name),
            _ => None,
        }
    }

    /// Try to borrow the `Foreign` carrier inside this value.
    pub fn as_foreign(&self) -> Option<&Foreign> {
        match self {
            Self::Foreign(f) => Some(f),
            _ => None,
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Self::None
    }
}

/// Iterator over (key, &value) pairs from either Map or Struct.
pub enum FieldIter<'a> {
    Map(indexmap::map::Iter<'a, Key, Value>),
    Struct {
        fields: &'a [Key],
        values: &'a [Value],
        idx: usize,
    },
}

impl<'a> Iterator for FieldIter<'a> {
    type Item = (&'a Key, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Map(iter) => iter.next(),
            Self::Struct { fields, values, idx } => {
                if *idx < fields.len() {
                    let i = *idx;
                    *idx += 1;
                    Some((&fields[i], &values[i]))
                } else {
                    None
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Map(iter) => iter.size_hint(),
            Self::Struct { fields, idx, .. } => {
                let remaining = fields.len() - *idx;
                (remaining, Some(remaining))
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Foreign(fv) => write!(f, "<foreign:{}>", fv.type_name),
            Self::String(s) => write!(f, "\"{s}\""),
            Self::List(l) => {
                write!(f, "[")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Self::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Self::Struct { layout, values } => {
                write!(f, "{{")?;
                for (i, (k, v)) in layout.fields.iter().zip(values.iter()).enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Self::Bytes(b) => write!(f, "<bytes[{}]>", b.len()),
        }
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::float(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_navigation() {
        let mut state = Value::tree([
            ("cell", Value::tree([
                ("mass", Value::float(1.0)),
                ("proteins", Value::tree([
                    ("atp_synthase", Value::float(100.0)),
                ])),
            ])),
        ]);

        let path: Path = vec!["cell".into(), "proteins".into(), "atp_synthase".into()];
        assert_eq!(state.get_path(&path), Some(&Value::float(100.0)));

        let new_path: Path = vec!["cell".into(), "proteins".into(), "flagellin".into()];
        state.set_path(&new_path, Value::float(50.0));
        assert_eq!(state.get_path(&new_path), Some(&Value::float(50.0)));
    }

    #[test]
    fn test_deep_merge() {
        let mut base = Value::tree([
            ("a", Value::tree([
                ("x", Value::float(1.0)),
                ("y", Value::float(2.0)),
            ])),
        ]);

        let update = Value::tree([
            ("a", Value::tree([
                ("y", Value::float(3.0)),
                ("z", Value::float(4.0)),
            ])),
        ]);

        base.merge(update);

        let path_x: Path = vec!["a".into(), "x".into()];
        let path_y: Path = vec!["a".into(), "y".into()];
        let path_z: Path = vec!["a".into(), "z".into()];
        assert_eq!(base.get_path(&path_x), Some(&Value::float(1.0)));
        assert_eq!(base.get_path(&path_y), Some(&Value::float(3.0)));
        assert_eq!(base.get_path(&path_z), Some(&Value::float(4.0)));
    }
}
