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
//!
//! ## The threading rule (the invariant)
//!
//! There is **one `Core` per runtime context**, `Arc`-shared (every field is an
//! `Arc`, so sharing is free). It is assembled once — by `chrysalis::compile`, or
//! by a host — and reaches everything that needs it by exactly two mechanisms,
//! chosen by *lifecycle*, never by local convenience:
//!
//! - **Push** — [`Engine::set_core`](crate::Engine::set_core) /
//!   [`Composite::from_config`](crate::Composite::from_config) — for things the
//!   engine creates at runtime: the engine, every node (`set_core` during
//!   discovery), every subengine. The creator hands down the Core it holds. The
//!   [`BigraphicalReactiveSystem`](crate::BigraphicalReactiveSystem) is a node, so
//!   it captures the WHOLE Core here (its `apply` reads `core.types`; a reactum
//!   evaluated against it can introspect the rest).
//! - **Pull** — a late-bound `Arc<OnceLock<Core>>` handle — for the one
//!   compile-time artifact that necessarily *predates* the Core: the chrysalis
//!   `Evaluator`. It predates the Core because of an intrinsic cycle — the
//!   `ProcessRegistry`'s factories capture the evaluator, and the Core *contains*
//!   the registry. This is the SAME `OnceLock` cycle-breaker the `Composite`
//!   factory uses; the evaluator reads `types`/`methods`/`processes`/`protocols`
//!   from the one shared Core via this handle.
//!
//! **Invariant:** no component stores a registry *subset*. A consumer that needs
//! only (say) the type registry still receives the whole Core and reads the part
//! it uses — so adding a new need (a reactum that instantiates a process, or
//! introspects available types) requires no re-threading. New Core-holders MUST
//! pick push or pull by lifecycle; they MUST NOT take an `Arc<ProcessRegistry>`
//! (or any single registry) as a stand-in for the Core. (The
//! `From<Arc<ProcessRegistry>>` impl below is only for genuinely registry-only
//! callers — low-level tests with no types/methods/protocols to lose.)

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
        self.protocols.instantiate(&parsed, config, self)
    }

    /// Instantiate a live process from a SPEC ENVELOPE — the `{_type, address,
    /// config, …}` map that node-discovery reads out of state (the data form a
    /// process/composite constructor produces). The envelope's SHAPE is owned
    /// HERE so callers don't re-implement field extraction: this is the node
    /// rung of "eval state-data → runnable" (`docs/schema-algebra.md` ·
    /// `homoiconic-unification.md` §3 Stage 4a), the analogue of the BRS's
    /// `ReactionRule::from_data_value` (rules) and `Pattern::from_value`
    /// (patterns). The surface `eval` (chrysalis) CALLS this — thin layer, no
    /// clone of the envelope shape. Wiring (`inputs`/`outputs`) is applied by the
    /// engine when the node is placed (`discover_processes`); this returns the
    /// instantiated node itself.
    pub fn instantiate_spec(&self, spec: &Value) -> Result<ProcessNode, ProtocolError> {
        let address = spec.get_field("address").ok_or_else(|| {
            ProtocolError::MalformedAddress("a process spec needs an `address` field".into())
        })?;
        let config = spec.get_field("config").cloned().unwrap_or(Value::None);
        self.instantiate(address, config)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Update;
    use crate::ports::PortSchema;
    use crate::process::Step;

    #[derive(Debug)]
    struct NoopStep;
    impl Step for NoopStep {
        fn inputs(&self) -> PortSchema {
            indexmap::IndexMap::new()
        }
        fn outputs(&self) -> PortSchema {
            indexmap::IndexMap::new()
        }
        fn update(&self, _state: &Value) -> Update {
            Update::Noop
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn core_with_noop() -> Core {
        let mut r = ProcessRegistry::new();
        r.register("Noop", |_config| ProcessNode::Step(Box::new(NoopStep)));
        Core::from(Arc::new(r))
    }

    #[test]
    fn instantiate_spec_brings_a_spec_envelope_to_life() {
        // The NODE rung of "eval state-data → runnable": a `{_type, address}`
        // envelope (the data a process/composite constructor produces) becomes a
        // live node — the analogue of the BRS evaling a `{_pat:"Rule"}` map and
        // `find_matches` consuming `Pattern::from_value`. The surface `eval` (4a)
        // calls THIS, never re-implementing the envelope shape.
        let core = core_with_noop();
        let spec = Value::tree([
            ("_type", Value::String("step".into())),
            ("address", Value::String("local:Noop".into())),
        ]);
        let node = core.instantiate_spec(&spec).expect("spec envelope → node");
        assert!(matches!(node, ProcessNode::Step(_)));
    }

    #[test]
    fn instantiate_spec_rejects_a_spec_without_address() {
        // `address` is what instantiation needs; its absence is a clear error,
        // not a silent drop (the envelope shape is validated in one place).
        let core = core_with_noop();
        let spec = Value::tree([("_type", Value::String("step".into()))]);
        let err = core.instantiate_spec(&spec).unwrap_err();
        assert!(matches!(err, ProtocolError::MalformedAddress(_)));
    }
}
