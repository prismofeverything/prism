//! Composite — a Process that wraps an inner Engine.
//!
//! In process-bigraph, every composite IS an engine. A composite runs its
//! own internal processes and exposes input/output ports that bridge to
//! its internal state. When embedded in a parent engine, the parent sees
//! it as a single Process node.
//!
//! Bridge pattern — ASYMMETRIC, and the asymmetry is load-bearing:
//! - Input bridge: **STATE in.** Maps external port names to internal
//!   state paths; external input values are SET (overwritten) into the
//!   internal state at those paths before the inner engine runs.
//! - Output bridge: **UPDATES out.** Maps internal state paths to external
//!   port names; after running, the composite emits a *delta* per port —
//!   NOT raw state. A regular port diffs pre/post; a **passthrough** port
//!   (one whose internal path is absent from the inner state) instead
//!   forwards the inner update's RAW delta with `_add`/`_remove` intact.
//!
//! Why the asymmetry matters: structural change has to cross the boundary
//! as an *update*, not as replaced state. A subengine that OMITS a slot
//! makes that port a passthrough, so an inner step's `{_remove, _add}`
//! reaches the parent unchanged — this is how a cell divides itself
//! (removes itself, adds daughters) up into its container. See
//! `tests/growth_division.rs` and the chrysalis grow/divide fixture.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Mutex;

use indexmap::IndexMap;

use prism_schema::{Key, Path, Value};

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
        core: &crate::core::Core,
    ) -> Option<Self> {
        let map = config.as_map()?;

        // Parse inner document
        let inner_state = map.get("state").cloned().unwrap_or(Value::map());

        // Parse bridges
        let bridge_val = map.get("bridge").and_then(|v| v.as_map())?;
        let input_bridge = parse_bridge(bridge_val.get("inputs")?)?;
        let output_bridge = parse_bridge(bridge_val.get("outputs")?)?;

        let interval = map.get("interval")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        // Carry a REAL inner schema (law #10: the Composite runs on the
        // algebra, not `Schema::Any`). PREFER the DECLARED schema the spec
        // carries (`config.schema`) — it has the apply-critical types inference
        // can't recover (additive `Array`/`Delta`, `Link`s). Resolve it over the
        // inferred floor so it stays schema-complete, exactly like the top-level
        // engine's `resolve(infer(state), declared)`. Bare `infer` only when no
        // schema was carried (which would degrade an additive field to `List`
        // and break the output bridge).
        let inferred = prism_schema::algebra::infer(&inner_state);
        let inner_schema = match map.get("schema").and_then(prism_schema::value_to_schema) {
            Some(declared) => prism_schema::algebra::resolve(&inferred, &declared),
            None => inferred,
        };
        let inner_topology = crate::topology::Topology {
            processes: IndexMap::new(),
            initial_state: inner_state,
            state_schema: inner_schema,
        };
        let mut engine = Engine::new(inner_topology, HashMap::new());
        // The subengine inherits the WHOLE core (types + processes + methods +
        // protocols), so a Custom-typed / method-using / `rest:`-addressed process
        // works inside a composite exactly as at the top level.
        engine.set_core(core.clone());

        // Discover all process specs in the inner state
        engine.discover_all_processes();

        // Port schemas are the inner schema AT THE BRIDGED PATHS (not `Any`):
        // each port carries its real type, so the engine PROMOTES an output
        // port's schema onto the parent slot it writes — additive `Array`
        // fields then apply element-wise across the bridge (nested GOTCHA #14)
        // instead of replacing.
        let input_schemas: PortSchema = input_bridge.mappings.iter()
            .map(|(port, path)| (port.clone(), engine.schema().schema_at_path(path).clone()))
            .collect();
        let output_schemas: PortSchema = output_bridge.mappings.iter()
            .map(|(port, path)| (port.clone(), engine.schema().schema_at_path(path).clone()))
            .collect();

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
        for (port, internal_path) in &self.input_bridge.mappings {
            if let Some(val) = state.get_field(port.as_str()) {
                engine.state_mut().set_path(internal_path, val.clone());
            }
        }

        // 2. Identify passthrough ports (not in inner state — used for
        //    _add/_remove that passes through to parent).
        let mut passthrough = std::collections::HashSet::new();
        for (_port, internal_path) in &self.output_bridge.mappings {
            if engine.state().get_path(internal_path).is_none() {
                if let Some(root) = internal_path.first() {
                    passthrough.insert(root.clone());
                }
            }
        }
        engine.set_passthrough_paths(passthrough.clone());
        engine.take_passthrough_deltas();

        // 3. Snapshot ONLY non-passthrough output ports.
        //    Passthrough ports skip snapshot entirely (no cloning!).
        let mut pre_run: IndexMap<String, Value> = IndexMap::new();
        for (port, internal_path) in &self.output_bridge.mappings {
            let root = internal_path.first().map(|k| k.as_str()).unwrap_or("");
            if passthrough.contains(root) {
                continue; // Skip — handled by passthrough
            }
            let val = engine.state().get_path(internal_path)
                .cloned()
                .unwrap_or(Value::None);
            pre_run.insert(port.clone(), val);
        }

        // 4. If the inner engine has no temporal processes, queue bridged
        //    paths so steps fire from bridge input alone. If there ARE
        //    processes, they'll trigger steps naturally via their outputs.
        if engine.fronts_count() == 0 {
            let bridged_paths: Vec<Path> = self.input_bridge.mappings.values().cloned().collect();
            engine.queue_changes(bridged_paths);
        }

        // 5. Run inner engine.
        engine.run(interval);

        // 5. Build output deltas.
        //    Passthrough ports: use captured raw deltas (preserves _add/_remove).
        //    Regular ports: diff pre/post (preserves List/Array structure).
        let pt_deltas = engine.take_passthrough_deltas();
        let mut output: IndexMap<Key, Value> = IndexMap::new();
        for (port, internal_path) in &self.output_bridge.mappings {
            let port_key = Key::from(port.as_str());
            let root = internal_path.first().map(|k| k.clone()).unwrap_or_default();

            if let Some(pt_delta) = pt_deltas.get(&root) {
                // Passthrough: raw delta with _add/_remove intact
                if !is_zero_delta(pt_delta) {
                    output.insert(port_key, pt_delta.clone());
                }
            } else {
                // Regular: the output update is `diff` of pre/post under the
                // inner slot's schema (the algebra's view/bridge-out) — a
                // numeric `Delta`, a per-key Map delta with `_add`/`_remove`,
                // etc. This replaces the hand-rolled `compute_delta`.
                let new_val = engine.state().get_path(internal_path)
                    .cloned().unwrap_or(Value::None);
                let old_val = pre_run.get(port).unwrap_or(&Value::None);
                let slot_schema = engine.schema().schema_at_path(internal_path);
                if let Some(delta) = prism_schema::algebra::diff_with(
                    engine.type_registry().map(|r| &**r),
                    slot_schema,
                    old_val,
                    &new_val,
                ) {
                    if !is_zero_delta(&delta) {
                        output.insert(port_key, delta);
                    }
                }
            }
        }

        // Clear passthrough for next tick
        engine.set_passthrough_paths(std::collections::HashSet::new());

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

/// Check if a delta is effectively zero (no change).
fn is_zero_delta(val: &Value) -> bool {
    match val {
        Value::Float(f) => f.0.abs() < 1e-15,
        Value::Int(i) => *i == 0,
        Value::Map(m) => m.is_empty() || m.values().all(is_zero_delta),
        Value::Struct { values, .. } => values.is_empty() || values.iter().all(is_zero_delta),
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
                .filter_map(|v| v.as_str().map(Key::from))
                .collect();
            mappings.insert(port.to_string(), path);
        }
    }
    Some(Bridge { mappings })
}
