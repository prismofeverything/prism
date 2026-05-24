//! `Simulate` — the **engine-faithful event-source runner**, the general sibling
//! of [`RunProcess`](crate::process_runner::RunProcess).
//!
//! Both drive a wrapped Process over a runtime. The difference is one line, and
//! it is the whole point:
//!
//! - `RunProcess` takes each update's `state` and uses it AS the next full state
//!   (right for full-output integrators that emit their whole trajectory).
//! - `Simulate` treats each update as an **event** and folds it via
//!   [`algebra::apply`] under the state schema — so a delta-emitting process
//!   accumulates and an `Overwrite`/field process replaces, exactly as the
//!   engine would. It captures the run as a **delta-log** `Trace[T]`
//!   ([`prism_trace`]), the currency `prism_viz::plot` and chrysalis streaming
//!   invocation consume. See `docs/delta-traces.md`.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{ProcessNode, ProcessRegistry, Schema, Step, Update, Value};
use prism_schema::algebra;

#[derive(Debug)]
pub struct Simulate {
    /// Registered type name of the wrapped process, `local:` stripped.
    address: String,
    /// Config for the wrapped process.
    process_config: Value,
    /// Total time to run.
    runtime: f64,
    /// Step size per `update`.
    timestep: f64,
    /// The engine's registry, injected via `set_registry`.
    registry: Option<Arc<ProcessRegistry>>,
}

impl Simulate {
    /// Config: `{ proc: <process-spec>, runtime, timestep }` — same shape as
    /// [`RunProcess`](crate::process_runner::RunProcess), where `<process-spec>`
    /// is a term value `{address, config}` (e.g. `Kinetics[network: …]`).
    pub fn from_config(config: &Value) -> Self {
        let spec = config.get_field("proc");
        let address = spec
            .and_then(|s| s.get_field("address"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim_start_matches("local:")
            .to_string();
        let process_config =
            spec.and_then(|s| s.get_field("config")).cloned().unwrap_or_else(Value::map);
        Simulate {
            address,
            process_config,
            runtime: config.get_field("runtime").and_then(|v| v.as_f64()).unwrap_or(1.0),
            timestep: config.get_field("timestep").and_then(|v| v.as_f64()).unwrap_or(0.1),
            registry: None,
        }
    }
}

impl Step for Simulate {
    fn inputs(&self) -> IndexMap<String, Schema> {
        // The sim's whole state flows through this one port (the section wires it
        // in). It is whatever shape the inner expects — a Map, or a Tree of
        // `biomass:Float + substrates:Map[Float]`, … — so the generic runner's
        // input is `Any` (a polymorphic passthrough), not Map[Float].
        IndexMap::from([("state".to_string(), Schema::Any)])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(
            "trace".to_string(),
            Schema::Custom { name: "Trace".to_string(), parameters: IndexMap::new() },
        )])
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
            _ => return Update::Noop, // Simulate wraps a Process
        };

        // The element folded/carried is the inner's WHOLE output interface as a
        // record schema (`biomass:Float, substrates:Map[Float], …`) — NOT a single
        // `state` port. This is what makes Simulate engine-faithful for ANY
        // process: the sim's state IS the inner's port-view (same-name wiring, the
        // engine default), and each update folds via `apply` under each port's real
        // type (additive sorts accumulate, Overwrite/Array replace). RunProcess's
        // single-`state`-port wrapping is the integrator special case; this is the
        // general one. The element is KNOWN (the inner's declared outputs), carried
        // as the trace's `T`, never re-inferred.
        let element = Schema::Tree {
            branches: inner.outputs().into_iter().map(|(k, v)| (k.into(), v)).collect(),
        };

        // Simulate's own `state` input port holds the sim's natural state.
        let init = state.get_field("state").cloned().unwrap_or_else(Value::map);
        let n_steps = (self.runtime / self.timestep).round().max(0.0) as usize;

        let mut cur = init.clone();
        let mut samples: Vec<(f64, Value)> = vec![(0.0, init)];
        for step in 0..n_steps {
            // Same-name: the sim state IS the inner's input view (extra fields
            // tolerated, like the engine's projection). The inner's update is an
            // EVENT (a delta); fold it via the schema — additive sorts accumulate,
            // Overwrite/Array replace. THIS distinguishes Simulate from RunProcess
            // (which uses each update as the full next state).
            let update = inner.update(&cur, self.timestep).into_value().unwrap_or_else(Value::map);
            cur = algebra::apply(&element, &cur, &update);
            samples.push(((step + 1) as f64 * self.timestep, cur.clone()));
        }

        Update::value(Value::tree([("trace", prism_trace::trace_of(&self.address, &element, samples))]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_bigraph::Process;

    /// A `Map[Float]` value from `(name, magnitude)` pairs.
    fn mf(pairs: &[(&str, f64)]) -> Value {
        Value::tree(pairs.iter().map(|(k, v)| (*k, Value::float(*v))))
    }

    fn cfg(addr: &str, runtime: f64, timestep: f64) -> Value {
        Value::tree([
            ("proc", Value::tree([("address", Value::from(addr)), ("config", Value::map())])),
            ("runtime", Value::float(runtime)),
            ("timestep", Value::float(timestep)),
        ])
    }

    /// A delta-emitting process with a NATURAL port: each update is `+1` on the
    /// `n` field directly (same-name, like spatio-flux processes) — NOT wrapped in
    /// a `state` port. Folding it via `apply` accumulates; using it as the full
    /// next state would not. This is the contract Simulate now serves: any-port
    /// processes, wired same-name.
    #[derive(Debug)]
    struct Increment;
    impl Process for Increment {
        fn inputs(&self) -> IndexMap<String, Schema> {
            IndexMap::from([("n".to_string(), Schema::float())])
        }
        fn outputs(&self) -> IndexMap<String, Schema> {
            IndexMap::from([("n".to_string(), Schema::float())])
        }
        fn update(&self, _state: &Value, _interval: f64) -> Update {
            Update::value(Value::tree([("n", Value::float(1.0))]))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    fn ns(trace: &Value) -> Vec<f64> {
        prism_trace::frames(trace)
            .iter()
            .map(|f| f.get_field("n").and_then(Value::as_f64).unwrap_or(f64::NAN))
            .collect()
    }

    #[test]
    fn folds_deltas_via_apply() {
        let mut reg = ProcessRegistry::new();
        reg.register("Increment", |_cfg| ProcessNode::Process(Box::new(Increment)));
        let mut sim = Simulate::from_config(&cfg("Increment", 3.0, 1.0));
        sim.set_registry(Arc::new(reg));

        let out = sim.update(&Value::tree([("state", mf(&[("n", 0.0)]))]));
        let trace = out.into_value().and_then(|v| v.get_field("trace").cloned()).expect("trace");

        // Engine-faithful: the `+1` event ACCUMULATES (apply adds under Float).
        assert_eq!(ns(&trace), vec![0.0, 1.0, 2.0, 3.0]);
        // The distinguishing assertion: a REPLACE policy (each `{n: 1}` taken as
        // the full next state) would give [0, 1, 1, 1]. Simulate differs precisely
        // by folding via `apply` (accumulate), not replacing.
        assert_ne!(ns(&trace), vec![0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn zero_steps_is_a_single_frame() {
        let mut reg = ProcessRegistry::new();
        reg.register("Increment", |_cfg| ProcessNode::Process(Box::new(Increment)));
        let mut sim = Simulate::from_config(&cfg("Increment", 0.0, 1.0));
        sim.set_registry(Arc::new(reg));
        let out = sim.update(&Value::tree([("state", mf(&[("n", 5.0)]))]));
        let trace = out.into_value().and_then(|v| v.get_field("trace").cloned()).unwrap();
        assert_eq!(prism_trace::len(&trace), 1);
        assert_eq!(ns(&trace), vec![5.0]);
    }

    #[test]
    fn no_registry_is_noop() {
        let sim = Simulate::from_config(&cfg("Increment", 3.0, 1.0));
        assert!(sim.update(&Value::tree([("state", mf(&[("n", 0.0)]))])).is_noop());
    }

    /// End to end: capture (Simulate) → replay (prism_trace::frames) → render
    /// (prism_viz::plot), through the REAL consumer — the `Trace.plot` method
    /// that `.ys` invokes as `traj.plot(title)`. This is the kernel proving out
    /// against an actual downstream, Arrow-free.
    #[test]
    fn trace_plots_through_the_method_path() {
        let mut reg = ProcessRegistry::new();
        reg.register("Increment", |_cfg| ProcessNode::Process(Box::new(Increment)));
        let mut sim = Simulate::from_config(&cfg("Increment", 3.0, 1.0));
        sim.set_registry(Arc::new(reg));
        let trace = sim
            .update(&Value::tree([("state", mf(&[("n", 0.0)]))]))
            .into_value()
            .and_then(|v| v.get_field("trace").cloned())
            .expect("trace");

        let mut methods = prism_schema::MethodRegistry::new();
        crate::register_methods(&mut methods);
        let figure =
            methods.dispatch(&trace, "plot", &[Value::from("increment")]).expect("Trace.plot");

        assert_eq!(figure.get_field("_type").and_then(Value::as_str), Some("Figure"));
        let root = figure.get_field("root").expect("figure carries the plot place-graph");
        // The element is Map[Float] ⇒ a line chart; rendering the REPLAYED frames
        // yields a real SVG with a data line.
        let svg = prism_viz::svg::to_svg(root);
        assert!(
            svg.starts_with("<svg") && svg.contains("<polyline"),
            "a line chart from the replayed delta-log trace:\n{svg}"
        );
    }
}
