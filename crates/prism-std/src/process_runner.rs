//! `RunProcess` — a Step that runs a wrapped Process over a runtime, collecting
//! its state history into a `TimeSeries` Value. Faithful to upstream
//! process-bigraph's `RunProcess(Step)` (parameter_scan.py): the integrators are
//! plain Processes; `RunProcess` turns one into a one-shot Step.
//!
//! It needs the `ProcessRegistry` to instantiate the wrapped process — injected
//! by the engine via `Step::set_registry` (the engine owns the registry and
//! hands it to nodes that compose other processes). The output `timeseries` is a
//! plain ys Value (`{_type:"TimeSeries", times, columns}`), no Foreign.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{ProcessNode, ProcessRegistry, Schema, Step, Update, Value};

#[derive(Debug)]
pub struct RunProcess {
    /// Registered type name of the wrapped process (e.g. "Rk4"), `local:` stripped.
    address: String,
    /// Config for the wrapped process (e.g. `{network: …}`).
    process_config: Value,
    /// Total time to run the wrapped process.
    runtime: f64,
    /// Step size per `update`.
    timestep: f64,
    /// The engine's registry, injected at instantiation via `set_registry`.
    registry: Option<Arc<ProcessRegistry>>,
}

impl RunProcess {
    /// Config: `{ process: <spec>, runtime, timestep }`, where `<spec>` is a
    /// process term value `{address, config, …}` (e.g. `Rk4[network: …]`).
    pub fn from_config(config: &Value) -> Self {
        let spec = config.get_field("proc");
        let address = spec
            .and_then(|s| s.get_field("address"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim_start_matches("local:")
            .to_string();
        let process_config = spec
            .and_then(|s| s.get_field("config"))
            .cloned()
            .unwrap_or_else(Value::map);
        RunProcess {
            address,
            process_config,
            runtime: config.get_field("runtime").and_then(|v| v.as_f64()).unwrap_or(1.0),
            timestep: config.get_field("timestep").and_then(|v| v.as_f64()).unwrap_or(0.1),
            registry: None,
        }
    }
}

impl Step for RunProcess {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(
            "state".to_string(),
            Schema::Map {
                value: Box::new(Schema::float()),
            },
        )])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            (
                "timeseries".to_string(),
                Schema::Custom {
                    name: "TimeSeries".to_string(),
                    parameters: IndexMap::new(),
                },
            ),
            // The FULL state trace (every frame), not just the scalar columns —
            // the single-item state "extended through time". Feeds the
            // schema-driven `prism_viz::plot(schema, trace.frames)`, which derives
            // the characteristic view from the output port's type. (`timeseries`
            // stays for the integrator comparison + its TimeSeries methods.)
            (
                "trace".to_string(),
                Schema::Custom { name: "Trace".to_string(), parameters: IndexMap::new() },
            ),
        ])
    }

    fn set_registry(&mut self, registry: Arc<ProcessRegistry>) {
        self.registry = Some(registry);
    }

    fn update(&self, state: &Value) -> Update {
        let registry = match &self.registry {
            Some(r) => r,
            None => return Update::Noop,
        };
        let inner = match registry.create(&self.address, self.process_config.clone()) {
            Some(ProcessNode::Process(p)) => p,
            _ => return Update::Noop, // RunProcess wraps a Process
        };

        // Run the wrapped process, feeding each step's output back as the next
        // input, recording the state history.
        let init = state.get_field("state").cloned().unwrap_or_else(Value::map);
        let n_steps = (self.runtime / self.timestep).round().max(0.0) as usize;
        let mut local = Value::tree([("state", init.clone())]);
        let mut history: Vec<Value> = vec![init];
        let mut times: Vec<Value> = vec![Value::float(0.0)];
        for step in 0..n_steps {
            let next = inner
                .update(&local, self.timestep)
                .into_value()
                .and_then(|v| v.get_field("state").cloned())
                .unwrap_or_else(Value::map);
            times.push(Value::float((step + 1) as f64 * self.timestep));
            history.push(next.clone());
            local = Value::tree([("state", next)]);
        }

        // The full state trace: every frame, as the single-item state extended
        // through time. `prism_viz::plot(output_schema, frames)` dispatches on
        // the type to its characteristic view. (A future optimization stores
        // this as `initial + diffs` via `prism_schema::diff`/`apply` — change-
        // only — but full frames are the simplest plot-ready form.)
        let trace = Value::tree([
            ("_type", Value::from("Trace")),
            ("name", Value::from(self.address.as_str())),
            ("times", Value::List(times.clone())),
            ("frames", Value::List(history.clone())),
        ]);

        // Transpose the history (list of `{species: float}`) into columns
        // `{species: [float]}`, then assemble the TimeSeries Value.
        let species: Vec<String> = history
            .first()
            .and_then(|v| v.as_map())
            .map(|m| m.keys().map(|k| k.to_string()).collect())
            .unwrap_or_default();
        let columns = Value::tree(species.iter().map(|sp| {
            let col: Vec<Value> = history
                .iter()
                .map(|h| h.get_field(sp).cloned().unwrap_or(Value::float(0.0)))
                .collect();
            (sp.clone(), Value::List(col))
        }));

        let timeseries = Value::tree([
            ("_type", Value::from("TimeSeries")),
            // The wrapped process's name (e.g. "Rk4"), so the Output step can
            // write `a.csv(path / a.name)` to a meaningful filename.
            ("name", Value::from(self.address.as_str())),
            ("times", Value::List(times)),
            ("columns", columns),
        ]);
        Update::value(Value::tree([("timeseries", timeseries), ("trace", trace)]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
