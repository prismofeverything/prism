//! Deterministic mass-action ODE: a reaction network + two single-step
//! integrators (forward Euler, RK4), each a plain prism `Process`.
//!
//! A `Process` advances ONE step per `update(state, interval)` — reusable,
//! repeatable. `RunProcess` (`process_runner.rs`) drives one over a runtime to
//! produce a `TimeSeries`. Both integrators realize the **same target** (the
//! deterministic mass-action ODE — the mean-field limit of the chemical master
//! equation a Gillespie BRS samples) by different **methods**: the two
//! `DeterministicMassAction` fulfillers (`docs/process-contracts.md`).
//!
//! `TimeSeries` here is a plain ys VALUE (`{_type:"TimeSeries", times, columns}`),
//! not a Foreign Rust struct — `species_mse`/`overlay` are registered methods on
//! the `"TimeSeries"` type that read that Value (the "all types are declared
//! types" rule).

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, StateMap, Update, Value};
use prism_schema::{MethodError, MethodRegistry, MethodResult};

/// A single mass-action reaction: `reactants -> products` at rate `k`.
/// Stoichiometry entries are `(species_index, coefficient)`; the mass-action
/// propensity is `k · ∏ x[i]^coeff` over the reactants.
#[derive(Clone, Debug)]
pub struct Reaction {
    pub reactants: Vec<(usize, u32)>,
    pub products: Vec<(usize, u32)>,
    pub k: f64,
}

/// A mass-action reaction network: named species + reactions over them. The
/// shared *model*; an integrator is a *method* realizing the deterministic
/// mass-action *target* over it.
#[derive(Clone, Debug)]
pub struct MassActionNetwork {
    pub species: Vec<String>,
    pub reactions: Vec<Reaction>,
}

impl MassActionNetwork {
    pub fn new(species: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            species: species.into_iter().map(Into::into).collect(),
            reactions: Vec::new(),
        }
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.species.iter().position(|s| s == name)
    }

    pub fn with_reaction(mut self, r: Reaction) -> Self {
        self.reactions.push(r);
        self
    }

    /// `dx/dt` at state `x`: each reaction's propensity `v = k · ∏ xᵢ^νᵢ` over
    /// reactants contributes `(νprod − νreac)·v` to each species.
    pub fn derivatives(&self, x: &[f64]) -> Vec<f64> {
        let mut dx = vec![0.0; self.species.len()];
        for r in &self.reactions {
            let mut v = r.k;
            for &(i, coeff) in &r.reactants {
                v *= x[i].powi(coeff as i32);
            }
            for &(i, coeff) in &r.reactants {
                dx[i] -= coeff as f64 * v;
            }
            for &(i, coeff) in &r.products {
                dx[i] += coeff as f64 * v;
            }
        }
        dx
    }
}

/// Which fixed-step explicit scheme — the **method** axis of the contract.
/// Both share the deterministic mass-action **target**.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Integrator {
    /// Forward (explicit) Euler — first order. Maps to KISAO:0000030.
    ForwardEuler,
    /// Classical four-stage Runge-Kutta — fourth order. Maps to KISAO:0000032.
    Rk4,
}

/// One forward-Euler step of size `dt`.
fn euler_step(net: &MassActionNetwork, x: &[f64], dt: f64) -> Vec<f64> {
    let k1 = net.derivatives(x);
    x.iter().zip(&k1).map(|(xi, d)| xi + dt * d).collect()
}

/// One classical RK4 step of size `dt`.
fn rk4_step(net: &MassActionNetwork, x: &[f64], dt: f64) -> Vec<f64> {
    let axpy = |x: &[f64], k: &[f64], s: f64| -> Vec<f64> {
        x.iter().zip(k).map(|(xi, ki)| xi + s * ki).collect()
    };
    let k1 = net.derivatives(x);
    let k2 = net.derivatives(&axpy(x, &k1, dt / 2.0));
    let k3 = net.derivatives(&axpy(x, &k2, dt / 2.0));
    let k4 = net.derivatives(&axpy(x, &k3, dt));
    (0..x.len())
        .map(|i| x[i] + dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
        .collect()
}

// ── The integrator as a plain (stepwise) Process ───────────────────────

/// A one-step mass-action integrator: `update(state, interval)` advances the
/// concentrations by one Euler/RK4 step of size `interval` and returns the full
/// next state. Reusable — call it repeatedly (that is what `RunProcess` does).
/// `Rk4` / `ForwardEuler` register this; the network is config.
#[derive(Clone, Debug)]
pub struct MassActionProcess {
    pub network: MassActionNetwork,
    pub method: Integrator,
}

impl Process for MassActionProcess {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(
            "state".to_string(),
            Schema::Map {
                value: Box::new(Schema::float()),
            },
        )])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(
            "state".to_string(),
            Schema::Map {
                value: Box::new(Schema::float()),
            },
        )])
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let map = state.get_field("state").and_then(|v| v.as_map());
        let x: Vec<f64> = self
            .network
            .species
            .iter()
            .map(|s| {
                map.and_then(|m| m.get(s.as_str()))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            })
            .collect();
        let next = match self.method {
            Integrator::ForwardEuler => euler_step(&self.network, &x, interval),
            Integrator::Rk4 => rk4_step(&self.network, &x, interval),
        };
        // The full next state (RunProcess feeds it straight back next step).
        let out = Value::tree(
            self.network
                .species
                .iter()
                .cloned()
                .zip(next)
                .map(|(s, v)| (s, Value::float(v))),
        );
        Update::value(Value::tree([("state", out)]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Build a [`MassActionProcess`] from a config `{ network: {species, reactions} }`.
/// The `method` (Rk4 / ForwardEuler) is fixed by the registered factory name.
pub fn process_from_config(config: &Value, method: Integrator) -> MassActionProcess {
    let network = config
        .get_field("network")
        .map(network_from_value)
        .unwrap_or_else(|| MassActionNetwork::new(Vec::<String>::new()));
    MassActionProcess { network, method }
}

/// Decode `{ species: [...], reactions: [{reactants, products, k}] }` into a
/// [`MassActionNetwork`]; stoichiometry maps are `{species: coeff}`.
pub fn network_from_value(v: &Value) -> MassActionNetwork {
    let species: Vec<String> = v
        .get_field("species")
        .and_then(|s| s.as_list())
        .map(|l| l.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut net = MassActionNetwork::new(species);
    if let Some(rxns) = v.get_field("reactions").and_then(|r| r.as_list()) {
        for r in rxns {
            let k = r.get_field("k").and_then(|x| x.as_f64()).unwrap_or(1.0);
            let reactants = stoichiometry(&net, r.get_field("reactants"));
            let products = stoichiometry(&net, r.get_field("products"));
            net.reactions.push(Reaction {
                reactants,
                products,
                k,
            });
        }
    }
    net
}

/// Read a `{species: coeff}` map into `(species_index, coeff)` pairs.
fn stoichiometry(net: &MassActionNetwork, v: Option<&Value>) -> Vec<(usize, u32)> {
    let mut out = Vec::new();
    if let Some(m) = v.and_then(|v| v.as_map()) {
        for (sp, coeff) in m {
            if let Some(i) = net.index_of(sp.as_str()) {
                out.push((i, coeff.as_f64().unwrap_or(1.0) as u32));
            }
        }
    }
    out
}

// ── TimeSeries value-methods (a declared type, not Foreign) ─────────────
//
// `TimeSeries` is the Value `{_type:"TimeSeries", times:[…], columns:{sp:[…]}}`.
// `species_mse` / `overlay` are registered against the `"TimeSeries"` type and
// read that Value — so a ys-native `Compare` calls `a.species_mse(b)` and it
// dispatches here, with no Foreign anywhere ys can see.

fn ts_columns(v: &Value) -> Option<&StateMap> {
    v.get_field("columns").and_then(|c| c.as_map())
}

fn ts_floats(v: &Value) -> Vec<f64> {
    v.as_list()
        .map(|l| l.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

fn ts_err(method: &str, message: &str) -> MethodError {
    MethodError::BadArgs {
        type_name: "TimeSeries".to_string(),
        method: method.to_string(),
        message: message.to_string(),
    }
}

/// A native integrator object for `from integrators import rk4, euler`: the
/// value `{_type: "Integrator", method}` whose `integrate(network, state,
/// interval)` method advances one step. A `.ys` `process` wraps it, declaring
/// the ports + `fulfills` contract; the math stays here (the "import a function,
/// build the process in ys" model — `docs/process-contracts.md`).
pub fn integrator(method: &str) -> Value {
    Value::tree([
        ("_type", Value::from("Integrator")),
        ("method", Value::from(method)),
    ])
}

/// Register `TimeSeries` value-methods (`species_mse`, `overlay`) and the
/// `Integrator` `integrate` method on a `MethodRegistry`. A workflow that
/// composes the integrators merges this into the method registry so ys-native
/// bodies can call them.
pub fn register_methods(reg: &mut MethodRegistry) {
    // `rk4.integrate(network, state, interval)` — one explicit step of the
    // method named by the receiver, returning `{state: <next>}` (the output
    // port shape `RunProcess` feeds back in). Mirrors `MassActionProcess`.
    reg.register("Integrator", "integrate", |recv, args| {
        let method = recv.get_field("method").and_then(|v| v.as_str()).unwrap_or("rk4");
        let network = args
            .first()
            .map(network_from_value)
            .ok_or_else(|| ts_err("integrate", "argument 0 must be a reaction network"))?;
        let state = args.get(1).and_then(|v| v.as_map());
        let interval = args.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0);
        let x: Vec<f64> = network
            .species
            .iter()
            .map(|s| {
                state
                    .and_then(|m| m.get(s.as_str()))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            })
            .collect();
        let next = match method {
            "euler" | "ForwardEuler" => euler_step(&network, &x, interval),
            _ => rk4_step(&network, &x, interval),
        };
        let out = Value::tree(
            network
                .species
                .iter()
                .cloned()
                .zip(next)
                .map(|(s, v)| (s, Value::float(v))),
        );
        Ok(Value::tree([("state", out)]))
    });
    reg.register("TimeSeries", "species_mse", |recv, args| {
        let a = ts_columns(recv).ok_or_else(|| ts_err("species_mse", "receiver is not a TimeSeries"))?;
        let b = args
            .first()
            .and_then(ts_columns)
            .ok_or_else(|| ts_err("species_mse", "argument 0 must be a TimeSeries"))?;
        let mse = a.iter().filter_map(|(sp, acol)| {
            let bcol = b.get(sp.as_str())?;
            let (av, bv) = (ts_floats(acol), ts_floats(bcol));
            let n = av.len().min(bv.len());
            if n == 0 {
                return None;
            }
            let sse: f64 = (0..n).map(|i| (av[i] - bv[i]).powi(2)).sum();
            Some((sp.to_string(), Value::float(sse / n as f64)))
        });
        Ok(Value::tree(mse))
    });

    reg.register("TimeSeries", "overlay", |recv, args| {
        let a = ts_columns(recv).ok_or_else(|| ts_err("overlay", "receiver is not a TimeSeries"))?;
        let b = args
            .first()
            .and_then(ts_columns)
            .ok_or_else(|| ts_err("overlay", "argument 0 must be a TimeSeries"))?;
        // Optional second arg: a plot title (`a.overlay(b, 'Rk4 vs ForwardEuler')`).
        let title = args.get(1).and_then(|v| v.as_str()).unwrap_or("overlay");
        let times: Vec<f64> = recv
            .get_field("times")
            .map(ts_floats)
            .unwrap_or_default();
        let mut series: IndexMap<String, Vec<f64>> = IndexMap::new();
        for (sp, col) in a {
            series.insert(format!("{sp} (a)"), ts_floats(col));
        }
        for (sp, col) in b {
            series.insert(format!("{sp} (b)"), ts_floats(col));
        }
        let svg = prism_viz::render_timeseries_svg(title, &times, &series, false);
        Ok(Value::tree([
            ("_type", Value::from("Figure")),
            ("svg", Value::String(svg)),
        ]))
    });

    // ── Effectful writers (the self-outputting Output step) ─────────────
    // `a.csv(path)` / `mse.csv(path)` / `figure.svg(path)` write a file to
    // `<path>.<ext>` and return None. The `.ys` workflow emits its own artifacts.
    reg.register("TimeSeries", "csv", |recv, args| {
        let path = arg_path(args, "csv")?;
        write_file(&format!("{path}.csv"), &timeseries_to_csv(recv), "csv")
    });

    reg.register("Map", "csv", |recv, args| {
        let path = arg_path(args, "csv")?;
        let map = recv.as_map().ok_or_else(|| ts_err("csv", "receiver is not a map"))?;
        let mut out = String::from("key,value\n");
        for (k, v) in map {
            if k.as_str() == "_type" {
                continue;
            }
            let cell = v.as_f64().map(|f| f.to_string()).unwrap_or_default();
            out.push_str(&format!("{k},{cell}\n"));
        }
        write_file(&format!("{path}.csv"), &out, "csv")
    });

    reg.register("Figure", "svg", |recv, args| {
        let path = arg_path(args, "svg")?;
        let svg = recv.get_field("svg").and_then(|v| v.as_str()).unwrap_or("");
        write_file(&format!("{path}.svg"), svg, "svg")
    });
}

/// Read the path argument (first positional) of a writer method.
fn arg_path<'a>(args: &'a [Value], method: &str) -> Result<&'a str, MethodError> {
    args.first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| ts_err(method, "a path string argument is required"))
}

/// Write `contents` to `path`, creating parent directories. Returns `None`.
fn write_file(path: &str, contents: &str, method: &str) -> MethodResult {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ts_err(method, &format!("create dir for {path}: {e}")))?;
    }
    std::fs::write(path, contents).map_err(|e| ts_err(method, &format!("write {path}: {e}")))?;
    Ok(Value::None)
}

/// Render a `TimeSeries` value as CSV text: a `time` column then one per species.
fn timeseries_to_csv(ts: &Value) -> String {
    let times: Vec<f64> = ts
        .get_field("times")
        .map(ts_floats)
        .unwrap_or_default();
    let columns = ts_columns(ts);
    let species: Vec<String> = columns
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();

    let mut out = String::from("time");
    for sp in &species {
        out.push(',');
        out.push_str(sp);
    }
    out.push('\n');
    for (i, t) in times.iter().enumerate() {
        out.push_str(&t.to_string());
        for sp in &species {
            let v = columns
                .and_then(|m| m.get(sp.as_str()))
                .and_then(|c| c.as_list())
                .and_then(|l| l.get(i))
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN);
            out.push(',');
            out.push_str(&v.to_string());
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `A -> B` at rate `k`. Closed form: `A(t) = A₀·e^{−kt}`.
    fn a_to_b(k: f64) -> MassActionNetwork {
        MassActionNetwork::new(["A", "B"]).with_reaction(Reaction {
            reactants: vec![(0, 1)],
            products: vec![(1, 1)],
            k,
        })
    }

    /// Loop the stepwise process over `[0, t_end]` — the trajectory `RunProcess`
    /// produces — returning per-step `[A, B]` snapshots and their times.
    fn run(net: &MassActionNetwork, init: &[f64], t_end: f64, dt: f64, m: Integrator) -> Vec<(f64, Vec<f64>)> {
        let n = (t_end / dt).round() as usize;
        let mut x = init.to_vec();
        let mut out = vec![(0.0, x.clone())];
        for s in 0..n {
            x = match m {
                Integrator::ForwardEuler => euler_step(net, &x, dt),
                Integrator::Rk4 => rk4_step(net, &x, dt),
            };
            out.push(((s + 1) as f64 * dt, x.clone()));
        }
        out
    }

    #[test]
    fn rk4_matches_analytic_decay_and_conserves_mass() {
        let k = 0.7;
        for (t, x) in run(&a_to_b(k), &[1.0, 0.0], 5.0, 0.01, Integrator::Rk4) {
            assert!((x[0] - (-k * t).exp()).abs() < 1e-6, "A(t={t})={}", x[0]);
            assert!((x[0] + x[1] - 1.0).abs() < 1e-9, "A+B not conserved at t={t}");
        }
    }

    #[test]
    fn euler_lags_but_tracks() {
        let k = 0.7;
        let max_err = run(&a_to_b(k), &[1.0, 0.0], 5.0, 0.05, Integrator::ForwardEuler)
            .into_iter()
            .map(|(t, x)| (x[0] - (-k * t).exp()).abs())
            .fold(0.0_f64, f64::max);
        assert!(max_err > 1e-4 && max_err < 5e-2, "euler max err {max_err}");
    }

    #[test]
    fn process_advances_exactly_one_step() {
        let net = a_to_b(0.7);
        let proc = MassActionProcess { network: net.clone(), method: Integrator::Rk4 };
        let state = Value::tree([(
            "state",
            Value::tree([("A", Value::float(1.0)), ("B", Value::float(0.0))]),
        )]);
        let next = proc
            .update(&state, 0.1)
            .into_value()
            .and_then(|v| v.get_field("state").cloned())
            .expect("state output");
        let a = next.get_field("A").and_then(|v| v.as_f64()).unwrap();
        let expected = rk4_step(&net, &[1.0, 0.0], 0.1)[0];
        assert!((a - expected).abs() < 1e-12, "{a} vs {expected}");
    }

    #[test]
    fn timeseries_methods_dispatch_on_a_value() {
        let mut reg = MethodRegistry::new();
        register_methods(&mut reg);

        // Two TimeSeries Values that differ on A: a Value, not a Foreign.
        let ts = |col: &[f64]| {
            Value::tree([
                ("_type", Value::from("TimeSeries")),
                ("times", Value::List(vec![Value::float(0.0), Value::float(1.0)])),
                (
                    "columns",
                    Value::tree([("A", Value::List(col.iter().map(|x| Value::float(*x)).collect()))]),
                ),
            ])
        };
        let x = ts(&[1.0, 0.5]);
        let y = ts(&[1.0, 0.4]);

        // MSE on A = mean([0, (0.5-0.4)^2]) = 0.01/2 = 0.005.
        let mse = reg
            .dispatch(&x, "species_mse", std::slice::from_ref(&y))
            .expect("species_mse");
        let a_mse = mse.get_field("A").and_then(|v| v.as_f64()).expect("A mse");
        assert!((a_mse - 0.005).abs() < 1e-9, "mse {a_mse}");

        // overlay → a Figure VALUE carrying an SVG (no Foreign).
        let fig = reg
            .dispatch(&x, "overlay", std::slice::from_ref(&y))
            .expect("overlay");
        assert_eq!(fig.get_field("_type").and_then(|v| v.as_str()), Some("Figure"));
        let svg = fig.get_field("svg").and_then(|v| v.as_str()).expect("svg");
        assert!(svg.contains("<svg"), "overlay should be an SVG document");
    }
}
