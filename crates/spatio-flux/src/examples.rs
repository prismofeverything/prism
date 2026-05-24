//! Example simulation documents reproducing the spatio-flux test suite.
//!
//! Each function returns a (Document, ProcessRegistry) pair that can
//! be executed with `runner::run_document()`.

use indexmap::IndexMap;

use prism_bigraph::document::ProcessDocument;
use prism_bigraph::{Document, ProcessNode, ProcessRegistry, Value};

use crate::processes::diffusion_advection::DiffusionAdvection;
use crate::processes::monod_kinetics::{self, MonodKinetics};
use crate::processes::particles::{BrownianMovement, ParticleExchange, make_particle};

// ── Helper: build fields as flat arrays ──

fn make_field(nx: usize, ny: usize, value: f64) -> Value {
    Value::List(vec![Value::float(value); nx * ny])
}

fn make_gradient_field(nx: usize, ny: usize, min_val: f64, max_val: f64) -> Value {
    let mut field = Vec::with_capacity(nx * ny);
    for y in 0..ny {
        let frac = y as f64 / (ny - 1).max(1) as f64;
        let val = min_val + frac * (max_val - min_val);
        for _ in 0..nx {
            field.push(Value::float(val));
        }
    }
    Value::List(field)
}

// ── 1. Monod Kinetics (well-mixed) ──

pub fn monod_kinetics_doc() -> (Document, ProcessRegistry) {
    let mut doc = Document::new();
    doc.state = Value::tree([
        ("biomass", Value::float(0.1)),
        (
            "substrates",
            Value::tree([
                ("glucose", Value::float(10.0)),
                ("acetate", Value::float(0.0)),
            ]),
        ),
    ]);

    doc.processes.insert(
        "kinetics".into(),
        ProcessDocument {
            process_type: "monod_kinetics".into(),
            config: Value::None,
            inputs: IndexMap::from([
                ("biomass".into(), vec!["biomass".into()]),
                ("substrates".into(), vec!["substrates".into()]),
            ]),
            outputs: IndexMap::from([
                ("biomass".into(), vec!["biomass".into()]),
                ("substrates".into(), vec!["substrates".into()]),
            ]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );

    let mut registry = ProcessRegistry::new();
    registry.register("monod_kinetics", |_config| {
        ProcessNode::Process(Box::new(MonodKinetics::new(
            monod_kinetics::models::overflow_metabolism(),
            1.0,
        )))
    });

    (doc, registry)
}

// ── 2. Diffusion Process ──

pub fn diffusion_doc() -> (Document, ProcessRegistry) {
    let (nx, ny) = (10, 20);

    let mut doc = Document::new();
    doc.state = Value::tree([(
        "fields",
        Value::tree([
            ("glucose", make_gradient_field(nx, ny, 0.0, 10.0)),
            ("acetate", make_field(nx, ny, 0.0)),
        ]),
    )]);

    doc.processes.insert(
        "diffusion".into(),
        ProcessDocument {
            process_type: "diffusion_advection".into(),
            config: Value::None,
            inputs: IndexMap::from([("fields".into(), vec!["fields".into()])]),
            outputs: IndexMap::from([("fields".into(), vec!["fields".into()])]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );

    let mut registry = ProcessRegistry::new();
    let proc_nx = nx;
    let proc_ny = ny;
    registry.register("diffusion_advection", move |_config| {
        ProcessNode::Process(Box::new(DiffusionAdvection::new(
            (proc_nx, proc_ny),
            (40.0, 80.0),
            IndexMap::from([("glucose".into(), 1e-1), ("acetate".into(), 1e-1)]),
            1.0,
        )))
    });

    (doc, registry)
}

// ── 3. Brownian Particles ──

pub fn brownian_particles_doc() -> (Document, ProcessRegistry) {
    let bounds = (50.0, 50.0);
    let exchange_mols = IndexMap::new();
    let particle = make_particle((25.0, 25.0), 0.5, &exchange_mols);

    let mut doc = Document::new();
    doc.state = Value::tree([("particles", Value::tree([("p0", particle)]))]);

    doc.processes.insert(
        "movement".into(),
        ProcessDocument {
            process_type: "brownian_movement".into(),
            config: Value::None,
            inputs: IndexMap::from([("particles".into(), vec!["particles".into()])]),
            outputs: IndexMap::from([("particles".into(), vec!["particles".into()])]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );

    let mut registry = ProcessRegistry::new();
    registry.register("brownian_movement", move |_config| {
        ProcessNode::Process(Box::new(BrownianMovement {
            bounds,
            diffusion_rate: 0.5,
            advection_rate: (0.0, 0.0),
            interval: 1.0,
        }))
    });

    (doc, registry)
}

// ── 4. Particles + Kinetics + Fields (br_particles_kinetics) ──

pub fn particles_kinetics_doc() -> (Document, ProcessRegistry) {
    let bounds = (50.0, 50.0);
    let (nx, ny) = (10, 10);

    let exchange_mols =
        IndexMap::from([("glucose".to_string(), 0.0), ("acetate".to_string(), 0.0)]);
    let particle = make_particle((25.0, 25.0), 0.1, &exchange_mols);

    let mut doc = Document::new();
    doc.state = Value::tree([
        (
            "fields",
            Value::tree([
                ("glucose", make_field(nx, ny, 10.0)),
                ("acetate", make_field(nx, ny, 0.0)),
            ]),
        ),
        ("particles", Value::tree([("p0", particle)])),
    ]);

    // Brownian movement
    doc.processes.insert(
        "movement".into(),
        ProcessDocument {
            process_type: "brownian_movement".into(),
            config: Value::None,
            inputs: IndexMap::from([("particles".into(), vec!["particles".into()])]),
            outputs: IndexMap::from([("particles".into(), vec!["particles".into()])]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );

    // Particle-field exchange (step)
    doc.processes.insert(
        "exchange".into(),
        ProcessDocument {
            process_type: "particle_exchange".into(),
            config: Value::None,
            inputs: IndexMap::from([
                ("particles".into(), vec!["particles".into()]),
                ("fields".into(), vec!["fields".into()]),
            ]),
            outputs: IndexMap::from([
                ("particles".into(), vec!["particles".into()]),
                ("fields".into(), vec!["fields".into()]),
            ]),
            interval: None, // step
            priority: 0.0,
        },
    );

    let mut registry = ProcessRegistry::new();
    registry.register("brownian_movement", move |_config| {
        ProcessNode::Process(Box::new(BrownianMovement {
            bounds,
            diffusion_rate: 0.5,
            advection_rate: (0.0, 0.0),
            interval: 1.0,
        }))
    });
    registry.register("particle_exchange", move |_config| {
        ProcessNode::Step(Box::new(ParticleExchange {
            n_bins: (nx, ny),
            bounds,
            depth: 1.0,
        }))
    });

    (doc, registry)
}

// ── 5. COMETS-like: Spatial Kinetics + Diffusion ──

pub fn comets_diffusion_doc() -> (Document, ProcessRegistry) {
    let (nx, ny) = (10, 10);
    let bounds = (50.0, 50.0);

    // Glucose gradient (more at top), biomass strip at top-center
    let glucose_field = make_gradient_field(nx, ny, 0.0, 10.0);

    // Biomass: small amount in top-center strip
    let mut biomass_vals = vec![0.0; nx * ny];
    for x in 3..7 {
        biomass_vals[(ny - 1) * nx + x] = 0.1;
    }
    let biomass_field = Value::List(biomass_vals.into_iter().map(Value::float).collect());

    let mut doc = Document::new();
    doc.state = Value::tree([(
        "fields",
        Value::tree([
            ("glucose", glucose_field),
            ("acetate", make_field(nx, ny, 0.0)),
            ("biomass", biomass_field),
        ]),
    )]);

    // Diffusion process
    doc.processes.insert(
        "diffusion".into(),
        ProcessDocument {
            process_type: "diffusion_advection".into(),
            config: Value::None,
            inputs: IndexMap::from([("fields".into(), vec!["fields".into()])]),
            outputs: IndexMap::from([("fields".into(), vec!["fields".into()])]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );

    let mut registry = ProcessRegistry::new();
    registry.register("diffusion_advection", move |_config| {
        ProcessNode::Process(Box::new(DiffusionAdvection::new(
            (nx, ny),
            bounds,
            IndexMap::from([
                ("glucose".into(), 1e-1),
                ("acetate".into(), 1e-1),
                ("biomass".into(), 1e-3),
            ]),
            1.0,
        )))
    });

    (doc, registry)
}

// ── 6. COMETS + Brownian Particles + Kinetics ──

pub fn comets_particles_kinetics_doc() -> (Document, ProcessRegistry) {
    let (nx, ny) = (10, 10);
    let bounds = (50.0, 50.0);

    let exchange_mols =
        IndexMap::from([("glucose".to_string(), 0.0), ("acetate".to_string(), 0.0)]);
    let particle = make_particle((25.0, 40.0), 0.1, &exchange_mols);

    let mut doc = Document::new();
    doc.state = Value::tree([
        (
            "fields",
            Value::tree([
                ("glucose", make_gradient_field(nx, ny, 0.0, 10.0)),
                ("acetate", make_field(nx, ny, 0.0)),
            ]),
        ),
        ("particles", Value::tree([("p0", particle)])),
    ]);

    // Diffusion
    doc.processes.insert(
        "diffusion".into(),
        ProcessDocument {
            process_type: "diffusion_advection".into(),
            config: Value::None,
            inputs: IndexMap::from([("fields".into(), vec!["fields".into()])]),
            outputs: IndexMap::from([("fields".into(), vec!["fields".into()])]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );

    // Brownian movement
    doc.processes.insert(
        "movement".into(),
        ProcessDocument {
            process_type: "brownian_movement".into(),
            config: Value::None,
            inputs: IndexMap::from([("particles".into(), vec!["particles".into()])]),
            outputs: IndexMap::from([("particles".into(), vec!["particles".into()])]),
            interval: Some(1.0),
            priority: 0.0,
        },
    );

    // Exchange (step)
    doc.processes.insert(
        "exchange".into(),
        ProcessDocument {
            process_type: "particle_exchange".into(),
            config: Value::None,
            inputs: IndexMap::from([
                ("particles".into(), vec!["particles".into()]),
                ("fields".into(), vec!["fields".into()]),
            ]),
            outputs: IndexMap::from([
                ("particles".into(), vec!["particles".into()]),
                ("fields".into(), vec!["fields".into()]),
            ]),
            interval: None,
            priority: 0.0,
        },
    );

    let mut registry = ProcessRegistry::new();
    registry.register("diffusion_advection", move |_config| {
        ProcessNode::Process(Box::new(DiffusionAdvection::new(
            (nx, ny),
            bounds,
            IndexMap::from([("glucose".into(), 1e-1), ("acetate".into(), 1e-1)]),
            1.0,
        )))
    });
    registry.register("brownian_movement", move |_config| {
        ProcessNode::Process(Box::new(BrownianMovement {
            bounds,
            diffusion_rate: 0.5,
            advection_rate: (0.0, 0.0),
            interval: 1.0,
        }))
    });
    registry.register("particle_exchange", move |_config| {
        ProcessNode::Step(Box::new(ParticleExchange {
            n_bins: (nx, ny),
            bounds,
            depth: 1.0,
        }))
    });

    (doc, registry)
}

/// All available example names.
pub fn list_examples() -> Vec<&'static str> {
    vec![
        "monod_kinetics",
        "diffusion_process",
        "brownian_particles",
        "br_particles_kinetics",
        "comets_diffusion",
        "comets_br_particles_kinetics",
    ]
}

/// Get an example by name. Returns (Document, ProcessRegistry, duration, emit_interval).
pub fn get_example(name: &str) -> Option<(Document, ProcessRegistry, f64, f64)> {
    match name {
        "monod_kinetics" => {
            let (doc, reg) = monod_kinetics_doc();
            Some((doc, reg, 60.0, 1.0))
        }
        "diffusion_process" => {
            let (doc, reg) = diffusion_doc();
            Some((doc, reg, 60.0, 1.0))
        }
        "brownian_particles" => {
            let (doc, reg) = brownian_particles_doc();
            Some((doc, reg, 200.0, 1.0))
        }
        "br_particles_kinetics" => {
            let (doc, reg) = particles_kinetics_doc();
            Some((doc, reg, 60.0, 1.0))
        }
        "comets_diffusion" => {
            let (doc, reg) = comets_diffusion_doc();
            Some((doc, reg, 60.0, 1.0))
        }
        "comets_br_particles_kinetics" => {
            let (doc, reg) = comets_particles_kinetics_doc();
            Some((doc, reg, 60.0, 1.0))
        }
        _ => None,
    }
}
