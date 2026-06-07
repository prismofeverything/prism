//! `RunProcess` — a Step that runs a wrapped Process over a runtime, collecting
//! its state history into a `TimeSeries` Value. Faithful to upstream
//! process-bigraph's `RunProcess(Step)` (parameter_scan.py): the integrators are
//! plain Processes; `RunProcess` turns one into a one-shot Step.
//!
//! It needs the `Core` to instantiate the wrapped process — injected by the
//! engine via `Step::set_core` (the threading rule's "push": the engine hands the
//! whole Core to nodes that compose other processes). The output `timeseries` is
//! a plain ys Value (`{_type:"TimeSeries", times, columns}`), no Foreign.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Core, ProcessNode, Schema, Step, Update, Value};

#[derive(Debug)]
pub struct RunProcess {
    /// FULL address of the wrapped process — `"local:Rk4"` or the typed
    /// `{_type:"rest", process, host, port}` — so any protocol survives to
    /// `Core::instantiate` (not pre-trimmed to a local class name).
    address: Value,
    /// Display name of the wrapped process ("Rk4", "CopasiCvode") — the
    /// TimeSeries label / output filename.
    name: String,
    /// Config for the wrapped process (e.g. `{network: …}` or `{sbml: …}`).
    process_config: Value,
    /// Total time to run the wrapped process.
    runtime: f64,
    /// Step size per `update`.
    timestep: f64,
    /// The engine's unified Core, injected via `set_core`; the wrapped proc is
    /// instantiated through `Core::instantiate` (protocol-aware).
    core: Option<Core>,
}

impl RunProcess {
    /// Config: `{ process: <spec>, runtime, timestep }`, where `<spec>` is a
    /// process term value `{address, config, …}` (e.g. `Rk4[network: …]`).
    pub fn from_config(config: &Value) -> Self {
        // A spec — whether for a leaf process/step or a composite — carries
        // `{address, config, inputs, outputs}` at the TOP LEVEL: the node
        // envelope is uniform (#47). A composite's `config` happens to contain
        // `{state, bridge, schema}` instead of plain params; leaf / composite
        // distinction lives inside config, not in the envelope.
        let spec = config.get_field("proc");
        let address = spec
            .and_then(|s| s.get_field("address"))
            .cloned()
            .unwrap_or(Value::None);
        // Display name: the local class, or a remote address's `process` field.
        let name = match &address {
            Value::String(s) => s.trim_start_matches("local:").to_string(),
            Value::Map(m) => m
                .get("process")
                .or_else(|| m.get("data"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_default(),
            _ => String::new(),
        };
        let process_config = spec
            .and_then(|s| s.get_field("config"))
            .cloned()
            .unwrap_or_else(Value::map);
        RunProcess {
            address,
            name,
            process_config,
            runtime: config.get_field("runtime").and_then(|v| v.as_f64()).unwrap_or(1.0),
            timestep: config.get_field("timestep").and_then(|v| v.as_f64()).unwrap_or(0.1),
            core: None,
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

    fn set_core(&mut self, core: Core) {
        self.core = Some(core);
    }

    fn update(&self, state: &Value) -> Update {
        let core = match &self.core {
            Some(c) => c,
            None => return Update::Noop,
        };
        // Instantiate the wrapped proc through the protocol-aware entry point, so
        // `proc` may be local OR rest/parallel/stream-addressed (e.g. CopasiCvode).
        let inner = match core.instantiate(&self.address, self.process_config.clone()) {
            Ok(ProcessNode::Process(p)) => p,
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

        // The state trace as a **delta-log** (`initial` + `deltas`), unified with
        // `Simulate` through `prism_trace::trace_of`. The observable frames are
        // unchanged (`prism_trace::frames` replays them), but storage is change-
        // only — the "future optimization" is now the default. The element type
        // `Trace[T]` is the inner's `state` output schema: KNOWN, carried as the
        // trace's parameter, never re-inferred — so `plot` dispatches on the real
        // schema. (`times`/`history` are reused for the `timeseries` below.)
        let element = inner.outputs().get("state").cloned().unwrap_or(Schema::Any);
        let trace = prism_trace::trace_of(
            &self.name,
            &element,
            times.iter().filter_map(|t| t.as_f64()).zip(history.iter().cloned()),
        );

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
            ("name", Value::from(self.name.as_str())),
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
