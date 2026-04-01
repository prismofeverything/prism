//! Simulation runner: build, execute, and save composite documents.

use std::path::Path;

use prism_bigraph::{Document, Engine, ProcessRegistry, Value};
use prism_viz::{render_dot, DotOptions};

/// Results from a simulation run: time series of state snapshots.
#[derive(Clone, Debug)]
pub struct SimulationResults {
    pub name: String,
    pub times: Vec<f64>,
    pub states: Vec<Value>,
}

impl SimulationResults {
    /// Get the final state.
    pub fn final_state(&self) -> Option<&Value> {
        self.states.last()
    }

    /// Get a time series for a specific path.
    pub fn time_series(&self, path: &[prism_schema::Key]) -> Vec<Option<f64>> {
        self.states
            .iter()
            .map(|s| s.get_path(path).and_then(|v| v.as_f64()))
            .collect()
    }
}

/// Run a composite document: instantiate, execute, save outputs.
pub fn run_document(
    doc: &Document,
    registry: &ProcessRegistry,
    name: &str,
    duration: f64,
    emit_interval: f64,
    out_dir: impl AsRef<Path>,
) -> Result<SimulationResults, String> {
    let out_dir = out_dir.as_ref();
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;

    // Save the document
    doc.save_all(out_dir, name)
        .map_err(|e| format!("save error: {e}"))?;

    // Save DOT visualization
    let dot = render_dot(doc, &DotOptions::default());
    let dot_path = out_dir.join(format!("{name}.dot"));
    std::fs::write(&dot_path, &dot).map_err(|e| e.to_string())?;

    // Try to render to PNG (silently skip if graphviz not installed)
    let _ = prism_viz::render_to_file(
        &dot,
        out_dir.join(format!("{name}.png")),
        "png",
    );

    // Build topology and instantiate processes
    let topology = doc.to_topology();
    let instances = registry
        .instantiate_topology(&topology)
        .map_err(|e| format!("instantiation error: {e}"))?;

    // Run simulation with periodic state capture
    let mut engine = Engine::new(topology, instances);
    let mut results = SimulationResults {
        name: name.to_string(),
        times: vec![0.0],
        states: vec![engine.state().clone()],
    };

    let n_steps = (duration / emit_interval).ceil() as usize;
    for _ in 0..n_steps {
        engine.run(emit_interval);
        results.times.push(engine.time());
        results.states.push(engine.state().clone());
    }

    Ok(results)
}
