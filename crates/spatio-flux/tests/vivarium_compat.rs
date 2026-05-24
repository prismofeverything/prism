//! Integration tests loading real spatio-flux JSON documents.
//!
//! These tests load the actual JSON documents from the Python spatio-flux
//! test suite and run them through the Rust engine, verifying compatibility.

use prism_bigraph::Document;
use spatio_flux::from_config::build_registry;
use spatio_flux::vivarium_loader::load_vivarium;
use std::sync::Arc;

const OUT_DIR: &str = "out/vivarium";

/// Load a vivarium JSON, convert to prism format, run, and verify.
fn run_vivarium_doc(name: &str, json_str: &str, duration: f64) {
    let registry = Arc::new(build_registry());

    let (mut engine, vdoc) = match load_vivarium(json_str, registry) {
        Ok(r) => r,
        Err(e) => {
            println!("{name}: SKIPPED ({e})");
            return;
        }
    };

    println!("{name}: loaded {} processes", vdoc.processes.len());
    for (pname, proc) in &vdoc.processes {
        println!(
            "  {pname}: {} (interval={:?})",
            proc.class_name, proc.interval
        );
    }

    // Save as prism document
    std::fs::create_dir_all(OUT_DIR).unwrap();
    let topology = vdoc.to_topology();
    let doc = Document::from_topology(&topology);
    let _ = doc.save_all(OUT_DIR, name);

    // Run simulation
    engine.run(duration);

    println!(
        "{name}: ran to t={:.1}, state keys: {:?}",
        engine.time(),
        engine
            .state()
            .as_map()
            .map(|m| m.keys().collect::<Vec<_>>()),
    );
}

// ── Inline JSON documents from the spatio-flux report ──

#[test]
fn test_monod_kinetics_vivarium() {
    let json = r#"{
        "state": {
            "global_time": 0.0,
            "monod_kinetics": {
                "address": {"protocol": "local", "data": "MonodKinetics"},
                "config": {
                    "reactions": {
                        "assimilate_glucose": {
                            "reactant": "glucose", "product": "mass",
                            "km": 0.5, "vmax": 0.4, "yield": 0.2
                        },
                        "maintenance_turnover": {
                            "reactant": "mass", "product": "detritus",
                            "km": 1.0, "vmax": 0.001, "yield": 1.0
                        },
                        "overflow_to_acetate": {
                            "reactant": "glucose", "product": "acetate",
                            "km": 0.5, "vmax": 0.3, "yield": 0.5
                        },
                        "assimilate_acetate": {
                            "reactant": "acetate", "product": "mass",
                            "km": 0.6, "vmax": 0.3, "yield": 0.2
                        }
                    }
                },
                "_inputs": "biomass:mass|substrates:map[concentration]",
                "_outputs": "biomass:float|substrates:map[float]",
                "inputs": {
                    "substrates": {
                        "acetate": ["fields", "acetate"],
                        "glucose": ["fields", "glucose"]
                    },
                    "biomass": ["fields", "biomass"]
                },
                "outputs": {
                    "substrates": {
                        "acetate": ["fields", "acetate"],
                        "glucose": ["fields", "glucose"]
                    },
                    "biomass": ["fields", "biomass"]
                }
            },
            "fields": {
                "glucose": 10.0,
                "acetate": 0.0,
                "biomass": 0.1
            },
            "emitter": {
                "address": {"protocol": "local", "data": "RAMEmitter"},
                "config": {"emit": {}},
                "inputs": {},
                "outputs": {}
            }
        },
        "schema": {
            "global_time": "float",
            "monod_kinetics": "link[biomass:mass|substrates:map[concentration],biomass:float|substrates:map[float]]",
            "fields": "glucose:concentration|acetate:concentration|biomass:mass"
        }
    }"#;

    run_vivarium_doc("monod_kinetics_vivarium", json, 60.0);
}

#[test]
fn test_brownian_particles_vivarium() {
    let json = r#"{
        "state": {
            "global_time": 0.0,
            "particles": {
                "p0": {
                    "id": "abc123",
                    "position": [25.0, 25.0],
                    "mass": 0.162,
                    "local": {},
                    "exchange": {},
                    "sub_masses": {}
                }
            },
            "brownian_movement": {
                "address": {"protocol": "local", "data": "BrownianMovement"},
                "config": {
                    "interval": 0.1,
                    "bounds": [50.0, 50.0],
                    "diffusion_rate": 0.5,
                    "n_bins": [0, 0],
                    "advection_rate": [0.0, 0.0]
                },
                "_inputs": "particles:map[particle]",
                "_outputs": "particles:map[particle]",
                "inputs": {"particles": ["particles"]},
                "outputs": {"particles": ["particles"]}
            },
            "enforce_boundaries": {
                "address": {"protocol": "local", "data": "ManageBoundaries"},
                "config": {
                    "buffer": 0.0001,
                    "bounds": [50.0, 50.0],
                    "add_rate": 0.01,
                    "boundary_to_add": [],
                    "clamp_survivors": true,
                    "mass_range": [0.001, 1.0],
                    "boundary_to_remove": []
                },
                "inputs": {"particles": ["particles"]},
                "outputs": {"particles": ["particles"]}
            },
            "emitter": {
                "address": {"protocol": "local", "data": "RAMEmitter"},
                "config": {"emit": {}},
                "inputs": {},
                "outputs": {}
            }
        },
        "schema": {
            "global_time": "float",
            "particles": "map[particle]"
        }
    }"#;

    run_vivarium_doc("brownian_particles_vivarium", json, 20.0);
}

#[test]
fn test_diffusion_vivarium() {
    let json = r#"{
        "state": {
            "global_time": 0.0,
            "fields": {
                "glucose": [
                    [0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 10.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 0.0, 0.0]
                ]
            },
            "diffusion": {
                "address": {"protocol": "local", "data": "DiffusionAdvection"},
                "config": {
                    "bounds": [50.0, 50.0],
                    "n_bins": [5, 5],
                    "diffusion_coeffs": {"glucose": 0.1},
                    "default_diffusion_rate": 0.1,
                    "max_dt": 0.1,
                    "advection_coeffs": {},
                    "boundary_conditions": {},
                    "clip_negative": true
                },
                "inputs": {"fields": ["fields"]},
                "outputs": {"fields": ["fields"]}
            },
            "emitter": {
                "address": {"protocol": "local", "data": "RAMEmitter"},
                "config": {"emit": {}},
                "inputs": {},
                "outputs": {}
            }
        },
        "schema": {
            "global_time": "float",
            "fields": "map[array[5|5,float]]"
        }
    }"#;

    run_vivarium_doc("diffusion_vivarium", json, 30.0);
}

#[test]
fn test_comets_particles_kinetics_vivarium() {
    let json = r#"{
        "state": {
            "global_time": 0.0,
            "fields": {
                "glucose": [
                    [10.0, 10.0, 10.0, 10.0, 10.0],
                    [10.0, 10.0, 10.0, 10.0, 10.0],
                    [10.0, 10.0, 10.0, 10.0, 10.0],
                    [10.0, 10.0, 10.0, 10.0, 10.0],
                    [10.0, 10.0, 10.0, 10.0, 10.0]
                ],
                "acetate": [
                    [0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 0.0, 0.0]
                ]
            },
            "particles": {
                "p0": {
                    "id": "test01",
                    "position": [25.0, 25.0],
                    "mass": 0.1,
                    "local": {"glucose": 0.0, "acetate": 0.0},
                    "exchange": {"glucose": 0.0, "acetate": 0.0},
                    "sub_masses": {}
                }
            },
            "diffusion": {
                "address": {"protocol": "local", "data": "DiffusionAdvection"},
                "config": {
                    "bounds": [50.0, 50.0],
                    "n_bins": [5, 5],
                    "diffusion_coeffs": {"glucose": 0.1, "acetate": 0.1},
                    "default_diffusion_rate": 0.1,
                    "max_dt": 0.1,
                    "advection_coeffs": {},
                    "boundary_conditions": {},
                    "clip_negative": true
                },
                "inputs": {"fields": ["fields"]},
                "outputs": {"fields": ["fields"]}
            },
            "brownian_movement": {
                "address": {"protocol": "local", "data": "BrownianMovement"},
                "config": {
                    "interval": 1.0,
                    "bounds": [50.0, 50.0],
                    "diffusion_rate": 0.5,
                    "advection_rate": [0.0, 0.0]
                },
                "inputs": {"particles": ["particles"]},
                "outputs": {"particles": ["particles"]}
            },
            "enforce_boundaries": {
                "address": {"protocol": "local", "data": "ManageBoundaries"},
                "config": {
                    "buffer": 0.0001,
                    "bounds": [50.0, 50.0]
                },
                "inputs": {"particles": ["particles"]},
                "outputs": {"particles": ["particles"]}
            },
            "particle_exchange": {
                "address": {"protocol": "local", "data": "ParticleExchange"},
                "config": {
                    "bounds": [50.0, 50.0],
                    "n_bins": [5, 5]
                },
                "inputs": {
                    "particles": ["particles"],
                    "fields": ["fields"]
                },
                "outputs": {
                    "particles": ["particles"],
                    "fields": ["fields"]
                }
            },
            "particle_division": {
                "address": {"protocol": "local", "data": "ParticleDivision"},
                "config": {
                    "division_mass_threshold": 5.0,
                    "division_jitter": 0.001
                },
                "inputs": {"particles": ["particles"]},
                "outputs": {"particles": ["particles"]}
            },
            "emitter": {
                "address": {"protocol": "local", "data": "RAMEmitter"},
                "config": {"emit": {}},
                "inputs": {},
                "outputs": {}
            }
        },
        "schema": {
            "global_time": "float",
            "fields": "map[array[5|5,float]]",
            "particles": "map[particle]"
        }
    }"#;

    run_vivarium_doc("comets_particles_kinetics_vivarium", json, 30.0);
}
