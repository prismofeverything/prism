//! Spatial Dynamic FBA — runs independent FBA at each grid cell.
//!
//! Each cell in the 2D grid has an assigned organism model. Each timestep,
//! the process iterates over all cells, computes Monod uptake bounds from
//! local substrate concentrations, solves the FBA LP, and applies the
//! exchange fluxes to the local grid cell.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

use prism_bigraph::{Process, Schema, Update, Value};

use super::dfba::KineticParam;
use super::fba::{load_model_cached, CobraModel};
use crate::processes::fields::flatten_field;

/// Spatial dFBA process operating on a 2D lattice.
#[derive(Clone, Debug)]
pub struct SpatialDFBA {
    /// Grid dimensions (nx, ny).
    pub n_bins: (usize, usize),
    /// Model assignment per cell: model_grid[row][col] = model_name.
    pub model_grid: Vec<Vec<String>>,
    /// Loaded COBRA models keyed by name (shared via Arc cache).
    pub models: HashMap<String, Arc<CobraModel>>,
    /// Kinetic params per model: model_name -> substrate -> KineticParam.
    pub kinetic_params: HashMap<String, IndexMap<String, KineticParam>>,
    /// Per-model substrate → exchange reaction ID.
    pub substrate_reactions: HashMap<String, IndexMap<String, String>>,
    /// Config bounds per model.
    pub config_bounds: HashMap<String, IndexMap<String, (Option<f64>, Option<f64>)>>,
    /// Substrate molecule IDs.
    pub mol_ids: Vec<String>,
    /// Process interval.
    pub interval: f64,
}

impl Process for SpatialDFBA {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("fields".into(), Schema::map(Schema::list(Schema::float()))),
            ("biomass".into(), Schema::Any),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        // Use Schema::Any so the engine falls back to the state schema
        // (which declares fields as map[array[...]] with additive semantics)
        IndexMap::from([
            ("fields".into(), Schema::Any),
            ("biomass".into(), Schema::Any),
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

        // Read fields as 2D arrays (flattened to 1D, row-major)
        let fields_map = match map.get("fields").and_then(|v| v.as_map()) {
            Some(f) => f,
            None => return Update::Noop,
        };

        let (nx, ny) = self.n_bins;
        let n_cells = nx * ny;

        // Read all substrate fields (save originals for delta computation)
        let mut substrate_arrays: IndexMap<String, Vec<f64>> = IndexMap::new();
        let mut substrate_originals: IndexMap<String, Vec<f64>> = IndexMap::new();
        for mol_id in &self.mol_ids {
            let arr = fields_map
                .get(mol_id)
                .map(flatten_field)
                .unwrap_or_else(|| vec![0.0; n_cells]);
            substrate_originals.insert(mol_id.clone(), arr.clone());
            substrate_arrays.insert(mol_id.clone(), arr);
        }

        // Read biomass field
        let biomass_key = map.get("biomass");
        let biomass_original = biomass_key
            .map(flatten_field)
            .unwrap_or_else(|| vec![0.0; n_cells]);
        let mut biomass_arr = biomass_original.clone();

        let dt = interval;

        // Iterate over each grid cell
        for row in 0..ny {
            for col in 0..nx {
                let cell_idx = row * nx + col;
                let model_name = &self.model_grid[row][col];

                let model = match self.models.get(model_name) {
                    Some(m) => m,
                    None => continue,
                };

                let biomass = if cell_idx < biomass_arr.len() {
                    biomass_arr[cell_idx]
                } else {
                    0.0
                };

                if biomass <= 1e-12 {
                    continue;
                }

                // Build bounds with Monod kinetic limits
                let mut lb = model.lower_bounds.clone();
                let mut ub = model.upper_bounds.clone();

                // Apply config bounds
                if let Some(cb) = self.config_bounds.get(model_name) {
                    for (rxn_id, (lo, hi)) in cb {
                        if let Some(idx) = model.reaction_index(rxn_id) {
                            if let Some(l) = lo { lb[idx] = *l; }
                            if let Some(h) = hi { ub[idx] = *h; }
                        }
                    }
                }

                // Get per-model substrate reactions
                let model_sr = match self.substrate_reactions.get(model_name) {
                    Some(sr) => sr,
                    None => continue,
                };

                // Apply Monod uptake bounds
                let kp = self.kinetic_params.get(model_name);
                for (substrate, rxn_id) in model_sr {
                    if let Some(idx) = model.reaction_index(rxn_id) {
                        let conc = substrate_arrays
                            .get(substrate)
                            .and_then(|a| a.get(cell_idx))
                            .copied()
                            .unwrap_or(0.0);

                        let bound = if let Some(params) = kp.and_then(|p| p.get(substrate)) {
                            if conc > 1e-12 {
                                -params.vmax * conc / (params.km + conc)
                            } else {
                                0.0
                            }
                        } else {
                            lb[idx]
                        };
                        lb[idx] = bound;
                        // Ensure lb <= ub (some models have negative ub for forced uptake)
                        if lb[idx] > ub[idx] {
                            ub[idx] = lb[idx];
                        }
                    }
                }

                // Solve FBA
                let solution = match model.solve_with_bounds(&lb, &ub) {
                    Some(sol) => sol,
                    None => continue,
                };

                let mu = solution.objective_value;

                // Apply exchange fluxes to this cell
                for (substrate, rxn_id) in model_sr {
                    if let Some(idx) = model.reaction_index(rxn_id) {
                        let flux = solution.fluxes[idx];
                        if let Some(arr) = substrate_arrays.get_mut(substrate) {
                            if cell_idx < arr.len() {
                                arr[cell_idx] = (arr[cell_idx] + flux * biomass * dt).max(0.0);
                            }
                        }
                    }
                }

                // Update biomass
                if cell_idx < biomass_arr.len() {
                    biomass_arr[cell_idx] += mu * biomass * dt;
                }
            }
        }

        // Build output as DELTAS (new - original)
        let mut result_fields: IndexMap<String, Value> = IndexMap::new();
        for (mol_id, arr) in &substrate_arrays {
            let orig = substrate_originals.get(mol_id).unwrap();
            let delta: Vec<f64> = arr.iter().zip(orig.iter()).map(|(a, o)| a - o).collect();
            let original_val = fields_map.get(mol_id).unwrap_or(&Value::None);
            result_fields.insert(
                mol_id.clone(),
                crate::processes::fields::rebuild_field(&delta, original_val),
            );
        }

        let biomass_delta: Vec<f64> = biomass_arr.iter().zip(biomass_original.iter())
            .map(|(a, o)| a - o).collect();
        let biomass_orig_val = biomass_key.unwrap_or(&Value::None);
        let biomass_out = crate::processes::fields::rebuild_field(&biomass_delta, biomass_orig_val);

        Update::value(Value::tree([
            ("fields", Value::Map(result_fields)),
            ("biomass", biomass_out),
        ]))
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

/// Construct SpatialDFBA from vivarium config.
pub fn spatial_dfba_from_config(config: &Value) -> SpatialDFBA {
    let map = config.as_map().cloned().unwrap_or_default();

    let n_bins_val = map.get("n_bins").and_then(|v| v.as_list());
    let nx = n_bins_val.and_then(|l| l.first()).and_then(|v| v.as_f64()).unwrap_or(5.0) as usize;
    let ny = n_bins_val.and_then(|l| l.get(1)).and_then(|v| v.as_f64()).unwrap_or(5.0) as usize;

    // Parse model_grid
    let mut model_grid: Vec<Vec<String>> = Vec::new();
    if let Some(grid) = map.get("model_grid").and_then(|v| v.as_list()) {
        for row in grid {
            if let Some(row_list) = row.as_list() {
                let row_names: Vec<String> = row_list
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                model_grid.push(row_names);
            }
        }
    }

    // Parse mol_ids
    let mol_ids: Vec<String> = map
        .get("mol_ids")
        .and_then(|v| v.as_list())
        .map(|l| l.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    // Parse models configs
    let mut models: HashMap<String, Arc<CobraModel>> = HashMap::new();
    let mut kinetic_params: HashMap<String, IndexMap<String, KineticParam>> = HashMap::new();
    let mut config_bounds: HashMap<String, IndexMap<String, (Option<f64>, Option<f64>)>> = HashMap::new();

    if let Some(models_map) = map.get("models").and_then(|v| v.as_map()) {
        for (model_name, model_config) in models_map {
            let mc = model_config.as_map().cloned().unwrap_or_default();

            // Load COBRA model (cached)
            let model_file = mc.get("model_file").and_then(|v| v.as_str()).unwrap_or("textbook");
            if let Ok(m) = load_model_cached(model_file) {
                models.insert(model_name.clone(), m);
            }

            // Kinetic params
            let mut kp = IndexMap::new();
            if let Some(kp_map) = mc.get("kinetic_params").and_then(|v| v.as_map()) {
                for (substrate, params) in kp_map {
                    if let Some(list) = params.as_list() {
                        let km = list.first().and_then(|v| v.as_f64()).unwrap_or(0.5);
                        let vmax = list.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0);
                        kp.insert(substrate.clone(), KineticParam { km, vmax });
                    }
                }
            }
            kinetic_params.insert(model_name.clone(), kp);

            // Bounds
            let mut cb = IndexMap::new();
            if let Some(b) = mc.get("bounds").and_then(|v| v.as_map()) {
                for (rxn_id, bound_val) in b {
                    if let Some(bmap) = bound_val.as_map() {
                        let lower = bmap.get("lower").and_then(|v| v.as_f64());
                        let upper = bmap.get("upper").and_then(|v| v.as_f64());
                        cb.insert(rxn_id.clone(), (lower, upper));
                    }
                }
            }
            config_bounds.insert(model_name.clone(), cb);
        }
    }

    // Parse per-model substrate_update_reactions
    let mut substrate_reactions: HashMap<String, IndexMap<String, String>> = HashMap::new();
    if let Some(models_map) = map.get("models").and_then(|v| v.as_map()) {
        for (model_name, mc) in models_map {
            let mut sr_map = IndexMap::new();
            if let Some(sr) = mc.as_map().and_then(|m| m.get("substrate_update_reactions")).and_then(|v| v.as_map()) {
                for (substrate, rxn_id) in sr {
                    if let Some(rxn) = rxn_id.as_str() {
                        sr_map.insert(substrate.clone(), rxn.to_string());
                    }
                }
            }
            substrate_reactions.insert(model_name.clone(), sr_map);
        }
    }

    let interval = map.get("interval").and_then(|v| v.as_f64()).unwrap_or(1.0);

    SpatialDFBA {
        n_bins: (nx, ny),
        model_grid,
        models,
        kinetic_params,
        substrate_reactions,
        config_bounds,
        mol_ids,
        interval,
    }
}
