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
use prism_schema::{MethodRegistry, Value};

use crate::factory::ProcessRegistry;
use crate::process::ProcessNode;
use crate::protocol::{ParsedAddress, ProtocolError, ProtocolRegistry};

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
        self.register_protocol_address_types();
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
        self.register_protocol_address_types();
        self
    }

    /// Register every protocol's [`address_type`](crate::protocol::Protocol::address_type)
    /// into the type registry, so a process address is a first-class typed value
    /// (`check`/`serialize`/`realize`/`divide` via the closed algebra). Idempotent
    /// — re-run whenever protocols or types change. See docs/protocols-as-types.md.
    fn register_protocol_address_types(&mut self) {
        let address_types = self.protocols.address_types();
        if address_types.is_empty() {
            return;
        }
        let mut types = (*self.types).clone();
        for (name, schema) in address_types {
            types.register(name, schema, None);
        }
        self.types = Arc::new(types);
    }

    /// `local:` process classes referenced anywhere in `state` (via an `address`)
    /// that this core's process registry does NOT have — recursing through the
    /// whole tree, including composite `config`/`state` subdocuments. Empty ⇒ every
    /// process reference resolves. Remote (`rest:`/`parallel:`) addresses are
    /// validated by the remote side, so they are not reported here.
    ///
    /// A document referencing a process the core can't build is an error, not a
    /// silent drop — callers (`Engine::from_state`, the rest-process server) reject
    /// it with this list rather than running a partial graph.
    pub fn missing_process_refs(&self, state: &Value) -> Vec<String> {
        missing_process_refs(state, &self.processes)
    }

    /// Instantiate a live process from an `address` value — the SINGLE entry
    /// point for "address → process". Parses the address (a `"local:Class"`
    /// string, the legacy `{protocol, data}` map, or the typed `{_type, …}`
    /// form) and dispatches through the protocol registry, so
    /// `local`/`rest`/`parallel`/`stream` all behave identically. Every site
    /// that builds a process from a spec routes HERE; none special-cases `local`
    /// (the instantiation half of core unification).
    pub fn instantiate(
        &self,
        address: &Value,
        config: Value,
    ) -> Result<ProcessNode, ProtocolError> {
        let parsed = ParsedAddress::parse(address)?;
        self.protocols.instantiate(&parsed, config, &self.processes)
    }
}

/// `local:` process classes referenced in `state` that `registry` can't build.
/// Free-function form of [`Core::missing_process_refs`], for callers that hold a
/// process registry rather than a whole core (e.g. the rest-process server).
pub fn missing_process_refs(state: &Value, registry: &ProcessRegistry) -> Vec<String> {
    let mut out = Vec::new();
    collect_missing_processes(state, registry, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_missing_processes(value: &Value, registry: &ProcessRegistry, out: &mut Vec<String>) {
    if let Some(map) = value.as_map() {
        if let Some(addr) = map.get("address") {
            if let Ok(parsed) = ParsedAddress::parse(addr) {
                // Only `local` addresses are resolved against this registry;
                // remote protocols resolve on their own server.
                if parsed.protocol == "local" {
                    if let Some(class) = parsed.data.as_str() {
                        if class != "RAMEmitter" && !registry.contains(class) {
                            out.push(class.to_string());
                        }
                    }
                }
            }
        }
        for v in map.values() {
            collect_missing_processes(v, registry, out);
        }
    } else if let Value::List(items) = value {
        for v in items {
            collect_missing_processes(v, registry, out);
        }
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
