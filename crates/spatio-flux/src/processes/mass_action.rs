//! Deterministic mass-action ODE integrators.
//!
//! Two fixed-step explicit integrators — forward Euler and classical
//! RK4 — over a mass-action reaction network. Both realize the **same
//! target** (the deterministic mass-action ODE, i.e. the mean-field /
//! large-copy-number limit of the chemical master equation that a
//! Gillespie `BigraphicalReactiveSystem` samples); they differ only in
//! **method**. They are the two `DeterministicMassAction` fulfillers for
//! the process-contract demonstration (`docs/process-contracts.md`):
//! same model, same target, different method — so their trajectories are
//! legitimately comparable and any divergence is method-induced.
//!
//! The network here is the deterministic reading of the same reactions a
//! BRS fires stochastically: one model, multiple realizations under
//! different contracts (Gillespie → CME; these → mass-action ODE; FBA →
//! constraint-based steady-state flux). HiGHS/FBA targets a *different*
//! object, which is why it cannot stand in here — see the doc.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Foreign, Process, Schema, Update, Value};
use prism_schema::{MethodError, MethodRegistry};

/// A single mass-action reaction: `reactants -> products` at rate `k`.
///
/// Stoichiometry entries are `(species_index, coefficient)`. The
/// mass-action propensity is `k · ∏ x[i]^coeff` over the reactants.
#[derive(Clone, Debug)]
pub struct Reaction {
    pub reactants: Vec<(usize, u32)>,
    pub products: Vec<(usize, u32)>,
    pub k: f64,
}

/// A mass-action reaction network: named species plus reactions over
/// them. This is the shared *model*; an integrator is a *method*
/// realizing the deterministic mass-action *target* over it.
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

    /// Index of a species by name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.species.iter().position(|s| s == name)
    }

    /// Builder: append a reaction.
    pub fn with_reaction(mut self, r: Reaction) -> Self {
        self.reactions.push(r);
        self
    }

    /// `dx/dt` at state `x`. For each reaction the propensity is
    /// `v = k · ∏ xᵢ^νᵢ` over reactants; it contributes `(νprod − νreac)·v`
    /// to each participating species.
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

/// A simulated trajectory: sample `times` plus one value column per
/// species, each column aligned with `times`. The column shape matches
/// `spatio_flux::report::render_timeseries_svg` so plotting (the
/// `overlay` analysis method, rung-1 step 2) is a direct fit.
#[derive(Clone, Debug)]
pub struct TimeSeries {
    pub times: Vec<f64>,
    pub columns: IndexMap<String, Vec<f64>>,
}

impl TimeSeries {
    /// Mean squared error per species against another trajectory sampled
    /// on the same time grid. Only species present in both are reported;
    /// each MSE averages over the overlapping prefix length.
    ///
    /// This is the comparison `biocompose`'s `CompareResults` performs —
    /// but here the two inputs are guaranteed (by a shared contract) to
    /// approximate the same mathematical object, so the number has a
    /// warrant. See `docs/process-contracts.md`.
    pub fn species_mse(&self, other: &TimeSeries) -> IndexMap<String, f64> {
        let mut out = IndexMap::new();
        for (name, a) in &self.columns {
            if let Some(b) = other.columns.get(name) {
                let n = a.len().min(b.len());
                if n == 0 {
                    continue;
                }
                let sse: f64 = (0..n).map(|i| (a[i] - b[i]).powi(2)).sum();
                out.insert(name.clone(), sse / n as f64);
            }
        }
        out
    }
}

/// Which fixed-step explicit scheme to integrate with — the **method**
/// axis of the process contract. Both schemes share the deterministic
/// mass-action **target**; that shared target is what makes a comparison
/// between their outputs legitimate.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Integrator {
    /// Forward (explicit) Euler — first order. Maps to KISAO:0000030.
    ForwardEuler,
    /// Classical four-stage Runge-Kutta — fourth order. Maps to KISAO:0000032.
    Rk4,
}

/// Integrate `network` from `init` over `[0, t_end]` with fixed step
/// `dt`, recording the state after every step (including `t = 0`).
/// `init` is indexed like `network.species`.
pub fn integrate(
    network: &MassActionNetwork,
    init: &[f64],
    t_end: f64,
    dt: f64,
    method: Integrator,
) -> TimeSeries {
    assert_eq!(
        init.len(),
        network.species.len(),
        "init must have one value per species"
    );
    assert!(dt > 0.0, "dt must be positive");

    let n_steps = (t_end / dt).round().max(0.0) as usize;
    let mut x = init.to_vec();
    let mut times = Vec::with_capacity(n_steps + 1);
    let mut cols: Vec<Vec<f64>> = vec![Vec::with_capacity(n_steps + 1); network.species.len()];

    times.push(0.0);
    for (i, xi) in x.iter().enumerate() {
        cols[i].push(*xi);
    }

    for step in 0..n_steps {
        x = match method {
            Integrator::ForwardEuler => euler_step(network, &x, dt),
            Integrator::Rk4 => rk4_step(network, &x, dt),
        };
        times.push((step + 1) as f64 * dt);
        for (i, xi) in x.iter().enumerate() {
            cols[i].push(*xi);
        }
    }

    let columns = network.species.iter().cloned().zip(cols).collect();
    TimeSeries { times, columns }
}

fn euler_step(net: &MassActionNetwork, x: &[f64], dt: f64) -> Vec<f64> {
    let k1 = net.derivatives(x);
    x.iter().zip(&k1).map(|(xi, d)| xi + dt * d).collect()
}

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

// ── Native process: the integrator as a one-shot Step ──────────────────
//
// `Rk4` / `ForwardEuler` are RUST-NATIVE processes (defined here, exposing the
// process interface) that chrysalis calls via `extern`. A one-shot uniform-
// time-course step (cf. biocompose's `*UTCStep`): given the initial
// concentrations it integrates the whole `[0, t_end]` and emits the full
// `trajectory` as a Foreign `TimeSeries`. Network / method / dt / t_end are
// config; `init` is the input. The first of a growing library of ready-to-call
// native building blocks (the other process kind is ys-native, defined in terms
// of operations on the types these expose).

/// One-shot mass-action ODE integrator, exposed as a native `Step`.
#[derive(Clone, Debug)]
pub struct MassActionIntegrator {
    pub network: MassActionNetwork,
    pub method: Integrator,
    pub t_end: f64,
    pub dt: f64,
}

impl Process for MassActionIntegrator {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(
            "init".to_string(),
            Schema::Map {
                value: Box::new(Schema::float()),
            },
        )])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(
            "trajectory".to_string(),
            Schema::Custom {
                name: "TimeSeries".to_string(),
                parameters: IndexMap::new(),
            },
        )])
    }

    // One-shot: integrate the whole `[0, t_end]` each invocation, ignoring the
    // tick interval. Implemented as a `Process` so the engine schedules it
    // directly — its `trajectory` output then triggers the downstream `Compare`
    // step. (A `Step` would only fire in the construction-time dependency sweep,
    // which a ys-compiled spec discovered later misses.)
    fn interval(&self) -> f64 {
        1.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let init_map = state.get_field("init").and_then(|v| v.as_map());
        let init: Vec<f64> = self
            .network
            .species
            .iter()
            .map(|s| {
                init_map
                    .and_then(|m| m.get(s.as_str()))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            })
            .collect();
        let ts = integrate(&self.network, &init, self.t_end, self.dt, self.method);
        Update::value(Value::tree([(
            "trajectory",
            Value::Foreign(Foreign::new("TimeSeries", ts)),
        )]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Build a [`MassActionIntegrator`] from a vivarium-style config:
/// `{ network: { species: [...], reactions: [{reactants, products, k}] },
/// t_end, dt }`. The `method` (Rk4 / ForwardEuler) is fixed by the registered
/// factory name.
pub fn integrator_from_config(config: &Value, method: Integrator) -> MassActionIntegrator {
    let network = config
        .get_field("network")
        .map(network_from_value)
        .unwrap_or_else(|| MassActionNetwork::new(Vec::<String>::new()));
    let t_end = config.get_field("t_end").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let dt = config.get_field("dt").and_then(|v| v.as_f64()).unwrap_or(0.01);
    MassActionIntegrator {
        network,
        method,
        t_end,
        dt,
    }
}

/// Decode `{ species: [...], reactions: [{reactants, products, k}] }` into a
/// [`MassActionNetwork`]; stoichiometry maps are `{species: coeff}`.
fn network_from_value(v: &Value) -> MassActionNetwork {
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

// ── TimeSeries value-methods (the ys-native side) ──────────────────────
//
// Registered against the `TimeSeries` type so a *ys-native* process body can
// call `a.species_mse(b)` / `a.overlay(b)`, dispatched via the `MethodRegistry`
// (the second process kind reaching native operations — docs/chrysalis-design.md
// "Two kinds of process"). This is how a contract-typed `Compare` step computes
// its analysis + plot from two trajectories.

fn as_timeseries(v: &Value) -> Option<&TimeSeries> {
    match v {
        Value::Foreign(f) => f.downcast_ref::<TimeSeries>(),
        _ => None,
    }
}

fn ts_err(method: &str, message: &str) -> MethodError {
    MethodError::BadArgs {
        type_name: "TimeSeries".to_string(),
        method: method.to_string(),
        message: message.to_string(),
    }
}

/// Register `TimeSeries` value-methods (`species_mse`, `overlay`) on a
/// `MethodRegistry`. A workflow that composes these native integrators merges
/// this into chrysalis's method registry so ys-native bodies can call them.
pub fn register_methods(reg: &mut MethodRegistry) {
    reg.register("TimeSeries", "species_mse", |recv, args| {
        let a = as_timeseries(recv).ok_or_else(|| ts_err("species_mse", "receiver is not a TimeSeries"))?;
        let b = args
            .first()
            .and_then(as_timeseries)
            .ok_or_else(|| ts_err("species_mse", "argument 0 must be a TimeSeries"))?;
        Ok(Value::tree(
            a.species_mse(b).into_iter().map(|(k, v)| (k, Value::float(v))),
        ))
    });

    reg.register("TimeSeries", "overlay", |recv, args| {
        let a = as_timeseries(recv).ok_or_else(|| ts_err("overlay", "receiver is not a TimeSeries"))?;
        let b = args
            .first()
            .and_then(as_timeseries)
            .ok_or_else(|| ts_err("overlay", "argument 0 must be a TimeSeries"))?;
        let mut series: IndexMap<String, Vec<f64>> = IndexMap::new();
        for (sp, col) in &a.columns {
            series.insert(format!("{sp} (a)"), col.clone());
        }
        for (sp, col) in &b.columns {
            series.insert(format!("{sp} (b)"), col.clone());
        }
        let svg = crate::report::render_timeseries_svg("overlay", &a.times, &series, false);
        Ok(Value::Foreign(Foreign::new("Figure", svg)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `A -> B` at rate `k` — the network used across the contract demo.
    /// Closed form: `A(t) = A₀·e^{−kt}`, `B(t) = A₀·(1 − e^{−kt})`.
    fn a_to_b(k: f64) -> MassActionNetwork {
        MassActionNetwork::new(["A", "B"]).with_reaction(Reaction {
            reactants: vec![(0, 1)],
            products: vec![(1, 1)],
            k,
        })
    }

    #[test]
    fn rk4_matches_analytic_decay_and_conserves_mass() {
        let k = 0.7;
        let net = a_to_b(k);
        let ts = integrate(&net, &[1.0, 0.0], 5.0, 0.01, Integrator::Rk4);
        for (i, &t) in ts.times.iter().enumerate() {
            let exact_a = (-k * t).exp();
            let a = ts.columns["A"][i];
            let b = ts.columns["B"][i];
            assert!((a - exact_a).abs() < 1e-6, "A(t={t}) = {a}, exact {exact_a}");
            assert!((a + b - 1.0).abs() < 1e-9, "A+B not conserved at t={t}");
        }
    }

    #[test]
    fn euler_is_first_order_and_lags() {
        let k = 0.7;
        let net = a_to_b(k);
        let ts = integrate(&net, &[1.0, 0.0], 5.0, 0.05, Integrator::ForwardEuler);
        let max_err = ts
            .times
            .iter()
            .enumerate()
            .map(|(i, &t)| (ts.columns["A"][i] - (-k * t).exp()).abs())
            .fold(0.0_f64, f64::max);
        // Measurably worse than RK4, but still in the right ballpark.
        assert!(max_err > 1e-4, "euler should be measurably off: {max_err}");
        assert!(max_err < 5e-2, "euler should still track the solution: {max_err}");
    }

    #[test]
    fn same_target_different_method_diverges_then_converges() {
        // The contract claim made executable: Rk4 and ForwardEuler share
        // the DeterministicMassAction target, so their trajectories are
        // legitimately comparable; the MSE between them is method-induced
        // and shrinks as the step refines.
        let net = a_to_b(0.7);
        let mse_a_at = |dt: f64| -> f64 {
            let r = integrate(&net, &[1.0, 0.0], 5.0, dt, Integrator::Rk4);
            let e = integrate(&net, &[1.0, 0.0], 5.0, dt, Integrator::ForwardEuler);
            r.species_mse(&e)["A"]
        };
        let coarse = mse_a_at(0.2);
        let fine = mse_a_at(0.02);
        assert!(coarse > 0.0, "methods must differ at a coarse step");
        assert!(fine < coarse, "MSE must shrink as dt refines: {fine} !< {coarse}");
    }

    #[test]
    fn registered_as_a_native_process_and_runs() {
        use prism_bigraph::ProcessNode;

        // `Rk4` resolves through the spatio-flux registry — the same factory an
        // `extern Rk4` in chrysalis binds to. Config carries the network + dt +
        // t_end (A→B at k=0.7); `init` is the input.
        let reg = crate::from_config::build_registry();
        let config = Value::tree([
            (
                "network",
                Value::tree([
                    ("species", Value::List(vec![Value::from("A"), Value::from("B")])),
                    (
                        "reactions",
                        Value::List(vec![Value::tree([
                            ("reactants", Value::tree([("A", Value::float(1.0))])),
                            ("products", Value::tree([("B", Value::float(1.0))])),
                            ("k", Value::float(0.7)),
                        ])]),
                    ),
                ]),
            ),
            ("t_end", Value::float(5.0)),
            ("dt", Value::float(0.01)),
        ]);

        let ProcessNode::Process(proc) =
            reg.create("Rk4", config).expect("Rk4 factory registered")
        else {
            panic!("Rk4 should be a Process");
        };

        let state = Value::tree([(
            "init",
            Value::tree([("A", Value::float(1.0)), ("B", Value::float(0.0))]),
        )]);
        let traj = proc
            .update(&state, 1.0)
            .into_value()
            .and_then(|v| v.get_field("trajectory").cloned())
            .expect("trajectory output");
        let Value::Foreign(f) = traj else {
            panic!("trajectory should be a Foreign TimeSeries");
        };
        let ts = f.downcast_ref::<TimeSeries>().expect("downcast TimeSeries");

        let last = ts.times.len() - 1;
        let a_final = ts.columns["A"][last];
        assert!(
            (a_final - (-0.7 * 5.0_f64).exp()).abs() < 1e-5,
            "native Rk4 step should match the analytic decay e^(-kt); got {a_final}"
        );
    }

    #[test]
    fn timeseries_methods_dispatch_via_registry() {
        use prism_bigraph::Foreign;
        use prism_schema::MethodRegistry;

        let mut reg = MethodRegistry::new();
        register_methods(&mut reg);

        let net = a_to_b(0.7);
        let rk4 = integrate(&net, &[1.0, 0.0], 5.0, 0.01, Integrator::Rk4);
        let euler = integrate(&net, &[1.0, 0.0], 5.0, 0.01, Integrator::ForwardEuler);
        let a = Value::Foreign(Foreign::new("TimeSeries", rk4));
        let b = Value::Foreign(Foreign::new("TimeSeries", euler));

        // `a.species_mse(b)` → a Map of per-species MSE; A's is method-induced
        // (small, but nonzero — the two methods genuinely differ).
        let mse = reg
            .dispatch(&a, "species_mse", std::slice::from_ref(&b))
            .expect("species_mse dispatch");
        let a_mse = mse.get_field("A").and_then(|v| v.as_f64()).expect("A's MSE");
        assert!(a_mse > 0.0 && a_mse < 1e-2, "method-induced MSE on A: {a_mse}");

        // `a.overlay(b)` → a Foreign Figure carrying an SVG document.
        let fig = reg
            .dispatch(&a, "overlay", std::slice::from_ref(&b))
            .expect("overlay dispatch");
        let Value::Foreign(f) = fig else {
            panic!("overlay should return a Figure");
        };
        assert_eq!(f.type_name, "Figure");
        let svg = f.downcast_ref::<String>().expect("Figure wraps an SVG string");
        assert!(svg.contains("<svg"), "overlay should be an SVG document");
    }
}
