//! Type registry for dynamic schema lookup and process registration.
//!
//! Mirrors bigraph-schema's Core — a central place where types and
//! process factories are registered, enabling runtime composition
//! of simulations from configuration.

use std::collections::HashMap;

use crate::schema::Schema;
use crate::value::Value;

/// A registered type: its schema and optional default value.
#[derive(Clone, Debug)]
pub struct TypeEntry {
    pub schema: Schema,
    pub default: Option<Value>,
}

/// Central registry for types and schemas.
///
/// This is the Rust equivalent of bigraph-schema's `Core` object.
/// It holds registered types that can be looked up by name at runtime,
/// enabling dynamic composition from configuration files.
#[derive(Clone, Debug)]
pub struct TypeRegistry {
    types: HashMap<String, TypeEntry>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            types: HashMap::new(),
        };
        registry.register_builtins();
        registry
    }

    fn register_builtins(&mut self) {
        self.register("any", Schema::Any, None);
        self.register("bool", Schema::bool(), Some(Value::Bool(false)));
        self.register("boolean", Schema::bool(), Some(Value::Bool(false)));
        self.register("integer", Schema::integer(), Some(Value::Int(0)));
        self.register("int", Schema::integer(), Some(Value::Int(0)));
        self.register("float", Schema::float(), Some(Value::float(0.0)));
        self.register("string", Schema::string(), Some(Value::String(String::new())));
        self.register("delta", Schema::delta(), Some(Value::float(0.0)));
    }

    /// Register a named type.
    pub fn register(
        &mut self,
        name: impl Into<String>,
        schema: Schema,
        default: Option<Value>,
    ) {
        self.types.insert(
            name.into(),
            TypeEntry { schema, default },
        );
    }

    /// Look up a type by name.
    pub fn get(&self, name: &str) -> Option<&TypeEntry> {
        self.types.get(name)
    }

    /// Look up just the schema for a type name.
    pub fn schema(&self, name: &str) -> Option<&Schema> {
        self.types.get(name).map(|e| &e.schema)
    }

    /// Get the default value for a named type.
    pub fn default_value(&self, name: &str) -> Option<Value> {
        self.types.get(name).map(|e| {
            e.default.clone().unwrap_or_else(|| e.schema.default_value())
        })
    }

    /// List all registered type names.
    pub fn type_names(&self) -> Vec<&str> {
        self.types.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins() {
        let reg = TypeRegistry::new();
        assert!(reg.get("float").is_some());
        assert!(reg.get("integer").is_some());
        assert!(reg.get("string").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_custom_type() {
        let mut reg = TypeRegistry::new();
        reg.register(
            "concentration",
            Schema::float_default(0.0),
            Some(Value::float(0.0)),
        );
        assert!(reg.get("concentration").is_some());
        assert_eq!(reg.default_value("concentration"), Some(Value::float(0.0)));
    }
}
