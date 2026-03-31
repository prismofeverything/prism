//! Monod (Michaelis-Menten) kinetics process.
//!
//! rate = vmax * [S] / (km + [S])
//! flux = rate * biomass * interval  (for substrate-driven reactions)

use std::any::Any;

use indexmap::IndexMap;

use prism_bigraph::{Process, Schema, Update, Value};

/// Configuration for a single Monod reaction.
#[derive(Clone, Debug)]
pub struct Reaction {
    pub reactant: String,
    pub product: String,
    pub km: f64,
    pub vmax: f64,
    pub yield_coeff: f64,
}

/// Monod kinetics process: general-purpose Michaelis-Menten with yield.
#[derive(Clone, Debug)]
pub struct MonodKinetics {
    pub reactions: Vec<Reaction>,
    pub interval: f64,
    /// Scale factor applied to biomass yield. Default 1.0.
    /// Reduces growth rate without changing consumption rate.
    pub yield_scale: f64,
}

impl MonodKinetics {
    pub fn new(reactions: Vec<Reaction>, interval: f64) -> Self {
        Self { reactions, interval, yield_scale: 1.0 }
    }

    /// Get all substrate molecule IDs referenced by reactions.
    pub fn substrate_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for rxn in &self.reactions {
            if !ids.contains(&rxn.reactant) && rxn.reactant != "biomass" {
                ids.push(rxn.reactant.clone());
            }
            if !ids.contains(&rxn.product) && rxn.product != "biomass" {
                ids.push(rxn.product.clone());
            }
        }
        ids
    }
}

impl Process for MonodKinetics {
    fn inputs(&self) -> IndexMap<String, Schema> {
        let mut ports = IndexMap::new();
        ports.insert("biomass".to_string(), Schema::float());
        ports.insert("substrates".to_string(), Schema::map(Schema::float()));
        ports
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        let mut ports = IndexMap::new();
        ports.insert("biomass".to_string(), Schema::float());
        ports.insert("substrates".to_string(), Schema::map(Schema::float()));
        ports
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

        let substrates = match map.get("substrates").and_then(|v| v.as_map()) {
            Some(s) => s,
            None => return Update::Noop,
        };

        let mut biomass_delta = 0.0;
        // Track running substrate values (start from current concentrations)
        let mut substrate_values: IndexMap<String, f64> = IndexMap::new();
        for (mol_id, val) in substrates {
            substrate_values.insert(mol_id.clone(), val.as_f64().unwrap_or(0.0));
        }

        for rxn in &self.reactions {
            let concentration = substrate_values
                .get(&rxn.reactant)
                .copied()
                .unwrap_or(0.0);

            // Monod rate: vmax * [S] / (km + [S])
            let rate = if concentration > 0.0 {
                rxn.vmax * concentration / (rxn.km + concentration)
            } else {
                0.0
            };

            // Flux is biomass-proportional for substrate reactions
            let flux = if rxn.reactant == "biomass" || rxn.reactant == "mass" {
                rate * interval
            } else {
                rate * biomass * interval
            };

            // Don't consume more than available
            let flux = if rxn.reactant != "biomass" && rxn.reactant != "mass" {
                flux.min(concentration.max(0.0))
            } else {
                flux
            };

            // Reactant consumed
            if rxn.reactant != "biomass" && rxn.reactant != "mass" {
                *substrate_values.entry(rxn.reactant.clone()).or_insert(0.0) -= flux;
            }

            // Product produced (with yield)
            if rxn.product == "biomass" || rxn.product == "mass" {
                biomass_delta += flux * rxn.yield_coeff * self.yield_scale;
            } else {
                *substrate_values.entry(rxn.product.clone()).or_insert(0.0) +=
                    flux * rxn.yield_coeff;
            }
        }

        // Output DELTAS for substrates (negative = consumed, positive = produced).
        // The engine uses additive apply by default for numeric types.
        let substrate_deltas: IndexMap<String, Value> = substrate_values
            .iter()
            .map(|(k, &final_val)| {
                let initial = substrates.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
                (k.clone(), Value::float(final_val - initial))
            })
            .collect();

        Update::value(Value::tree([
            ("biomass", Value::float(biomass_delta)), // delta, not absolute
            ("substrates", Value::Map(substrate_deltas)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Pre-built model configs matching Python spatio-flux registry.
pub mod models {
    use super::*;

    pub fn glucose_only() -> Vec<Reaction> {
        vec![Reaction {
            reactant: "glucose".into(),
            product: "biomass".into(),
            km: 0.5,
            vmax: 0.2,
            yield_coeff: 0.5,
        }]
    }

    pub fn acetate_only() -> Vec<Reaction> {
        vec![Reaction {
            reactant: "acetate".into(),
            product: "biomass".into(),
            km: 0.5,
            vmax: 0.1,
            yield_coeff: 0.3,
        }]
    }

    pub fn overflow_metabolism() -> Vec<Reaction> {
        vec![
            Reaction {
                reactant: "glucose".into(),
                product: "biomass".into(),
                km: 0.5,
                vmax: 0.2,
                yield_coeff: 0.5,
            },
            Reaction {
                reactant: "glucose".into(),
                product: "acetate".into(),
                km: 0.5,
                vmax: 0.1,
                yield_coeff: 0.8,
            },
        ]
    }

    pub fn cross_feeding() -> Vec<Reaction> {
        vec![
            Reaction {
                reactant: "glucose".into(),
                product: "biomass".into(),
                km: 0.5,
                vmax: 0.2,
                yield_coeff: 0.5,
            },
            Reaction {
                reactant: "glucose".into(),
                product: "acetate".into(),
                km: 0.5,
                vmax: 0.1,
                yield_coeff: 0.8,
            },
            Reaction {
                reactant: "acetate".into(),
                product: "biomass".into(),
                km: 0.5,
                vmax: 0.1,
                yield_coeff: 0.3,
            },
        ]
    }
}
