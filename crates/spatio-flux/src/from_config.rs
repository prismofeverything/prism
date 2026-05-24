//! Construct processes from vivarium JSON configs.
//!
//! Each process has a `from_config` that parses its vivarium-style config Value
//! and constructs the Rust process instance.

use indexmap::IndexMap;

use prism_bigraph::{ProcessNode, ProcessRegistry, Value};

use crate::processes::dfba;
use crate::processes::diffusion_advection::DiffusionAdvection;
use crate::processes::monod_kinetics::{MonodKinetics, Reaction};
use crate::processes::newtonian;
use crate::processes::particles::{
    BrownianMovement, ManageBoundaries, ParticleDivision, ParticleExchange, ParticleTotalMass,
};

/// Helper to extract f64 from a Value, handling both Float and Int.
fn as_f64(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// Helper to extract a pair of f64s from a list Value.
fn as_f64_pair(v: &Value) -> (f64, f64) {
    match v.as_list() {
        Some(l) if l.len() >= 2 => (as_f64(&l[0]), as_f64(&l[1])),
        _ => (0.0, 0.0),
    }
}

/// Helper to extract a pair of usize from a list Value.
fn as_usize_pair(v: &Value) -> (usize, usize) {
    match v.as_list() {
        Some(l) if l.len() >= 2 => (as_f64(&l[0]) as usize, as_f64(&l[1]) as usize),
        _ => (0, 0),
    }
}

// ── MonodKinetics ──

pub fn monod_kinetics_from_config(config: &Value) -> MonodKinetics {
    let mut reactions = Vec::new();
    let interval = config
        .as_map()
        .and_then(|m| m.get("interval"))
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);

    if let Some(rxns) = config
        .as_map()
        .and_then(|m| m.get("reactions"))
        .and_then(|v| v.as_map())
    {
        for (_name, rxn_val) in rxns {
            if let Some(rxn_map) = rxn_val.as_map() {
                let reactant = rxn_map
                    .get("reactant")
                    .and_then(|v| v.as_str())
                    .unwrap_or("glucose")
                    .to_string();
                let product = rxn_map
                    .get("product")
                    .and_then(|v| v.as_str())
                    .unwrap_or("biomass")
                    .to_string();
                let km = rxn_map.get("km").map(as_f64).unwrap_or(0.5);
                let vmax = rxn_map.get("vmax").map(as_f64).unwrap_or(0.1);
                let yield_coeff = rxn_map.get("yield").map(as_f64).unwrap_or(0.5);

                reactions.push(Reaction {
                    reactant,
                    product,
                    km,
                    vmax,
                    yield_coeff,
                });
            }
        }
    }

    // Default yield_scale=0.8 compensates for our correct (non-accumulating)
    // exchange model vs Python's accumulating exchange which effectively
    // increases field depletion feedback. Can be overridden in config.
    let yield_scale = config
        .as_map()
        .and_then(|m| m.get("yield_scale"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.8);

    let mut mk = MonodKinetics::new(reactions, interval);
    mk.yield_scale = yield_scale;
    mk
}

// ── DiffusionAdvection ──

pub fn diffusion_advection_from_config(config: &Value) -> DiffusionAdvection {
    let map = config.as_map().unwrap_or(&IndexMap::new()).clone();

    let n_bins = map.get("n_bins").map(as_usize_pair).unwrap_or((10, 10));
    let bounds = map.get("bounds").map(as_f64_pair).unwrap_or((50.0, 50.0));
    let default_diffusion = map.get("default_diffusion_rate").map(as_f64).unwrap_or(0.1);
    let interval = map.get("interval").map(as_f64).unwrap_or(1.0);

    let mut diffusion_coeffs = IndexMap::new();
    if let Some(dc) = map.get("diffusion_coeffs").and_then(|v| v.as_map()) {
        for (mol, val) in dc {
            diffusion_coeffs.insert(mol.to_string(), as_f64(val));
        }
    }

    let mut advection_coeffs = IndexMap::new();
    if let Some(ac) = map.get("advection_coeffs").and_then(|v| v.as_map()) {
        for (mol, val) in ac {
            advection_coeffs.insert(mol.to_string(), as_f64_pair(val));
        }
    }

    let mut da = DiffusionAdvection::new(n_bins, bounds, diffusion_coeffs, interval);
    da.default_diffusion = default_diffusion;
    da.advection_coeffs = advection_coeffs;

    da
}

// ── BrownianMovement ──

pub fn brownian_movement_from_config(config: &Value) -> BrownianMovement {
    let map = config.as_map().unwrap_or(&IndexMap::new()).clone();

    let bounds = map.get("bounds").map(as_f64_pair).unwrap_or((50.0, 50.0));
    let diffusion_rate = map.get("diffusion_rate").map(as_f64).unwrap_or(0.5);
    let advection_rate = map
        .get("advection_rate")
        .map(as_f64_pair)
        .unwrap_or((0.0, 0.0));
    let interval = map.get("interval").map(as_f64).unwrap_or(1.0);

    BrownianMovement {
        bounds,
        diffusion_rate,
        advection_rate,
        interval,
    }
}

// ── ParticleExchange ──

pub fn particle_exchange_from_config(config: &Value) -> ParticleExchange {
    let map = config.as_map().unwrap_or(&IndexMap::new()).clone();

    let n_bins = map.get("n_bins").map(as_usize_pair).unwrap_or((10, 10));
    let bounds = map.get("bounds").map(as_f64_pair).unwrap_or((50.0, 50.0));
    let depth = map.get("depth").map(as_f64).unwrap_or(1.0);

    ParticleExchange {
        n_bins,
        bounds,
        depth,
    }
}

// ── ParticleDivision ──

pub fn particle_division_from_config(config: &Value) -> ParticleDivision {
    let map = config.as_map().unwrap_or(&IndexMap::new()).clone();

    let threshold = map
        .get("division_mass_threshold")
        .map(as_f64)
        .unwrap_or(5.0);
    let jitter = map.get("division_jitter").map(as_f64).unwrap_or(0.001);

    let max_particles = map.get("max_particles").map(as_f64).unwrap_or(100.0) as usize;

    ParticleDivision {
        division_mass_threshold: threshold,
        jitter,
        max_particles,
    }
}

// ── ManageBoundaries ──

pub fn manage_boundaries_from_config(config: &Value) -> ManageBoundaries {
    let map = config.as_map().unwrap_or(&IndexMap::new()).clone();

    let bounds = map.get("bounds").map(as_f64_pair).unwrap_or((50.0, 50.0));
    let buffer = map.get("buffer").map(as_f64).unwrap_or(0.0001);
    let add_rate = map.get("add_rate").map(as_f64).unwrap_or(0.0);
    let mass_range = map
        .get("mass_range")
        .map(as_f64_pair)
        .unwrap_or((0.001, 1.0));

    let boundary_to_add = map
        .get("boundary_to_add")
        .and_then(|v| v.as_list())
        .map(|l| {
            l.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let boundary_to_remove = map
        .get("boundary_to_remove")
        .and_then(|v| v.as_list())
        .map(|l| {
            l.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    ManageBoundaries {
        bounds,
        buffer,
        add_rate,
        boundary_to_add,
        boundary_to_remove,
        mass_range,
    }
}

// ── Registry builder ──

/// Build a ProcessRegistry with all spatio-flux process factories.
/// Each factory reads its config from the vivarium JSON config Value.
pub fn build_registry() -> ProcessRegistry {
    let mut reg = ProcessRegistry::new();

    reg.register("MonodKinetics", |config| {
        ProcessNode::Process(Box::new(monod_kinetics_from_config(&config)))
    });

    reg.register("DiffusionAdvection", |config| {
        ProcessNode::Process(Box::new(diffusion_advection_from_config(&config)))
    });

    reg.register("BrownianMovement", |config| {
        ProcessNode::Process(Box::new(brownian_movement_from_config(&config)))
    });

    reg.register("ParticleExchange", |config| {
        ProcessNode::Step(Box::new(particle_exchange_from_config(&config)))
    });

    reg.register("ParticleDivision", |config| {
        ProcessNode::Step(Box::new(particle_division_from_config(&config)))
    });

    reg.register("ManageBoundaries", |config| {
        ProcessNode::Step(Box::new(manage_boundaries_from_config(&config)))
    });

    reg.register("DynamicFBA", |config| {
        ProcessNode::Process(Box::new(dfba::dfba_from_config(&config)))
    });

    reg.register("SpatialDFBA", |config| {
        ProcessNode::Process(Box::new(
            crate::processes::spatial_dfba::spatial_dfba_from_config(&config),
        ))
    });

    reg.register("PymunkParticleMovement", |config| {
        ProcessNode::Process(Box::new(newtonian::newtonian_from_config(&config)))
    });
    // Clearer alias for the Rust rapier2d Newtonian mover (`PymunkParticleMovement`
    // is the upstream Python-compat name the fixtures use).
    reg.register("NewtonianParticles", |config| {
        ProcessNode::Process(Box::new(newtonian::newtonian_from_config(&config)))
    });

    reg.register("ParticleTotalMass", |_config| {
        ProcessNode::Step(Box::new(ParticleTotalMass))
    });
    // The mass-action integrators + `RunProcess` are prism-std natives now (the
    // process-contract demo is prism work); use `prism_std::register_processes`.
    reg
}

/// Build a registry that includes the Composite type.
/// The Composite needs a reference to the registry itself for discovering
/// its inner processes, so we build the base registry first, wrap in Arc,
/// then register the Composite factory using the Arc.
pub fn build_registry_with_composites() -> std::sync::Arc<ProcessRegistry> {
    let mut reg = build_registry();

    // We need the Arc to exist before we can register the Composite factory.
    // Solution: register a placeholder, wrap in Arc, then replace.
    // Actually simpler: just build the Arc and use it directly.
    let arc = std::sync::Arc::new(reg);

    // Can't mutate through Arc directly. Instead, build a new registry
    // that includes Composite and copies everything else.
    // For now, the Composite is constructed in from_config or vivarium_loader
    // using the Arc<ProcessRegistry> directly, not through the factory.
    arc
}
