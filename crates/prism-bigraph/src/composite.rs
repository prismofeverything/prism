//! Composite — a Process that wraps an inner Engine.
//!
//! In process-bigraph, every composite IS an engine. A composite runs its
//! own internal processes and exposes input/output ports that bridge to
//! its internal state. When embedded in a parent engine, the parent sees
//! it as a single Process node.
//!
//! Bridge pattern:
//! - Input bridge: maps external port names to internal state paths.
//!   When update() is called, external input values are written into
//!   the internal state at the bridged paths.
//! - Output bridge: maps internal state paths to external port names.
//!   After running the internal engine, output values are read from
//!   the internal state at the bridged paths and returned as the update.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Mutex;

use indexmap::IndexMap;

use prism_schema::{Path, Schema, Value};

use crate::engine::Engine;
use crate::ports::PortSchema;
use crate::process::Process;
use crate::update::Update;

/// Bridge specification: maps port names to internal state paths.
#[derive(Clone, Debug)]
pub struct Bridge {
    /// External port name → internal state path
    pub mappings: IndexMap<String, Path>,
}

/// A composite process: wraps an inner Engine with bridged I/O.
///
/// When the parent engine calls `update(state, interval)`:
/// 1. Input values are projected into the inner engine's state via input bridges
/// 2. The inner engine runs for `interval`
/// 3. Output values are read from the inner engine's state via output bridges
/// 4. Output deltas are returned to the parent
#[derive(Debug)]
pub struct Composite {
    /// The inner engine (behind Mutex for interior mutability in Process trait).
    inner: Mutex<Engine>,

    /// Input bridge: port name → internal state path.
    /// Values from parent are SET (overwrite) into these paths before running.
    input_bridge: Bridge,

    /// Output bridge: port name → internal state path.
    /// Values are READ from these paths after running and returned as output.
    output_bridge: Bridge,

    /// Port schemas for inputs.
    input_schemas: PortSchema,

    /// Port schemas for outputs.
    output_schemas: PortSchema,

    /// Process interval (how often the parent engine fires this composite).
    pub interval: f64,

}

impl Composite {
    pub fn new(
        engine: Engine,
        input_bridge: Bridge,
        output_bridge: Bridge,
        input_schemas: PortSchema,
        output_schemas: PortSchema,
        interval: f64,
    ) -> Self {
        Self {
            inner: Mutex::new(engine),
            input_bridge,
            output_bridge,
            input_schemas,
            output_schemas,
            interval,
        }
    }

    /// Build a Composite from a vivarium-style config Value.
    ///
    /// Expected config structure:
    /// ```json
    /// {
    ///   "state": { ... inner state ... },
    ///   "bridge": {
    ///     "inputs": { "port_name": ["internal", "path"] },
    ///     "outputs": { "port_name": ["internal", "path"] }
    ///   },
    ///   "interval": 1.0
    /// }
    /// ```
    ///
    /// The inner state should contain process specs that will be discovered
    /// when the inner engine runs.
    pub fn from_config(
        config: &Value,
        registry: std::sync::Arc<crate::factory::ProcessRegistry>,
    ) -> Option<Self> {
        let map = config.as_map()?;

        // Parse inner document
        let inner_state = map.get("state").cloned().unwrap_or(Value::map());

        // Parse bridges
        let bridge_val = map.get("bridge").and_then(|v| v.as_map())?;
        let input_bridge = parse_bridge(bridge_val.get("inputs")?)?;
        let output_bridge = parse_bridge(bridge_val.get("outputs")?)?;

        // Build schemas from bridge structure (default to Any)
        let input_schemas: PortSchema = input_bridge.mappings.keys()
            .map(|k| (k.clone(), Schema::Any))
            .collect();
        let output_schemas: PortSchema = output_bridge.mappings.keys()
            .map(|k| (k.clone(), Schema::Any))
            .collect();

        let interval = map.get("interval")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        // Build inner engine from state (processes discovered from embedded specs)
        let inner_topology = crate::topology::Topology {
            processes: IndexMap::new(),
            initial_state: inner_state,
            state_schema: Schema::Any,
        };
        let mut engine = Engine::new(inner_topology, HashMap::new());
        engine.set_registry(registry);

        // Discover all process specs in the inner state
        engine.discover_all_processes();

        Some(Self::new(engine, input_bridge, output_bridge,
                       input_schemas, output_schemas, interval))
    }
}

impl Process for Composite {
    fn inputs(&self) -> PortSchema {
        self.input_schemas.clone()
    }

    fn outputs(&self) -> PortSchema {
        self.output_schemas.clone()
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let mut engine = self.inner.lock().unwrap();

        // 1. Bridge inputs: write external values into internal state
        if let Some(input_map) = state.as_map() {
            for (port, internal_path) in &self.input_bridge.mappings {
                if let Some(val) = input_map.get(port) {
                    engine.state_mut().set_path(internal_path, val.clone());
                }
            }
        }

        // 2. Snapshot AFTER bridging inputs
        let mut pre_run: IndexMap<String, Value> = IndexMap::new();
        for (port, internal_path) in &self.output_bridge.mappings {
            let val = engine.state().get_path(internal_path)
                .cloned()
                .unwrap_or(Value::None);
            pre_run.insert(port.clone(), val);
        }

        // 3. Run inner engine
        engine.run(interval);

        static CC: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = CC.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Debug output removed — composite bridge verified working.

        // 4. Compute deltas (what the inner processes changed)
        let mut output = IndexMap::new();
        for (port, internal_path) in &self.output_bridge.mappings {
            let new_val = engine.state().get_path(internal_path)
                .cloned()
                .unwrap_or(Value::None);
            let old_val = pre_run.get(port).unwrap_or(&Value::None);
            let delta = compute_delta(old_val, &new_val);
            if !is_zero_delta(&delta) {
                output.insert(port.clone(), delta);
            }
        }

        if output.is_empty() {
            Update::Noop
        } else {
            Update::value(Value::Map(output))
        }
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

// Safety: Composite is Send+Sync because inner Engine is behind a Mutex.
// The Mutex ensures only one thread accesses the engine at a time.

/// Compute the delta between old and new values recursively.
/// For Floats: new - old (additive delta).
/// For Maps: recurse into each key, producing a map of deltas.
/// For Lists: element-wise delta if same length, else replace.
fn compute_delta(old: &Value, new: &Value) -> Value {
    match (old, new) {
        (Value::Float(o), Value::Float(n)) => Value::float(n.0 - o.0),
        (Value::Int(o), Value::Int(n)) => Value::Int(n - o),
        (Value::Map(old_map), Value::Map(new_map)) => {
            // Fast path: if key sets are identical, skip structural detection.
            // Only build HashSets when key counts differ (structural change likely).
            let keys_changed = old_map.len() != new_map.len()
                || old_map.keys().any(|k| !new_map.contains_key(k));

            if keys_changed {
                let old_keys: std::collections::HashSet<&String> = old_map.keys().collect();
                let new_keys: std::collections::HashSet<&String> = new_map.keys().collect();
                let removed: Vec<&String> = old_keys.difference(&new_keys).copied().collect();
                let added: Vec<&String> = new_keys.difference(&old_keys).copied().collect();
                let mut delta = IndexMap::new();

                if !removed.is_empty() {
                    delta.insert("_remove".to_string(), Value::List(
                        removed.iter().map(|k| Value::String((*k).clone())).collect()
                    ));
                }
                if !added.is_empty() {
                    let adds: IndexMap<String, Value> = added.iter()
                        .map(|k| ((*k).clone(), new_map.get(*k).unwrap().clone()))
                        .collect();
                    delta.insert("_add".to_string(), Value::Map(adds));
                }

                // For surviving keys, compute normal deltas — but skip
                // keys that were removed (they're gone) or added (they're new).
                for (k, new_v) in new_map {
                    if added.contains(&k) || removed.contains(&k) { continue; }
                    let old_v = old_map.get(k).unwrap_or(&Value::None);
                    let d = compute_delta(old_v, new_v);
                    if !is_zero_delta(&d) {
                        delta.insert(k.clone(), d);
                    }
                }

                return Value::Map(delta);
            }

            // No structural changes — normal per-key delta
            let mut delta = IndexMap::new();
            for (k, new_v) in new_map {
                let old_v = old_map.get(k).unwrap_or(&Value::None);
                let d = compute_delta(old_v, new_v);
                if !is_zero_delta(&d) {
                    delta.insert(k.clone(), d);
                }
            }
            Value::Map(delta)
        }
        (Value::List(old_list), Value::List(new_list)) if old_list.len() == new_list.len() => {
            let deltas: Vec<Value> = old_list.iter().zip(new_list.iter())
                .map(|(o, n)| compute_delta(o, n))
                .collect();
            Value::List(deltas)
        }
        _ => new.clone(), // Fallback: full replacement
    }
}

/// Check if a delta is effectively zero (no change).
fn is_zero_delta(val: &Value) -> bool {
    match val {
        Value::Float(f) => f.0.abs() < 1e-15,
        Value::Int(i) => *i == 0,
        Value::Map(m) => m.is_empty() || m.values().all(is_zero_delta),
        Value::List(l) => l.iter().all(is_zero_delta),
        Value::None => true,
        _ => false,
    }
}

fn parse_bridge(val: &Value) -> Option<Bridge> {
    let map = val.as_map()?;
    let mut mappings = IndexMap::new();
    for (port, path_val) in map {
        if let Some(path_list) = path_val.as_list() {
            let path: Path = path_list.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            mappings.insert(port.clone(), path);
        }
    }
    Some(Bridge { mappings })
}
