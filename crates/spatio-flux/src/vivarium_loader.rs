//! High-level loader: parse vivarium JSON → Topology + instantiated processes.
//!
//! This bridges the gap between the vivarium document format and prism's
//! Topology/Engine by using the process registry to determine whether
//! each node is a Process or Step.

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

use prism_bigraph::composite::{Bridge, Composite};
use prism_bigraph::process::ProcessNode;
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::vivarium::VivariumDocument;
use prism_bigraph::{Core, Engine, Key, ProcessRegistry, Schema, Value};

/// Load a vivarium JSON document and construct a running Engine.
///
/// Uses the registry to instantiate processes and determine intervals
/// (processes have intervals, steps don't).
pub fn load_vivarium(
    json_str: &str,
    registry: Arc<ProcessRegistry>,
) -> Result<(Engine, VivariumDocument), String> {
    let vdoc = VivariumDocument::from_json(json_str).map_err(|e| format!("parse error: {e}"))?;

    let (engine, _) = instantiate_vivarium(&vdoc, registry)?;
    Ok((engine, vdoc))
}

/// Instantiate a VivariumDocument into a Topology + process instances.
/// The registry is shared with the engine for dynamic process discovery.
pub fn instantiate_vivarium(
    vdoc: &VivariumDocument,
    registry: Arc<ProcessRegistry>,
) -> Result<(Engine, Topology), String> {
    let mut topology = Topology::new();
    topology.initial_state = vdoc.state.clone();
    topology.state_schema = vdoc.parse_state_schema();

    let mut instances: HashMap<String, ProcessNode> = HashMap::new();

    for (name, vproc) in &vdoc.processes {
        // Use pre-resolved wires (with .. resolved) if available, else flatten raw
        let inputs = vproc
            .resolved_inputs
            .clone()
            .unwrap_or_else(|| prism_bigraph::vivarium::flatten_wires_pub(&vproc.inputs));
        let outputs = vproc
            .resolved_outputs
            .clone()
            .unwrap_or_else(|| prism_bigraph::vivarium::flatten_wires_pub(&vproc.outputs));

        // Instantiate via registry to determine Process vs Step
        let node = registry
            .create(&vproc.class_name, vproc.config.clone())
            .ok_or_else(|| format!("unknown process type '{}' for '{}'", vproc.class_name, name))?;

        // Determine interval: if it's a Process, use its declared interval
        let interval = match &node {
            ProcessNode::Process(p) => Some(vproc.interval.unwrap_or_else(|| p.interval())),
            ProcessNode::Step(_) => None,
        };

        topology.processes.insert(
            name.clone(),
            ProcessSpec {
                process_type: vproc.class_name.clone(),
                config: vproc.config.clone(),
                inputs,
                outputs,
                interval,
                priority: 0.0,
            },
        );

        instances.insert(name.clone(), node);
    }

    // Handle composite containers: create Composite process nodes for them
    for composite_name in &vdoc.composites {
        // The composite's internal state is the container map from the state tree
        let inner_state = vdoc
            .state
            .as_map()
            .and_then(|m| m.get(composite_name.as_str()))
            .cloned()
            .unwrap_or(Value::map());

        // Auto-detect bridge: scan inner process wiring for paths that
        // reference parent state (paths NOT starting with the composite name).
        // The inner processes wire to paths like ["fields", "glucose", 0, 0]
        // which need to be bridged from the parent state.
        let mut bridge_paths: IndexMap<String, Vec<Key>> = IndexMap::new();

        // Get inner process specs to find what they wire to
        if let Some(iter) = inner_state.iter_fields() {
            for (_proc_name, proc_val) in iter {
                // Check inputs and outputs wiring
                for wire_key in ["inputs", "outputs"] {
                    if let Some(wires) = proc_val.get_field(wire_key) {
                        collect_bridge_roots(wires, &mut bridge_paths);
                    }
                }
            }
        }

        // Build bridge: each root path (e.g., "fields") maps both in and out
        let mut input_mappings = IndexMap::new();
        let mut output_mappings = IndexMap::new();
        for (root, path) in &bridge_paths {
            input_mappings.insert(root.clone(), path.clone());
            output_mappings.insert(root.clone(), path.clone());
        }
        let input_bridge = Bridge {
            mappings: input_mappings,
        };
        let output_bridge = Bridge {
            mappings: output_mappings,
        };

        // Build port schemas from the parsed state schema
        let state_schema = &topology.state_schema;
        let input_schemas: IndexMap<String, Schema> = input_bridge
            .mappings
            .keys()
            .map(|k| (k.clone(), Schema::Any))
            .collect();
        // Build output schemas from the parsed state schema.
        // The composite outputs DELTAS which the parent applies additively.
        // For field arrays, the schema tells the parent to use Array element-wise apply.
        let output_schemas: IndexMap<String, Schema> = output_bridge
            .mappings
            .keys()
            .map(|k| {
                let schema = if let Schema::Tree { branches } = state_schema {
                    branches.get(k.as_str()).cloned().unwrap_or(Schema::Any)
                } else {
                    Schema::Any
                };
                (k.clone(), schema)
            })
            .collect();

        // Build inner engine: the state is the parent's state (bridged fields
        // are injected before each run). Inner processes are discovered.
        // The inner engine needs a copy of the parent state for the bridged paths.
        let mut inner_full_state: IndexMap<Key, Value> = IndexMap::new();
        // Include the composite's own children (process specs)
        if let Some(iter) = inner_state.iter_fields() {
            for (k, v) in iter {
                inner_full_state.insert(k.clone(), v.clone());
            }
        }
        // Include bridged state from parent
        if let Some(parent_map) = vdoc.state.as_map() {
            for (root, _) in &bridge_paths {
                if let Some(val) = parent_map.get(root.as_str()) {
                    inner_full_state.insert(Key::from(root.as_str()), val.clone());
                }
            }
        }

        let inner_topology = Topology {
            processes: IndexMap::new(),
            initial_state: Value::Map(inner_full_state),
            state_schema: topology.state_schema.clone(), // inherit parent schema
        };
        let mut inner_engine = Engine::new(inner_topology, HashMap::new());
        inner_engine.set_core(Core::from(Arc::clone(&registry)));

        // Discover all process specs in the inner state
        inner_engine.discover_all_processes();

        let composite = Composite::new(
            inner_engine,
            input_bridge,
            output_bridge,
            input_schemas,
            output_schemas,
            1.0,
        );

        let spec = ProcessSpec {
            process_type: "Composite".to_string(),
            config: Value::None,
            inputs: bridge_paths.clone(),
            outputs: bridge_paths,
            interval: Some(1.0),
            priority: 0.0,
        };

        topology.processes.insert(composite_name.clone(), spec);
        instances.insert(
            composite_name.clone(),
            ProcessNode::Process(Box::new(composite)),
        );
    }

    // Debug: show what's in topology
    if !vdoc.composites.is_empty() {
        eprintln!("[vivarium] composites: {:?}", vdoc.composites);
        eprintln!(
            "[vivarium] topology processes: {:?}",
            topology.processes.keys().collect::<Vec<_>>()
        );
    }

    let mut engine = Engine::new(topology.clone(), instances);
    // Enable dynamic process discovery: hand the engine its Core (registry-only
    // here — vivarium has no custom types/methods/protocols to thread).
    engine.set_core(Core::from(Arc::clone(&registry)));
    Ok((engine, topology))
}

/// Collect root path names from process wiring for bridge auto-detection.
/// If a process wires to ["..", "fields", "glucose", 0, 0], the root is "fields".
fn collect_bridge_roots(wires: &Value, roots: &mut IndexMap<String, Vec<Key>>) {
    // Find the first non-".." string element in a path
    fn find_root(path: &[Value]) -> Option<String> {
        path.iter()
            .filter_map(|v| v.as_str())
            .find(|s| *s != "..")
            .map(|s| s.to_string())
    }

    match wires {
        Value::Map(map) => {
            for (_port, target) in map {
                match target {
                    Value::List(path) => {
                        if let Some(root) = find_root(path) {
                            roots
                                .entry(root.clone())
                                .or_insert_with(|| vec![Key::from(root.as_str())]);
                        }
                    }
                    Value::Map(sub_map) => {
                        for (_sub, sub_target) in sub_map {
                            if let Value::List(path) = sub_target {
                                if let Some(root) = find_root(path) {
                                    roots
                                        .entry(root.clone())
                                        .or_insert_with(|| vec![Key::from(root.as_str())]);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
