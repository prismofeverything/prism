//! Topology — the wiring diagram that connects processes to state.
//!
//! A Topology defines which processes exist in a composition,
//! how their ports wire to the shared state tree, and the
//! hierarchical structure of the state itself.

use indexmap::IndexMap;

use prism_schema::{Path, Schema, Value};

use crate::ports::{Interface, Wires};

/// Configuration for a single process node in the topology.
#[derive(Clone, Debug)]
pub struct ProcessSpec {
    /// Identifier for the process type (used to look up the factory).
    pub process_type: String,

    /// Configuration for the process instance.
    pub config: Value,

    /// How input ports map to state paths.
    pub inputs: Wires,

    /// How output ports map to state paths.
    pub outputs: Wires,

    /// For processes: time interval between updates.
    /// For steps: `None`.
    pub interval: Option<f64>,

    /// Priority for step ordering (only relevant for steps).
    pub priority: f64,
}

impl ProcessSpec {
    pub fn interface(&self) -> Interface {
        Interface {
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            output_schemas: IndexMap::new(), // populated by engine from process node
            view_template: None,
            simple_inputs: false,
        }
    }

    pub fn is_step(&self) -> bool {
        self.interval.is_none()
    }
}

/// A complete topology: processes, their wiring, and the state schema.
#[derive(Clone, Debug)]
pub struct Topology {
    /// Schema for the shared state tree.
    pub state_schema: Schema,

    /// Initial state values.
    pub initial_state: Value,

    /// Process/step specifications keyed by name.
    pub processes: IndexMap<String, ProcessSpec>,
}

impl Default for Topology {
    fn default() -> Self {
        Self {
            state_schema: Schema::Any,
            initial_state: Value::None,
            processes: IndexMap::new(),
        }
    }
}

impl Topology {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a process to the topology.
    pub fn add_process(
        &mut self,
        name: impl Into<String>,
        process_type: impl Into<String>,
        config: Value,
        inputs: Wires,
        outputs: Wires,
        interval: f64,
    ) -> &mut Self {
        self.processes.insert(
            name.into(),
            ProcessSpec {
                process_type: process_type.into(),
                config,
                inputs,
                outputs,
                interval: Some(interval),
                priority: 0.0,
            },
        );
        self
    }

    /// Add a step to the topology.
    pub fn add_step(
        &mut self,
        name: impl Into<String>,
        process_type: impl Into<String>,
        config: Value,
        inputs: Wires,
        outputs: Wires,
        priority: f64,
    ) -> &mut Self {
        self.processes.insert(
            name.into(),
            ProcessSpec {
                process_type: process_type.into(),
                config,
                inputs,
                outputs,
                interval: None,
                priority,
            },
        );
        self
    }

    /// Get all process names (temporal processes only).
    pub fn process_names(&self) -> Vec<&str> {
        self.processes
            .iter()
            .filter(|(_, s)| !s.is_step())
            .map(|(n, _)| n.as_str())
            .collect()
    }

    /// Get all step names.
    pub fn step_names(&self) -> Vec<&str> {
        self.processes
            .iter()
            .filter(|(_, s)| s.is_step())
            .map(|(n, _)| n.as_str())
            .collect()
    }

    /// Collect all state paths referenced by any process wiring.
    pub fn all_state_paths(&self) -> Vec<Path> {
        let mut paths = Vec::new();
        for spec in self.processes.values() {
            for path in spec.inputs.values() {
                paths.push(path.clone());
            }
            for path in spec.outputs.values() {
                paths.push(path.clone());
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }
}
