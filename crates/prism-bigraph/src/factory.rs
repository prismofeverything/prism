//! Process factory registry — construct processes from type names.
//!
//! This enables dynamic composition: a topology can reference
//! process types by string, and the factory registry constructs
//! the appropriate instances at runtime.

use std::collections::HashMap;

use prism_schema::Value;

use crate::process::ProcessNode;

/// A factory function that constructs a ProcessNode from config.
pub type FactoryFn = Box<dyn Fn(Value) -> ProcessNode + Send + Sync>;

/// Registry of process factories keyed by type name.
#[derive(Default)]
pub struct ProcessRegistry {
    factories: HashMap<String, FactoryFn>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a process factory.
    pub fn register<F>(&mut self, type_name: impl Into<String>, factory: F)
    where
        F: Fn(Value) -> ProcessNode + Send + Sync + 'static,
    {
        self.factories.insert(type_name.into(), Box::new(factory));
    }

    /// Create a process instance from a type name and config.
    pub fn create(&self, type_name: &str, config: Value) -> Option<ProcessNode> {
        self.factories.get(type_name).map(|f| f(config))
    }

    /// Check if a type is registered.
    pub fn contains(&self, type_name: &str) -> bool {
        self.factories.contains_key(type_name)
    }

    /// List all registered type names.
    pub fn type_names(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Instantiate all processes in a topology.
    pub fn instantiate_topology(
        &self,
        topology: &crate::topology::Topology,
    ) -> Result<HashMap<String, ProcessNode>, String> {
        let mut instances = HashMap::new();
        for (name, spec) in &topology.processes {
            match self.create(&spec.process_type, spec.config.clone()) {
                Some(node) => {
                    instances.insert(name.clone(), node);
                }
                None => {
                    return Err(format!(
                        "unknown process type '{}' for node '{}'",
                        spec.process_type, name
                    ));
                }
            }
        }
        Ok(instances)
    }
}

impl std::fmt::Debug for ProcessRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessRegistry")
            .field("types", &self.type_names())
            .finish()
    }
}
