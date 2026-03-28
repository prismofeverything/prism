//! Core value type for the hierarchical state tree.
//!
//! `Value` is the universal currency of prism state — every node in the
//! hierarchical state tree is a `Value`, and every process update produces
//! and consumes `Value`s through its ports.

use std::fmt;

use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

/// A path through the hierarchical state tree.
/// Each element is a string key navigating one level deeper.
pub type Path = Vec<String>;

/// An ordered map preserving insertion order, used for tree nodes.
pub type StateMap = IndexMap<String, Value>;

/// The universal value type for all simulation state.
///
/// Mirrors the bigraph-schema type hierarchy:
/// - Atoms: Bool, Int, Float, String
/// - Containers: List, Map (ordered), Tree (recursive)
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

    pub fn tree(entries: impl IntoIterator<Item = (impl Into<String>, Value)>) -> Self {
        Self::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
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
        match self {
            Self::Map(m) => Some(m),
            _ => None,
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
    pub fn get_path(&self, path: &[String]) -> Option<&Value> {
        let mut current = self;
        for key in path {
            match current {
                Self::Map(map) => current = map.get(key)?,
                Self::List(list) => {
                    let idx: usize = key.parse().ok()?;
                    current = list.get(idx)?;
                }
                _ => return None,
            }
        }
        Some(current)
    }

    /// Set a value at a path, creating intermediate maps as needed.
    /// Numeric string keys index into existing lists (lists are not
    /// auto-created, but existing list elements can be updated).
    pub fn set_path(&mut self, path: &[String], value: Value) {
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
            Self::Bytes(_) => "bytes",
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Self::None
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
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
