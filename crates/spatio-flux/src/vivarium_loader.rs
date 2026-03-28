//! High-level loader: parse vivarium JSON → Topology + instantiated processes.
//!
//! This bridges the gap between the vivarium document format and prism's
//! Topology/Engine by using the process registry to determine whether
//! each node is a Process or Step.

use std::collections::HashMap;
use std::sync::Arc;

use prism_bigraph::process::ProcessNode;
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::vivarium::VivariumDocument;
use prism_bigraph::{Engine, ProcessRegistry};

/// Load a vivarium JSON document and construct a running Engine.
///
/// Uses the registry to instantiate processes and determine intervals
/// (processes have intervals, steps don't).
pub fn load_vivarium(
    json_str: &str,
    registry: Arc<ProcessRegistry>,
) -> Result<(Engine, VivariumDocument), String> {
    let vdoc =
        VivariumDocument::from_json(json_str).map_err(|e| format!("parse error: {e}"))?;

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
            .ok_or_else(|| {
                format!("unknown process type '{}' for '{}'", vproc.class_name, name)
            })?;

        // Determine interval: if it's a Process, use its declared interval
        let interval = match &node {
            ProcessNode::Process(p) => Some(
                vproc
                    .interval
                    .unwrap_or_else(|| p.interval()),
            ),
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

    let mut engine = Engine::new(topology.clone(), instances);
    // Enable dynamic process discovery for runtime composition
    engine.set_registry(Arc::clone(&registry));
    Ok((engine, topology))
}
