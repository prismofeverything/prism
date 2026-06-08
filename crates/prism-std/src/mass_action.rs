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

    /// Per-reaction mass-action propensity `v = k · ∏ xᵢ^νᵢ` over reactants —
    /// the rate each reaction fires at state `x`. Shared by the deterministic
    /// `derivatives` (summed into `dx/dt`) and the stochastic SSA (the Gillespie
    /// event rates): the SAME propensities, which is *why* the SSA ensemble mean
    /// tracks the ODE — one model, two targets, the demo's whole point.
    pub fn propensities(&self, x: &[f64]) -> Vec<f64> {
        self.reactions
            .iter()
            .map(|r| {
                let mut v = r.k;
                for &(i, coeff) in &r.reactants {
                    v *= x[i].powi(coeff as i32);
                }
                v
            })
            .collect()
    }

    /// `dx/dt` at state `x`: each reaction's propensity contributes
    /// `(νprod − νreac)·v` to each species.
    pub fn derivatives(&self, x: &[f64]) -> Vec<f64> {
        let mut dx = vec![0.0; self.species.len()];
        for (r, &v) in self.reactions.iter().zip(self.propensities(x).iter()) {
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

// ── Stochastic simulation (Gillespie direct) — the EXACT-CME target ─────
//
// The stochastic sibling of the integrators: the same network's reactions fired
// as discrete events at the same mass-action propensities, so a single path is a
// jagged sample of the chemical master equation whose ENSEMBLE mean tracks the
// deterministic ODE. A single seeded PRNG runs over the whole trajectory, so the
// path is reproducible from `seed` (repeatability) while different seeds give
// genuinely different paths (the ensemble) — exactly the distinction the
// `claims: Distributional` contract names.

/// A tiny deterministic PRNG (splitmix64). SSA needs randomness, but the point
/// of the stochastic lane is REPRODUCIBILITY — a fixed seed ⇒ a fixed path — so
/// we carry our own seeded generator rather than depend on a thread RNG.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `(0, 1]` — never 0, so `-ln(u)` (the exponential waiting time)
    /// is always finite.
    fn next_unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 1.0) / 9_007_199_254_740_992.0
    }
}

/// Sample the next reaction event `(absolute_time, reaction_index)` by the
/// Gillespie direct method, or `None` at an absorbing state (zero propensity).
fn schedule_event(
    net: &MassActionNetwork,
    x: &[f64],
    t: f64,
    rng: &mut SplitMix64,
) -> Option<(f64, usize)> {
    let props = net.propensities(x);
    let a0: f64 = props.iter().sum();
    if a0 <= 0.0 {
        return None;
    }
    let tau = -rng.next_unit().ln() / a0;
    // Pick the firing reaction with probability proportional to its propensity.
    let mut r = rng.next_unit() * a0;
    let mut j = props.len() - 1;
    for (i, &p) in props.iter().enumerate() {
        if r < p {
            j = i;
            break;
        }
        r -= p;
    }
    Some((t + tau, j))
}

/// Apply reaction `j`'s stoichiometry (products gained, reactants consumed) to
/// the count vector `x`.
fn fire(net: &MassActionNetwork, x: &mut [f64], j: usize) {
    let r = &net.reactions[j];
    for &(i, coeff) in &r.reactants {
        x[i] -= coeff as f64;
    }
    for &(i, coeff) in &r.products {
        x[i] += coeff as f64;
    }
}

/// One Gillespie-direct trajectory of `net` from counts `x0`, sampled on the
/// grid `0, dt, … runtime`. The state is piecewise-constant between events; at
/// each grid point we fire every pending event up to that time, then record the
/// counts. Reproducible from `seed`. Returns `(times, rows)` where `rows[k]` is
/// the count vector at `times[k]`.
fn ssa_trajectory(
    net: &MassActionNetwork,
    x0: &[f64],
    runtime: f64,
    dt: f64,
    seed: u64,
) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n_points = (runtime / dt).round().max(0.0) as usize + 1;
    let times: Vec<f64> = (0..n_points).map(|k| k as f64 * dt).collect();
    let mut rng = SplitMix64(seed);
    let mut x = x0.to_vec();
    let mut next = schedule_event(net, &x, 0.0, &mut rng);
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(n_points);
    for &target in &times {
        while let Some((event_time, j)) = next {
            if event_time > target {
                break;
            }
            fire(net, &mut x, j);
            next = schedule_event(net, &x, event_time, &mut rng);
        }
        rows.push(x.clone());
    }
    (times, rows)
}

/// `n_runs` independent Gillespie trajectories (seeds `base_seed + 0..n_runs`),
/// reduced to the per-species, per-grid-point ENSEMBLE mean and (population)
/// standard deviation — the distributional summary a `claims: Distributional`
/// comparison works on. A single path is noise; the ensemble mean is the signal
/// (it tracks the deterministic ODE), the std is the spread. Returns
/// `(times, mean[species][k], std[species][k])`.
fn ssa_ensemble(
    net: &MassActionNetwork,
    x0: &[f64],
    runtime: f64,
    dt: f64,
    base_seed: u64,
    n_runs: usize,
) -> (Vec<f64>, Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let n_species = x0.len();
    let runs = n_runs.max(1);
    let mut times: Vec<f64> = Vec::new();
    let mut sum: Vec<Vec<f64>> = Vec::new();
    let mut sumsq: Vec<Vec<f64>> = Vec::new();
    for run in 0..runs {
        let (t, rows) = ssa_trajectory(net, x0, runtime, dt, base_seed + run as u64);
        if run == 0 {
            times = t;
            sum = vec![vec![0.0; times.len()]; n_species];
            sumsq = vec![vec![0.0; times.len()]; n_species];
        }
        for (k, row) in rows.iter().enumerate() {
            for i in 0..n_species {
                sum[i][k] += row[i];
                sumsq[i][k] += row[i] * row[i];
            }
        }
    }
    let n = runs as f64;
    let mut mean = vec![vec![0.0; times.len()]; n_species];
    let mut std = vec![vec![0.0; times.len()]; n_species];
    for i in 0..n_species {
        for k in 0..times.len() {
            let m = sum[i][k] / n;
            mean[i][k] = m;
            std[i][k] = ((sumsq[i][k] / n) - m * m).max(0.0).sqrt();
        }
    }
    (times, mean, std)
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

/// A native stochastic-stepper object for `from stochastic import ssa`: the
/// value `{_type: "Stochastic", method}` whose `simulate(network, state,
/// runtime, timestep, seed)` runs one Gillespie trajectory and returns a
/// `TimeSeries`. The stochastic sibling of [`integrator`] — same network, the
/// EXACT-CME target rather than the deterministic limit. A `.ys` `step` wraps
/// it, declaring the ports + `fulfills ExactCME` contract.
pub fn stochastic(method: &str) -> Value {
    Value::tree([
        ("_type", Value::from("Stochastic")),
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
    // `ssa.simulate(network, state, runtime, timestep, seed)` — one whole
    // Gillespie trajectory of `network` from `state`, sampled on the timestep
    // grid, returned as a `TimeSeries` (the same shape `RunProcess` emits, so
    // `Compare`/`overlay` work on it unchanged). The stochastic counterpart of
    // `Integrator::integrate`; one seeded path = a sample of the exact CME.
    reg.register("Stochastic", "simulate", |recv, args| {
        let method = recv
            .get_field("method")
            .and_then(|v| v.as_str())
            .unwrap_or("ssa")
            .to_string();
        let network = args
            .first()
            .map(network_from_value)
            .ok_or_else(|| ts_err("simulate", "argument 0 must be a reaction network"))?;
        let state = args.get(1).and_then(|v| v.as_map());
        let runtime = args.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0);
        let timestep = args.get(3).and_then(|v| v.as_f64()).unwrap_or(0.1);
        let seed = args.get(4).and_then(|v| v.as_f64()).unwrap_or(0.0) as u64;
        let x0: Vec<f64> = network
            .species
            .iter()
            .map(|s| {
                state
                    .and_then(|m| m.get(s.as_str()))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            })
            .collect();
        let (times, rows) = ssa_trajectory(&network, &x0, runtime, timestep, seed);
        // Transpose the per-grid-point count rows into columns `{species: [..]}`.
        let columns = Value::tree(network.species.iter().enumerate().map(|(i, sp)| {
            let col: Vec<Value> = rows.iter().map(|row| Value::float(row[i])).collect();
            (sp.clone(), Value::List(col))
        }));
        Ok(Value::tree([
            ("_type", Value::from("TimeSeries")),
            ("name", Value::from(method.as_str())),
            ("times", Value::List(times.into_iter().map(Value::float).collect())),
            ("columns", columns),
        ]))
    });
    // `ssa.ensemble(network, state, runtime, timestep, base_seed, n_runs)` — the
    // distributional summary of `n_runs` Gillespie paths: per-species mean ± std
    // bands. A single path is a sample; the ENSEMBLE is what a `Distributional`
    // claim is about. Returns `{_type: "Ensemble", n_runs, times, mean, std}`.
    reg.register("Stochastic", "ensemble", |_recv, args| {
        let network = args
            .first()
            .map(network_from_value)
            .ok_or_else(|| ts_err("ensemble", "argument 0 must be a reaction network"))?;
        let state = args.get(1).and_then(|v| v.as_map());
        let runtime = args.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0);
        let timestep = args.get(3).and_then(|v| v.as_f64()).unwrap_or(0.1);
        let base_seed = args.get(4).and_then(|v| v.as_f64()).unwrap_or(0.0) as u64;
        let n_runs = args.get(5).and_then(|v| v.as_f64()).unwrap_or(64.0) as usize;
        let x0: Vec<f64> = network
            .species
            .iter()
            .map(|s| {
                state
                    .and_then(|m| m.get(s.as_str()))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            })
            .collect();
        let (times, mean, std) = ssa_ensemble(&network, &x0, runtime, timestep, base_seed, n_runs);
        let band = |data: &[Vec<f64>]| {
            Value::tree(network.species.iter().enumerate().map(|(i, sp)| {
                (sp.clone(), Value::List(data[i].iter().map(|v| Value::float(*v)).collect()))
            }))
        };
        Ok(Value::tree([
            ("_type", Value::from("Ensemble")),
            ("n_runs", Value::float(n_runs.max(1) as f64)),
            ("times", Value::List(times.into_iter().map(Value::float).collect())),
            ("mean", band(&mean)),
            ("std", band(&std)),
        ]))
    });
    // `a.distributional_distance(b)` — per species, the MAX over time of the
    // standardized mean difference |mean_a − mean_b| / √(SEₐ² + SE_b²), SE =
    // std/√n. For two ensembles of the SAME process this is O(1) — they agree
    // DISTRIBUTIONALLY — whereas the trajectory MSE of two single paths is large
    // (they do NOT agree pathwise). The `claims` axis says which metric is the
    // meaningful one; this is the `species_mse` of the stochastic lane.
    reg.register("Ensemble", "distributional_distance", |recv, args| {
        let other = args
            .first()
            .ok_or_else(|| ts_err("distributional_distance", "argument 0 must be an Ensemble"))?;
        let n_a = recv.get_field("n_runs").and_then(|v| v.as_f64()).unwrap_or(1.0).max(1.0);
        let n_b = other.get_field("n_runs").and_then(|v| v.as_f64()).unwrap_or(1.0).max(1.0);
        let (mean_a, std_a, mean_b, std_b) = match (
            recv.get_field("mean").and_then(|v| v.as_map()),
            recv.get_field("std").and_then(|v| v.as_map()),
            other.get_field("mean").and_then(|v| v.as_map()),
            other.get_field("std").and_then(|v| v.as_map()),
        ) {
            (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
            _ => {
                return Err(ts_err(
                    "distributional_distance",
                    "receiver and argument 0 must both be Ensembles",
                ))
            }
        };
        let dist = mean_a.iter().filter_map(|(sp, ma)| {
            let ma = ts_floats(ma);
            let mb = ts_floats(mean_b.get(sp.as_str())?);
            let sa = ts_floats(std_a.get(sp.as_str())?);
            let sb = ts_floats(std_b.get(sp.as_str())?);
            let n = ma.len().min(mb.len()).min(sa.len()).min(sb.len());
            let mut max_z = 0.0_f64;
            for i in 0..n {
                let se = (sa[i] * sa[i] / n_a + sb[i] * sb[i] / n_b).sqrt();
                let diff = (ma[i] - mb[i]).abs();
                let z = if se > 1e-9 {
                    diff / se
                } else if diff < 1e-9 {
                    0.0
                } else {
                    f64::INFINITY
                };
                max_z = max_z.max(z);
            }
            Some((sp.to_string(), Value::float(max_z)))
        });
        Ok(Value::tree(dist))
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
        // `time_series_chart` returns a place-graph SVG VALUE (not a string).
        // The Figure carries it under `root`, matching the plot-as-data shape
        // every other Figure consumer uses (`Figure.svg(path)` serializes via
        // `to_svg`).
        let root = prism_viz::plot::time_series_chart(&times, &series, title, false);
        Ok(Value::tree([
            ("_type", Value::from("Figure")),
            ("root", root),
        ]))
    });

    // `trace.plot(title)` — the schema-driven plot (decision #25): a `Trace`
    // (from `RunProcess`) is a single-item state extended through time; infer
    // that single-item schema from the frames and let `prism_viz::plot` dispatch
    // to the characteristic view (scalars → line plot; field → heatmap; …).
    // Returns a `Figure`. This is the type-general plot; `overlay` is the
    // two-series special case.
    reg.register("Trace", "plot", |recv, args| {
        // A `Trace` is a delta-log (`initial` + `deltas`); replay it to frames via
        // `prism_trace::frames` (capture→replay→render). Works for both `Simulate`
        // and `RunProcess` traces — they share the one delta-log shape.
        let frames = prism_trace::frames(recv);
        let title = args.first().and_then(|v| v.as_str()).unwrap_or("trace");
        // The element schema is carried WITH the trace (its type parameter `T`,
        // set by the producer from the inner's output type). We KNOW it — read
        // it; don't re-infer from the data.
        let schema = recv
            .get_field("element")
            .and_then(prism_schema::value_to_schema)
            .unwrap_or(prism_schema::Schema::Any);
        // `plot` returns the viz AS DATA — an SVG place-graph value. The Figure
        // carries it under `root` (so the plot is itself inspectable state);
        // `figure.svg(path)` serializes it with `to_svg`.
        let root = prism_viz::plot(&schema, &frames, title);
        Ok(Value::tree([("_type", Value::from("Figure")), ("root", root)]))
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
        // A Figure carries its place-graph SVG under `root` — `to_svg` is the
        // single serializer. (The legacy `svg: String` field is gone; every
        // Figure producer now emits a place-graph value.)
        let svg_text = recv
            .get_field("root")
            .map(prism_viz::svg::to_svg)
            .unwrap_or_default();
        write_file(&format!("{path}.svg"), &svg_text, "svg")
    });

    // `state.dot()` — render any bigraph state as Graphviz DOT *source* (a String,
    // viz-as-data like `plot`/`csv`). Registered on the STRUCTURAL `Map` row, so it
    // fires for every map state through the dispatch fallback — branded or not. This
    // wires the EXISTING `prism_viz::render_state_dot` into the open method matrix;
    // it had been a bare function, a capability outside our own method dispatch.
    reg.register("Map", "dot", |recv, _args| {
        Ok(Value::String(prism_viz::render_state_dot(
            recv,
            &prism_viz::DotOptions::default(),
        )))
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
    fn ssa_is_reproducible_and_conserves_mass() {
        let net = a_to_b(0.7);
        let (t, r1) = ssa_trajectory(&net, &[100.0, 0.0], 5.0, 0.1, 42);
        let (_, r2) = ssa_trajectory(&net, &[100.0, 0.0], 5.0, 0.1, 42);
        assert_eq!(r1, r2, "same seed ⇒ identical trajectory (reproducible)");
        assert_eq!(t.first(), Some(&0.0));
        assert_eq!(r1.first().unwrap(), &vec![100.0, 0.0], "starts at the init counts");
        for row in &r1 {
            assert!((row[0] + row[1] - 100.0).abs() < 1e-9, "A+B conserved");
            assert!(row[0] >= 0.0 && row[1] >= 0.0, "counts stay non-negative");
        }
        // A→B only: A is non-increasing along the path.
        assert!(r1.windows(2).all(|w| w[1][0] <= w[0][0]), "A only decreases");
    }

    #[test]
    fn ssa_differs_by_seed_but_ensemble_tracks_the_ode() {
        let net = a_to_b(0.7);
        let (_, a) = ssa_trajectory(&net, &[100.0, 0.0], 5.0, 0.1, 1);
        let (_, b) = ssa_trajectory(&net, &[100.0, 0.0], 5.0, 0.1, 2);
        assert_ne!(a, b, "different seeds ⇒ different sample paths (genuinely stochastic)");
        // The ensemble mean of A(5) should track the deterministic limit
        // A₀·e^{−k·t} = 100·e^{−3.5} ≈ 3.02 (a single path does not).
        let final_a = |seed| ssa_trajectory(&net, &[100.0, 0.0], 5.0, 0.1, seed).1.last().unwrap()[0];
        let mean: f64 = (1..=64u64).map(final_a).sum::<f64>() / 64.0;
        let ode = 100.0 * (-0.7 * 5.0_f64).exp();
        assert!((mean - ode).abs() < 5.0, "SSA ensemble mean {mean} should track ODE {ode}");
    }

    #[test]
    fn distributional_metric_agrees_where_pathwise_does_not() {
        // The revelation, quantified: the `claims` axis decides which notion of
        // "agree" is meaningful. For the CME target, two single SSA PATHS diverge
        // (pathwise comparison is wrong), yet two ENSEMBLES agree (distributional
        // comparison is right). Same data, opposite verdicts — the contract picks.
        let net = a_to_b(0.7);
        let x0 = [100.0, 0.0];

        // Two single paths: their per-step A values diverge — a large MSE.
        let (_, p1) = ssa_trajectory(&net, &x0, 5.0, 0.1, 1);
        let (_, p2) = ssa_trajectory(&net, &x0, 5.0, 0.1, 2);
        let path_mse: f64 =
            p1.iter().zip(&p2).map(|(a, b)| (a[0] - b[0]).powi(2)).sum::<f64>() / p1.len() as f64;
        assert!(path_mse > 2.0, "two SSA paths must diverge pathwise (MSE {path_mse})");

        // Two ensembles of the SAME process (disjoint seed ranges): their A means
        // agree within sampling error — a small standardized distance.
        let n = 64.0;
        let (_, ma, sa) = ssa_ensemble(&net, &x0, 5.0, 0.1, 1, 64);
        let (_, mb, sb) = ssa_ensemble(&net, &x0, 5.0, 0.1, 1000, 64);
        let max_z = (0..ma[0].len())
            .map(|k| {
                let se = (sa[0][k] * sa[0][k] / n + sb[0][k] * sb[0][k] / n).sqrt();
                if se > 1e-9 {
                    (ma[0][k] - mb[0][k]).abs() / se
                } else {
                    0.0
                }
            })
            .fold(0.0_f64, f64::max);
        assert!(max_z < 5.0, "two SSA ensembles must agree distributionally (max z {max_z})");
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

        // overlay → a Figure VALUE carrying its SVG under `root` as a place-
        // graph value (every Figure producer emits the same shape).
        let fig = reg
            .dispatch(&x, "overlay", std::slice::from_ref(&y))
            .expect("overlay");
        assert_eq!(fig.get_field("_type").and_then(|v| v.as_str()), Some("Figure"));
        let root = fig.get_field("root").expect("root");
        let svg = prism_viz::svg::to_svg(root);
        assert!(svg.starts_with("<svg"), "overlay should be an SVG document");
    }

    #[test]
    fn dot_method_renders_any_state_as_graphviz_with_brand_fallback() {
        // The viz `render_state_dot` capability, now a COLUMN in the method matrix.
        let mut reg = MethodRegistry::new();
        register_methods(&mut reg);

        // An unbranded bigraph state (a Map) dispatches ("Map","dot") exactly.
        let state = Value::tree([("cell", Value::tree([("mass", Value::float(2.0))]))]);
        let dot = reg.dispatch(&state, "dot", &[]).expect("dot");
        assert!(
            dot.as_str().unwrap_or_default().contains("digraph"),
            "dot returns Graphviz source"
        );

        // A BRANDED state ({_type: Cell}) has brand "Cell" — exact dispatch would
        // miss, but the STRUCTURAL fallback to ("Map","dot") still renders it.
        let branded =
            Value::tree([("_type", Value::from("Cell")), ("mass", Value::float(1.0))]);
        let dot2 = reg
            .dispatch(&branded, "dot", &[])
            .expect("dot on a branded state via the structural fallback");
        assert!(dot2.as_str().unwrap_or_default().contains("digraph"));
    }
}
