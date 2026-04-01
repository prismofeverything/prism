//! Port schemas and wiring — how processes connect to state.
//!
//! Each process declares ports (named channels with schemas).
//! Wiring maps those ports to paths in the hierarchical state tree.

use indexmap::IndexMap;

use prism_schema::{Key, Path, Schema, StructLayout, Value};

/// Resolve a path with `*` wildcards against a state tree.
///
/// `*` at any level expands to all children (map keys, list indices).
/// Returns a map keyed by the wildcard-expanded key, with values being
/// the resolved sub-values at the remaining path.
///
/// Example: `get_star_path(state, ["Compartments", "*", "volume"])`
/// returns `{"0": 100, "1": 200, "2": 300}` if Compartments has keys 0,1,2.
fn get_star_path(state: &Value, path: &[Key]) -> Value {
    if path.is_empty() {
        return state.clone();
    }

    if path[0] == "*" {
        // Expand across all children
        let rest = &path[1..];
        match state {
            Value::Map(map) => {
                let expanded: IndexMap<Key, Value> = map.iter()
                    .map(|(k, v)| (k.clone(), get_star_path(v, rest)))
                    .collect();
                Value::Map(expanded)
            }
            Value::Struct { layout, values } => {
                let expanded: IndexMap<Key, Value> = layout.fields.iter().zip(values.iter())
                    .map(|(k, v)| (k.clone(), get_star_path(v, rest)))
                    .collect();
                Value::Map(expanded)
            }
            Value::List(list) => {
                let expanded: IndexMap<Key, Value> = list.iter().enumerate()
                    .map(|(i, v)| (Key::from(i.to_string()), get_star_path(v, rest)))
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
            Value::Struct { layout, values } => {
                if let Some(idx) = layout.index_of(&path[0]) {
                    get_star_path(&values[idx], &path[1..])
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
fn set_star_path(state: &mut Value, path: &[Key], update: &Value) {
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
    /// Pre-allocated view template — the map structure is created once.
    /// On each view() call, only the leaf values are updated in-place.
    pub(crate) view_template: Option<Value>,
    /// Whether all input ports are "simple" (no dots, no stars).
    pub(crate) simple_inputs: bool,
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
            .map(|name| (name.clone(), vec![Key::from(name.as_str())]))
            .collect()
    }

    /// Pre-build the view template from the current input wiring.
    pub fn init_view_template(&mut self) {
        let simple = self.inputs.iter().all(|(name, path)| {
            !name.contains('.') && !path.iter().any(|s| s.as_str() == "*")
        });
        self.simple_inputs = simple;
    }

    /// Slice state according to input wiring.
    ///
    /// For simple wiring (no dots, no stars), builds a Map directly
    /// with pre-allocated capacity — avoids set_path overhead.
    pub fn view(&self, state: &Value) -> Value {
        if self.simple_inputs {
            // Fast path: build Map directly
            let mut map = IndexMap::with_capacity(self.inputs.len());
            for (port_name, path) in &self.inputs {
                let val = state.get_path(path).cloned().unwrap_or(Value::None);
                map.insert(Key::from(port_name.as_str()), val);
            }
            return Value::Map(map);
        }

        // Slow path: complex wiring (dots, stars)
        let mut view = Value::map();
        for (port_name, path) in &self.inputs {
            if path.iter().any(|s| s.as_str() == "*") {
                let val = get_star_path(state, path);
                let port_path: Vec<Key> =
                    port_name.split('.').map(Key::from).collect();
                view.set_path(&port_path, val);
            } else {
                let val = state.get_path(path).cloned().unwrap_or(Value::None);
                let port_path: Vec<Key> =
                    port_name.split('.').map(Key::from).collect();
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
                if let Some(val) = map.get(port_name.as_str()) {
                    if state_path.iter().any(|s| s.as_str() == "*") {
                        // Star path: expand the update across matching children.
                        // The value should be a map keyed by wildcard-expanded keys.
                        // Generate one projection per expanded key.
                        if let Value::Map(expanded) = val {
                            for (key, child_val) in expanded {
                                let concrete_path: Path = state_path.iter()
                                    .map(|s| if s.as_str() == "*" { key.clone() } else { s.clone() })
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
                let port_path: Vec<Key> =
                    port_name.split('.').map(Key::from).collect();
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

        let mut iface = Interface {
            inputs: IndexMap::from([
                ("glc".to_string(), vec![Key::from("cell"), Key::from("glucose")]),
                ("energy".to_string(), vec![Key::from("cell"), Key::from("atp")]),
            ]),
            outputs: IndexMap::from([
                ("glc".to_string(), vec![Key::from("cell"), Key::from("glucose")]),
                ("energy".to_string(), vec![Key::from("cell"), Key::from("atp")]),
            ]),
            output_schemas: IndexMap::new(),
            view_template: None,
            simple_inputs: false,
        };
        iface.init_view_template();

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
        assert_eq!(projected[0].0, vec![Key::from("cell"), Key::from("glucose")]);
    }
}
