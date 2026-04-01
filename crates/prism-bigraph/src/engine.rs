//! The composition engine — steps processes and reconciles state.
//!
//! This is the Rust equivalent of process-bigraph's Composite.
//! It manages the shared state tree, schedules temporal processes
//! by their intervals, triggers dependency-based steps, and
//! applies updates with proper merge semantics.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;
use ordered_float::OrderedFloat;

use prism_schema::{Key, Path, Schema, Value};

use crate::factory::ProcessRegistry;

/// Resolve wires for a process at a given path.
///
/// If the wire path starts with "..", resolve relative to the process's location
/// (each ".." pops one level). Otherwise, treat as absolute from the state root.
fn resolve_wires_from_process(
    wires: &Value,
    process_path: &[Key],
) -> IndexMap<String, Vec<Key>> {
    let parent: Vec<Key> = if !process_path.is_empty() {
        process_path[..process_path.len()-1].to_vec()
    } else {
        vec![]
    };

    fn resolve_one(path_list: &[Value], parent: &[Key]) -> Vec<Key> {
        let elems: Vec<Key> = path_list.iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(Key::from(s.as_str())),
                Value::Int(i) => Some(Key::from(i.to_string())),
                Value::Float(f) => Some(Key::from(format!("{}", f.0 as i64))),
                _ => None,
            })
            .collect();

        let mut resolved = parent.to_vec();
        for elem in &elems {
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
        result: &mut IndexMap<String, Vec<Key>>,
    ) {
        match target {
            Value::List(list) => {
                result.insert(prefix.to_string(), resolve_one(list, parent));
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
                    flatten_nested(&sub_prefix, sub_target, parent, result);
                }
            }
            _ => {}
        }
    }

    let mut result = IndexMap::new();
    if let Some(map) = wires.as_map() {
        for (port, target) in map {
            flatten_nested(port, target, &parent, &mut result);
        }
    }
    result
}
use crate::ports::Interface;
use crate::process::{Process, ProcessNode};
use crate::topology::{ProcessSpec, Topology};

/// Compute the difference between two Values.
/// For numbers: new - old. For maps: recursive diff. For lists/others: return new (replace).
fn value_diff(new: &Value, old: &Value) -> Value {
    match (new, old) {
        (Value::Float(a), Value::Float(b)) => Value::float(a.0 - b.0),
        (Value::Int(a), Value::Int(b)) => Value::Int(a - b),
        (Value::Float(a), Value::Int(b)) => Value::float(a.0 - *b as f64),
        (Value::Int(a), Value::Float(b)) => Value::float(*a as f64 - b.0),
        (Value::Map(new_map), Value::Map(old_map)) => {
            let mut diff = prism_schema::StateMap::new();
            for (k, nv) in new_map {
                match old_map.get(k) {
                    Some(ov) => {
                        let d = value_diff(nv, ov);
                        diff.insert(k.clone(), d);
                    }
                    None => {
                        // New key — include as-is
                        diff.insert(k.clone(), nv.clone());
                    }
                }
            }
            // Keys in old but not in new: omit (no change)
            Value::Map(diff)
        }
        // Lists, strings, etc: return new value (replace semantics)
        _ => new.clone(),
    }
}

/// Scheduling state for a temporal process.
#[derive(Debug)]
struct ProcessFront {
    next_time: f64,
    interval: f64,
}

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

    /// Passthrough paths: projections to these root paths are captured
    /// as raw deltas (not applied to state). Used by Composite bridge.
    passthrough_paths: HashSet<Key>,

    /// Captured passthrough deltas from the last run.
    passthrough_deltas: IndexMap<Key, Value>,

    /// Current simulation time.
    time: f64,

    /// Instantiated process/step nodes, keyed by name.
    nodes: HashMap<String, ProcessNode>,

    /// Wiring interfaces for each node.
    interfaces: HashMap<String, Interface>,

    /// Scheduling fronts for temporal processes.
    fronts: HashMap<String, ProcessFront>,

    /// Step dependency tracking: state path -> set of step names to trigger.
    step_triggers: HashMap<Path, HashSet<String>>,

    /// Specs retained for introspection.
    specs: HashMap<String, ProcessSpec>,

    /// Previous projected outputs per process/step, for computing diffs.
    /// Key: process name. Value: list of (path, value) from last projection.
    previous_outputs: HashMap<String, Vec<(Path, Value)>>,

    /// Process registry for dynamic process discovery.
    registry: Option<Arc<ProcessRegistry>>,
}

impl Engine {
    /// Create a new engine from a topology with pre-built process instances.
    ///
    /// `factories` maps process type names to constructor functions.
    /// Each factory receives the process config and returns a ProcessNode.
    pub fn new(
        topology: Topology,
        instances: HashMap<String, ProcessNode>,
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

        let mut engine = Self {
            state,
            schema: topology.state_schema,
            time: 0.0,
            nodes: instances,
            interfaces,
            fronts,
            step_triggers,
            specs,
            previous_outputs: HashMap::new(),
            registry: None,
            last_structural: false,
            pending_changes: Vec::new(),
            passthrough_paths: HashSet::new(),
            passthrough_deltas: IndexMap::new(),
        };

        // Fire steps on initialization in dependency order.
        // Build a simple topological sort: steps whose inputs come from
        // other steps' outputs must run after those steps.
        let step_names: Vec<String> = engine.specs.iter()
            .filter(|(_, s)| s.interval.is_none())
            .map(|(name, _)| name.clone())
            .collect();

        if !step_names.is_empty() {
            // Map: output_path → step_name that produces it
            let mut output_to_step: HashMap<Path, String> = HashMap::new();
            for name in &step_names {
                for path in engine.specs[name].outputs.values() {
                    output_to_step.insert(path.clone(), name.clone());
                }
            }

            // Build adjacency: step A → step B if B produces an input of A
            let mut deps: HashMap<String, HashSet<String>> = HashMap::new();
            for name in &step_names {
                let d: HashSet<String> = engine.specs[name].inputs.values()
                    .filter_map(|p| output_to_step.get(p).cloned())
                    .filter(|dep| dep != name)
                    .collect();
                deps.insert(name.clone(), d);
            }

            // Topological sort (Kahn's algorithm)
            let mut in_degree: HashMap<String, usize> = step_names.iter()
                .map(|n| (n.clone(), deps.get(n).map(|d| d.len()).unwrap_or(0)))
                .collect();
            let mut queue: Vec<String> = in_degree.iter()
                .filter(|(_, deg)| **deg == 0)
                .map(|(n, _)| n.clone())
                .collect();
            queue.sort(); // deterministic ordering
            let mut order: Vec<String> = Vec::new();
            while let Some(name) = queue.pop() {
                order.push(name.clone());
                // For each step that depends on this one, decrement in-degree
                for (other, other_deps) in &deps {
                    if other_deps.contains(&name) {
                        if let Some(deg) = in_degree.get_mut(other) {
                            *deg -= 1;
                            if *deg == 0 {
                                queue.push(other.clone());
                                queue.sort();
                            }
                        }
                    }
                }
            }

            // Run steps in dependency order
            for step_name in &order {
                let interface = match engine.interfaces.get(step_name) {
                    Some(i) => i.clone(),
                    None => continue,
                };
                let input_state = interface.view(&engine.state);
                let update = match engine.nodes.get(step_name) {
                    Some(ProcessNode::Step(s)) => s.update(&input_state),
                    _ => continue,
                };
                if let Some(update_value) = update.into_value() {
                    let projections = interface.project(&update_value);
                    engine.apply_projections(&projections);
                }
            }
        }

        engine
    }

    /// Create an engine from a schema, state, and registry.
    ///
    /// This is the Rust equivalent of Python's `Composite({'schema': ..., 'state': ...})`.
    /// Walks the state tree, finds process/step nodes (via `Schema::Link` in the
    /// schema or `_type` annotations in state), instantiates them via the registry,
    /// extracts wiring, and builds the engine.
    pub fn from_state(
        schema: Schema,
        state: Value,
        registry: Arc<ProcessRegistry>,
    ) -> Result<Self, String> {
        // Infer and merge schema from state annotations
        let merged_schema = Schema::infer_and_merge(&schema, &state);

        let mut topology = Topology::new();
        topology.state_schema = merged_schema.clone();

        // Walk the schema to find Link nodes and extract process specs from state
        let mut instances: HashMap<String, ProcessNode> = HashMap::new();
        let mut clean_state = state.clone();

        Self::extract_processes(
            &merged_schema,
            &state,
            &[],
            &registry,
            &mut topology.processes,
            &mut instances,
        );

        // Remove process-spec fields from state (keep only data)
        // Process nodes in state have address/config/inputs/outputs which
        // the engine manages — the data state shouldn't contain them.
        for name in topology.processes.keys() {
            let path: Vec<String> = name.split('.').map(|s| s.to_string()).collect();
            // Don't remove the node entirely — just let the engine manage it
        }

        topology.initial_state = state;
        let mut engine = Engine::new(topology, instances);
        engine.set_registry(registry);
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
    pub fn merge_schema(
        &mut self,
        schema_update: Schema,
        state_update: Value,
    ) {
        // 1. Merge schemas
        self.schema = self.schema.resolve(&schema_update);

        // 2. Apply state update using merged schema
        if !state_update.is_none() {
            let new_state = self.schema.apply_update(&self.state, &state_update);
            self.state = new_state;
        }

        // 3. Fill defaults for new schema branches not present in state
        if let Schema::Tree { branches } = &self.schema {
            if let Some(state_map) = self.state.as_map_mut() {
                for (key, child_schema) in branches {
                    if !state_map.contains_key(key) {
                        // New schema branch — fill with default
                        let default_val = child_schema.default_value();
                        state_map.insert(key.clone(), default_val);
                    }
                }
            }
        }

        // 4. Discover and instantiate new processes from the merged schema
        if let Some(registry) = &self.registry {
            let registry = Arc::clone(registry);
            let mut new_specs = IndexMap::new();
            let mut new_instances = HashMap::new();

            Self::extract_processes(
                &self.schema,
                &self.state,
                &[],
                &registry,
                &mut new_specs,
                &mut new_instances,
            );

            // Add only processes not already registered
            for (name, spec) in new_specs {
                if !self.nodes.contains_key(&name) {
                    if let Some(node) = new_instances.remove(&name) {
                        self.add_process(name, spec, node);
                    }
                }
            }
        }
    }

    /// Recursively extract process specs from schema + state.
    fn extract_processes(
        schema: &Schema,
        state: &Value,
        path: &[Key],
        registry: &ProcessRegistry,
        specs: &mut IndexMap<String, ProcessSpec>,
        instances: &mut HashMap<String, ProcessNode>,
    ) {
        match schema {
            Schema::Link { temporal, .. } => {
                // This node is a process/step — extract spec from state
                let map = match state.as_map() {
                    Some(m) => m,
                    None => return,
                };

                let class_name = map.get("address")
                    .and_then(|a| match a {
                        Value::Map(m) => m.get("data").and_then(|v| v.as_str().map(|s| s.to_string())),
                        Value::String(s) => {
                            if s.contains(':') { s.split(':').nth(1).map(|s| s.to_string()) }
                            else { Some(s.clone()) }
                        }
                        _ => None,
                    });

                let class_name = match class_name {
                    Some(n) if n != "RAMEmitter" => n,
                    _ => return,
                };

                let config = map.get("config").cloned().unwrap_or(Value::None);

                if let Some(node) = registry.create(&class_name, config.clone()) {
                    let inputs_val = map.get("inputs").cloned().unwrap_or(Value::None);
                    let outputs_val = map.get("outputs").cloned().unwrap_or(Value::None);

                    // Resolve wires: ".." navigates up from the process's own
                    // location; plain paths are absolute from root.
                    let inputs = resolve_wires_from_process(&inputs_val, path);
                    let outputs = resolve_wires_from_process(&outputs_val, path);

                    let interval = match (&node, temporal) {
                        (ProcessNode::Process(p), _) => {
                            let cfg_interval = map.get("interval")
                                .and_then(|v| v.as_f64())
                                .or_else(|| config.as_map()
                                    .and_then(|m| m.get("interval"))
                                    .and_then(|v| v.as_f64()));
                            Some(cfg_interval.unwrap_or_else(|| p.interval()))
                        }
                        (ProcessNode::Step(_), _) => None,
                    };

                    let name = path.join(".");
                    specs.insert(name.clone(), ProcessSpec {
                        process_type: class_name,
                        config,
                        inputs,
                        outputs,
                        interval,
                        priority: 0.0,
                    });
                    instances.insert(name, node);
                }
            }
            Schema::Tree { branches } => {
                if let Some(map) = state.as_map() {
                    for (key, child_schema) in branches {
                        let mut child_path = path.to_vec();
                        child_path.push(key.clone());
                        let child_state = map.get(key).cloned().unwrap_or(Value::None);
                        Self::extract_processes(
                            child_schema, &child_state, &child_path,
                            registry, specs, instances);
                    }
                    // Also check state keys not in schema (might have _type annotations)
                    for (key, child_state) in map {
                        if !branches.contains_key(key) && !key.starts_with('_') {
                            let mut child_path = path.to_vec();
                            child_path.push(key.clone());
                            let inferred = Schema::infer(child_state);
                            Self::extract_processes(
                                &inferred, child_state, &child_path,
                                registry, specs, instances);
                        }
                    }
                }
            }
            Schema::Map { value } => {
                if let Some(map) = state.as_map() {
                    for (key, child_state) in map {
                        let mut child_path = path.to_vec();
                        child_path.push(key.clone());
                        Self::extract_processes(
                            value, child_state, &child_path,
                            registry, specs, instances);
                    }
                }
            }
            _ => {}
        }
    }

    /// Set the process registry for dynamic process discovery.
    /// When set, new process nodes appearing in state (e.g., via _add)
    /// are automatically instantiated and wired.
    /// Number of temporal processes.
    pub fn fronts_count(&self) -> usize { self.fronts.len() }

    /// Queue pending state changes to trigger steps at the start of next run.
    pub fn queue_changes(&mut self, paths: Vec<Path>) {
        self.pending_changes.extend(paths);
    }

    /// Set passthrough paths — projections to these root paths are captured
    /// as raw deltas instead of applied to state.
    pub fn set_passthrough_paths(&mut self, paths: HashSet<Key>) {
        self.passthrough_paths = paths;
    }

    /// Take the passthrough deltas captured during the last run.
    pub fn take_passthrough_deltas(&mut self) -> IndexMap<Key, Value> {
        std::mem::take(&mut self.passthrough_deltas)
    }

    /// Public wrapper for trigger_steps.
    pub fn trigger_steps_pub(&mut self, changed_paths: &[Path]) {
        self.trigger_steps(changed_paths);
    }

    pub fn set_registry(&mut self, registry: Arc<ProcessRegistry>) {
        self.registry = Some(registry);
    }

    /// Current simulation time.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Read-only access to the state tree.
    pub fn state(&self) -> &Value {
        &self.state
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
        // Process pending changes from composite bridge inputs
        if !self.pending_changes.is_empty() {
            let pending = std::mem::take(&mut self.pending_changes);
            self.trigger_steps(&pending);
        }

        let end_time = self.time + duration;
        let mut iter_count = 0u64;
        while self.time < end_time {
            let next_time = self.next_fire_time(end_time);
            match next_time {
                Some(fire_time) => {
                    self.time = fire_time;
                    let firing: Vec<String> = self.fronts
                        .iter()
                        .filter(|(_, front)| (front.next_time - fire_time).abs() < 1e-10)
                        .map(|(name, _)| name.clone())
                        .collect();

                    let mut all_changed = Vec::new();
                    let mut any_structural = false;
                    for name in &firing {
                        let changed = self.run_process(name);
                        any_structural |= self.last_structural;
                        all_changed.extend(changed);
                    }

                    let step_changes = if self.step_triggers.is_empty() {
                        Vec::new()
                    } else {
                        self.trigger_steps(&all_changed)
                    };

                    // Only discover when structural changes occurred
                    if any_structural || !step_changes.is_empty() {
                        let mut discover_paths = all_changed;
                        discover_paths.extend(step_changes);
                        self.discover_processes(&discover_paths);
                    }

                    iter_count += 1;
                    if iter_count > 100_000 {
                        eprintln!("[engine] SAFETY: breaking after {iter_count} iterations at t={}", self.time);
                        self.time = end_time;
                        break;
                    }
                }
                None => { self.time = end_time; }
            }
        }
    }

    /// Run and return accumulated deltas at each modified path.
    /// Used by Composite to bypass snapshot+diff: the raw deltas are
    /// mapped through the bridge directly.
    pub fn run_collecting(&mut self, duration: f64) -> IndexMap<Path, Value> {
        let end_time = self.time + duration;
        let mut iter_count = 0u64;
        let mut accumulated: IndexMap<Path, Value> = IndexMap::new();

        while self.time < end_time {
            let next_time = self.next_fire_time(end_time);

            match next_time {
                Some(fire_time) => {
                    self.time = fire_time;

                    let firing: Vec<String> = self.fronts
                        .iter()
                        .filter(|(_, front)| (front.next_time - fire_time).abs() < 1e-10)
                        .map(|(name, _)| name.clone())
                        .collect();

                    let mut all_changed = Vec::new();
                    for name in &firing {
                        let deltas = self.run_process_raw(name);
                        for (path, value) in &deltas {
                            // Accumulate: merge delta into existing accumulated value
                            let existing = accumulated.get(path).cloned();
                            match existing {
                                Some(prev) => {
                                    // For floats, add; for maps, merge; for lists, replace
                                    let merged = Schema::Any.apply_update(&prev, value);
                                    accumulated.insert(path.clone(), merged);
                                }
                                None => {
                                    accumulated.insert(path.clone(), value.clone());
                                }
                            }
                            all_changed.push(path.clone());
                        }
                    }

                    if self.registry.is_some() {
                        self.discover_processes(&all_changed);
                    }
                    let step_deltas = self.trigger_steps_raw(&all_changed);
                    for (path, value) in &step_deltas {
                        let existing = accumulated.get(path).cloned();
                        match existing {
                            Some(prev) => {
                                accumulated.insert(path.clone(), Schema::Any.apply_update(&prev, value));
                            }
                            None => {
                                accumulated.insert(path.clone(), value.clone());
                            }
                        }
                    }

                    iter_count += 1;
                    if iter_count > 100_000 {
                        eprintln!("[engine] SAFETY: breaking after {iter_count} iterations at t={}", self.time);
                        self.time = end_time;
                        break;
                    }
                }
                None => {
                    self.time = end_time;
                }
            }
        }

        accumulated
    }

    /// Like run_process but also returns the raw projections (path → delta).
    fn run_process_raw(&mut self, name: &str) -> Vec<(Path, Value)> {
        let interval = match self.fronts.get(name) {
            Some(front) => front.interval,
            None => return Vec::new(),
        };

        let input_state = match self.interfaces.get(name) {
            Some(iface) => iface.view(&self.state),
            None => return Vec::new(),
        };

        let update = match self.nodes.get(name) {
            Some(ProcessNode::Process(p)) => p.update(&input_state, interval),
            _ => return Vec::new(),
        };

        self.fronts.get_mut(name).unwrap().next_time += interval;

        if let Some(update_value) = update.into_value() {
            let projections = match self.interfaces.get(name) {
                Some(iface) => iface.project(&update_value),
                None => return Vec::new(),
            };
            self.apply_projections(&projections);
            projections.into_iter()
                .map(|(path, value, _schema)| (path, value))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn trigger_steps_raw(&mut self, changed_paths: &[Path]) -> Vec<(Path, Value)> {
        let (_changes, deltas) = self.trigger_steps_impl(changed_paths);
        deltas
    }

    /// Run a single tick: advance to the next event and process it.
    /// Returns the time advanced to, or None if no events pending.
    pub fn tick(&mut self) -> Option<f64> {
        match self.next_fire_time(f64::INFINITY) {
            Some(fire_time) => {
                self.time = fire_time;
                let firing: Vec<String> = self.fronts
                    .iter()
                    .filter(|(_, front)| (front.next_time - fire_time).abs() < 1e-10)
                    .map(|(name, _)| name.clone())
                    .collect();
                let mut all_changed = Vec::new();
                for name in &firing {
                    all_changed.extend(self.run_process(name));
                }
                self.discover_processes(&all_changed);
                self.trigger_steps(&all_changed);
                Some(self.time)
            }
            None => None,
        }
    }

    /// Find the earliest fire time before `end_time`.
    fn next_fire_time(&self, end_time: f64) -> Option<f64> {
        self.fronts
            .values()
            .filter(|front| front.next_time < end_time)
            .map(|front| front.next_time)
            .min_by_key(|t| OrderedFloat(*t))
    }

    /// Execute one process and apply its update. Returns changed paths.
    /// Does NOT trigger steps — caller is responsible for that.
    fn run_process(&mut self, name: &str) -> Vec<Path> {
        let interval = match self.fronts.get(name) {
            Some(front) => front.interval,
            None => return Vec::new(),
        };

        // Build view and call process without cloning Interface.
        // We borrow interfaces and nodes immutably, then state mutably via the free fn.
        let input_state = match self.interfaces.get(name) {
            Some(iface) => iface.view(&self.state),
            None => return Vec::new(),
        };

        let update = match self.nodes.get(name) {
            Some(ProcessNode::Process(p)) => p.update(&input_state, interval),
            _ => return Vec::new(),
        };

        self.fronts.get_mut(name).unwrap().next_time += interval;

        if let Some(update_value) = update.into_value() {
            let projections = match self.interfaces.get(name) {
                Some(iface) => iface.project(&update_value),
                None => return Vec::new(),
            };
            let (changed, _) = self.apply_projections(&projections);
            changed
        } else {
            Vec::new()
        }
    }

    /// Apply projected updates to the state tree.
    /// Returns (changed_paths, had_structural_change).
    /// Structural changes are `_add`/`_remove` operations that may require
    /// process discovery. Skipping discovery on non-structural ticks is a
    /// major performance win.
    fn apply_projections(&mut self, projections: &[(Path, Value, Option<Schema>)]) -> (Vec<Path>, bool) {
        if self.passthrough_paths.is_empty() {
            let result = apply_projections_to(&mut self.state, &self.schema, projections);
            self.last_structural = result.1;
            return result;
        }

        // Separate passthrough from normal projections
        let mut normal = Vec::new();
        for proj in projections {
            let root = proj.0.first().map(|k| k.as_str()).unwrap_or("");
            if self.passthrough_paths.contains(root) {
                let root_key = Key::from(root);
                let mut delta = proj.1.clone();
                if proj.0.len() > 1 {
                    for key in proj.0[1..].iter().rev() {
                        delta = Value::tree([(key.as_str(), delta)]);
                    }
                }
                if let Some(existing) = self.passthrough_deltas.get(&root_key) {
                    delta = Schema::Any.apply_update(existing, &delta);
                }
                self.passthrough_deltas.insert(root_key, delta);
            } else {
                normal.push(proj.clone());
            }
        }

        let (mut changed, structural) = apply_projections_to(&mut self.state, &self.schema, &normal);
        let has_passthrough = !self.passthrough_deltas.is_empty();
        self.last_structural = structural || has_passthrough;
        for root in self.passthrough_paths.iter() {
            changed.push(vec![root.clone()]);
        }
        (changed, structural || has_passthrough)
    }

    /// Fire any steps whose inputs overlap with the changed paths.
    fn trigger_steps(&mut self, changed_paths: &[Path]) -> Vec<Path> {
        let (changes, _deltas) = self.trigger_steps_impl(changed_paths);
        changes
    }

    /// Fire steps and return both changed paths AND raw (path, delta) projections.
    fn trigger_steps_impl(&mut self, changed_paths: &[Path]) -> (Vec<Path>, Vec<(Path, Value)>) {
        let mut triggered: Vec<String> = Vec::new();

        for path in changed_paths {
            if let Some(steps) = self.step_triggers.get(path) {
                triggered.extend(steps.iter().cloned());
            }
            for i in 1..path.len() {
                let prefix = path[..i].to_vec();
                if let Some(steps) = self.step_triggers.get(&prefix) {
                    triggered.extend(steps.iter().cloned());
                }
            }
        }

        triggered.sort();
        triggered.dedup();
        triggered.sort_by(|a, b| {
            let pa = self.specs.get(a).map(|s| s.priority).unwrap_or(0.0);
            let pb = self.specs.get(b).map(|s| s.priority).unwrap_or(0.0);
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut already_run = HashSet::new();
        let mut queue = triggered;
        let mut all_step_changes = Vec::new();
        let mut all_step_deltas: Vec<(Path, Value)> = Vec::new();
        let mut any_structural = false;

        while let Some(step_name) = queue.pop() {
            if already_run.contains(&step_name) {
                continue;
            }
            already_run.insert(step_name.clone());

            let input_state = match self.interfaces.get(&step_name) {
                Some(iface) => iface.view(&self.state),
                None => continue,
            };

            let update = match self.nodes.get(&step_name) {
                Some(ProcessNode::Step(s)) => s.update(&input_state),
                _ => continue,
            };

            if let Some(update_value) = update.into_value() {
                let projections = match self.interfaces.get(&step_name) {
                    Some(iface) => iface.project(&update_value),
                    None => continue,
                };

                for (path, value, _schema) in &projections {
                    all_step_deltas.push((path.clone(), value.clone()));
                }

                let (newly_changed, structural) = self.apply_projections(
                    &projections);
                any_structural |= structural;

                // Cascade: find downstream steps triggered by this step's output
                for path in &newly_changed {
                    if let Some(steps) = self.step_triggers.get(path) {
                        for s in steps {
                            if !already_run.contains(s) {
                                queue.push(s.clone());
                            }
                        }
                    }
                    for i in 1..path.len() {
                        let prefix = path[..i].to_vec();
                        if let Some(steps) = self.step_triggers.get(&prefix) {
                            for s in steps {
                                if !already_run.contains(s) {
                                    queue.push(s.clone());
                                }
                            }
                        }
                    }
                }

                all_step_changes.extend(newly_changed);
            }
        }

        if any_structural && !all_step_changes.is_empty() {
            self.discover_processes(&all_step_changes);
        }

        (all_step_changes, all_step_deltas)
    }

    /// Dynamically add a process to the running engine.
    pub fn add_process(
        &mut self,
        name: String,
        spec: ProcessSpec,
        node: ProcessNode,
    ) {
        let mut interface = spec.interface();
        interface.output_schemas = node.outputs();
        self.interfaces.insert(name.clone(), interface);

        if let Some(interval) = spec.interval {
            // Schedule for NEXT cycle, not current time.
            // This prevents infinite discovery-fire-discover loops
            // when new processes are added mid-tick.
            self.fronts.insert(
                name.clone(),
                ProcessFront {
                    next_time: self.time + interval,
                    interval,
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
        self.nodes.insert(name, node);
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

    /// Compute the diff between current projections and previous projections.
    /// Returns only the incremental change to apply.
    ///
    /// For each (path, value) in the new projections, compute value - previous_value.
    /// This is how process-bigraph's Composite handles accumulated count values:
    /// the full output is stored, but only the increment gets applied to state.
    fn compute_diff(
        &mut self,
        name: &str,
        new_projections: &[(Path, Value)],
    ) -> Vec<(Path, Value)> {
        let previous = self.previous_outputs.get(name);

        let diff: Vec<(Path, Value)> = new_projections
            .iter()
            .map(|(path, new_val)| {
                // Find the previous value for this path
                let prev_val = previous
                    .and_then(|prev| {
                        prev.iter()
                            .find(|(p, _)| p == path)
                            .map(|(_, v)| v)
                    });

                match prev_val {
                    Some(prev) => {
                        // Compute diff: new - previous
                        let diff_val = value_diff(new_val, prev);
                        (path.clone(), diff_val)
                    }
                    None => {
                        // No previous — use the full value (first run)
                        (path.clone(), new_val.clone())
                    }
                }
            })
            .collect();

        // Store current projections as previous for next time
        self.previous_outputs
            .insert(name.to_string(), new_projections.to_vec());

        diff
    }

    /// Get the names of all nodes.
    /// Public wrapper for discover_processes (used by Composite for initial scan).
    pub fn discover_processes_pub(&mut self, changed_paths: &[Path]) {
        self.discover_processes(changed_paths);
    }

    /// Scan the entire top-level state for process specs and instantiate them.
    /// Used for initial discovery when building a Composite engine.
    pub fn discover_all_processes(&mut self) {
        let registry = match &self.registry {
            Some(r) => std::sync::Arc::clone(r),
            None => return,
        };
        let mut to_add = Vec::new();
        if let Some(map) = self.state.as_map().cloned() {
            self.scan_for_processes(&map, &[], &registry, &mut to_add);
        }
        for (name, spec, node) in to_add {
            if !self.nodes.contains_key(&name) {
                self.add_process(name, spec, node);
            }
        }
    }

    pub fn node_names(&self) -> Vec<&str> {
        self.nodes.keys().map(|s| s.as_str()).collect()
    }

    /// Scan changed state paths for new process nodes and instantiate them.
    /// Also remove processes whose parent state was deleted.
    fn discover_processes(&mut self, changed_paths: &[Path]) {
        let registry = match &self.registry {
            Some(r) => Arc::clone(r),
            None => return,
        };

        // Collect processes to add/remove (can't mutate self while iterating)
        let mut to_add: Vec<(String, ProcessSpec, ProcessNode)> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();

        // Deduplicate changed paths to avoid scanning the same subtree repeatedly.
        // Multiple agents may output to the same path (e.g. "environment"), which
        // without dedup causes O(N²) clone+scan work.
        let mut unique_paths: Vec<&Path> = changed_paths.iter().collect();
        unique_paths.sort();
        unique_paths.dedup();

        // Check each changed path for new process nodes underneath
        for path in unique_paths {
            let state_at_path = self.state.get_path(path).cloned();
            if let Some(Value::Map(map)) = state_at_path {
                // Scan children for process specs (maps with "address" key)
                self.scan_for_processes(&map, path, &registry, &mut to_add);
            }
        }

        // Check for removed processes (processes whose state path no longer exists)
        let existing_names: Vec<String> = self.specs.keys().cloned().collect();
        for name in &existing_names {
            // If process name contains dots, check if its parent state exists
            if let Some(dot_pos) = name.rfind('.') {
                let parent_path: Vec<Key> = name[..dot_pos]
                    .split('.')
                    .map(|s| Key::from(s))
                    .collect();
                if self.state.get_path(&parent_path).is_none() {
                    to_remove.push(name.clone());
                }
            }
        }

        // Apply changes
        for name in to_remove {
            self.remove_process(&name);
        }
        for (name, spec, node) in to_add {
            if !self.nodes.contains_key(&name) {
                self.add_process(name, spec, node);
            }
        }
    }

    /// Recursively scan a map for process specs.
    ///
    /// Uses schema-driven discovery when available: if the schema at a path
    /// is `Schema::Link`, the node is a process/step. Falls back to scanning
    /// for "address" fields when schema is `Any` (dynamic composition).
    fn scan_for_processes(
        &self,
        map: &prism_schema::StateMap,
        parent_path: &[Key],
        registry: &ProcessRegistry,
        results: &mut Vec<(String, ProcessSpec, ProcessNode)>,
    ) {
        for (key, val) in map {
            if let Value::Map(child_map) = val {
                let mut child_path = parent_path.to_vec();
                child_path.push(key.clone());
                let child_name = child_path.join(".");

                // Already registered? Skip.
                if self.nodes.contains_key(&child_name) {
                    continue;
                }

                // Check schema first — if it declares Link, this is a process
                let child_schema = self.schema.schema_at_path(&child_path);
                let is_link = matches!(child_schema, Schema::Link { .. });
                if is_link {
                }

                // Fall back to address-scanning when schema is Any
                let has_address = !is_link
                    && child_map.get("address")
                        .map(|a| a.as_map().is_some() || a.as_str().is_some())
                        .unwrap_or(false);

                if !is_link && !has_address {
                    // Not a process — recurse into children
                    self.scan_for_processes(child_map, &child_path, registry, results);
                    continue;
                }

                // Extract class name from address
                let class_name = child_map
                    .get("address")
                    .and_then(|a| match a {
                        Value::Map(m) => m.get("data")
                            .and_then(|v| v.as_str().map(|s| s.to_string())),
                        Value::String(s) => {
                            // "local:ClassName" or just "ClassName"
                            if s.contains(':') {
                                s.split(':').nth(1).map(|s| s.to_string())
                            } else {
                                Some(s.clone())
                            }
                        }
                        _ => None,
                    });

                let class_name = match class_name {
                    Some(name) if name != "RAMEmitter" => name,
                    _ => continue,
                };

                // Get config
                let config = child_map
                    .get("config")
                    .cloned()
                    .unwrap_or(Value::None);

                // Try to instantiate via registry
                if let Some(node) = registry.create(&class_name, config.clone()) {
                    let inputs_val = child_map
                        .get("inputs")
                        .cloned()
                        .unwrap_or(Value::None);
                    let outputs_val = child_map
                        .get("outputs")
                        .cloned()
                        .unwrap_or(Value::None);

                    // Resolve wires: ".." navigates up from the process's own
                    // location; plain paths are absolute from root.
                    let inputs = resolve_wires_from_process(&inputs_val, &child_path);
                    let outputs = resolve_wires_from_process(&outputs_val, &child_path
                    );

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
}

/// Apply projections to state — extracted as free function to avoid
/// borrow conflicts (callers can hold references to other Engine fields
/// while mutating state).
fn apply_projections_to(
    state: &mut Value,
    schema: &Schema,
    projections: &[(Path, Value, Option<Schema>)],
) -> (Vec<Path>, bool) {
    let mut changed = Vec::new();
    let mut structural = false;
    for (path, value, port_schema) in projections {
        let has_add_remove = value.as_map()
            .map(|m| m.contains_key("_add") || m.contains_key("_remove"))
            .unwrap_or(false);
        if has_add_remove {
            structural = true;
            // Fast path: apply _add/_remove in-place without cloning the target map.
            if let Some(upd_map) = value.as_map() {
                if let Some(Value::Map(target)) = state.get_path_mut(path) {
                    prism_schema::apply_add_remove(target, upd_map);
                    // Apply any non-structural keys (regular deltas alongside _add/_remove)
                    let resolve_schema = match port_schema {
                        Some(s) if !matches!(s, Schema::Any) => s,
                        _ => &schema.schema_at_path(path),
                    };
                    let val_schema = match resolve_schema {
                        Schema::Map { value: vs } => vs.as_ref(),
                        _ => &Schema::Any,
                    };
                    for (k, v) in upd_map {
                        if k == "_add" || k == "_remove" { continue; }
                        let existing = target.get(k).cloned().unwrap_or(Value::None);
                        target.insert(k.clone(), val_schema.apply_update(&existing, v));
                    }
                    changed.push(path.clone());
                    continue;
                }
            }
        }
        let current = state.get_path(path).cloned().unwrap_or(Value::None);
        let new_value = match port_schema {
            Some(s) if !matches!(s, Schema::Any) => s.apply_update(&current, value),
            _ => schema.schema_at_path(path).apply_update(&current, value),
        };
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
            Update::value(Value::tree([("level", Value::float(level * (self.rate - 1.0)))]))
        }

        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
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
}
