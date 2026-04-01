//! Real Flux Balance Analysis using LP solving.
//!
//! Parses COBRA JSON model files, builds the stoichiometry matrix,
//! and solves the LP each timestep with good_lp/minilp.
//!
//! maximize c^T · v  (biomass reaction flux)
//! subject to S · v = 0  (mass balance)
//!            lb ≤ v ≤ ub  (flux bounds)

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use highs::{HighsModelStatus, RowProblem, Sense};
use indexmap::IndexMap;
use serde::Deserialize;

/// Global cache of parsed COBRA models, keyed by resolved file path.
/// Avoids re-parsing the same model JSON when multiple dFBA processes
/// (e.g., one per particle) use the same model.
static MODEL_CACHE: LazyLock<Mutex<HashMap<String, Arc<CobraModel>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A parsed COBRA model ready for FBA.
#[derive(Clone, Debug)]
pub struct CobraModel {
    pub id: String,
    /// Metabolite IDs (row indices of S matrix).
    pub metabolites: Vec<String>,
    /// Reaction IDs (column indices of S matrix).
    pub reactions: Vec<String>,
    /// Stoichiometry: reaction_index -> [(metabolite_index, coefficient)]
    pub stoichiometry: Vec<Vec<(usize, f64)>>,
    /// Lower bounds per reaction.
    pub lower_bounds: Vec<f64>,
    /// Upper bounds per reaction.
    pub upper_bounds: Vec<f64>,
    /// Index of the biomass (objective) reaction.
    pub objective_idx: usize,
    /// Metabolite index lookup.
    pub met_index: HashMap<String, usize>,
    /// Reaction index lookup.
    pub rxn_index: HashMap<String, usize>,
}

/// Raw COBRA JSON structures for deserialization.
#[derive(Deserialize)]
struct CobraJson {
    metabolites: Vec<CobraMetabolite>,
    reactions: Vec<CobraReaction>,
    id: Option<String>,
}

#[derive(Deserialize)]
struct CobraMetabolite {
    id: String,
}

#[derive(Deserialize)]
struct CobraReaction {
    id: String,
    #[serde(default)]
    name: String,
    metabolites: HashMap<String, f64>,
    lower_bound: f64,
    upper_bound: f64,
}

impl CobraModel {
    /// Load a COBRA model from a JSON file.
    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let json = std::fs::read_to_string(path.as_ref())
            .map_err(|e| format!("read error: {e}"))?;
        Self::from_json_str(&json)
    }

    /// Parse a COBRA model from a JSON string.
    pub fn from_json_str(json: &str) -> Result<Self, String> {
        let raw: CobraJson =
            serde_json::from_str(json).map_err(|e| format!("parse error: {e}"))?;

        let metabolites: Vec<String> = raw.metabolites.iter().map(|m| m.id.clone()).collect();
        let reactions: Vec<String> = raw.reactions.iter().map(|r| r.id.clone()).collect();

        let met_index: HashMap<String, usize> = metabolites
            .iter()
            .enumerate()
            .map(|(i, m)| (m.clone(), i))
            .collect();
        let rxn_index: HashMap<String, usize> = reactions
            .iter()
            .enumerate()
            .map(|(i, r)| (r.clone(), i))
            .collect();

        let mut stoichiometry: Vec<Vec<(usize, f64)>> = vec![Vec::new(); reactions.len()];
        let mut lower_bounds = Vec::with_capacity(reactions.len());
        let mut upper_bounds = Vec::with_capacity(reactions.len());

        for (j, rxn) in raw.reactions.iter().enumerate() {
            lower_bounds.push(rxn.lower_bound);
            upper_bounds.push(rxn.upper_bound);
            for (met_id, &coeff) in &rxn.metabolites {
                if let Some(&i) = met_index.get(met_id) {
                    stoichiometry[j].push((i, coeff));
                }
            }
        }

        // Find biomass reaction (contains "biomass" or "Biomass" in id or name)
        let objective_idx = raw
            .reactions
            .iter()
            .position(|r| {
                r.id.to_lowercase().contains("biomass")
                    || r.name.to_lowercase().contains("biomass")
            })
            .ok_or_else(|| "no biomass reaction found".to_string())?;

        Ok(Self {
            id: raw.id.unwrap_or_default(),
            metabolites,
            reactions,
            stoichiometry,
            lower_bounds,
            upper_bounds,
            objective_idx,
            met_index,
            rxn_index,
        })
    }

    /// Get the index of a reaction by ID.
    pub fn reaction_index(&self, rxn_id: &str) -> Option<usize> {
        self.rxn_index.get(rxn_id).copied()
    }

    /// Solve FBA with the current bounds.
    /// Returns (objective_value, flux_vector) or None if infeasible.
    pub fn solve(&self) -> Option<FbaSolution> {
        self.solve_with_bounds(&self.lower_bounds, &self.upper_bounds)
    }

    /// Solve FBA with modified bounds.
    /// Uses a one-shot HiGHS solve (rebuilds the LP each time).
    pub fn solve_with_bounds(&self, lb: &[f64], ub: &[f64]) -> Option<FbaSolution> {
        let mut solver = FbaSolver::new(self);
        solver.solve(lb, ub)
    }

    /// Set bounds for a specific reaction by ID.
    pub fn set_bounds(&mut self, rxn_id: &str, lower: Option<f64>, upper: Option<f64>) {
        if let Some(&idx) = self.rxn_index.get(rxn_id) {
            if let Some(lb) = lower {
                self.lower_bounds[idx] = lb;
            }
            if let Some(ub) = upper {
                self.upper_bounds[idx] = ub;
            }
        }
    }
}

/// Result of an FBA solve.
#[derive(Clone, Debug)]
pub struct FbaSolution {
    pub fluxes: Vec<f64>,
    pub objective_value: f64,
}

/// Model registry: maps model names to file paths.
pub fn model_path(model_file: &str) -> Option<String> {
    let models_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/models");
    match model_file {
        "textbook" | "e_coli_core" | "ecoli core" => Some(format!("{models_dir}/e_coli_core.json")),
        "iAF1260.xml" | "iAF1260" => Some(format!("{models_dir}/iAF1260.json")),
        "iMM904.xml" | "iMM904" => Some(format!("{models_dir}/iMM904.json")),
        "iCN900.xml" | "iCN900" => Some(format!("{models_dir}/iCN900.json")),
        "iJN746.xml" | "iJN746" => Some(format!("{models_dir}/iJN746.json")),
        "iNF517.xml" | "iNF517" => Some(format!("{models_dir}/iNF517.json")),
        _ => {
            // Try as-is
            let p = format!("{models_dir}/{model_file}");
            if std::path::Path::new(&p).exists() {
                Some(p)
            } else {
                None
            }
        }
    }
}

/// Load a COBRA model with caching. Returns a shared reference to the
/// parsed model. Multiple dFBA processes using the same model file
/// (e.g., one per particle after division) share the same parsed data.
pub fn load_model_cached(model_file: &str) -> Result<Arc<CobraModel>, String> {
    let path = model_path(model_file)
        .ok_or_else(|| format!("model file '{model_file}' not found"))?;

    let mut cache = MODEL_CACHE.lock().unwrap();
    if let Some(model) = cache.get(&path) {
        return Ok(Arc::clone(model));
    }

    let model = Arc::new(CobraModel::from_json_file(&path)?);
    cache.insert(path, Arc::clone(&model));
    Ok(model)
}

/// Persistent FBA solver for a COBRA model.
///
/// Caches the row-wise stoichiometry structure so it doesn't need
/// to be recomputed each solve. Rebuilds the HiGHS problem with
/// updated bounds each call (the safe highs crate doesn't expose
/// bound-update APIs, but avoiding the stoichiometry transpose
/// and allocation is still a win).
pub struct FbaSolver {
    /// Pre-computed row terms: for each active metabolite,
    /// list of (reaction_index, coefficient).
    row_terms: Vec<Vec<(usize, f64)>>,
    n_rxns: usize,
    objective_idx: usize,
}

// FbaSolver has no FFI handles — fully safe.
unsafe impl Send for FbaSolver {}

impl FbaSolver {
    /// Build solver from a CobraModel (caches stoichiometry structure).
    pub fn new(model: &CobraModel) -> Self {
        let n_mets = model.metabolites.len();
        let mut met_terms: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n_mets];
        for (j, col) in model.stoichiometry.iter().enumerate() {
            for &(i, coeff) in col {
                met_terms[i].push((j, coeff));
            }
        }
        // Keep only non-empty rows
        let row_terms: Vec<Vec<(usize, f64)>> = met_terms
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();

        FbaSolver {
            row_terms,
            n_rxns: model.reactions.len(),
            objective_idx: model.objective_idx,
        }
    }

    /// Solve FBA with the given bounds.
    pub fn solve(&self, lb: &[f64], ub: &[f64]) -> Option<FbaSolution> {
        let mut pb = RowProblem::default();

        // Add columns with current bounds
        let cols: Vec<highs::Col> = (0..self.n_rxns)
            .map(|j| {
                let cost = if j == self.objective_idx { -1.0 } else { 0.0 };
                pb.add_column(cost, lb[j]..ub[j])
            })
            .collect();

        // Add cached rows (stoichiometry doesn't change)
        for terms in &self.row_terms {
            let row: Vec<(highs::Col, f64)> =
                terms.iter().map(|&(j, c)| (cols[j], c)).collect();
            pb.add_row(0.0..=0.0, row);
        }

        let mut model = pb.optimise(Sense::Minimise);
        model.set_option("output_flag", false);
        let solved = model.solve();

        match solved.status() {
            HighsModelStatus::Optimal => {
                let solution = solved.get_solution();
                let fluxes = solution.columns().to_vec();
                let objective_value = fluxes[self.objective_idx];
                Some(FbaSolution {
                    fluxes,
                    objective_value,
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_ecoli_core() {
        let path = model_path("textbook").unwrap();
        let model = CobraModel::from_json_file(&path).unwrap();

        assert_eq!(model.metabolites.len(), 72);
        assert_eq!(model.reactions.len(), 95);
        assert!(model.reaction_index("EX_glc__D_e").is_some());
        assert!(model.reaction_index("EX_ac_e").is_some());
        assert!(model.reaction_index("EX_o2_e").is_some());

        println!(
            "Biomass reaction: {} (index {})",
            model.reactions[model.objective_idx], model.objective_idx
        );
    }

    #[test]
    fn test_fba_solve_ecoli_core() {
        let path = model_path("textbook").unwrap();
        let mut model = CobraModel::from_json_file(&path).unwrap();

        // Default solve (unlimited glucose)
        let sol = model.solve().expect("FBA should be feasible");
        println!("Growth rate: {:.4}", sol.objective_value);
        assert!(sol.objective_value > 0.8, "growth rate should be ~0.87: {}", sol.objective_value);

        // Check glucose uptake flux
        let glc_idx = model.reaction_index("EX_glc__D_e").unwrap();
        let ac_idx = model.reaction_index("EX_ac_e").unwrap();
        let o2_idx = model.reaction_index("EX_o2_e").unwrap();
        println!("Glucose flux: {:.4}", sol.fluxes[glc_idx]);
        println!("Acetate flux: {:.4}", sol.fluxes[ac_idx]);
        println!("O2 flux: {:.4}", sol.fluxes[o2_idx]);

        // With O2 limitation (like spatio-flux config)
        model.set_bounds("EX_o2_e", Some(-2.0), None);
        model.set_bounds("ATPM", Some(1.0), Some(1.0));

        let sol2 = model.solve().expect("FBA should be feasible with O2 limit");
        println!("\nWith O2=-2.0, ATPM=1.0:");
        println!("Growth rate: {:.4}", sol2.objective_value);
        println!("Glucose flux: {:.4}", sol2.fluxes[glc_idx]);
        println!("Acetate flux: {:.4}", sol2.fluxes[ac_idx]);
        println!("O2 flux: {:.4}", sol2.fluxes[o2_idx]);

        // Acetate should be secreted (positive flux)
        assert!(
            sol2.fluxes[ac_idx] > 0.0,
            "acetate should be secreted under O2 limitation: {}",
            sol2.fluxes[ac_idx]
        );
    }

    #[test]
    fn test_fba_with_monod_bounds() {
        let path = model_path("textbook").unwrap();
        let mut model = CobraModel::from_json_file(&path).unwrap();

        // Apply O2 and ATPM constraints (from spatio-flux config)
        model.set_bounds("EX_o2_e", Some(-2.0), None);
        model.set_bounds("ATPM", Some(1.0), Some(1.0));

        // Simulate Monod kinetics: at glucose=10mM, km=0.5, vmax=1.0
        // uptake_bound = -vmax * [S] / (km + [S]) = -1.0 * 10/(0.5+10) = -0.952
        let glucose_bound = -1.0 * 10.0 / (0.5 + 10.0);
        model.set_bounds("EX_glc__D_e", Some(glucose_bound), Some(0.0));

        let sol = model.solve().expect("FBA should be feasible");
        let glc_idx = model.reaction_index("EX_glc__D_e").unwrap();
        let ac_idx = model.reaction_index("EX_ac_e").unwrap();

        println!("Glucose bound: {glucose_bound:.4}");
        println!("Growth rate: {:.4}", sol.objective_value);
        println!("Glucose flux: {:.4}", sol.fluxes[glc_idx]);
        println!("Acetate flux: {:.4}", sol.fluxes[ac_idx]);

        assert!(sol.objective_value > 0.0, "should grow");
        assert!(sol.fluxes[ac_idx] > 0.0, "should secrete acetate");
    }
}
