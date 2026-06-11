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

    /// **Link** another core into this one — the package linker's join (#67). A
    /// package is a `Core` (types · processes · methods · protocols); *depending
    /// on* a package is **merging its Core**. This is the idempotent
    /// join-SEMILATTICE the categorical semantics demand: commutative + associative
    /// + idempotent (the CALM property [`mesh_safety`](prism_schema::algebra) gates
    /// for the mesh), so the colimit over a dependency DAG is well-defined — a
    /// **diamond** dependency (A→D, B→D, prog→A,B) merges D **once**, because D's
    /// registry entries are the SAME `Arc`s in both branches (`ptr_eq` ⇒ no
    /// re-union, no false self-conflict).
    ///
    /// Per registry, "the same key in both sides" is reconciled by that registry's
    /// own identity: TYPES structurally (`schema`/`default`/`inherits` agree — the
    /// shared builtins reconcile to themselves), PROCESSES and METHODS by `Arc`
    /// identity (opaque closures; a shared dep is pointer-identical), PROTOCOLS by
    /// name (behaviour-opaque infrastructure, never a conflict). A genuine same-name
    /// **incompatible** definition — two different schemas for one type, two
    /// different factories for one class — is a [`CoreMergeConflict`] (semver's job
    /// to prevent: the version is part of a package's identity). `self` is
    /// left-biased on every compatible tie.
    ///
    /// `Ok` ⇒ the linked core (every protocol's address type re-registered, so the
    /// Core invariant holds); `Err` ⇒ *every* conflict, so the user sees them all.
    pub fn merge(&self, other: &Core) -> Result<Core, Vec<CoreMergeConflict>> {
        let mut conflicts = Vec::new();

        let (types, type_conflicts) = self.types.merge(&other.types);
        conflicts.extend(
            type_conflicts
                .into_iter()
                .map(|name| CoreMergeConflict { registry: "type", name }),
        );
        let (processes, process_conflicts) = self.processes.merge(&other.processes);
        conflicts.extend(
            process_conflicts
                .into_iter()
                .map(|name| CoreMergeConflict { registry: "process", name }),
        );
        let (methods, method_conflicts) = self.methods.merge(&other.methods);
        conflicts.extend(
            method_conflicts
                .into_iter()
                .map(|name| CoreMergeConflict { registry: "method", name }),
        );
        let protocols = self.protocols.merge(&other.protocols);

        if !conflicts.is_empty() {
            return Err(conflicts);
        }

        let mut merged = Core {
            types: Arc::new(types),
            processes: Arc::new(processes),
            methods: Arc::new(methods),
            protocols: Arc::new(protocols),
        };
        // Preserve the Core invariant: a newly-merged protocol's address type must
        // be registered so an address stays a first-class typed value. Idempotent.
        merged.register_protocol_address_types();
        Ok(merged)
    }

    /// This core's OWN contribution over a shared `base`: every registry entry whose
    /// NAME the base does not already provide. The **dual** of [`merge`](Core::merge)
    /// (merge joins; `own_over` takes the part of self strictly above the floor),
    /// and the projection the package resolver (#67) needs because a *compiled* Core
    /// is not a package's theory — it bundles the theory WITH the base it compiled
    /// against (the std/builtin floor + the per-compile generic `Composite`/`Brs`
    /// factories). `dep_core.own_over(base)` recovers just the package's declared
    /// theory, so the resolver can `merge` THEORIES over a common floor without the
    /// shared infrastructure false-conflicting (`Composite` defined in both cores).
    pub fn own_over(&self, base: &Core) -> Core {
        let mut core = Core {
            types: Arc::new(self.types.own_over(&base.types)),
            processes: Arc::new(self.processes.own_over(&base.processes)),
            methods: Arc::new(self.methods.own_over(&base.methods)),
            protocols: Arc::new(self.protocols.own_over(&base.protocols)),
        };
        // Keep the Core invariant (addresses are typed) for any own protocol.
        core.register_protocol_address_types();
        core
    }

    /// **Link a whole resolved dependency set onto this base** — the n-ary join, the
    /// **colimit** over a dependency DAG (#67, Phase 2). `self` is the shared apex
    /// (the std/builtin floor every package is projected over); `parts` are the
    /// packages' OWN theories (each already [`own_over`](Core::own_over) this base,
    /// so the floor is contributed exactly ONCE and never false-conflicts). The
    /// result is `self ⊔ part₀ ⊔ part₁ ⊔ …`, folded through [`merge`](Core::merge).
    ///
    /// This is the categorical content of *transitive resolution = a colimit* made
    /// executable. Because `merge` is a commutative + associative + idempotent
    /// join-semilattice, the fold is **confluent**: any topological order of `parts`
    /// yields the same linked theory (laws in `tests/core_merge.rs`), and a **diamond**
    /// dependency — a shared transitive dep reached by two paths, Arc-identical after
    /// the resolver memoizes its compilation — is merged **once at the apex**, not
    /// double-unioned into a self-conflict. The resolver's job is upstream (pick one
    /// version per name so the apex is well-defined, memoize so a shared dep is the
    /// SAME `Arc`); composition is this one prism call — the linker is never cloned in
    /// chrysalis ([[feedback_chrysalis_thin_layer]]).
    ///
    /// Conflicts are **accumulated across the whole fold**, not short-circuited: a
    /// `part` that genuinely clashes (a same-name incompatible def the version solver
    /// failed to prevent) is recorded and skipped, and folding continues, so `Err`
    /// reports EVERY conflicting package at once (matching `merge`'s "show them all").
    /// `Ok` ⇒ the fully linked core (the Core invariant — typed addresses — held by
    /// each `merge` step).
    pub fn colimit(&self, parts: &[Core]) -> Result<Core, Vec<CoreMergeConflict>> {
        let mut linked = self.clone();
        let mut conflicts = Vec::new();
        for part in parts {
            match linked.merge(part) {
                Ok(next) => linked = next,
                Err(mut part_conflicts) => conflicts.append(&mut part_conflicts),
            }
        }
        if conflicts.is_empty() {
            Ok(linked)
        } else {
            Err(conflicts)
        }
    }
}

/// A same-name **incompatible** definition found by [`Core::merge`] — the linker
/// conflict the resolver / semver layer (#67) must prevent by picking one version
/// per package (the version is part of a package's identity). `registry` is which
/// of the four it lives in (`"type"` / `"process"` / `"method"`; protocols join by
/// name and never conflict); `name` is the colliding key (a type name, a process
/// class, or a `"Type.method"` slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMergeConflict {
    pub registry: &'static str,
    pub name: String,
}

impl std::fmt::Display for CoreMergeConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} `{}` is defined incompatibly in both cores",
            self.registry, self.name
        )
    }
}

impl std::error::Error for CoreMergeConflict {}

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
