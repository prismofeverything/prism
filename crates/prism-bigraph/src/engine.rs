//! The composition engine — steps processes and reconciles state.
//!
//! This is the Rust equivalent of process-bigraph's Composite.
//! It manages the shared state tree, schedules temporal processes
//! by their intervals, triggers dependency-based steps, and
//! applies updates with proper merge semantics.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;

use prism_schema::algebra;
use prism_schema::{Key, Path, Schema, Value};

use crate::core::Core;
use crate::factory::ProcessRegistry;
use crate::step_cache::StepCache;

/// Nest `leaf` at `path` inside `schema`, growing `Tree` branches as needed (an
/// empty `path` sets the whole node to `leaf`). Used by `apply_reconciled` to
/// reassemble a top-level store's schema from its per-port (possibly nested)
/// writer schemas, so a nested `Overwrite` output is honored.
fn insert_nested(schema: &mut Schema, path: &[Key], leaf: Schema) {
    if path.is_empty() {
        *schema = leaf;
        return;
    }
    if !matches!(schema, Schema::Tree { .. }) {
        *schema = Schema::Tree {
            branches: IndexMap::new(),
        };
    }
    if let Schema::Tree { branches } = schema {
        let child = branches
            .entry(path[0].clone())
            .or_insert_with(|| Schema::Tree {
                branches: IndexMap::new(),
            });
        insert_nested(child, &path[1..], leaf);
    }
}

/// The process class named by an address. A `local` address carries the class as
/// a bare string (`local:Cell` → `"Cell"`); a remote address (`rest`/`parallel`)
/// carries a map whose `process` field is the class (`{process: Cell, host, …}`).
/// Returning the class for BOTH is what lets discovery instantiate a remote node
/// found in state, not just a local one.
fn address_class(parsed: &crate::protocol::ParsedAddress) -> Option<String> {
    if let Some(s) = parsed.data.as_str() {
        return Some(s.to_string());
    }
    parsed
        .data
        .as_map()
        .and_then(|m| m.get("process"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Resolve wires for a process at a given path.
///
/// If the wire path starts with "..", resolve relative to the process's location
/// (each ".." pops one level). Otherwise, treat as absolute from the state root.
fn resolve_wires_from_process(wires: &Value, process_path: &[Key]) -> IndexMap<String, Vec<Key>> {
    // A wire resolves against a BASE. Two bases, distinguished by a leading
    // marker:
    //   - default → the process's CONTAINER (`parent`). A bare segment `["x"]`
    //     is a sibling slot; `[]` is the container itself; `[".."]` pops up.
    //     This is the place-graph-relative wiring composites have always used.
    //   - leading `"%"` → the process's OWN node (`own` = `process_path`). So
    //     `["%","mass"]` is the node's own `mass` field (e.g. `cells.N.mass`),
    //     the slot a composite exports its FACE onto so a parent can match it
    //     WITHOUT knowing the dynamic key `N`. The engine supplies the path; the
    //     process never hard-codes its own key (deployment-agnostic). This is the
    //     self-node wire — the seam division's exposed face + intent ride on.
    let parent: Vec<Key> = if !process_path.is_empty() {
        process_path[..process_path.len() - 1].to_vec()
    } else {
        vec![]
    };
    let own: Vec<Key> = process_path.to_vec();

    fn resolve_one(path_list: &[Value], parent: &[Key], own: &[Key]) -> Vec<Key> {
        let elems: Vec<Key> = path_list
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(Key::from(s.as_str())),
                Value::Int(i) => Some(Key::from(i.to_string())),
                Value::Float(f) => Some(Key::from(format!("{}", f.0 as i64))),
                _ => None,
            })
            .collect();

        // The self-node marker `%` rebases onto the process's own node; it is
        // consumed (not a path segment).
        let (base, rest): (&[Key], &[Key]) = match elems.first() {
            Some(first) if first.as_str() == "%" => (own, &elems[1..]),
            _ => (parent, &elems[..]),
        };
        let mut resolved = base.to_vec();
        for elem in rest {
            if elem.as_str() == ".." {
                resolved.pop();
            } else {
                resolved.push(elem.clone());
            }
        }
        resolved
    }

    fn flatten_nested(
        prefix: &str,
        target: &Value,
        parent: &[Key],
        own: &[Key],
        result: &mut IndexMap<String, Vec<Key>>,
    ) {
        match target {
            Value::List(list) => {
                result.insert(prefix.to_string(), resolve_one(list, parent, own));
            }
            Value::Map(map) => {
                // Nested wires: {"substrates": {"glucose": ["fields","glucose",5,5]}}
                // Flatten to "substrates.glucose" → resolved path
                for (key, sub_target) in map {
                    let sub_prefix = if prefix.is_empty() {
                        key.to_string()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    flatten_nested(&sub_prefix, sub_target, parent, own, result);
                }
            }
            _ => {}
        }
    }

    let mut result = IndexMap::new();
    if let Some(map) = wires.as_map() {
        for (port, target) in map {
            flatten_nested(port, target, &parent, &own, &mut result);
        }
    }
    result
}
use crate::ports::Interface;
use crate::process::ProcessNode;
use crate::topology::{ProcessSpec, Topology};

/// Scheduling state for a temporal process.
#[derive(Debug)]
struct ProcessFront {
    /// Time of this process's next event: when it is next due to invoke, and —
    /// once invoked — when its `pending` update applies (both `process_time +
    /// interval`).
    next_time: f64,
    interval: f64,
    /// Deferred update computed by invoke but not yet applied. The run loop invokes
    /// all due processes against the SAME snapshot, advances time, flushes batching
    /// protocols, then collects + applies these together — so processes in a tick
    /// never see each other's mid-tick mutations (invoke/apply separation, faithful
    /// to process-bigraph). A `Defer` is immediate for a local process and
    /// runtime-filled for a batching protocol (Ray/Pool). See docs/execution-model.md.
    pending: Option<crate::defer::Defer<crate::Update>>,
}

/// Default structural-explosion BACKSTOP (nodes per engine) — *not* a sim-size
/// limit. Set high so genuinely large sims never suffer (a 1000×1000 spatial grid
/// is 1e6 nodes); a runaway is *exponential* (→ ∞) so it trips any finite cap
/// fast regardless. It exists only to fail a non-terminating discovery/division
/// loud + fast instead of OOMing the machine. A debug/test that wants a tight
/// fast-fail lowers it via [`Engine::set_max_nodes`] (the grow/divide tests use
/// 64); a genuinely huge sim raises it. A runaway is a code bug to fix.
const DEFAULT_MAX_NODES: usize = 10_000_000;

/// A running composition of processes and shared state.
#[derive(Debug)]
pub struct Engine {
    /// The shared state tree.
    state: Value,

    /// Schema for the state tree.
    schema: Schema,

    /// Whether the last apply_projections had structural changes.
    last_structural: bool,

    /// Pending changed paths to trigger steps at start of next run.
    pending_changes: Vec<Path>,

    /// Bridge-out roots: a composite TAPS the reconciled inner UPDATE at these
    /// roots each tick (accumulated over inner ticks) and forwards it intact —
    /// the one way anything leaves a composite (identical to a `stream:` delta-
    /// frame). Set by `Composite` to all its output ports' inner roots. This
    /// replaced `diff(pre,post)`, which was lossy (dropped structural `_remove`)
    /// and redundant (the delta already existed).
    bridge_out_paths: HashSet<Key>,

    /// Of `bridge_out_paths`, the roots that are pure OUTWARD CONDUITS (no inner
    /// slot — e.g. a cell's division output): captured + forwarded but NOT applied
    /// to inner state (else daughters would nest inside the cell). An inner-slot
    /// output (e.g. `mass`) is captured + forwarded AND applied normally. Same
    /// forward rule; this only gates inner application.
    passthrough_paths: HashSet<Key>,

    /// Captured bridge-out deltas (the forwarded updates) from the last run.
    bridge_out_deltas: IndexMap<Key, Value>,

    /// Current simulation time.
    time: f64,

    /// Instantiated process/step nodes, keyed by name.
    nodes: HashMap<String, ProcessNode>,

    /// Structural-explosion guard: the most process nodes this engine will hold
    /// before `add_process` panics. A runaway (division that doesn't terminate —
    /// daughters not falling below threshold because mass wasn't conserved/halved)
    /// otherwise creates nodes without bound and OOMs the machine. This fails it
    /// FAST and loud instead — a code bug, not patience (see
    /// memory feedback_grow_divide_runaway). Per-engine, so it also bounds a
    /// composite's inner engine. Raise it via [`Engine::set_max_nodes`] for a
    /// genuinely large sim.
    max_nodes: usize,

    /// Wiring interfaces for each node.
    interfaces: HashMap<String, Interface>,

    /// Scheduling fronts for temporal processes.
    fronts: HashMap<String, ProcessFront>,

    /// Step dependency tracking: state path -> set of step names to trigger.
    step_triggers: HashMap<Path, HashSet<String>>,

    /// Steps already fired by the init `settle_steps` sweep, so each fires once
    /// at init no matter how many times discovery runs (`from_state` auto-discovers
    /// and callers may discover again) — i.e. settle is idempotent. Reactive
    /// re-firing during `run` is separate (`trigger_steps`, not gated by this).
    fired_init_steps: HashSet<String>,

    /// Specs retained for introspection.
    specs: HashMap<String, ProcessSpec>,

    /// The unified runtime core: type schemas (`Custom` dispatch), process/step
    /// factories (discovery), value-methods (chrysalis dispatch), and protocols
    /// (`local`/`rest`/…). Threaded as one object so a subengine (a `Composite`)
    /// inherits the WHOLE core — see [`Core`]. (Was four separate registry
    /// fields; `Composite::from_config` used to receive only the process one.)
    core: Core,

    /// Per-tick batching runtimes registered by protocols that need
    /// to coalesce per-process `invoke()` calls into one batched RPC
    /// (Ray, Pool). Flushed by [`Engine::flush_protocol_runtimes`].
    /// Empty for synchronous-only setups — flush is a no-op.
    protocol_runtimes: crate::protocol_runtime::ProtocolRuntimes,

    /// Optional incremental-step cache. When set, the step scheduler skips a step
    /// whose output is cached and fresh (reloading it instead of firing), and the
    /// step DAG cascades staleness downstream. `None` ⇒ every step always fires
    /// (the default; identical to pre-cache behaviour). See [`StepCache`].
    step_cache: Option<StepCache>,
}

impl Engine {
    /// Create a new engine from a topology with pre-built process instances.
    ///
    /// `factories` maps process type names to constructor functions.
    /// Each factory receives the process config and returns a ProcessNode.
    pub fn new(topology: Topology, instances: HashMap<String, ProcessNode>) -> Self {
        Self::new_with_cache(topology, instances, None)
    }

    /// Like [`Engine::new`] but with an incremental-step cache in place *before*
    /// the init settle, so the init step DAG itself is cached/skipped. Pass `None`
    /// for the default (every step fires).
    pub fn new_with_cache(
        topology: Topology,
        instances: HashMap<String, ProcessNode>,
        step_cache: Option<StepCache>,
    ) -> Self {
        let mut fronts = HashMap::new();
        let mut interfaces = HashMap::new();
        let mut step_triggers: HashMap<Path, HashSet<String>> = HashMap::new();
        let mut specs = HashMap::new();

        // Initialize state from topology
        let mut state = topology.initial_state.clone();

        // Set up each process/step
        for (name, spec) in &topology.processes {
            let mut interface = spec.interface();
            // Populate output schemas from the process/step node
            if let Some(node) = instances.get(name) {
                let declared_outputs = node.outputs();
                interface.output_schemas = declared_outputs;
            }
            interfaces.insert(name.clone(), interface);

            // Merge initial state from the process instance
            if let Some(node) = instances.get(name) {
                let init = node.initial_state();
                if !init.is_none() {
                    // Project initial state through output wiring
                    let iface = &interfaces[name];
                    for (path, val, _schema) in iface.project(&init) {
                        state.set_path(&path, val);
                    }
                }
            }

            if let Some(interval) = spec.interval {
                // Temporal process: schedule it
                fronts.insert(
                    name.clone(),
                    ProcessFront {
                        next_time: 0.0,
                        interval,
                        pending: None,
                    },
                );
                // Store interval in state tree so Steps can wire to it
                // (e.g. ManageBoundaries reads ["newtonian_particles", "interval"])
                state.set_path(
                    &[Key::from(name.as_str()), Key::from("interval")],
                    Value::float(interval),
                );
            } else {
                // Step: register triggers based on input wiring
                for path in spec.inputs.values() {
                    step_triggers
                        .entry(path.clone())
                        .or_default()
                        .insert(name.clone());
                }
            }

            specs.insert(name.clone(), spec.clone());
        }

        // Establish a real schema even when the topology declares only `Any`:
        // infer structure from the state, refined by whatever the topology
        // declared — the same rule as `from_state`, so no engine ever runs
        // schema-free (an `Any` root sends `reconcile`/`apply` down the opaque
        // last-wins path, which silently drops concurrent partial updates).
        let schema = algebra::resolve(&Schema::infer(&state), &topology.state_schema);
        let mut engine = Self {
            state,
            schema,
            time: 0.0,
            nodes: instances,
            max_nodes: DEFAULT_MAX_NODES,
            interfaces,
            fronts,
            step_triggers,
            fired_init_steps: HashSet::new(),
            specs,
            core: Core::new(),
            protocol_runtimes: crate::protocol_runtime::ProtocolRuntimes::new(),
            last_structural: false,
            pending_changes: Vec::new(),
            bridge_out_paths: HashSet::new(),
            passthrough_paths: HashSet::new(),
            bridge_out_deltas: IndexMap::new(),
            step_cache,
        };

        // Settle the step network on init: fire every step whose inputs are
        // present, cascading downstream as outputs appear (the same
        // `trigger_steps` mechanism the run loop uses after each process tick).
        engine.settle_steps();

        engine
    }

    /// Create an engine from a schema, state, and registry.
    ///
    /// This is the Rust equivalent of Python's `Composite({'schema': ..., 'state': ...})`.
    /// Walks the state tree, finds process/step nodes (via `Schema::Link` in the
    /// schema or `_type` annotations in state), instantiates them via the registry,
    /// extracts wiring, and builds the engine.
    pub fn from_state(schema: Schema, state: Value, core: impl Into<Core>) -> Result<Self, String> {
        Self::from_state_core(schema, state, core.into())
    }

    /// Like [`Engine::from_state`] but takes the process + protocol registries
    /// separately (folded into a [`Core`]). Use when protocols are built apart
    /// from a core; addresses for non-`local` protocols (`rest:`, `parallel:`)
    /// must be resolvable at construction so processes schedule in the first tick.
    pub fn from_state_with_protocols(
        schema: Schema,
        state: Value,
        registry: Arc<ProcessRegistry>,
        protocols: Arc<crate::protocol::ProtocolRegistry>,
    ) -> Result<Self, String> {
        Self::from_state_core(
            schema,
            state,
            Core::new()
                .with_processes(registry)
                .with_protocols(protocols),
        )
    }

    /// The shared `from_state` body: build + discover against a whole [`Core`].
    fn from_state_core(schema: Schema, state: Value, core: Core) -> Result<Self, String> {
        // Resolve the schema inferred from state (structure + `_type`
        // annotations) with the declared `schema`. The declared schema is the
        // refining side: it wins ties and contributes the apply-critical types
        // (`Array`/`Delta`/`Custom`/links) that inference can't recover, while
        // inference fills the branches the declaration leaves as `Any`. (This
        // replaces the old `Schema::infer_and_merge` — a hand-rolled resolve.)
        let merged_schema = algebra::resolve(&Schema::infer(&state), &schema);

        let mut topology = Topology::new();
        topology.state_schema = merged_schema.clone();

        // One discovery path: `discover_all_processes` → `scan_for_processes`
        // finds every process node (schema-`Link`-typed AND, for dynamic
        // entities inside a `Map`, address-marked). Same path
        // `Composite::from_config` uses.
        topology.initial_state = state;
        let mut engine = Engine::new(topology, HashMap::new());
        engine.set_core(core);
        // Register each protocol's batching runtime (e.g. the `parallel` pool) so
        // the engine flushes it between invoke and collect. Only the TOP-LEVEL
        // engine does this — a subengine (`Composite::from_config`) shares the same
        // pool and its slot-`Defer`s already block on `.get()`, so a nested flush
        // would only over-synchronize (and could deadlock a fixed pool under nested
        // `parallel:`). One barrier, at the top.
        engine.register_core_protocol_runtimes();
        // Reject a document that references a process the core can't build — a
        // missing reference is an error, not a silently-dropped node.
        engine.check_references()?;
        // Self-contained: instantiate processes + settle the step network now,
        // so `from_state` returns a ready engine like a freshly-built Composite.
        // Idempotent — callers that also call `discover_all_processes` are no-ops.
        engine.discover_all_processes();
        Ok(engine)
    }

    /// Merge new schema and state into a running engine.
    ///
    /// This is the Rust equivalent of Python's `Composite.merge()`.
    /// The schema is mutable state — merging can add new Link nodes
    /// (which triggers process instantiation), change types (which
    /// affects how apply_update works), or restructure the tree.
    ///
    /// Steps:
    /// 1. Merge the new schema into the existing schema
    /// 2. Merge the new state into the existing state (using the merged schema)
    /// 3. Find any new Link nodes and instantiate their processes
    pub fn merge_schema(&mut self, schema_update: Schema, state_update: Value) {
        // 1. Merge schemas — the new declaration refines the existing one
        // (join / set-union of branches, more-specific subtype wins).
        self.schema = algebra::resolve(&self.schema, &schema_update);

        // 2. Apply state update using merged schema
        if !state_update.is_none() {
            let new_state = algebra::apply_with(
                Some(self.core.types.as_ref()),
                &self.schema,
                &self.state,
                &state_update,
            );
            self.state = new_state;
        }

        // 3. Fill defaults for new schema branches not present in state
        if let Schema::Tree { branches } = &self.schema {
            if let Some(state_map) = self.state.as_map_mut() {
                for (key, child_schema) in branches {
                    if !state_map.contains_key(key) {
                        // New schema branch — fill with default
                        let default_val =
                            algebra::default_with(Some(self.core.types.as_ref()), child_schema);
                        state_map.insert(key.clone(), default_val);
                    }
                }
            }
        }

        // 4. Discover and instantiate any new process nodes the merged schema
        // (and state) introduced. The same single-pass discovery the engine uses
        // at init — schema-`Link`-typed AND address-marked — so a merged Link
        // node lands as a registered process and step network settles.
        self.discover_all_processes();
    }

    /// Set the process registry for dynamic process discovery.
    /// When set, new process nodes appearing in state (e.g., via _add)
    /// are automatically instantiated and wired.
    /// Number of temporal processes.
    pub fn fronts_count(&self) -> usize {
        self.fronts.len()
    }

    /// Queue pending state changes to trigger steps at the start of next run.
    pub fn queue_changes(&mut self, paths: Vec<Path>) {
        self.pending_changes.extend(paths);
    }

    /// Set the bridge-out roots a composite TAPS + forwards each tick (the inner
    /// reconciled update at these roots is captured intact). `conduits` ⊆ these
    /// are the pure outward conduits (no inner slot) that must NOT be applied to
    /// inner state; the rest are captured AND applied normally.
    pub fn set_bridge_out_paths(&mut self, capture: HashSet<Key>, conduits: HashSet<Key>) {
        self.bridge_out_paths = capture;
        self.passthrough_paths = conduits;
    }

    /// Take the bridge-out deltas (forwarded updates) captured during the last run.
    pub fn take_bridge_out_deltas(&mut self) -> IndexMap<Key, Value> {
        std::mem::take(&mut self.bridge_out_deltas)
    }

    /// Public wrapper for trigger_steps.
    pub fn trigger_steps_pub(&mut self, changed_paths: &[Path]) {
        self.trigger_steps(changed_paths);
    }

    pub fn set_registry(&mut self, registry: Arc<ProcessRegistry>) {
        self.core.processes = registry;
    }

    /// Replace the whole runtime [`Core`] (types + processes + methods +
    /// protocols). A subengine built from a parent's core inherits all of them.
    pub fn set_core(&mut self, core: Core) {
        self.core = core;
    }

    /// Borrow the runtime [`Core`].
    pub fn core(&self) -> &Core {
        &self.core
    }

    /// Verify every process reference in the current state resolves against the
    /// core. `Err` lists any `local:` classes the core can't build — so a document
    /// referencing a missing process is rejected up front, not silently run as a
    /// partial graph. See [`Core::missing_process_refs`].
    pub fn check_references(&self) -> Result<(), String> {
        let missing = self.core.missing_process_refs(&self.state);
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "document references {} unregistered process(es): {}",
                missing.len(),
                missing.join(", ")
            ))
        }
    }

    /// Attach an incremental-step [`StepCache`]. Set this *before* steps fire (the
    /// init settle / `discover_all_processes`) for the workflow's steps to be
    /// cached/skipped. `None`-equivalent behaviour is the default when unset.
    pub fn set_step_cache(&mut self, cache: StepCache) {
        self.step_cache = Some(cache);
    }

    /// Attach a `TypeRegistry` for `Schema::Custom` dispatch (RT.4).
    /// When set, the engine's merge logic consults this registry's
    /// `TypeMethods` for paths with custom-typed schema; otherwise
    /// the existing structural Schema semantics apply.
    pub fn set_type_registry(&mut self, registry: Arc<prism_schema::registry::TypeRegistry>) {
        self.core.types = registry;
    }

    /// Borrow the type registry (always present in the core; may be empty).
    pub fn type_registry(&self) -> Option<&Arc<prism_schema::registry::TypeRegistry>> {
        Some(&self.core.types)
    }

    /// Attach a `MethodRegistry` for value-receiver method dispatch.
    /// Used by chrysalis to look up methods on values inside
    /// expression bodies.
    pub fn set_method_registry(&mut self, registry: Arc<prism_schema::MethodRegistry>) {
        self.core.methods = registry;
    }

    /// Borrow the method registry (always present in the core; may be empty).
    pub fn method_registry(&self) -> Option<&Arc<prism_schema::MethodRegistry>> {
        Some(&self.core.methods)
    }

    /// Register every batching runtime exposed by the core's protocols
    /// ([`crate::protocol::ProtocolRegistry::runtimes`]) — the auto-wired form of
    /// [`Engine::register_protocol_runtime`]. Idempotent per registration; call
    /// once after `set_core` on the top-level engine.
    pub fn register_core_protocol_runtimes(&mut self) {
        for rt in self.core.protocols.runtimes() {
            self.protocol_runtimes.register(rt);
        }
    }

    /// Register a protocol-level batching runtime. The engine calls
    /// [`crate::protocol_runtime::ProtocolRuntime::flush_pending`] on
    /// each registered runtime during the orchestrator's flush phase
    /// (between the invoke pass and apply_updates). Sync-only setups
    /// don't need to register anything — flush is a no-op then.
    pub fn register_protocol_runtime(
        &mut self,
        runtime: Arc<dyn crate::protocol_runtime::ProtocolRuntime>,
    ) {
        self.protocol_runtimes.register(runtime);
    }

    /// Flush every registered protocol runtime — called by the run loop
    /// (`advance_to_next_event`) between the invoke pass and the collect
    /// phase, so a batching runtime dispatches its enqueued invokes here.
    /// A no-op when no batching protocols are registered (local Defers are
    /// immediate). See docs/execution-model.md.
    pub fn flush_protocol_runtimes(&self) {
        self.protocol_runtimes.flush_all();
    }

    /// Number of registered protocol runtimes — diagnostic.
    pub fn protocol_runtime_count(&self) -> usize {
        self.protocol_runtimes.len()
    }

    /// Borrow the protocol registry. Always present — defaults to a
    /// registry containing only the `local` protocol if none was set
    /// explicitly.
    pub fn protocol_registry(&self) -> &Arc<crate::protocol::ProtocolRegistry> {
        &self.core.protocols
    }

    /// Replace the protocol registry. Use when registering additional
    /// protocols (parallel, rest, ray, …).
    pub fn set_protocol_registry(&mut self, registry: Arc<crate::protocol::ProtocolRegistry>) {
        self.core.protocols = registry;
    }

    /// Register an additional protocol with the active registry.
    /// Convenience over `set_protocol_registry`. Creates a new
    /// registry under the hood, copying existing entries.
    pub fn register_protocol(&mut self, protocol: Arc<dyn crate::protocol::Protocol>) {
        let mut new_registry = crate::protocol::ProtocolRegistry::new();
        // Re-register any protocols already present (apart from default local).
        for name in self.core.protocols.names() {
            if let Some(existing) = self.core.protocols.get(name) {
                new_registry.register(Arc::clone(existing));
            }
        }
        new_registry.register(protocol);
        self.core.protocols = Arc::new(new_registry);
    }

    /// Current simulation time.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Read-only access to the state tree.
    pub fn state(&self) -> &Value {
        &self.state
    }

    /// Read-only access to the engine's state schema (declared at
    /// `Topology::state_schema` construction time). Useful for
    /// introspection / serialization.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Borrow the registered process/step specs by name. Used by
    /// renderers + introspection to discover wiring without needing
    /// to reconstruct the original Topology.
    pub fn specs(&self) -> &HashMap<String, ProcessSpec> {
        &self.specs
    }

    /// Build a snapshot `Topology` from the engine's current state
    /// (state_schema, current state, registered process specs). Useful
    /// for renderers that want the full place-graph + wiring picture.
    pub fn snapshot_topology(&self) -> Topology {
        let mut processes: indexmap::IndexMap<String, ProcessSpec> = indexmap::IndexMap::new();
        for (name, spec) in &self.specs {
            processes.insert(name.clone(), spec.clone());
        }
        Topology {
            state_schema: self.schema.clone(),
            initial_state: self.state.clone(),
            processes,
        }
    }

    /// Mutable access to the state tree (for composites bridging inputs).
    pub fn state_mut(&mut self) -> &mut Value {
        &mut self.state
    }

    /// Read a value at a specific path in the state tree.
    pub fn get(&self, path: &[Key]) -> Option<&Value> {
        self.state.get_path(path)
    }

    /// Run the simulation for the given duration.
    pub fn run(&mut self, duration: f64) {
        // Process pending changes from composite bridge inputs.
        if !self.pending_changes.is_empty() {
            let pending = std::mem::take(&mut self.pending_changes);
            self.trigger_steps(&pending);
        }

        let end_time = self.time + duration;
        let mut iter_count = 0u64;
        while self.time < end_time {
            let before = self.time;
            self.advance_to_next_event(end_time);
            iter_count += 1;
            if iter_count > 100_000 {
                eprintln!(
                    "[engine] SAFETY: breaking after {iter_count} iterations at t={}",
                    self.time
                );
                self.time = end_time;
                break;
            }
            if self.time <= before {
                // No event advanced time (e.g. only steps remain) — done.
                self.time = end_time;
                break;
            }
        }
    }

    /// Run a single tick: advance to the next process event and apply it.
    /// Returns the time advanced to, or `None` if no events are pending.
    pub fn tick(&mut self) -> Option<f64> {
        let before = self.time;
        self.advance_to_next_event(f64::INFINITY);
        (self.time > before).then_some(self.time)
    }

    /// One simulation step, faithful to process-bigraph's run loop —
    /// **invoke → advance → flush → collect/apply → trigger/discover**:
    ///
    /// 1. **invoke** every *due* process (`next_time <= time`) against the
    ///    current state, stashing its update in the front and advancing that
    ///    front to `process_time + interval`. Nothing is applied yet, so every
    ///    process this step reads the SAME snapshot — one process can never see
    ///    another's mid-step mutation (the core correctness property).
    /// 2. **advance** `time` by `full_step` — the smallest interval to any
    ///    process's next event.
    /// 3. **apply** the stashed updates whose time has now come, together.
    /// 4. **trigger** steps on the applied changes and **discover** any processes
    ///    the structural changes created. Because this happens AFTER the advance,
    ///    new processes are stamped at the new `time` and run on the NEXT step —
    ///    never re-run in the step that created them (what `+interval` was faking).
    fn advance_to_next_event(&mut self, end_time: f64) {
        let names: Vec<String> = self.fronts.keys().cloned().collect();

        // 1. invoke pass — all reads against the pre-step snapshot.
        let mut full_step = f64::INFINITY;
        for name in &names {
            let (process_time, front_interval) = match self.fronts.get(name) {
                Some(f) => (f.next_time, f.interval),
                None => continue,
            };
            if process_time <= self.time {
                // `interval` is an ordinary wired input — a process runs its
                // CURRENT local interval, read fresh each tick from the resolved
                // `interval` input (wired by default to its own slot, overridable
                // to a shared clock). Falls back to the stored `[name, "interval"]`
                // slot, then the front's construction value — so a step can drive
                // the timestep dynamically (e.g. a Gillespie τ).
                let input = match self.interfaces.get(name) {
                    Some(iface) => iface.view(&self.state),
                    None => continue,
                };
                let interval = input
                    .get_field("interval")
                    .and_then(Value::as_f64)
                    .or_else(|| {
                        self.state
                            .get_path(&[Key::from(name.as_str()), Key::from("interval")])
                            .and_then(Value::as_f64)
                    })
                    .filter(|dt| dt.is_finite() && *dt > 0.0)
                    .unwrap_or(front_interval);
                let future = process_time + interval;
                full_step = full_step.min(future - self.time);
                if future <= end_time {
                    // invoke (not update): the result is a Defer — immediate for a
                    // local process, runtime-filled for a batching protocol. It is
                    // resolved in the collect/apply pass, after the flush below.
                    let defer = match self.nodes.get(name) {
                        Some(ProcessNode::Process(p)) => p.invoke(&input, interval),
                        _ => continue,
                    };
                    let front = self.fronts.get_mut(name).expect("front exists");
                    front.next_time = future;
                    front.pending = Some(defer);
                }
            } else {
                // Not due yet, but its event bounds how far time may move.
                full_step = full_step.min(process_time - self.time);
            }
        }

        // No process event within reach — jump to the end (steps already settled).
        if !full_step.is_finite() || self.time + full_step > end_time {
            self.time = end_time;
            return;
        }

        // 2. advance global time.
        self.time += full_step;

        // flush pass — batching protocol runtimes (Ray/Pool/parallel pool) resolve
        // their enqueued invokes here, before we collect; local Defers are immediate.
        self.flush_protocol_runtimes();

        // 3. collect + apply pass — resolve every stashed Defer now due and apply
        //    them as ONE reconciled batch, so updates targeting the same store
        //    combine (deltas sum, _add/_remove batch) instead of clobbering.
        let mut projection_sets: Vec<Vec<(Path, Value, Option<Schema>)>> = Vec::new();
        for name in &names {
            let ready = self
                .fronts
                .get(name)
                .is_some_and(|f| f.next_time <= self.time && f.pending.is_some());
            if !ready {
                continue;
            }
            let defer = self.fronts.get_mut(name).unwrap().pending.take().unwrap();
            // Collect: resolve the deferred update. A Noop carries no value → skip.
            let pending = match defer.get().into_value() {
                Some(v) => v,
                None => continue,
            };
            let projections = match self.interfaces.get(name) {
                Some(iface) => iface.project(&pending),
                None => continue,
            };
            projection_sets.push(projections);
        }
        let (all_changed, any_structural) = self.apply_reconciled(&projection_sets);

        // 4. trigger steps + discover newly-created processes.
        let step_changes = if self.step_triggers.is_empty() {
            Vec::new()
        } else {
            self.trigger_steps(&all_changed)
        };
        if any_structural || !step_changes.is_empty() {
            let mut discover_paths = all_changed;
            discover_paths.extend(step_changes);
            self.discover_processes(&discover_paths);
        }
    }

    /// Reconcile a tick's batch of state-shaped updates into one and apply it —
    /// THE single apply path for a timestep. Processes (all due this tick) and
    /// steps (each layer) funnel their updates through here, so updates targeting
    /// the same store are *combined* by `reconcile` (deltas sum, `_add`/`_remove`
    /// batch, overwrite last-wins) rather than clobbering one another, and the
    /// whole batch lands atomically. Returns (changed paths, had structural).
    fn apply_reconciled(
        &mut self,
        projection_sets: &[Vec<(Path, Value, Option<Schema>)>],
    ) -> (Vec<Path>, bool) {
        // Each projected output becomes one single-path fragment; `reconcile`
        // combines them all (overlapping parent/child writes compose schema-aware
        // — no hand-rolled merge). Remember each writer's port schema per slot so
        // the promote-to-additive (e.g. an `Array`/`Delta` field) survives.
        let mut fragments: Vec<Value> = Vec::new();
        let mut port_schema: HashMap<Path, Schema> = HashMap::new();
        for projs in projection_sets {
            for (path, value, sch) in projs {
                let mut fragment = Value::map();
                fragment.set_path(path, value.clone());
                fragments.push(fragment);
                if let Some(s) = sch {
                    port_schema.entry(path.clone()).or_insert_with(|| s.clone());
                }
            }
        }
        if fragments.is_empty() {
            return (Vec::new(), false);
        }
        let combined =
            match algebra::reconcile_with(Some(self.core.types.as_ref()), &self.schema, &fragments)
            {
                Some(c) => c,
                None => return (Vec::new(), false),
            };
        // Apply the combined update one top-level store at a time (each subtree
        // carries its own nested `_add`/`_remove`), carrying the writer's port
        // schema so `apply` promotes the slot's additive type (law #5).
        // Reassemble each top-level store's schema by NESTING every writer's port
        // schema at its full path — so a nested `Overwrite` (e.g.
        // `[event, "interval"]`, a Gillespie τ) is honored, not just top-level
        // slots. (Previously the schema was looked up by the top-level key alone,
        // silently dropping nested ones → nested overwrites fell back to additive.)
        let mut store_schemas: HashMap<Key, Schema> = HashMap::new();
        for (full_path, leaf) in &port_schema {
            let Some(top) = full_path.first() else {
                continue;
            };
            let entry = store_schemas
                .entry(top.clone())
                .or_insert_with(|| Schema::Tree {
                    branches: IndexMap::new(),
                });
            insert_nested(entry, &full_path[1..], leaf.clone());
        }
        let projections: Vec<(Path, Value, Option<Schema>)> = match combined.as_map() {
            Some(m) => m
                .iter()
                .map(|(k, v)| (vec![k.clone()], v.clone(), store_schemas.get(k).cloned()))
                .collect(),
            None => return (Vec::new(), false),
        };
        self.apply_projections(&projections)
    }

    /// Apply projected updates to the state tree.
    /// Returns (changed_paths, had_structural_change).
    /// Structural changes are `_add`/`_remove` operations that may require
    /// process discovery. Skipping discovery on non-structural ticks is a
    /// major performance win.
    fn apply_projections(
        &mut self,
        projections: &[(Path, Value, Option<Schema>)],
    ) -> (Vec<Path>, bool) {
        if self.bridge_out_paths.is_empty() {
            let result = apply_projections_to(
                &mut self.state,
                &self.schema,
                projections,
                Some(self.core.types.as_ref()),
            );
            self.last_structural = result.1;
            return result;
        }

        // The ONE way out of a composite: TAP the reconciled inner update at each
        // bridge-out root (accumulate over inner ticks) and forward it intact —
        // no diff. A bridge-out root is ALSO applied to inner state normally,
        // UNLESS it is a pure conduit (`passthrough_paths`, no inner slot — e.g.
        // the division output), which must not land inner (else daughters nest).
        let mut normal = Vec::new();
        for proj in projections {
            let root = proj.0.first().map(|k| k.as_str()).unwrap_or("");
            if self.bridge_out_paths.contains(root) {
                let root_key = Key::from(root);
                let mut delta = proj.1.clone();
                if proj.0.len() > 1 {
                    for key in proj.0[1..].iter().rev() {
                        delta = Value::tree([(key.as_str(), delta)]);
                    }
                }
                if let Some(existing) = self.bridge_out_deltas.get(&root_key) {
                    // Accumulate concurrent / multi-inner-tick deltas at this root
                    // via `reconcile` (the update monoid): union structural
                    // `_add`/`_remove`, fold value deltas. A conduit root is absent
                    // from the inner schema, so fall back to a dynamic `Map` schema
                    // — enough for reconcile to union the structural sentinels.
                    let root_schema = self.schema.schema_at_path(std::slice::from_ref(&root_key));
                    let map_any;
                    let s = if matches!(root_schema, Schema::Any) {
                        map_any = Schema::map(Schema::Any);
                        &map_any
                    } else {
                        root_schema
                    };
                    delta = algebra::reconcile_with(
                        Some(self.core.types.as_ref()),
                        s,
                        &[existing.clone(), delta.clone()],
                    )
                    .unwrap_or(delta);
                }
                self.bridge_out_deltas.insert(root_key, delta);
            }
            // Apply to inner state unless this root is a pure conduit.
            if !self.passthrough_paths.contains(root) {
                normal.push(proj.clone());
            }
        }

        let (mut changed, structural) = apply_projections_to(
            &mut self.state,
            &self.schema,
            &normal,
            Some(self.core.types.as_ref()),
        );
        // A conduit forwards a (possibly structural) delta outward without
        // applying it inner — mark the tick structural so the PARENT re-discovers
        // (e.g. division → new cell nodes), and surface the conduit roots as
        // changed so the forwarded delta reaches the parent.
        let conduit_delta = self
            .passthrough_paths
            .iter()
            .any(|r| self.bridge_out_deltas.contains_key(r));
        self.last_structural = structural || conduit_delta;
        for root in self.passthrough_paths.iter() {
            changed.push(vec![root.clone()]);
        }
        (changed, structural || conduit_delta)
    }

    /// Fire any steps whose inputs overlap with the changed paths.
    fn trigger_steps(&mut self, changed_paths: &[Path]) -> Vec<Path> {
        let (changes, _deltas) = self.trigger_steps_impl(changed_paths);
        changes
    }

    /// Fire steps and return both changed paths AND raw (path, delta) projections.
    fn trigger_steps_impl(&mut self, changed_paths: &[Path]) -> (Vec<Path>, Vec<(Path, Value)>) {
        // The steps reachable from `changed_paths` through the output→input
        // dependency graph (a triggered step's output may trigger another step),
        // then run them once each in dependency layers.
        let mut set: HashSet<String> = HashSet::new();
        let mut frontier = self.steps_triggered_by(changed_paths);
        while !frontier.is_empty() {
            let mut next: Vec<String> = Vec::new();
            for s in frontier {
                if set.insert(s.clone()) {
                    let out_paths: Vec<Path> = self
                        .specs
                        .get(&s)
                        .map(|spec| spec.outputs.values().cloned().collect())
                        .unwrap_or_default();
                    next.extend(self.steps_triggered_by(&out_paths));
                }
            }
            frontier = next;
        }
        let steps: Vec<String> = set.into_iter().collect();
        self.run_step_layers(steps)
    }

    /// Run a set of steps once each, in dependency *layers*: a step that consumes
    /// another's output runs in a later layer than its producer (process-bigraph's
    /// `wire_step_layers`). Each layer invokes against ONE snapshot and applies its
    /// updates *reconciled* together (`apply_reconciled`), so a consumer sees its
    /// producer's output via layer ordering (not a mid-tick leak), while
    /// independent same-layer steps neither see nor clobber each other. The single
    /// step-run path for settle (init) and trigger (reactive).
    fn run_step_layers(&mut self, steps: Vec<String>) -> (Vec<Path>, Vec<(Path, Value)>) {
        // Map each output path to its producing step (among `steps`).
        let mut producer: HashMap<Path, String> = HashMap::new();
        for name in &steps {
            if let Some(spec) = self.specs.get(name) {
                for out in spec.outputs.values() {
                    producer.insert(out.clone(), name.clone());
                }
            }
        }
        // deps[B] = the steps (among `steps`) that produce one of B's inputs.
        let deps: HashMap<String, HashSet<String>> = steps
            .iter()
            .map(|name| {
                let d: HashSet<String> = self
                    .specs
                    .get(name)
                    .map(|spec| {
                        spec.inputs
                            .values()
                            .filter_map(|inp| producer.get(inp))
                            .filter(|&p| p != name)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                (name.clone(), d)
            })
            .collect();

        let mut remaining: Vec<String> = steps;
        let mut done: HashSet<String> = HashSet::new();
        let mut all_changed: Vec<Path> = Vec::new();
        let mut all_deltas: Vec<(Path, Value)> = Vec::new();
        let mut any_structural = false;
        // Steps that recomputed this run (forced, uncached/stale, or downstream of
        // a stale step) — drives the cascade to dependents in later layers.
        let mut stale_set: HashSet<String> = HashSet::new();

        while !remaining.is_empty() {
            // Next layer: every remaining step whose producers have all run.
            let mut layer: Vec<String> = remaining
                .iter()
                .filter(|s| {
                    deps.get(s.as_str())
                        .map_or(true, |d| d.iter().all(|p| done.contains(p)))
                })
                .cloned()
                .collect();
            // A dependency cycle leaves nothing ready — break it by running the
            // remainder in one batch (a cycle has no valid layering anyway).
            if layer.is_empty() {
                layer = std::mem::take(&mut remaining);
            }

            // Per step: FRESH ⇒ reload its cached output; STALE ⇒ fire it. STALE =
            // forced, not cached-fresh, or downstream of a stale step. Both feed one
            // combined projection set for the layer's reconciled apply, so a skipped
            // step's cached output still reaches its consumers. With no cache
            // attached every step is STALE ⇒ identical to firing each.
            let mut projection_sets: Vec<Vec<(Path, Value, Option<Schema>)>> = Vec::new();
            let mut to_store: Vec<(String, Value)> = Vec::new();
            for name in &layer {
                let upstream_stale = deps
                    .get(name.as_str())
                    .map_or(false, |d| d.iter().any(|p| stale_set.contains(p)));
                let stale = match self.step_cache.as_ref() {
                    None => true,
                    Some(c) => c.is_forced(name) || !c.is_fresh(name) || upstream_stale,
                };
                // FRESH reloads the cached output; a missing/unreadable entry falls
                // back to firing.
                let cached = if stale {
                    None
                } else {
                    self.step_cache.as_ref().and_then(|c| c.load(name))
                };
                match cached {
                    Some(value) => {
                        if let Some(iface) = self.interfaces.get(name) {
                            let projections = iface.project(&value);
                            for (path, val, _) in &projections {
                                all_deltas.push((path.clone(), val.clone()));
                            }
                            projection_sets.push(projections);
                        }
                    }
                    None => {
                        stale_set.insert(name.clone());
                        if let Some((value, projections)) = self.invoke_one_step(name) {
                            for (path, val, _) in &projections {
                                all_deltas.push((path.clone(), val.clone()));
                            }
                            if self.step_cache.is_some() {
                                to_store.push((name.clone(), value));
                            }
                            projection_sets.push(projections);
                        }
                    }
                }
            }

            let (changed, structural) = self.apply_reconciled(&projection_sets);
            any_structural |= structural;
            all_changed.extend(changed);

            // Persist recomputed outputs AFTER they have been applied.
            if let Some(cache) = self.step_cache.as_ref() {
                for (name, value) in &to_store {
                    cache.store(name, value);
                }
            }

            for s in &layer {
                done.insert(s.clone());
            }
            remaining.retain(|s| !done.contains(s));
        }

        if any_structural && !all_changed.is_empty() {
            self.discover_processes(&all_changed);
        }
        (all_changed, all_deltas)
    }

    /// Steps triggered by any of `paths` (or a path prefix), priority-ordered and
    /// deduped. (Order is moot within a reconciled wave, but kept deterministic.)
    fn steps_triggered_by(&self, paths: &[Path]) -> Vec<String> {
        let mut steps: Vec<String> = Vec::new();
        for path in paths {
            if let Some(s) = self.step_triggers.get(path) {
                steps.extend(s.iter().cloned());
            }
            for i in 1..path.len() {
                if let Some(s) = self.step_triggers.get(&path[..i].to_vec()) {
                    steps.extend(s.iter().cloned());
                }
            }
        }
        steps.sort();
        steps.dedup();
        steps.sort_by(|a, b| {
            let pa = self.specs.get(a).map(|s| s.priority).unwrap_or(0.0);
            let pb = self.specs.get(b).map(|s| s.priority).unwrap_or(0.0);
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
        });
        steps
    }

    /// Invoke ONE step against the CURRENT state snapshot (no mutation), returning
    /// its raw output value (for caching) and the projected state update (ready for
    /// `reconcile`). The per-step invoke primitive used by `run_step_layers`, which
    /// decides FRESH (reload from cache) vs STALE (call this) for each step.
    fn invoke_one_step(&self, name: &str) -> Option<(Value, Vec<(Path, Value, Option<Schema>)>)> {
        let iface = self.interfaces.get(name)?;
        let input_state = iface.view(&self.state);
        let value = match self.nodes.get(name)? {
            ProcessNode::Step(s) => s.update(&input_state).into_value()?,
            _ => return None,
        };
        let projections = iface.project(&value);
        Some((value, projections))
    }

    /// Set the structural-explosion cap (nodes before `add_process` panics).
    pub fn set_max_nodes(&mut self, n: usize) {
        self.max_nodes = n;
    }

    /// Dynamically add a process to the running engine.
    pub fn add_process(&mut self, name: String, spec: ProcessSpec, mut node: ProcessNode) {
        // Structural-explosion guard. Runaway division (daughters that don't fall
        // below threshold — mass not conserved/halved — so they re-divide
        // unbounded) otherwise creates nodes without end and OOMs the machine.
        // Fail FAST and loud here instead: this is a code bug (division must
        // TERMINATE), not patience. (memory feedback_grow_divide_runaway.)
        if self.nodes.len() >= self.max_nodes {
            panic!(
                "structural explosion: engine reached {} process nodes (cap {}) while adding {name:?} — \
                 runaway discovery/division that is not terminating. Fix the non-termination \
                 (conserve + halve mass so daughters drop below threshold; remove the divided mother); \
                 do not just raise the cap.",
                self.nodes.len(),
                self.max_nodes
            );
        }
        let mut interface = spec.interface();
        interface.output_schemas = node.outputs();
        // The instance's REAL interface, captured before `node` is moved — used
        // below to thread its Link into the state schema (filling `Any` holes).
        let link = Self::link_from_instance(&node);
        self.interfaces.insert(name.clone(), interface);

        if let Some(interval) = spec.interval {
            // A process's first event is at its creation time, `self.time`:
            //  - at init (t=0) it's invoked in the first step;
            //  - mid-run, the run loop creates processes only AFTER advancing time
            //    and applying updates (invoke→advance→apply→discover), so
            //    `self.time` here is already the post-advance time — the new
            //    process is invoked on the NEXT step, never re-run in the step that
            //    created it. No `+interval` deferral, matching process-bigraph's
            //    `empty_front(global_time)`.
            self.fronts.insert(
                name.clone(),
                ProcessFront {
                    next_time: self.time,
                    interval,
                    pending: None,
                },
            );
        } else {
            for path in spec.inputs.values() {
                self.step_triggers
                    .entry(path.clone())
                    .or_default()
                    .insert(name.clone());
            }
        }

        self.specs.insert(name.clone(), spec);
        // Hand the engine's registry to nodes that compose other processes
        // (e.g. `RunProcess` instantiates the process it wraps). The engine
        // owns the registry; every node it builds shares it.
        {
            let registry = &self.core.processes;
            match &mut node {
                ProcessNode::Process(p) => {
                    p.set_registry(Arc::clone(registry));
                    p.set_core(self.core.clone());
                }
                ProcessNode::Step(s) => {
                    s.set_registry(Arc::clone(registry));
                    s.set_core(self.core.clone());
                }
            }
        }
        // Thread the instance's real Link into the state schema: at a process
        // node reached through `Tree`s, replace the inferred spec-`Tree` (or fill
        // an `Any`) with the instance's ProcessLink/StepLink — so downstream
        // apply/diff/promote are type-aware, not stuck on the raw
        // `{address, inputs, outputs}` shape `infer` saw. Never clobbers a
        // declared Link/CompositeLink and skips Map-nested nodes (their value
        // schema governs the type). The instance is the source of truth for a
        // process's interface — the schema-as-state reconcile. See
        // docs/state-schema-unification.md.
        self.stamp_instance_link(&name, &link);
        self.nodes.insert(name, node);
    }

    /// Build the schema `Link` describing a process node from its instance — the
    /// authoritative interface (`inputs()`/`outputs()` + `interval()`/
    /// `priority()`). A leaf process → `ProcessLink`; a step → `StepLink`.
    /// (Composites already carry a declared `CompositeLink`, so they never reach
    /// the stamping path — see [`Engine::stamp_instance_link`].)
    fn link_from_instance(node: &ProcessNode) -> Schema {
        let keyed = |m: crate::ports::PortSchema| -> IndexMap<Key, Schema> {
            m.into_iter().map(|(k, v)| (Key::from(k), v)).collect()
        };
        match node {
            ProcessNode::Process(p) => Schema::ProcessLink {
                inputs: keyed(p.inputs()),
                outputs: keyed(p.outputs()),
                interval: p.interval(),
            },
            ProcessNode::Step(s) => Schema::StepLink {
                inputs: keyed(s.inputs()),
                outputs: keyed(s.outputs()),
                priority: s.priority(),
            },
        }
    }

    /// Thread `link` (the instance's real interface) into the state schema at
    /// `name`'s path, via the schema algebra (`resolve`, which prefers the more
    /// specific update — so the `Link` replaces an inferred spec-`Tree` or fills
    /// an `Any`). No-op unless [`Engine::is_stampable_node_path`] holds, so a
    /// declared `Link`/`CompositeLink` is never clobbered and a `Map`-nested node
    /// (governed by its value schema) is left alone.
    fn stamp_instance_link(&mut self, name: &str, link: &Schema) {
        let path: Vec<Key> = name.split('.').map(Key::from).collect();
        if !Self::is_stampable_node_path(&self.schema, &path) {
            return;
        }
        let update = Self::nest_schema(&path, link.clone());
        self.schema = algebra::resolve(&self.schema, &update);
    }

    /// A nested `Tree` placing `leaf` at `path` (`[a,b] → Tree{a: Tree{b: leaf}}`).
    fn nest_schema(path: &[Key], leaf: Schema) -> Schema {
        match path.split_first() {
            None => leaf,
            Some((head, rest)) => {
                let mut branches: IndexMap<Key, Schema> = IndexMap::new();
                branches.insert(head.clone(), Self::nest_schema(rest, leaf));
                Schema::Tree { branches }
            }
        }
    }

    /// Whether `name`'s path is a process node the engine may (re)type from its
    /// instance: the path reaches its slot through `Tree`s only, and that slot is
    /// either an `Any` hole or an *inferred* plain spec-`Tree`
    /// (`{address, inputs, outputs}`, which lost the port types). `false` if the
    /// slot is an already-declared `Link`/`CompositeLink` (chrysalis typed it) or
    /// any ancestor is a `Map`/`Array`/`Link`/leaf (governed by its value schema).
    fn is_stampable_node_path(schema: &Schema, path: &[Key]) -> bool {
        match path.split_first() {
            None => matches!(
                schema,
                Schema::Any | Schema::Tree { .. } | Schema::Link { .. }
            ),
            Some((head, rest)) => match schema {
                Schema::Tree { branches } => match branches.get(head) {
                    Some(child) => Self::is_stampable_node_path(child, rest),
                    None => true, // missing branch in a Tree → fillable
                },
                Schema::Any => true, // unspecified ancestor → fillable
                _ => false,          // Map/Array/Link/leaf → governed elsewhere
            },
        }
    }

    /// Remove a process from the running engine.
    pub fn remove_process(&mut self, name: &str) {
        self.nodes.remove(name);
        self.interfaces.remove(name);
        self.fronts.remove(name);
        self.specs.remove(name);

        // Clean up step triggers
        for steps in self.step_triggers.values_mut() {
            steps.remove(name);
        }
    }

    /// Settle the step network at init / after discovery: repeatedly fire any
    /// ready (all inputs present) step that hasn't fired yet, until none remain.
    /// A *source* step (no inputs) is ready immediately; a consumer becomes
    /// ready once its producers have fired — so a one-shot Step DAG runs in
    /// dependency order without any explicit topological sort (readiness IS the
    /// order). Reactive re-firing on later state changes is `trigger_steps`
    /// during `run`. This is the single init-firing path (replaced three).
    fn settle_steps(&mut self) {
        // Run every not-yet-fired step once, in dependency layers (see
        // `run_step_layers`). A consumer lands a layer after its producer, so it
        // sees the producer's output via layer ordering — not a mid-tick leak.
        let steps: Vec<String> = self
            .nodes
            .iter()
            .filter(|(n, node)| {
                matches!(node, ProcessNode::Step(_)) && !self.fired_init_steps.contains(n.as_str())
            })
            .map(|(n, _)| n.clone())
            .collect();
        for name in &steps {
            self.fired_init_steps.insert(name.clone());
        }
        self.run_step_layers(steps);
    }

    /// Scan the entire state for process specs and instantiate them, then
    /// settle the step network. Used for initial discovery (building a
    /// Composite engine) and any later "rediscover from root" caller — the
    /// from-root case of [`Engine::discover_processes`], followed by a settle
    /// so any newly-registered steps fire their cascade.
    pub fn discover_all_processes(&mut self) {
        self.discover_processes(&[Vec::new()]);
        self.settle_steps();
    }

    pub fn node_names(&self) -> Vec<&str> {
        self.nodes.keys().map(|s| s.as_str()).collect()
    }

    /// Borrow a process node by name. Returns `None` if no node is
    /// registered under that name.
    pub fn node(&self, name: &str) -> Option<&ProcessNode> {
        self.nodes.get(name)
    }

    /// Iterate every `(name, &ProcessNode)` currently registered in
    /// the engine. Order matches `node_names()`.
    pub fn nodes(&self) -> impl Iterator<Item = (&str, &ProcessNode)> {
        self.nodes.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Borrow a process node by name and try to downcast its inner
    /// `Process` (or `Step`) to a concrete type `T`. Returns `None`
    /// if the name doesn't resolve or the downcast fails.
    ///
    /// Use this when a caller needs the typed configuration of a
    /// registered process (e.g. to query domain-specific methods that
    /// aren't on the `Process` trait itself).
    pub fn node_as<T: 'static>(&self, name: &str) -> Option<&T> {
        match self.nodes.get(name)? {
            ProcessNode::Process(p) => p.as_any().downcast_ref::<T>(),
            ProcessNode::Step(s) => s.as_any().downcast_ref::<T>(),
        }
    }

    /// Scan changed state paths for new process nodes and instantiate them.
    /// Also remove processes whose parent state was deleted.
    fn discover_processes(&mut self, changed_paths: &[Path]) {
        let registry = Arc::clone(&self.core.processes);

        // Collect processes to add/remove (can't mutate self while iterating)
        let mut to_add: Vec<(String, ProcessSpec, ProcessNode)> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();

        // Deduplicate changed paths to avoid scanning the same subtree repeatedly.
        // Multiple agents may output to the same path (e.g. "environment"), which
        // without dedup causes O(N²) clone+scan work.
        let mut unique_paths: Vec<&Path> = changed_paths.iter().collect();
        unique_paths.sort();
        unique_paths.dedup();

        // Remove a process whose OWN node is gone — true removal via a structural
        // `_remove`/`_divide` that deleted its slot (e.g. a mother cell replaced
        // by its daughters), which also covers an ancestor subtree being removed
        // (the own path is then absent too). Checking only the *parent* missed the
        // single-key case, so a divided-away mother's instance LINGERED as a
        // zombie — re-creating its node via its own output bridge and re-dividing
        // (the grow/divide runaway). A re-`_add` at the same key leaves the node
        // present (new config), handled by the config-changed re-add below — not
        // here. This must happen BEFORE scanning so replaced entries re-discover.
        let existing_names: Vec<String> = self.specs.keys().cloned().collect();
        for name in &existing_names {
            let own_path: Vec<Key> = name.split('.').map(Key::from).collect();
            if self.state.get_path(&own_path).is_none() {
                to_remove.push(name.clone());
            }
        }
        for name in &to_remove {
            self.remove_process(name);
        }

        // Check each changed path for new process nodes underneath
        for path in unique_paths {
            let state_at_path = self.state.get_path(path).cloned();
            if let Some(Value::Map(map)) = state_at_path {
                self.scan_for_processes(&map, path, &registry, &mut to_add);
            }
        }

        // Apply additions — if _add replaced an existing entry, the process
        // still exists (wasn't removed above since parent still exists).
        // Remove it so the new config/wiring takes effect.
        for (name, spec, node) in to_add {
            if self.nodes.contains_key(&name) {
                self.remove_process(&name);
            }
            self.add_process(name, spec, node);
        }
    }

    /// Recursively scan a map for process specs.
    ///
    /// Schema-driven discovery: a node is a process iff its schema is a
    /// `Schema::Link` variant. For nodes the schema doesn't yet declare
    /// (e.g. dynamic adds into an `Any` slot), the state value's `_type:
    /// "process"|"step"|"link"|"composite"` hint stands in — upstream
    /// process-bigraph's explicit kind marker. The bare presence of an
    /// `address` field is NEVER a type hint; many non-process values
    /// legitimately carry one.
    fn scan_for_processes(
        &self,
        map: &prism_schema::StateMap,
        parent_path: &[Key],
        registry: &Arc<ProcessRegistry>,
        results: &mut Vec<(String, ProcessSpec, ProcessNode)>,
    ) {
        for (key, val) in map {
            if let Value::Map(child_map) = val {
                let mut child_path = parent_path.to_vec();
                child_path.push(key.clone());
                let child_name = child_path.join(".");

                // Already registered? Skip if config hasn't changed.
                if self.nodes.contains_key(&child_name) {
                    // Check if this is a replaced entry (_add with same key).
                    // Compare stored config with current state config.
                    let config_changed = self
                        .specs
                        .get(&child_name)
                        .map(|spec| {
                            let current_config = child_map.get("config");
                            current_config != Some(&spec.config)
                        })
                        .unwrap_or(false);
                    if !config_changed {
                        continue;
                    }
                }

                // Schema-FIRST discovery: a process node is one the SCHEMA
                // declares as a Link (chrysalis emits Links; declared `Map[Cell]`
                // stays a Map under resolve so per-entity nodes resolve to their
                // `CompositeLink`). For nodes the schema doesn't yet declare —
                // dynamic adds into an `Any` slot, raw fixtures without a
                // declared schema — the state value MUST carry an explicit
                // `_type` hint ("process"|"step"|"link"|"composite"; upstream
                // process-bigraph convention; consumed by `Schema::infer`).
                // `address` is for *instantiation*, never for *recognition* —
                // many non-process values can also carry an `address` field
                // (a contact card, a webhook config, a remote-resource handle).
                let is_link = self.schema.schema_at_path(&child_path).is_link_kind();
                let type_hint = child_map.get("_type").and_then(|v| v.as_str());
                let has_type_hint = matches!(
                    type_hint,
                    Some("process" | "step" | "link" | "composite")
                );
                if !is_link && !has_type_hint {
                    // Not a process node — recurse to find nested links.
                    self.scan_for_processes(child_map, &child_path, registry, results);
                    continue;
                }

                // Parse address through the protocol abstraction.
                let address_val = match child_map.get("address") {
                    Some(v) => v.clone(),
                    None => continue,
                };
                let parsed = match crate::protocol::ParsedAddress::parse(&address_val) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let class_name = match address_class(&parsed) {
                    Some(name) if name != "RAMEmitter" => name,
                    _ => continue,
                };

                // Get config
                let config = child_map.get("config").cloned().unwrap_or(Value::None);

                // Dispatch through the protocol registry.
                let node = match self
                    .core
                    .protocols
                    .instantiate(&parsed, config.clone(), registry)
                {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                let inputs_val = child_map.get("inputs").cloned().unwrap_or(Value::None);
                let outputs_val = child_map.get("outputs").cloned().unwrap_or(Value::None);

                // Resolve wires: ".." navigates up from the process's own
                // location; plain paths are absolute from root.
                let inputs = resolve_wires_from_process(&inputs_val, &child_path);
                let outputs = resolve_wires_from_process(&outputs_val, &child_path);

                let interval = match &node {
                    ProcessNode::Process(p) => {
                        let cfg_interval = config
                            .as_map()
                            .and_then(|m| m.get("interval"))
                            .and_then(|v| v.as_f64());
                        Some(cfg_interval.unwrap_or_else(|| p.interval()))
                    }
                    ProcessNode::Step(_) => None,
                };

                let spec = ProcessSpec {
                    process_type: class_name,
                    config,
                    inputs,
                    outputs,
                    interval,
                    priority: 0.0,
                };

                results.push((child_name, spec, node));
            }
        }
    }
}

/// Apply projections to state — extracted as free function to avoid
/// borrow conflicts (callers can hold references to other Engine fields
/// while mutating state).
fn apply_projections_to(
    state: &mut Value,
    schema: &Schema,
    projections: &[(Path, Value, Option<Schema>)],
    type_registry: Option<&prism_schema::registry::TypeRegistry>,
) -> (Vec<Path>, bool) {
    let mut changed = Vec::new();
    let mut structural = false;
    // Track keys removed by _remove so we don't re-create them via set_path.
    let mut removed_prefixes: Vec<Path> = Vec::new();
    for (path, value, port_schema) in projections {
        // Skip projections into paths that were removed by a prior _remove
        if removed_prefixes
            .iter()
            .any(|prefix| path.len() > prefix.len() && path[..prefix.len()] == prefix[..])
        {
            continue;
        }

        // Structural change? Track removed keys so later projections in this
        // batch don't re-create them.
        if let Some(m) = value.as_map() {
            if m.contains_key("_add") || m.contains_key("_remove") {
                structural = true;
                if let Some(Value::List(keys)) = m.get("_remove") {
                    for key in keys {
                        if let Some(k) = key.as_str() {
                            let mut removed_path = path.clone();
                            removed_path.push(Key::from(k));
                            removed_prefixes.push(removed_path);
                        }
                    }
                }
            }
        }

        // The schema to apply at this slot is the state/library schema at the
        // path, with the writer's output-port schema PROMOTED onto it (local
        // resolve, law #5). promote keeps the slot's additive type (e.g.
        // `Array`/`Delta`) where the port is loose — the #14 diffusion fix:
        // a native process's loose `Map`/`Any` output no longer replaces an
        // additive field — and lets a more-specific port type (e.g.
        // `overwrite[float]`) win where it is declared. apply then handles
        // `_add`/`_remove`/`_divide` internally; no inline munging here.
        let library = schema.schema_at_path(path);
        let resolved = match port_schema {
            Some(s) => algebra::promote(library, s),
            None => library.clone(),
        };
        let current = state.get_path(path).cloned().unwrap_or(Value::None);
        let new_value = algebra::apply_with(type_registry, &resolved, &current, value);
        state.set_path(path, new_value);
        changed.push(path.clone());
    }
    (changed, structural)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Process;
    use crate::update::Update;
    use indexmap::IndexMap;

    // Simple test process: multiplies level by rate each tick
    #[derive(Debug)]
    struct GrowthProcess {
        rate: f64,
    }

    impl Process for GrowthProcess {
        fn inputs(&self) -> IndexMap<String, prism_schema::Schema> {
            IndexMap::from([("level".to_string(), prism_schema::Schema::float())])
        }

        fn outputs(&self) -> IndexMap<String, prism_schema::Schema> {
            IndexMap::from([("level".to_string(), prism_schema::Schema::float())])
        }

        fn interval(&self) -> f64 {
            1.0
        }

        fn update(&self, state: &Value, _interval: f64) -> Update {
            let level = state
                .as_map()
                .and_then(|m| m.get("level"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            // Output delta: level * (rate - 1) so engine adds it to get level * rate
            Update::value(Value::tree([(
                "level",
                Value::float(level * (self.rate - 1.0)),
            )]))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn self_node_wire_resolves_to_own_node() {
        // A process at `cells.c0`. Wires resolve against two bases:
        //   - default (container-relative): `["mass"]` → `cells.mass` (a sibling
        //     slot in the container), `[]` → `cells` (the container itself).
        //   - self-node (`%`): `["%","mass"]` → `cells.c0.mass` (the node's OWN
        //     field — where a composite exports its face), `["%"]` → `cells.c0`.
        // The process never names its own key `c0`; the engine supplies the path.
        let process_path = [Key::from("cells"), Key::from("c0")];
        let wires = Value::Map(IndexMap::from([
            // self-node face: own `mass`
            (
                Key::from("face"),
                Value::List(vec![Value::String("%".into()), Value::String("mass".into())]),
            ),
            // self-node root: the node itself
            (Key::from("node"), Value::List(vec![Value::String("%".into())])),
            // container-relative sibling (the existing default)
            (
                Key::from("sibling"),
                Value::List(vec![Value::String("mass".into())]),
            ),
            // container itself (empty wire — the existing `@`/`%`-as-here behavior)
            (Key::from("container"), Value::List(vec![])),
            // up one level from the container, then a field (existing `..`)
            (
                Key::from("up"),
                Value::List(vec![Value::String("..".into()), Value::String("glucose".into())]),
            ),
        ]));
        let resolved = resolve_wires_from_process(&wires, &process_path);
        assert_eq!(
            resolved.get("face"),
            Some(&vec![Key::from("cells"), Key::from("c0"), Key::from("mass")]),
            "`%`-rooted wire lands on the node's OWN field (cells.c0.mass)"
        );
        assert_eq!(
            resolved.get("node"),
            Some(&vec![Key::from("cells"), Key::from("c0")]),
            "`[%]` is the node itself"
        );
        assert_eq!(
            resolved.get("sibling"),
            Some(&vec![Key::from("cells"), Key::from("mass")]),
            "a bare segment stays container-relative (cells.mass) — unchanged"
        );
        assert_eq!(
            resolved.get("container"),
            Some(&vec![Key::from("cells")]),
            "the empty wire is the container — unchanged"
        );
        assert_eq!(
            resolved.get("up"),
            Some(&vec![Key::from("glucose")]),
            "`..` pops from the container to root, then `glucose` — unchanged"
        );
    }

    #[test]
    fn test_engine_basic() {
        let mut topology = Topology::new();
        topology.initial_state = Value::tree([("level", Value::float(1.0))]);

        topology.add_process(
            "growth",
            "growth",
            Value::None,
            IndexMap::from([("level".to_string(), vec![Key::from("level")])]),
            IndexMap::from([("level".to_string(), vec![Key::from("level")])]),
            1.0,
        );

        let instances = HashMap::from([(
            "growth".to_string(),
            ProcessNode::Process(Box::new(GrowthProcess { rate: 1.1 })),
        )]);

        let mut engine = Engine::new(topology, instances);
        assert_eq!(engine.time(), 0.0);

        engine.run(3.0);
        assert_eq!(engine.time(), 3.0);

        let level = engine
            .get(&[Key::from("level")])
            .and_then(|v| v.as_f64())
            .unwrap();
        // After 3 ticks at rate 1.1: 1.0 * 1.1 * 1.1 * 1.1 ≈ 1.331
        assert!((level - 1.331).abs() < 0.001);
    }

    #[test]
    fn native_node_with_type_hint_gets_a_real_link_in_schema() {
        // A native process spec sits in state at a `Tree` path with the engine
        // schema `Any`. The state value carries an explicit `_type: "process"`
        // hint (upstream process-bigraph convention); discovery finds it via
        // that hint, not by guessing from the `address` field — bare `address`
        // is not a type hint (user profiles, webhook configs, etc. also have
        // one). After discovery the node carries the instance's REAL
        // `ProcessLink` (the threaded interface) — no `Any` left at the node.
        let wire = || Value::List(vec![Value::String("level".into())]);
        let state = Value::tree([
            ("level", Value::float(1.0)),
            (
                "growth",
                Value::tree([
                    ("_type", Value::String("process".into())),
                    ("address", Value::String("local:Growth".into())),
                    ("inputs", Value::tree([("level", wire())])),
                    ("outputs", Value::tree([("level", wire())])),
                ]),
            ),
        ]);

        let engine = Engine::builder()
            .schema(Schema::Any)
            .state(state)
            .register_process("Growth", |_cfg| {
                ProcessNode::Process(Box::new(GrowthProcess { rate: 1.1 }))
            })
            .build()
            .unwrap(); // builder auto-discovers → the address scan finds + stamps

        // The node was `Any` (address-discovered); now it's the instance's real
        // ProcessLink with the `level: Float` port threaded through — the
        // downstream schema is no longer `Any` at the process node.
        match engine.schema().schema_at_path(&[Key::from("growth")]) {
            Schema::ProcessLink {
                inputs, outputs, ..
            } => {
                assert!(
                    matches!(inputs.get(&Key::from("level")), Some(Schema::Float { .. })),
                    "input `level` threaded from the instance, got {:?}",
                    inputs.get(&Key::from("level"))
                );
                assert!(
                    matches!(outputs.get(&Key::from("level")), Some(Schema::Float { .. })),
                    "output `level` threaded from the instance"
                );
            }
            other => panic!("native node should carry a real ProcessLink, not {other:?}"),
        }
    }
}
