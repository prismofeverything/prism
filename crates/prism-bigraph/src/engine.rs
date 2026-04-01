//! The composition engine — steps processes and reconciles state.
//!
//! This is the Rust equivalent of process-bigraph's Composite.
//! It manages the shared state tree, schedules temporal processes
//! by their intervals, triggers dependency-based steps, and
//! applies updates with proper merge semantics.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ordered_float::OrderedFloat;

use prism_schema::{Path, Schema, Value};

use crate::factory::ProcessRegistry;
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
                    &[name.clone(), "interval".to_string()],
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

        Self {
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
        }
    }

    /// Set the process registry for dynamic process discovery.
    /// When set, new process nodes appearing in state (e.g., via _add)
    /// are automatically instantiated and wired.
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
    pub fn get(&self, path: &[String]) -> Option<&Value> {
        self.state.get_path(path)
    }

    /// Run the simulation for the given duration.
    pub fn run(&mut self, duration: f64) {
        let end_time = self.time + duration;
        let mut iter_count = 0u64;

        while self.time < end_time {
            // Find ALL processes that fire at the next time point
            let next_time = self.next_fire_time(end_time);

            match next_time {
                Some(fire_time) => {
                    self.time = fire_time;

                    // Collect all processes firing at this time
                    let firing: Vec<String> = self.fronts
                        .iter()
                        .filter(|(_, front)| (front.next_time - fire_time).abs() < 1e-10)
                        .map(|(name, _)| name.clone())
                        .collect();

                    // Run all of them and collect changed paths
                    let mut all_changed = Vec::new();
                    for name in &firing {
                        let changed = self.run_process(name);
                        all_changed.extend(changed);
                    }

                    // Only scan for new processes when structural changes
                    // (_add/_remove) occurred — skip on pure value updates.
                    // The structural flag is set during trigger_steps below.
                    // Process outputs rarely contain _add/_remove directly,
                    // but composites might, so always discover after processes.
                    self.discover_processes(&all_changed);

                    // Trigger steps ONCE after all processes at this time have run
                    self.trigger_steps(&all_changed);

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
        let interface = match self.interfaces.get(name) {
            Some(i) => i.clone(),
            None => return Vec::new(),
        };
        let interval = match self.fronts.get(name) {
            Some(front) => front.interval,
            None => return Vec::new(),
        };

        let input_state = interface.view(&self.state);

        let update = match self.nodes.get(name) {
            Some(ProcessNode::Process(p)) => p.update(&input_state, interval),
            _ => return Vec::new(),
        };

        // Advance the schedule
        self.fronts.get_mut(name).unwrap().next_time += interval;

        if let Some(update_value) = update.into_value() {
            let projections = interface.project(&update_value);
            // Processes output deltas — apply directly, no diff needed
            let (changed, _structural) = self.apply_projections(&projections);
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
        let mut changed = Vec::new();
        let mut structural = false;
        for (path, value, port_schema) in projections {
            // Detect structural changes (_add/_remove in the update value)
            if !structural {
                if let Some(map) = value.as_map() {
                    if map.contains_key("_add") || map.contains_key("_remove") {
                        structural = true;
                    }
                }
            }
            let current = self.state.get_path(path).cloned().unwrap_or(Value::None);
            // Use port-specific schema if it's more specific than Any.
            // Otherwise walk the state schema for this path.
            let new_value = match port_schema {
                Some(schema) if !matches!(schema, Schema::Any) => {
                    schema.apply_update(&current, value)
                }
                _ => {
                    let path_schema = self.schema.schema_at_path(path);
                    path_schema.apply_update(&current, value)
                }
            };
            self.state.set_path(path, new_value);
            changed.push(path.clone());
        }
        (changed, structural)
    }

    /// Fire any steps whose inputs overlap with the changed paths.
    fn trigger_steps(&mut self, changed_paths: &[Path]) {
        let mut triggered: Vec<String> = Vec::new();

        for path in changed_paths {
            // Check exact path matches
            if let Some(steps) = self.step_triggers.get(path) {
                triggered.extend(steps.iter().cloned());
            }
            // Check prefix matches (a change to "cell.glucose" should
            // trigger a step wired to "cell")
            for i in 1..path.len() {
                let prefix = path[..i].to_vec();
                if let Some(steps) = self.step_triggers.get(&prefix) {
                    triggered.extend(steps.iter().cloned());
                }
            }
        }

        // Deduplicate and sort by priority
        triggered.sort();
        triggered.dedup();
        triggered.sort_by(|a, b| {
            let pa = self.specs.get(a).map(|s| s.priority).unwrap_or(0.0);
            let pb = self.specs.get(b).map(|s| s.priority).unwrap_or(0.0);
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal) // higher first
        });

        // Run triggered steps with cascade: a step's output can trigger
        // downstream steps, but each step runs at most once per cycle.
        // Process discovery is deferred until after all steps complete
        // to avoid adding new steps mid-cascade.
        let mut already_run = HashSet::new();
        let mut queue = triggered;
        let mut all_step_changes = Vec::new();
        let mut any_structural = false;

        while let Some(step_name) = queue.pop() {
            if already_run.contains(&step_name) {
                continue;
            }
            already_run.insert(step_name.clone());

            let interface = match self.interfaces.get(&step_name) {
                Some(i) => i.clone(),
                None => continue,
            };

            let input_state = interface.view(&self.state);

            let update = match self.nodes.get(&step_name) {
                Some(ProcessNode::Step(s)) => s.update(&input_state),
                _ => continue,
            };

            if let Some(update_value) = update.into_value() {
                let projections = interface.project(&update_value);
                let (newly_changed, structural) = self.apply_projections(&projections);
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
                    // Also check prefix matches
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

        // Only discover new processes when structural changes (_add/_remove)
        // occurred. Most ticks only update positions/masses — skipping
        // discovery on those saves significant overhead.
        if any_structural && !all_step_changes.is_empty() {
            self.discover_processes(&all_step_changes);
        }
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

        // Check each changed path for new process nodes underneath
        for path in changed_paths {
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
                let parent_path: Vec<String> = name[..dot_pos]
                    .split('.')
                    .map(|s| s.to_string())
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

    /// Recursively scan a map for process specs (entries with "address").
    fn scan_for_processes(
        &self,
        map: &prism_schema::StateMap,
        parent_path: &[String],
        registry: &ProcessRegistry,
        results: &mut Vec<(String, ProcessSpec, ProcessNode)>,
    ) {
        for (key, val) in map {
            if let Value::Map(child_map) = val {
                // Check if this is a process node (has "address" with "data")
                if let Some(Value::Map(addr)) = child_map.get("address") {
                    if let Some(Value::String(class_name)) = addr.get("data") {
                        if class_name == "RAMEmitter" {
                            continue;
                        }

                        let mut proc_path = parent_path.to_vec();
                        proc_path.push(key.clone());
                        let proc_name = proc_path.join(".");

                        // Already registered?
                        if self.nodes.contains_key(&proc_name) {
                            continue;
                        }

                        // Get config
                        let config = child_map
                            .get("config")
                            .cloned()
                            .unwrap_or(Value::None);

                        // Try to instantiate
                        if let Some(node) = registry.create(class_name, config.clone()) {
                            // Resolve wiring relative to process location
                            let inputs_val = child_map
                                .get("inputs")
                                .cloned()
                                .unwrap_or(Value::None);
                            let outputs_val = child_map
                                .get("outputs")
                                .cloned()
                                .unwrap_or(Value::None);

                            let inputs = crate::vivarium::flatten_wires_with_context_pub(
                                &inputs_val,
                                parent_path,
                            );
                            let outputs = crate::vivarium::flatten_wires_with_context_pub(
                                &outputs_val,
                                parent_path,
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
                                process_type: class_name.clone(),
                                config,
                                inputs,
                                outputs,
                                interval,
                                priority: 0.0,
                            };

                            results.push((proc_name, spec, node));
                        }
                    }
                } else {
                    // Recurse into non-process maps (e.g., into each particle).
                    // Skip entries that are already registered as process nodes
                    // (e.g., composites — their internals are private).
                    let mut child_path = parent_path.to_vec();
                    child_path.push(key.clone());
                    let child_name = child_path.join(".");
                    if !self.nodes.contains_key(&child_name) {
                        self.scan_for_processes(child_map, &child_path, registry, results);
                    }
                }
            }
        }
    }
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
            IndexMap::from([("level".to_string(), vec!["level".to_string()])]),
            IndexMap::from([("level".to_string(), vec!["level".to_string()])]),
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
            .get(&["level".to_string()])
            .and_then(|v| v.as_f64())
            .unwrap();
        // After 3 ticks at rate 1.1: 1.0 * 1.1 * 1.1 * 1.1 ≈ 1.331
        assert!((level - 1.331).abs() < 0.001);
    }
}
