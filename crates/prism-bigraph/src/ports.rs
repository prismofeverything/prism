//! Port schemas and wiring — how processes connect to state.
//!
//! Each process declares ports (named channels with schemas).
//! Wiring maps those ports to paths in the hierarchical state tree.

use indexmap::IndexMap;

use prism_schema::{Path, Schema, Value};

/// Resolve a path with `*` wildcards against a state tree.
///
/// `*` at any level expands to all children (map keys, list indices).
/// Returns a map keyed by the wildcard-expanded key, with values being
/// the resolved sub-values at the remaining path.
///
/// Example: `get_star_path(state, ["Compartments", "*", "volume"])`
/// returns `{"0": 100, "1": 200, "2": 300}` if Compartments has keys 0,1,2.
fn get_star_path(state: &Value, path: &[String]) -> Value {
    if path.is_empty() {
        return state.clone();
    }

    if path[0] == "*" {
        // Expand across all children
        let rest = &path[1..];
        match state {
            Value::Map(map) => {
                let expanded: IndexMap<String, Value> = map.iter()
                    .map(|(k, v)| (k.clone(), get_star_path(v, rest)))
                    .collect();
                Value::Map(expanded)
            }
            Value::List(list) => {
                let expanded: IndexMap<String, Value> = list.iter().enumerate()
                    .map(|(i, v)| (i.to_string(), get_star_path(v, rest)))
                    .collect();
                Value::Map(expanded)
            }
            _ => Value::None,
        }
    } else {
        // Navigate into the named child
        match state {
            Value::Map(map) => {
                if let Some(child) = map.get(&path[0]) {
                    get_star_path(child, &path[1..])
                } else {
                    Value::None
                }
            }
            Value::List(list) => {
                if let Ok(idx) = path[0].parse::<usize>() {
                    if let Some(child) = list.get(idx) {
                        get_star_path(child, &path[1..])
                    } else {
                        Value::None
                    }
                } else {
                    Value::None
                }
            }
            _ => Value::None,
        }
    }
}

/// Write a value back through a star path, distributing across all matching children.
///
/// If the update is a map and the path contains `*`, each key in the update
/// is written to the corresponding child of the state at the star position.
fn set_star_path(state: &mut Value, path: &[String], update: &Value) {
    if path.is_empty() {
        return;
    }

    if path[0] == "*" {
        let rest = &path[1..];
        if let (Value::Map(state_map), Value::Map(update_map)) = (state, update) {
            for (key, upd_val) in update_map {
                if let Some(child) = state_map.get_mut(key) {
                    if rest.is_empty() {
                        *child = upd_val.clone();
                    } else {
                        set_star_path(child, rest, upd_val);
                    }
                }
            }
        }
    } else {
        match state {
            Value::Map(map) => {
                if let Some(child) = map.get_mut(&path[0]) {
                    if path.len() == 1 {
                        *child = update.clone();
                    } else {
                        set_star_path(child, &path[1..], update);
                    }
                }
            }
            Value::List(list) => {
                if let Ok(idx) = path[0].parse::<usize>() {
                    if let Some(child) = list.get_mut(idx) {
                        if path.len() == 1 {
                            *child = update.clone();
                        } else {
                            set_star_path(child, &path[1..], update);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// A single port declaration: a name and its schema.
pub type PortSchema = IndexMap<String, Schema>;

/// Wiring: maps port names to paths in the state tree.
/// When a process declares port "glucose" and it's wired to
/// `["cell", "metabolites", "glucose"]`, the engine slices
/// that path from the state tree and delivers it as the
/// "glucose" key in the process's input state.
pub type Wires = IndexMap<String, Path>;

/// Complete interface for a process: input and output wiring.
#[derive(Clone, Debug, Default)]
pub struct Interface {
    pub inputs: Wires,
    pub outputs: Wires,
    /// Per-output-port schemas from the process declaration.
    /// Used to determine apply semantics (e.g., Overwrite vs additive).
    pub output_schemas: IndexMap<String, Schema>,
}

impl Interface {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build an interface where each port maps to a same-named
    /// key at the current level (identity wiring).
    pub fn identity(port_schema: &PortSchema) -> Wires {
        port_schema
            .keys()
            .map(|name| (name.clone(), vec![name.clone()]))
            .collect()
    }

    /// Slice state according to input wiring: for each input port,
    /// follow its wire path into the state tree and collect the value.
    ///
    /// Handles dot-separated port names by reconstructing nested maps:
    /// `"substrates.glucose" → val` becomes `{"substrates": {"glucose": val}}`
    pub fn view(&self, state: &Value) -> Value {
        let mut view = Value::map();
        for (port_name, path) in &self.inputs {
            // Check for star paths: ['Compartments', '*', 'volume']
            if path.contains(&"*".to_string()) {
                let val = get_star_path(state, path);
                let port_path: Vec<String> =
                    port_name.split('.').map(|s| s.to_string()).collect();
                view.set_path(&port_path, val);
            } else {
                let val = state.get_path(path).cloned().unwrap_or(Value::None);
                let port_path: Vec<String> =
                    port_name.split('.').map(|s| s.to_string()).collect();
                view.set_path(&port_path, val);
            }
        }
        view
    }

    /// Project an update from port-local coordinates back to
    /// state-tree coordinates via output wiring.
    ///
    /// Handles dot-separated port names by walking into nested update maps:
    /// output key `"substrates.glucose"` matches `update["substrates"]["glucose"]`
    /// Project with schema: returns (path, value, optional port schema).
    pub fn project(&self, update: &Value) -> Vec<(Path, Value, Option<Schema>)> {
        let mut projections = Vec::new();
        if let Value::Map(map) = update {
            for (port_name, state_path) in &self.outputs {
                let schema = self.output_schemas.get(port_name).cloned();

                // Try direct match first (non-nested port)
                if let Some(val) = map.get(port_name) {
                    if state_path.contains(&"*".to_string()) {
                        // Star path: expand the update across matching children.
                        // The value should be a map keyed by wildcard-expanded keys.
                        // Generate one projection per expanded key.
                        if let Value::Map(expanded) = val {
                            for (key, child_val) in expanded {
                                let concrete_path: Path = state_path.iter()
                                    .map(|s| if s == "*" { key.clone() } else { s.clone() })
                                    .collect();
                                projections.push((concrete_path, child_val.clone(), schema.clone()));
                            }
                        }
                    } else {
                        projections.push((state_path.clone(), val.clone(), schema));
                    }
                    continue;
                }

                // Try dot-separated nested lookup
                let port_path: Vec<String> =
                    port_name.split('.').map(|s| s.to_string()).collect();
                if port_path.len() > 1 {
                    if let Some(val) = update.get_path(&port_path) {
                        projections.push((state_path.clone(), val.clone(), schema));
                    }
                }
            }
        }
        projections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_and_project() {
        let state = Value::tree([
            ("cell", Value::tree([
                ("glucose", Value::float(5.0)),
                ("atp", Value::float(100.0)),
            ])),
        ]);

        let iface = Interface {
            inputs: IndexMap::from([
                ("glc".to_string(), vec!["cell".into(), "glucose".into()]),
                ("energy".to_string(), vec!["cell".into(), "atp".into()]),
            ]),
            outputs: IndexMap::from([
                ("glc".to_string(), vec!["cell".into(), "glucose".into()]),
                ("energy".to_string(), vec!["cell".into(), "atp".into()]),
            ]),
        };

        let view = iface.view(&state);
        let map = view.as_map().unwrap();
        assert_eq!(map["glc"].as_f64(), Some(5.0));
        assert_eq!(map["energy"].as_f64(), Some(100.0));

        let update = Value::tree([
            ("glc", Value::float(-1.0)),
            ("energy", Value::float(-2.0)),
        ]);
        let projected = iface.project(&update);
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].0, vec!["cell".to_string(), "glucose".to_string()]);
    }
}
