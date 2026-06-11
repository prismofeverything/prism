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

use crate::registry::TypeRegistry;
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

    /// Join two method registries — the **linker's method half** (behind
    /// [`Core::merge`]). A method is opaque behaviour (a closure), so two
    /// registrations of the same `(type, method)` slot are "the same" only if they
    /// are the SAME `Arc` (a shared dependency Arc-cloned through the merge —
    /// `ptr_eq`), which makes the join **idempotent**; a same-slot DIFFERENT
    /// closure is a conflict. A slot in only one side is taken; `self` is
    /// left-biased on an identical tie.
    ///
    /// Returns the merged registry + the conflicting `"Type.method"` slot names.
    pub fn merge(&self, other: &MethodRegistry) -> (MethodRegistry, Vec<String>) {
        let mut merged = self.clone();
        let mut conflicts = Vec::new();
        for (ty, methods) in &other.by_type {
            let row = merged.by_type.entry(ty.clone()).or_default();
            for (method, f) in methods {
                match row.get(method) {
                    None => {
                        row.insert(method.clone(), Arc::clone(f));
                    }
                    Some(existing) if Arc::ptr_eq(existing, f) => {
                        // idempotent: the same shared closure
                    }
                    Some(_) => conflicts.push(format!("{ty}.{method}")),
                }
            }
        }
        (merged, conflicts)
    }

    /// This registry's OWN methods over a shared `base` — every `(type, method)`
    /// slot the base does not already provide. The package resolver's projection to
    /// recover a dependency's own method theory from its compiled Core (which
    /// bundles the std method floor). The dual of [`merge`](Self::merge).
    pub fn own_over(&self, base: &MethodRegistry) -> MethodRegistry {
        let mut by_type: HashMap<String, HashMap<String, MethodFn>> = HashMap::new();
        for (ty, methods) in &self.by_type {
            for (method, f) in methods {
                let in_base = base
                    .by_type
                    .get(ty)
                    .map_or(false, |base_methods| base_methods.contains_key(method));
                if !in_base {
                    by_type
                        .entry(ty.clone())
                        .or_default()
                        .insert(method.clone(), Arc::clone(f));
                }
            }
        }
        MethodRegistry { by_type }
    }

    /// Look up a method without dispatching it.
    pub fn lookup(&self, type_name: &str, method: &str) -> Option<&MethodFn> {
        self.by_type.get(type_name)?.get(method)
    }

    /// Dispatch a method on `receiver`, resolving the type-row by FALLBACK:
    /// the receiver's brand ([`value_type_name`]), then its STRUCTURAL variant —
    /// so a method registered on `Map` fires for a branded `{_type: Cell}` state
    /// ([`structural_name`]). For the `inherits`-chain fallback too (a method on a
    /// supertype `composite` firing for a `Cell`), use
    /// [`dispatch_with`](Self::dispatch_with), which threads the `TypeRegistry`.
    /// This brings the open type×method matrix's dispatch in line with the closed
    /// `TypeMethods` algebra, whose `TypeRegistry::methods` already walks `inherits`.
    pub fn dispatch(&self, receiver: &Value, method: &str, args: &[Value]) -> MethodResult {
        self.dispatch_resolved(None, receiver, method, args)
    }

    /// Like [`dispatch`](Self::dispatch) but also tries the brand's `inherits`
    /// ancestors in `types` (between the brand and the structural fallback) — full
    /// subsumption dispatch, identical to how `TypeMethods` resolves.
    pub fn dispatch_with(
        &self,
        types: &TypeRegistry,
        receiver: &Value,
        method: &str,
        args: &[Value],
    ) -> MethodResult {
        self.dispatch_resolved(Some(types), receiver, method, args)
    }

    /// The shared resolution walk: brand → (its `is_a` ancestors, if `types` given)
    /// → structural variant. First `(row, method)` that exists wins; else `NotFound`
    /// reported against the brand.
    fn dispatch_resolved(
        &self,
        types: Option<&TypeRegistry>,
        receiver: &Value,
        method: &str,
        args: &[Value],
    ) -> MethodResult {
        let brand = value_type_name(receiver);
        if let Some(f) = self.lookup(brand, method) {
            return f(receiver, args);
        }
        if let Some(types) = types {
            for ancestor in types.ancestors(brand) {
                if let Some(f) = self.lookup(&ancestor, method) {
                    return f(receiver, args);
                }
            }
        }
        let structural = structural_name(receiver);
        if structural != brand {
            if let Some(f) = self.lookup(structural, method) {
                return f(receiver, args);
            }
        }
        Err(MethodError::NotFound {
            type_name: brand.to_string(),
            method: method.to_string(),
        })
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

/// The receiver's STRUCTURAL variant name — like [`value_type_name`] but ignoring
/// the `_type` brand on a `Map`. The final method-dispatch fallback row, so a method
/// registered on `Map` (e.g. `dot` over any bigraph value) fires for every map
/// state, branded or not.
pub fn structural_name(value: &Value) -> &'static str {
    match value {
        Value::None => "None",
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        Value::String(_) => "String",
        Value::List(_) => "List",
        Value::Map(_) => "Map",
        Value::Struct { .. } => "Struct",
        // A `Foreign`'s structural identity IS its type name (no generic fallback);
        // `value_type_name` already returns that, so this row is never the extra try.
        Value::Foreign(_) => "Foreign",
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

    #[test]
    fn dispatch_falls_back_brand_then_is_a_then_structural() {
        use crate::schema::Schema;
        let mut methods = MethodRegistry::new();
        // A method on the SUPERTYPE row `composite`, and one on the STRUCTURAL row `Map`.
        methods.register("composite", "kind", |_r, _a| {
            Ok(Value::String("from-composite".into()))
        });
        methods.register("Map", "shape", |_r, _a| Ok(Value::String("from-map".into())));

        let mut types = TypeRegistry::new();
        types.register("composite", Schema::Any, None);
        types.register_full("Cell", Schema::Any, None, None, vec!["composite".into()]); // Cell <: composite

        let cell = Value::tree([
            ("_type", Value::String("Cell".into())),
            ("mass", Value::float(1.0)),
        ]);

        // No ("Cell","kind") exists; exact dispatch can't find it...
        assert!(methods.dispatch(&cell, "kind", &[]).is_err());
        // ...but with the type hierarchy it walks Cell -> composite (the is_a fallback).
        assert_eq!(
            methods.dispatch_with(&types, &cell, "kind", &[]).unwrap().as_str(),
            Some("from-composite"),
        );
        // The structural fallback (Map) needs no registry: a branded map still finds
        // a ("Map", _) method through plain dispatch.
        assert_eq!(
            methods.dispatch(&cell, "shape", &[]).unwrap().as_str(),
            Some("from-map"),
        );
    }
}
