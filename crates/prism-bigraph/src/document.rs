//! Composite document serialization — save and load simulations as JSON.
//!
//! A "document" is the complete specification of a simulation:
//! processes, their wiring, state schema, and initial state.
//! Documents can be serialized to JSON for storage, sharing,
//! and reconstruction of simulations.

use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use prism_schema::{Key, Value};

use crate::topology::{ProcessSpec, Topology};

/// A serializable composite document.
///
/// This is the JSON-level representation of a simulation composition.
/// It captures everything needed to reconstruct and run a simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    /// State schema as a nested description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,

    /// Initial state values.
    pub state: Value,

    /// Process/step specifications.
    pub processes: IndexMap<String, ProcessDocument>,

    /// Optional metadata (name, description, author, etc.)
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub metadata: IndexMap<String, Value>,
}

/// A serializable process specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessDocument {
    /// Process type identifier (used to look up the factory).
    #[serde(rename = "type")]
    pub process_type: String,

    /// Process configuration.
    #[serde(default)]
    pub config: Value,

    /// Input port wiring: port_name → state path.
    pub inputs: IndexMap<String, Vec<String>>,

    /// Output port wiring: port_name → state path.
    pub outputs: IndexMap<String, Vec<String>>,

    /// Time interval (present for processes, absent for steps).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<f64>,

    /// Priority (for steps).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub priority: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Document {
    /// Create a new empty document.
    pub fn new() -> Self {
        Self {
            schema: None,
            state: Value::None,
            processes: IndexMap::new(),
            metadata: IndexMap::new(),
        }
    }

    /// Convert a Topology into a serializable Document.
    pub fn from_topology(topology: &Topology) -> Self {
        let mut processes = IndexMap::new();

        for (name, spec) in &topology.processes {
            let to_string_paths =
                |wires: &IndexMap<String, Vec<Key>>| -> IndexMap<String, Vec<String>> {
                    wires
                        .iter()
                        .map(|(k, v)| (k.clone(), v.iter().map(|k| k.to_string()).collect()))
                        .collect()
                };
            processes.insert(
                name.clone(),
                ProcessDocument {
                    process_type: spec.process_type.clone(),
                    config: spec.config.clone(),
                    inputs: to_string_paths(&spec.inputs),
                    outputs: to_string_paths(&spec.outputs),
                    interval: spec.interval,
                    priority: spec.priority,
                },
            );
        }

        Self {
            schema: None,
            state: topology.initial_state.clone(),
            processes,
            metadata: IndexMap::new(),
        }
    }

    /// Convert this Document back into a Topology.
    pub fn to_topology(&self) -> Topology {
        let mut topology = Topology::new();
        topology.initial_state = self.state.clone();

        let to_key_paths = |wires: &IndexMap<String, Vec<String>>| -> IndexMap<String, Vec<Key>> {
            wires
                .iter()
                .map(|(k, v)| (k.clone(), v.iter().map(|s| Key::from(s.as_str())).collect()))
                .collect()
        };

        for (name, pdoc) in &self.processes {
            topology.processes.insert(
                name.clone(),
                ProcessSpec {
                    process_type: pdoc.process_type.clone(),
                    config: pdoc.config.clone(),
                    inputs: to_key_paths(&pdoc.inputs),
                    outputs: to_key_paths(&pdoc.outputs),
                    interval: pdoc.interval,
                    priority: pdoc.priority,
                },
            );
        }

        topology
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Save to a JSON file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let json = self.to_json().map_err(DocumentError::Serialize)?;
        std::fs::write(path, json).map_err(DocumentError::Io)?;
        Ok(())
    }

    /// Load from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let json = std::fs::read_to_string(path).map_err(DocumentError::Io)?;
        Self::from_json(&json).map_err(DocumentError::Deserialize)
    }

    /// Save the initial state separately.
    pub fn save_state(&self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let json = serde_json::to_string_pretty(&self.state).map_err(DocumentError::Serialize)?;
        std::fs::write(path, json).map_err(DocumentError::Io)?;
        Ok(())
    }

    /// Save the schema separately.
    pub fn save_schema(&self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        if let Some(schema) = &self.schema {
            let json = serde_json::to_string_pretty(schema).map_err(DocumentError::Serialize)?;
            std::fs::write(path, json).map_err(DocumentError::Io)?;
        }
        Ok(())
    }

    /// Save all files: document, state, and schema with conventional names.
    pub fn save_all(&self, dir: impl AsRef<Path>, name: &str) -> Result<(), DocumentError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).map_err(DocumentError::Io)?;

        self.save(dir.join(format!("{name}.json")))?;
        self.save_state(dir.join(format!("{name}_state.json")))?;
        self.save_schema(dir.join(format!("{name}_schema.json")))?;

        Ok(())
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialize(serde_json::Error),
    #[error("Deserialization error: {0}")]
    Deserialize(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let mut topology = Topology::new();
        topology.initial_state = Value::tree([
            ("biomass", Value::float(1.0)),
            ("glucose", Value::float(10.0)),
        ]);

        topology.add_process(
            "growth",
            "monod_kinetics",
            Value::tree([("rate", Value::float(0.1))]),
            IndexMap::from([
                ("biomass".into(), vec!["biomass".into()]),
                ("glucose".into(), vec!["glucose".into()]),
            ]),
            IndexMap::from([
                ("biomass".into(), vec!["biomass".into()]),
                ("glucose".into(), vec!["glucose".into()]),
            ]),
            1.0,
        );

        topology.add_step(
            "division",
            "division",
            Value::tree([("threshold", Value::float(2.0))]),
            IndexMap::from([("biomass".into(), vec!["biomass".into()])]),
            IndexMap::from([("biomass".into(), vec!["biomass".into()])]),
            0.0,
        );

        // Document → JSON → Document → Topology
        let doc = Document::from_topology(&topology);
        let json = doc.to_json().unwrap();
        let doc2 = Document::from_json(&json).unwrap();
        let topology2 = doc2.to_topology();

        assert_eq!(topology2.processes.len(), 2);
        assert_eq!(topology2.processes["growth"].process_type, "monod_kinetics");
        assert!(topology2.processes["growth"].interval.is_some());
        assert!(topology2.processes["division"].interval.is_none());

        // State roundtrip
        let biomass = topology2
            .initial_state
            .get_path(&["biomass".into()])
            .and_then(|v| v.as_f64());
        assert_eq!(biomass, Some(1.0));
    }

    #[test]
    fn test_json_format() {
        let mut doc = Document::new();
        doc.state = Value::tree([("x", Value::float(1.0))]);
        doc.processes.insert(
            "grow".into(),
            ProcessDocument {
                process_type: "growth".into(),
                config: Value::None,
                inputs: IndexMap::from([("x".into(), vec!["x".into()])]),
                outputs: IndexMap::from([("x".into(), vec!["x".into()])]),
                interval: Some(1.0),
                priority: 0.0,
            },
        );

        let json = doc.to_json().unwrap();
        assert!(json.contains("\"type\": \"growth\""));
        assert!(json.contains("\"interval\": 1.0"));
    }
}
