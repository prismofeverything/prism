//! Port schemas and wiring — how processes connect to state.
//!
//! Each process declares ports (named channels with schemas).
//! Wiring maps those ports to paths in the hierarchical state tree.

use indexmap::IndexMap;

use prism_schema::{Path, Schema, Value};

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
            let val = state.get_path(path).cloned().unwrap_or(Value::None);
            // Split dot-separated port names into nested path
            let port_path: Vec<String> =
                port_name.split('.').map(|s| s.to_string()).collect();
            view.set_path(&port_path, val);
        }
        view
    }

    /// Project an update from port-local coordinates back to
    /// state-tree coordinates via output wiring.
    ///
    /// Handles dot-separated port names by walking into nested update maps:
    /// output key `"substrates.glucose"` matches `update["substrates"]["glucose"]`
    pub fn project(&self, update: &Value) -> Vec<(Path, Value)> {
        let mut projections = Vec::new();
        if let Value::Map(map) = update {
            for (port_name, state_path) in &self.outputs {
                // Try direct match first (non-nested port)
                if let Some(val) = map.get(port_name) {
                    projections.push((state_path.clone(), val.clone()));
                    continue;
                }

                // Try dot-separated nested lookup
                let port_path: Vec<String> =
                    port_name.split('.').map(|s| s.to_string()).collect();
                if port_path.len() > 1 {
                    if let Some(val) = update.get_path(&port_path) {
                        projections.push((state_path.clone(), val.clone()));
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
