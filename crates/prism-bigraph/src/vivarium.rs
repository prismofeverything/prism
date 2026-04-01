//! Vivarium-compatible document loader.
//!
//! Parses the Python process-bigraph JSON format where processes
//! are embedded in the state tree (identified by having an `address` field).
//! Extracts process specs, initial state, and wiring to construct a Topology.

use indexmap::IndexMap;

use prism_schema::{Schema, Value};

use crate::topology::{ProcessSpec, Topology};

/// A parsed process from a vivarium document.
#[derive(Clone, Debug)]
pub struct VivariumProcess {
    /// The process class name (from address.data).
    pub class_name: String,
    /// Process configuration.
    pub config: Value,
    /// Input wiring — may be flat or nested.
    pub inputs: Value,
    /// Output wiring — may be flat or nested.
    pub outputs: Value,
    /// Type expression for inputs.
    pub inputs_type: Option<String>,
    /// Type expression for outputs.
    pub outputs_type: Option<String>,
    /// Whether this is a step (no interval) or process (has interval).
    pub interval: Option<f64>,
    /// Resolved input wires (after `..` resolution and path flattening).
    pub resolved_inputs: Option<IndexMap<String, Vec<String>>>,
    /// Resolved output wires.
    pub resolved_outputs: Option<IndexMap<String, Vec<String>>>,
}

/// Result of parsing a vivarium JSON document.
#[derive(Clone, Debug)]
pub struct VivariumDocument {
    /// Process specs extracted from the state tree.
    pub processes: IndexMap<String, VivariumProcess>,
    /// The state tree with process nodes removed (pure data).
    pub state: Value,
    /// Schema section if present.
    pub schema: Option<Value>,
    /// Composite containers: top-level keys whose children are ALL process specs.
    /// These should be instantiated as Composite processes, not flattened.
    pub composites: Vec<String>,
}

impl VivariumDocument {
    /// Parse a vivarium JSON document.
    ///
    /// Walks the state tree looking for nodes that have an `address` field.
    /// Those are process/step specs. Everything else is data state.
    pub fn parse(json: &Value) -> Self {
        let mut processes = IndexMap::new();
        let mut state = Value::map();
        let mut composites = Vec::new();

        let root_state = match json.as_map().and_then(|m| m.get("state")) {
            Some(s) => s,
            None => json, // treat the whole thing as state
        };

        let schema = json
            .as_map()
            .and_then(|m| m.get("schema"))
            .cloned();

        if let Some(state_map) = root_state.as_map() {
            let mut clean_state = IndexMap::new();

            for (key, value) in state_map {
                if is_process_node(value) {
                    if let Some(proc) = extract_process(value) {
                        if proc.class_name != "RAMEmitter" {
                            processes.insert(key.clone(), proc);
                        }
                    }
                    // Keep process specs in state — they're active state
                    clean_state.insert(key.clone(), value.clone());
                } else if key == "global_time" {
                    clean_state.insert(key.clone(), value.clone());
                } else if is_composite_container(value) {
                    // This is a container of process specs → treat as a Composite.
                    // Don't flatten; keep as-is in state for the Composite engine.
                    composites.push(key.clone());
                    clean_state.insert(key.clone(), value.clone());
                } else {
                    // Recursively check for processes inside particles etc.
                    let (cleaned, nested_procs) =
                        extract_nested_processes(value, &[key.clone()]);
                    for (path_name, proc) in nested_procs {
                        processes.insert(path_name, proc);
                    }
                    clean_state.insert(key.clone(), cleaned);
                }
            }

            state = Value::Map(clean_state);
        }

        Self {
            processes,
            state,
            schema,
            composites,
        }
    }

    /// Parse the schema section into a Schema tree.
    /// The schema maps state keys to type expression strings.
    pub fn parse_state_schema(&self) -> Schema {
        let schema_val = match &self.schema {
            Some(v) => v,
            None => return Schema::Any,
        };
        let map = match schema_val.as_map() {
            Some(m) => m,
            None => return Schema::Any,
        };

        let mut branches = IndexMap::new();
        for (key, val) in map {
            if let Some(type_str) = val.as_str() {
                // Check if it's a tree expression (key1:type1|key2:type2)
                if type_str.contains('|') && type_str.contains(':') && !type_str.starts_with("link") {
                    branches.insert(key.clone(), prism_schema::parse_tree_expression(type_str));
                } else {
                    branches.insert(key.clone(), prism_schema::parse_type_expression(type_str));
                }
            } else if let Some(sub_map) = val.as_map() {
                // Nested schema (for things like particles with sub-schemas)
                // Recurse with a sub-document
                let sub_doc = VivariumDocument {
                    processes: IndexMap::new(),
                    state: Value::map(),
                    schema: Some(Value::Map(sub_map.clone())),
                    composites: vec![],
                };
                branches.insert(key.clone(), sub_doc.parse_state_schema());
            }
        }

        if branches.is_empty() {
            Schema::Any
        } else {
            Schema::Tree { branches }
        }
    }

    /// Convert to a prism Topology.
    ///
    /// Flattens nested wiring into simple path vectors.
    pub fn to_topology(&self) -> Topology {
        let mut topology = Topology::new();
        topology.initial_state = self.state.clone();
        topology.state_schema = self.parse_state_schema();

        for (name, vproc) in &self.processes {
            // Use pre-resolved wires if available (handles .. and nesting)
            let inputs = vproc
                .resolved_inputs
                .clone()
                .unwrap_or_else(|| flatten_wires(&vproc.inputs));
            let outputs = vproc
                .resolved_outputs
                .clone()
                .unwrap_or_else(|| flatten_wires(&vproc.outputs));

            topology.processes.insert(
                name.clone(),
                ProcessSpec {
                    process_type: vproc.class_name.clone(),
                    config: vproc.config.clone(),
                    inputs,
                    outputs,
                    interval: vproc.interval,
                    priority: 0.0,
                },
            );
        }

        // Add composite containers as single "Composite" process entries
        for composite_name in &self.composites {
            // Auto-detect bridge from child process wiring
            let mut bridge_roots: IndexMap<String, Vec<String>> = IndexMap::new();
            if let Some(Value::Map(container)) = self.state.as_map()
                .and_then(|m| m.get(composite_name))
            {
                for (_key, child) in container {
                    if let Some(child_map) = child.as_map() {
                        for wire_key in ["inputs", "outputs"] {
                            if let Some(wires) = child_map.get(wire_key).and_then(|v| v.as_map()) {
                                for (_port, target) in wires {
                                    // Extract root path from wiring
                                    // Extract the first non-".." element as the root
                                    let find_root = |path: &[Value]| -> Option<String> {
                                        path.iter()
                                            .filter_map(|v| v.as_str())
                                            .find(|s| *s != "..")
                                            .map(|s| s.to_string())
                                    };
                                    let root = match target {
                                        Value::List(path) => find_root(path),
                                        Value::Map(sub) => sub.values().next()
                                            .and_then(|v| v.as_list())
                                            .and_then(|l| find_root(l)),
                                        _ => None,
                                    };
                                    if let Some(r) = root {
                                        bridge_roots.entry(r.clone())
                                            .or_insert_with(|| vec![r]);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            topology.processes.insert(
                composite_name.clone(),
                ProcessSpec {
                    process_type: "Composite".to_string(),
                    config: Value::None,
                    inputs: bridge_roots.clone(),
                    outputs: bridge_roots,
                    interval: Some(1.0),
                    priority: 0.0,
                },
            );
        }

        topology
    }

    /// Load from a JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        let value: Value = serde_json::from_str(json_str)?;
        Ok(Self::parse(&value))
    }

    /// Load from a file.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let json_str = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::from_json(&json_str).map_err(|e| e.to_string())
    }
}

/// Check if a value is a process node (has an `address` field).
fn is_process_node(value: &Value) -> bool {
    value
        .as_map()
        .is_some_and(|m| m.contains_key("address"))
}

/// Check if a value is a composite container: a map where ALL children
/// are process specs (have "address" key) and there are multiple children.
fn is_composite_container(value: &Value) -> bool {
    if let Some(map) = value.as_map() {
        if map.len() < 2 {
            return false;
        }
        map.values().all(|v| is_process_node(v))
    } else {
        false
    }
}

/// Extract a VivariumProcess from a process node in the state tree.
fn extract_process(value: &Value) -> Option<VivariumProcess> {
    let map = value.as_map()?;

    // Get class name from address.data
    let address = map.get("address")?.as_map()?;
    let class_name = address.get("data")?.as_str()?.to_string();

    let config = map.get("config").cloned().unwrap_or(Value::None);
    let inputs = map.get("inputs").cloned().unwrap_or(Value::None);
    let outputs = map.get("outputs").cloned().unwrap_or(Value::None);

    let inputs_type = map
        .get("_inputs")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let outputs_type = map
        .get("_outputs")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Check for interval in config
    let interval = config
        .as_map()
        .and_then(|c| c.get("interval"))
        .and_then(|v| v.as_f64());

    Some(VivariumProcess {
        class_name,
        config,
        inputs,
        outputs,
        inputs_type,
        outputs_type,
        interval,
        resolved_inputs: None,
        resolved_outputs: None,
    })
}

/// Recursively extract processes nested inside state (e.g., inside particles
/// or composites). Tracks the nesting path for resolving `..` in wire paths.
fn extract_nested_processes(
    value: &Value,
    path: &[String],
) -> (Value, Vec<(String, VivariumProcess)>) {
    let mut nested_procs = Vec::new();

    match value {
        Value::Map(map) => {
            let mut cleaned = IndexMap::new();
            for (key, val) in map {
                if is_process_node(val) {
                    if let Some(mut proc) = extract_process(val) {
                        if proc.class_name != "RAMEmitter" {
                            proc.resolved_inputs =
                                Some(flatten_wires_with_context(&proc.inputs, path));
                            proc.resolved_outputs =
                                Some(flatten_wires_with_context(&proc.outputs, path));

                            let proc_name = if path.is_empty() {
                                key.clone()
                            } else {
                                format!("{}.{}", path.join("."), key)
                            };
                            nested_procs.push((proc_name, proc));
                        }
                    }
                    // KEEP process specs in state (not stripped).
                    // Dynamic discovery needs them for particle division/spawning.
                    // The engine skips already-registered processes.
                    cleaned.insert(key.clone(), val.clone());
                } else {
                    let mut child_path = path.to_vec();
                    child_path.push(key.clone());
                    let (cleaned_val, child_procs) =
                        extract_nested_processes(val, &child_path);
                    nested_procs.extend(child_procs);
                    cleaned.insert(key.clone(), cleaned_val);
                }
            }
            (Value::Map(cleaned), nested_procs)
        }
        _ => (value.clone(), nested_procs),
    }
}

/// Flatten nested wiring into simple path vectors.
///
/// Vivarium wiring can be:
/// - Simple: `{"particles": ["particles"]}` → `{"particles": ["particles"]}`
/// - Nested: `{"substrates": {"glucose": ["fields", "glucose"]}}` → `{"substrates.glucose": ["fields", "glucose"]}`
/// Public access to flatten_wires for external loaders.
pub fn flatten_wires_pub(wires: &Value) -> IndexMap<String, Vec<String>> {
    flatten_wires(wires)
}

/// Public access to flatten wires with context path.
pub fn flatten_wires_with_context_pub(
    wires: &Value,
    process_path: &[String],
) -> IndexMap<String, Vec<String>> {
    flatten_wires_with_context(wires, process_path)
}

fn flatten_wires(wires: &Value) -> IndexMap<String, Vec<String>> {
    flatten_wires_with_context(wires, &[])
}

/// Flatten wires, resolving `..` relative to the process's nesting path.
fn flatten_wires_with_context(
    wires: &Value,
    process_path: &[String],
) -> IndexMap<String, Vec<String>> {
    let mut result = IndexMap::new();

    if let Some(map) = wires.as_map() {
        for (port, target) in map {
            match target {
                Value::List(path) => {
                    let resolved = resolve_wire_path(path, process_path);
                    if !resolved.is_empty() {
                        result.insert(port.clone(), resolved);
                    }
                }
                Value::Map(sub_map) => {
                    for (sub_port, sub_target) in sub_map {
                        if let Some(path_list) = sub_target.as_list() {
                            let resolved = resolve_wire_path(path_list, process_path);
                            if !resolved.is_empty() {
                                let qualified = format!("{port}.{sub_port}");
                                result.insert(qualified, resolved);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    result
}

/// Resolve a wire path, handling `..` (parent navigation) and integer indices.
///
/// `path_elements`: the raw path from the JSON (may contain strings, "..", and integers)
/// `process_path`: where this process sits in the state tree (for resolving `..`)
///
/// Example: process at `["spatial_dFBA"]`, path `["..", "fields", "glucose", 0, 0]`
/// → resolves to `["fields", "glucose", "0", "0"]`
fn resolve_wire_path(path_elements: &[Value], process_path: &[String]) -> Vec<String> {
    // Start from the process's location in the tree
    let mut resolved: Vec<String> = process_path.to_vec();

    for elem in path_elements {
        match elem {
            Value::String(s) if s == ".." => {
                // Navigate up
                resolved.pop();
            }
            Value::String(s) => {
                resolved.push(s.clone());
            }
            Value::Int(i) => {
                // Array index — encode as string
                resolved.push(i.to_string());
            }
            Value::Float(f) => {
                // Sometimes indices come as floats in JSON
                resolved.push(format!("{}", f.0 as i64));
            }
            _ => {}
        }
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_monod_kinetics() {
        let json_str = r#"{
            "state": {
                "global_time": 0.0,
                "monod_kinetics": {
                    "address": {"protocol": "local", "data": "MonodKinetics"},
                    "config": {
                        "reactions": {
                            "assimilate_glucose": {
                                "reactant": "glucose", "product": "mass",
                                "km": 0.5, "vmax": 0.4, "yield": 0.2
                            }
                        }
                    },
                    "_inputs": "biomass:mass|substrates:map[concentration]",
                    "_outputs": "biomass:float|substrates:map[float]",
                    "inputs": {
                        "substrates": {
                            "glucose": ["fields", "glucose"]
                        },
                        "biomass": ["fields", "biomass"]
                    },
                    "outputs": {
                        "substrates": {
                            "glucose": ["fields", "glucose"]
                        },
                        "biomass": ["fields", "biomass"]
                    }
                },
                "fields": {
                    "glucose": 10.0,
                    "biomass": 0.1
                }
            },
            "schema": {
                "global_time": "float",
                "monod_kinetics": "link[biomass:mass|substrates:map[concentration],biomass:float|substrates:map[float]]"
            }
        }"#;

        let doc = VivariumDocument::from_json(json_str).unwrap();

        // Should have extracted the monod_kinetics process
        assert_eq!(doc.processes.len(), 1);
        assert!(doc.processes.contains_key("monod_kinetics"));
        assert_eq!(doc.processes["monod_kinetics"].class_name, "MonodKinetics");

        // State should have fields but not the process
        let state_map = doc.state.as_map().unwrap();
        assert!(state_map.contains_key("fields"));
        assert!(state_map.contains_key("global_time"));
        assert!(!state_map.contains_key("monod_kinetics"));

        // Schema should be preserved
        assert!(doc.schema.is_some());

        // Convert to topology
        let topo = doc.to_topology();
        assert_eq!(topo.processes.len(), 1);

        // Check nested wiring was flattened
        let proc = &topo.processes["monod_kinetics"];
        assert_eq!(proc.process_type, "MonodKinetics");
        assert!(proc.inputs.contains_key("biomass"));
        assert!(proc.inputs.contains_key("substrates.glucose"));
    }

    #[test]
    fn test_parse_brownian_particles() {
        let json_str = r#"{
            "state": {
                "global_time": 0.0,
                "particles": {
                    "p0": {
                        "id": "abc",
                        "position": [25.0, 25.0],
                        "mass": 0.5,
                        "local": {},
                        "exchange": {},
                        "sub_masses": {}
                    }
                },
                "brownian_movement": {
                    "address": {"protocol": "local", "data": "BrownianMovement"},
                    "config": {
                        "interval": 0.1,
                        "bounds": [50.0, 50.0],
                        "diffusion_rate": 0.5,
                        "advection_rate": [0.0, 0.0]
                    },
                    "inputs": {"particles": ["particles"]},
                    "outputs": {"particles": ["particles"]}
                },
                "emitter": {
                    "address": {"protocol": "local", "data": "RAMEmitter"},
                    "config": {"emit": {}},
                    "inputs": {},
                    "outputs": {}
                }
            }
        }"#;

        let doc = VivariumDocument::from_json(json_str).unwrap();

        // Should have brownian_movement but NOT emitter
        assert_eq!(doc.processes.len(), 1);
        assert!(doc.processes.contains_key("brownian_movement"));
        assert!(!doc.processes.contains_key("emitter"));

        // Particles should be in state
        let state_map = doc.state.as_map().unwrap();
        assert!(state_map.contains_key("particles"));

        // Interval should be extracted from config
        let proc = &doc.processes["brownian_movement"];
        assert_eq!(proc.interval, Some(0.1));
    }
}
