//! The unified runtime **core** — one object bundling every registry the engine
//! and its composites need: type schemas, process factories, value-methods, and
//! protocols. prism's port of upstream `bigraph_schema/core.py`'s `Core`
//! (`registry` + `link_registry`), carried as ONE value so a subengine (a
//! `Composite`) inherits the WHOLE core.
//!
//! Before this, the engine held four separate registry fields and
//! `Composite::from_config` received only the process registry — so a subengine
//! silently lost types, methods, and protocols, and a `Custom`-typed,
//! method-using, or `rest:`-addressed process could not live inside a composite.
//! Threading one `Core` fixes that class of bug at the root.

use std::sync::Arc;

use prism_schema::registry::TypeRegistry;
use prism_schema::MethodRegistry;

use crate::factory::ProcessRegistry;
use crate::protocol::ProtocolRegistry;

/// All runtime registries, carried together. Cheap to clone — each field is an
/// `Arc`, so a clone shares the same registries (exactly what lets a subengine
/// run against its parent's core).
#[derive(Clone, Debug)]
pub struct Core {
    /// Custom type schemas and their methods (upstream `registry`).
    pub types: Arc<TypeRegistry>,
    /// Process / step factories by name (upstream `link_registry`).
    pub processes: Arc<ProcessRegistry>,
    /// Value-receiver methods (chrysalis surface-language dispatch).
    pub methods: Arc<MethodRegistry>,
    /// Address protocols: `local` (in-process) + `rest` / `parallel` (remote).
    pub protocols: Arc<ProtocolRegistry>,
}

impl Default for Core {
    /// Empty registries + the default protocol set (`local`).
    fn default() -> Self {
        Self {
            types: Arc::new(TypeRegistry::new()),
            processes: Arc::new(ProcessRegistry::new()),
            methods: Arc::new(MethodRegistry::new()),
            protocols: Arc::new(ProtocolRegistry::new()),
        }
    }
}

impl Core {
    /// An empty core (empty registries; `local` protocol only).
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: set the process/step factory registry.
    pub fn with_processes(mut self, processes: Arc<ProcessRegistry>) -> Self {
        self.processes = processes;
        self
    }
    /// Builder: set the Custom type registry.
    pub fn with_types(mut self, types: Arc<TypeRegistry>) -> Self {
        self.types = types;
        self
    }
    /// Builder: set the value-method registry.
    pub fn with_methods(mut self, methods: Arc<MethodRegistry>) -> Self {
        self.methods = methods;
        self
    }
    /// Builder: set the protocol registry.
    pub fn with_protocols(mut self, protocols: Arc<ProtocolRegistry>) -> Self {
        self.protocols = protocols;
        self
    }
}

/// Ergonomic migration: a core that is just a process registry — the common case
/// for callers that previously passed an `Arc<ProcessRegistry>` (they get the
/// default `local`-only protocols + empty type/method registries).
impl From<Arc<ProcessRegistry>> for Core {
    fn from(processes: Arc<ProcessRegistry>) -> Self {
        Core::new().with_processes(processes)
    }
}
