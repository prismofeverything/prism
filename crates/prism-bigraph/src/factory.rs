//! Process factory registry — construct processes from type names.
//!
//! This enables dynamic composition: a topology can reference
//! process types by string, and the factory registry constructs
//! the appropriate instances at runtime.

use std::collections::HashMap;
use std::sync::Arc;

use prism_schema::Value;

use crate::process::ProcessNode;

/// A factory function that constructs a ProcessNode from config. `Arc` (not
/// `Box`) so a [`ProcessRegistry`] is **`Clone`** — a domain's registry can be
/// cloned and extended with a program's own factories when threading ONE `Core`
/// through the `run()`/`compile` boundary (the canonical run-Core; the factories
/// are shared, not duplicated).
pub type FactoryFn = Arc<dyn Fn(Value) -> ProcessNode + Send + Sync>;

/// Registry of process factories keyed by type name.
#[derive(Default, Clone)]
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
        self.factories.insert(type_name.into(), Arc::new(factory));
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

    /// Join two process registries — the **linker's process half** (behind
    /// [`crate::Core::merge`]). A factory is opaque behaviour (a closure), so two
    /// registrations of the same class name are "the same" only if they are the
    /// SAME `Arc` (a shared dependency Arc-cloned through the merge — `ptr_eq`),
    /// making the join **idempotent**; a same-name DIFFERENT factory is a conflict.
    /// A class in only one side is taken; `self` is left-biased on an identical tie.
    ///
    /// Returns the merged registry + the conflicting class names (empty ⇒ clean).
    pub fn merge(&self, other: &ProcessRegistry) -> (ProcessRegistry, Vec<String>) {
        let mut merged = self.clone();
        let mut conflicts = Vec::new();
        for (name, factory) in &other.factories {
            match merged.factories.get(name) {
                None => {
                    merged.factories.insert(name.clone(), Arc::clone(factory));
                }
                Some(existing) if Arc::ptr_eq(existing, factory) => {
                    // idempotent: the same shared factory
                }
                Some(_) => conflicts.push(name.clone()),
            }
        }
        (merged, conflicts)
    }

    /// This registry's OWN factories over a shared `base` — every class name the
    /// base does not already provide. The package resolver's projection to recover
    /// a dependency's own process theory from its compiled Core: it drops not only
    /// the std factories but the **per-compile** generic ones every compile
    /// re-creates (`Composite`, `Brs` — fresh closures over each compile's Core
    /// handle), which would otherwise false-conflict in [`merge`](Self::merge) even
    /// though they are interchangeable infrastructure. The dual of `merge`.
    pub fn own_over(&self, base: &ProcessRegistry) -> ProcessRegistry {
        let factories = self
            .factories
            .iter()
            .filter(|(name, _)| !base.factories.contains_key(*name))
            .map(|(name, factory)| (name.clone(), Arc::clone(factory)))
            .collect();
        ProcessRegistry { factories }
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
