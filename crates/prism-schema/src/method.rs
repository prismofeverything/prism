//! Type-keyed method dispatch for runtime values.
//!
//! A [`MethodRegistry`] maps `(type_name, method_name)` to a Rust closure
//! that receives the receiver value plus arguments and returns a new
//! value. The receiver's type is determined by [`value_type_name`] —
//! either the structural variant tag ("Int", "Float", "List", "Map") or
//! a registered nominal name (the `_type` field on a `Value::Map`, or
//! the `type_name` carried by a `Value::Foreign`).
//!
//! This is the bridge that lets surface languages — chrysalis, today —
//! call into prism's Rust API without re-implementing the methods. A
//! single closure can wrap any Rust method; type-erasure happens at the
//! `Value` boundary.
//!
//! ## Boundary
//!
//! Methods are pure (or near-pure) value transformations. They take a
//! receiver and arguments, produce a result. **Engine-level operations
//! — scheduling, projection application, structural diff — are NOT
//! exposed here.** That direction is the engine calling chrysalis, not
//! the other way around.

use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;

use crate::value::Value;

/// Result type for method dispatch.
pub type MethodResult = Result<Value, MethodError>;

/// A registered method: receives `&receiver` and `&args`, returns a `Value`.
pub type MethodFn = Arc<dyn Fn(&Value, &[Value]) -> MethodResult + Send + Sync>;

#[derive(Debug, Error)]
pub enum MethodError {
    #[error("no method `{method}` registered for type `{type_name}`")]
    NotFound { type_name: String, method: String },

    #[error("method `{method}` on `{type_name}`: {message}")]
    BadArgs {
        type_name: String,
        method: String,
        message: String,
    },

    #[error("method `{method}` on `{type_name}` failed: {message}")]
    Failed {
        type_name: String,
        method: String,
        message: String,
    },
}

/// Type-keyed dispatch table.
///
/// Each `(type_name, method_name)` slot holds a single closure. Later
/// registrations overwrite earlier ones for the same key — useful for
/// override-by-load-order, but watch for accidental shadowing.
#[derive(Default, Clone)]
pub struct MethodRegistry {
    by_type: HashMap<String, HashMap<String, MethodFn>>,
}

impl std::fmt::Debug for MethodRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let summary: HashMap<&String, Vec<&String>> = self
            .by_type
            .iter()
            .map(|(t, ms)| (t, ms.keys().collect()))
            .collect();
        f.debug_struct("MethodRegistry")
            .field("methods", &summary)
            .finish()
    }
}

impl MethodRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a method against a type name. Returns the previous
    /// registration if any (useful for tests).
    pub fn register<F>(
        &mut self,
        type_name: impl Into<String>,
        method: impl Into<String>,
        f: F,
    ) -> Option<MethodFn>
    where
        F: Fn(&Value, &[Value]) -> MethodResult + Send + Sync + 'static,
    {
        let type_name = type_name.into();
        let method = method.into();
        self.by_type
            .entry(type_name)
            .or_default()
            .insert(method, Arc::new(f))
    }

    /// Look up a method without dispatching it.
    pub fn lookup(&self, type_name: &str, method: &str) -> Option<&MethodFn> {
        self.by_type.get(type_name)?.get(method)
    }

    /// Dispatch a method call on a receiver. The receiver's type is
    /// inferred via [`value_type_name`].
    pub fn dispatch(
        &self,
        receiver: &Value,
        method: &str,
        args: &[Value],
    ) -> MethodResult {
        let type_name = value_type_name(receiver).to_string();
        match self.lookup(&type_name, method) {
            Some(f) => f(receiver, args),
            None => Err(MethodError::NotFound { type_name, method: method.to_string() }),
        }
    }

    /// Iterate over all registered `(type, method)` pairs. Useful for
    /// debugging and reflection.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_type.iter().flat_map(|(t, ms)| {
            ms.keys().map(move |m| (t.as_str(), m.as_str()))
        })
    }
}

/// Infer a [`Value`]'s type name for method dispatch.
///
/// Priority:
/// 1. `Value::Foreign(f)` → `f.type_name`
/// 2. `Value::Map(m)` with `_type` field → that field's string value
/// 3. otherwise the structural variant name ("Int", "Float", ...)
pub fn value_type_name(value: &Value) -> &str {
    match value {
        Value::None => "None",
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        Value::String(_) => "String",
        Value::List(_) => "List",
        Value::Map(map) => map
            .get("_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Map"),
        Value::Struct { .. } => "Struct",
        Value::Foreign(f) => &f.type_name,
        Value::Bytes(_) => "Bytes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_finds_registered_method() {
        let mut reg = MethodRegistry::new();
        reg.register("Float", "double", |recv, _args| {
            let f = recv.as_f64().unwrap_or(0.0);
            Ok(Value::float(f * 2.0))
        });

        let result = reg
            .dispatch(&Value::float(3.5), "double", &[])
            .unwrap();
        assert_eq!(result.as_f64(), Some(7.0));
    }

    #[test]
    fn dispatch_missing_method_errors() {
        let reg = MethodRegistry::new();
        let err = reg
            .dispatch(&Value::Int(1), "nope", &[])
            .unwrap_err();
        assert!(matches!(err, MethodError::NotFound { .. }));
    }

    #[test]
    fn value_type_name_uses_underscore_type() {
        let v = Value::tree([
            ("_type", Value::String("Cell".into())),
            ("mass", Value::float(1.0)),
        ]);
        assert_eq!(value_type_name(&v), "Cell");
    }

    #[test]
    fn foreign_type_name_dispatched_correctly() {
        use crate::value::Foreign;
        let v = Value::Foreign(Foreign::new("Mesh", 42i32));
        assert_eq!(value_type_name(&v), "Mesh");
    }
}
