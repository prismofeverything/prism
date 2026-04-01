//! Dynamic Flux Balance Analysis (dFBA) process.
//!
//! Each timestep:
//! 1. Compute Michaelis-Menten uptake bounds for each substrate
//! 2. Solve the FBA LP with those bounds
//! 3. Apply exchange fluxes to substrates and biomass growth
//!
//! Uses real COBRA model files parsed by fba.rs and solved
//! with good_lp/minilp.

use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use prism_bigraph::{Process, Schema, Update, Value};

use super::fba::{load_model_cached, CobraModel, FbaSolver};

/// Kinetic parameters for a substrate: (km, vmax).
#[derive(Clone, Debug)]
pub struct KineticParam {
    pub km: f64,
    pub vmax: f64,
}

/// Dynamic FBA process backed by a real COBRA model + LP solver.
///
/// Maintains a persistent HiGHS solver instance that preserves the LP
/// structure and warm-start basis across ticks. Only column bounds are
/// updated each solve.
pub struct DynamicFBA {
    /// The COBRA model (stoichiometry matrix, bounds, objective).
    /// Shared via Arc so particles cloned from division share the
    /// parsed model data without re-reading from disk.
    pub model: Arc<CobraModel>,
    /// Michaelis-Menten parameters per substrate.
    pub kinetic_params: IndexMap<String, KineticParam>,
    /// Maps substrate name → exchange reaction ID in the model.
    pub substrate_reactions: IndexMap<String, String>,
    /// Additional fixed bounds from config (applied on top of model defaults).
    pub config_bounds: IndexMap<String, (Option<f64>, Option<f64>)>,
    /// Process interval.
    pub interval: f64,
    /// Persistent LP solver (lazy-initialized on first update).
    solver: Mutex<Option<FbaSolver>>,
}

impl Clone for DynamicFBA {
    fn clone(&self) -> Self {
        DynamicFBA {
            model: Arc::clone(&self.model),
            kinetic_params: self.kinetic_params.clone(),
            substrate_reactions: self.substrate_reactions.clone(),
            config_bounds: self.config_bounds.clone(),
            interval: self.interval,
            solver: Mutex::new(None), // new instance gets its own solver
        }
    }
}

impl fmt::Debug for DynamicFBA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicFBA")
            .field("model", &self.model.id)
            .field("interval", &self.interval)
            .finish()
    }
}

impl DynamicFBA {
    /// Compute Monod uptake bound for a substrate.
    /// Returns a negative value (uptake direction).
    fn uptake_bound(&self, substrate: &str, concentration: f64) -> f64 {
        if let Some(kp) = self.kinetic_params.get(substrate) {
            if concentration > 1e-12 {
                -kp.vmax * concentration / (kp.km + concentration)
            } else {
                0.0
            }
        } else {
            0.0 // No kinetic params → no uptake limit
        }
    }
}

impl Process for DynamicFBA {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("substrates".into(), Schema::map(Schema::float())),
            ("biomass".into(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("substrates".into(), Schema::map(Schema::float())),
            ("biomass".into(), Schema::float()),
        ])
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let map = match state.as_map() {
            Some(m) => m,
            None => return Update::Noop,
        };

        let biomass = map
            .get("biomass")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if biomass <= 1e-12 {
            return Update::Noop;
        }

        let substrates = match map.get("substrates").and_then(|v| v.as_map()) {
            Some(s) => s,
            None => return Update::Noop,
        };

        // Build bounds: start with model defaults, apply config overrides,
        // then apply Monod kinetic limits for substrates
        let mut lb = self.model.lower_bounds.clone();
        let mut ub = self.model.upper_bounds.clone();

        // Apply config bounds (O2 limitation, ATPM, etc.)
        for (rxn_id, (lo, hi)) in &self.config_bounds {
            if let Some(idx) = self.model.reaction_index(rxn_id) {
                if let Some(l) = lo {
                    lb[idx] = *l;
                }
                if let Some(h) = hi {
                    ub[idx] = *h;
                }
            }
        }

        // Apply Monod kinetic uptake bounds for each substrate.
        // For each substrate with kinetic params, the exchange reaction's
        // lower bound is set to the Monod uptake rate (negative = uptake).
        // This overrides the model's default lb, enabling uptake of
        // substrates that the model might only allow secretion for.
        for (substrate, rxn_id) in &self.substrate_reactions {
            if let Some(idx) = self.model.reaction_index(rxn_id) {
                let conc = substrates
                    .get(substrate)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let bound = self.uptake_bound(substrate, conc);
                // Set lower bound to kinetic limit (allows uptake up to Monod rate)
                lb[idx] = bound;
                // Ensure lb <= ub (some models have negative ub for forced uptake)
                if lb[idx] > ub[idx] {
                    ub[idx] = lb[idx];
                }
            }
        }

        // Solve FBA using persistent solver (warm-starts from previous basis)
        let mut solver_guard = self.solver.lock().unwrap();
        let solver = solver_guard.get_or_insert_with(|| FbaSolver::new(&self.model));
        let solution = match solver.solve(&lb, &ub) {
            Some(sol) => sol,
            None => return Update::Noop, // Infeasible
        };
        drop(solver_guard);

        let dt = interval; // time units per step

        // Growth rate (1/h) from the objective
        let mu = solution.objective_value;

        // Output DELTAS for substrates and biomass.
        // Engine uses additive apply for numeric types.
        let mut result: IndexMap<String, Value> = IndexMap::new();
        for (substrate, rxn_id) in &self.substrate_reactions {
            if let Some(idx) = self.model.reaction_index(rxn_id) {
                let flux = solution.fluxes[idx]; // mmol/gDW/h (negative=uptake)
                let delta = flux * biomass * dt;
                result.insert(substrate.clone(), Value::float(delta));
            }
        }
        // Non-managed substrates get zero delta
        for (mol_id, _) in substrates {
            result.entry(mol_id.clone()).or_insert(Value::float(0.0));
        }

        let biomass_delta = mu * biomass * dt;

        Update::value(Value::tree([
            ("substrates", Value::Map(result)),
            ("biomass", Value::float(biomass_delta)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Construct a DynamicFBA from its vivarium config.
pub fn dfba_from_config(config: &Value) -> DynamicFBA {
    let map = config.as_map().cloned().unwrap_or_default();

    // Load the COBRA model (cached — shared across particles)
    let model_file = map
        .get("model_file")
        .and_then(|v| v.as_str())
        .unwrap_or("textbook");

    let model = match load_model_cached(model_file) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Warning: failed to load model '{model_file}': {e}, using fallback");
            Arc::new(make_fallback_model())
        }
    };

    // Parse kinetic params
    let mut kinetic_params = IndexMap::new();
    if let Some(kp) = map.get("kinetic_params").and_then(|v| v.as_map()) {
        for (substrate, params) in kp {
            if let Some(list) = params.as_list() {
                let km = list.first().and_then(|v| v.as_f64()).unwrap_or(0.5);
                let vmax = list.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0);
                kinetic_params.insert(substrate.clone(), KineticParam { km, vmax });
            }
        }
    }

    // Parse substrate → reaction mapping
    let mut substrate_reactions = IndexMap::new();
    if let Some(sr) = map.get("substrate_update_reactions").and_then(|v| v.as_map()) {
        for (substrate, rxn_id) in sr {
            if let Some(rxn) = rxn_id.as_str() {
                substrate_reactions.insert(substrate.clone(), rxn.to_string());
            }
        }
    }

    // Parse additional bounds
    let mut config_bounds = IndexMap::new();
    if let Some(b) = map.get("bounds").and_then(|v| v.as_map()) {
        for (rxn_id, bound_val) in b {
            if let Some(bmap) = bound_val.as_map() {
                let lower = bmap.get("lower").and_then(|v| v.as_f64());
                let upper = bmap.get("upper").and_then(|v| v.as_f64());
                config_bounds.insert(rxn_id.clone(), (lower, upper));
            }
        }
    }

    let interval = map
        .get("interval")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);

    DynamicFBA {
        model,
        kinetic_params,
        substrate_reactions,
        config_bounds,
        interval,
        solver: Mutex::new(None),
    }
}

/// Minimal fallback model for when the real model file is unavailable.
fn make_fallback_model() -> CobraModel {
    CobraModel {
        id: "fallback".into(),
        metabolites: vec![],
        reactions: vec!["biomass".into()],
        stoichiometry: vec![vec![]],
        lower_bounds: vec![0.0],
        upper_bounds: vec![1000.0],
        objective_idx: 0,
        met_index: Default::default(),
        rxn_index: [("biomass".into(), 0)].into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dfba_real_fba() {
        // Build from the same config as the ecoli_core_dfba fixture
        let config = Value::tree([
            ("model_file", Value::from("textbook")),
            (
                "kinetic_params",
                Value::tree([
                    ("glucose", Value::List(vec![Value::float(0.5), Value::float(1.0)])),
                    ("acetate", Value::List(vec![Value::float(0.5), Value::float(2.0)])),
                ]),
            ),
            (
                "substrate_update_reactions",
                Value::tree([
                    ("glucose", Value::from("EX_glc__D_e")),
                    ("acetate", Value::from("EX_ac_e")),
                ]),
            ),
            (
                "bounds",
                Value::tree([
                    ("EX_o2_e", Value::tree([("lower", Value::float(-2.0))])),
                    ("ATPM", Value::tree([
                        ("lower", Value::float(1.0)),
                        ("upper", Value::float(1.0)),
                    ])),
                ]),
            ),
        ]);

        let proc = dfba_from_config(&config);
        assert_eq!(proc.model.metabolites.len(), 72);
        assert_eq!(proc.model.reactions.len(), 95);

        let mut glucose = 10.0;
        let mut acetate = 0.0;
        let mut biomass = 0.1;
        let mut peak_acetate = 0.0_f64;

        for t in 0..60 {
            let state = Value::tree([
                ("substrates", Value::tree([
                    ("glucose", Value::float(glucose)),
                    ("acetate", Value::float(acetate)),
                ])),
                ("biomass", Value::float(biomass)),
            ]);

            if let Some(result) = proc.update(&state, 1.0).into_value() {
                // Results are deltas — accumulate
                glucose += result
                    .get_path(&["substrates".into(), "glucose".into()])
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                acetate += result
                    .get_path(&["substrates".into(), "acetate".into()])
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                biomass += result
                    .get_path(&["biomass".into()])
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                glucose = glucose.max(0.0);
                acetate = acetate.max(0.0);
            }

            peak_acetate = peak_acetate.max(acetate);

            if t % 10 == 0 {
                println!(
                    "t={t}: glucose={glucose:.3}, acetate={acetate:.3}, biomass={biomass:.4}"
                );
            }
        }

        println!("Final: glucose={glucose:.3}, acetate={acetate:.3}, biomass={biomass:.4}");
        println!("Peak acetate: {peak_acetate:.3}");

        assert!(glucose < 0.1, "glucose should deplete: {glucose}");
        assert!(biomass > 0.3, "biomass should grow: {biomass}");
        assert!(peak_acetate > 0.5, "acetate should accumulate: {peak_acetate}");
        assert!(
            acetate < peak_acetate * 0.5,
            "acetate should decline (diauxic): current={acetate}, peak={peak_acetate}"
        );
    }
}
