//! Newtonian particle movement using rapier2d rigid-body physics.
//!
//! Simulates 2D particles with gravity, collisions, elasticity,
//! and boundary walls. Port of pymunk_particles.py.

use std::any::Any;

use indexmap::IndexMap;
use rapier2d::prelude::*;

use prism_bigraph::{Process, Schema, Update, Value};

/// 2D rigid-body particle physics using rapier2d.
#[derive(Clone, Debug)]
pub struct NewtonianParticles {
    pub bounds: (f64, f64),
    pub gravity: (f64, f64),
    pub elasticity: f64,
    pub damping: f64,
    pub interval: f64,
}

impl Process for NewtonianParticles {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("particles".into(), Schema::map(Schema::Any))])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("particles".into(), Schema::map(Schema::Any))])
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let particles = match state
            .as_map()
            .and_then(|m| m.get("particles"))
            .and_then(|v| v.as_map())
        {
            Some(p) => p,
            None => return Update::Noop,
        };

        if particles.is_empty() {
            return Update::Noop;
        }

        // Build rapier world
        let gravity = vector![self.gravity.0 as f32, self.gravity.1 as f32];
        let mut rigid_body_set = RigidBodySet::new();
        let mut collider_set = ColliderSet::new();
        let integration_parameters = IntegrationParameters {
            dt: interval as f32,
            ..Default::default()
        };
        let mut physics_pipeline = PhysicsPipeline::new();
        let mut island_manager = IslandManager::new();
        let mut broad_phase = DefaultBroadPhase::new();
        let mut narrow_phase = NarrowPhase::new();
        let mut impulse_joint_set = ImpulseJointSet::new();
        let mut multibody_joint_set = MultibodyJointSet::new();
        let mut ccd_solver = CCDSolver::new();

        // Create boundary walls
        let (w, h) = (self.bounds.0 as f32, self.bounds.1 as f32);
        let wall_body = rigid_body_set.insert(RigidBodyBuilder::fixed().build());
        // Bottom
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(w, 0.1)
                .translation(vector![w / 2.0, -0.1])
                .restitution(self.elasticity as f32)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );
        // Top
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(w, 0.1)
                .translation(vector![w / 2.0, h + 0.1])
                .restitution(self.elasticity as f32)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );
        // Left
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(0.1, h)
                .translation(vector![-0.1, h / 2.0])
                .restitution(self.elasticity as f32)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );
        // Right
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(0.1, h)
                .translation(vector![w + 0.1, h / 2.0])
                .restitution(self.elasticity as f32)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );

        // Create particle bodies
        let mut body_map: Vec<(String, RigidBodyHandle)> = Vec::new();

        for (pid, particle) in particles {
            let pos = particle
                .as_map()
                .and_then(|m| m.get("position"))
                .and_then(|v| v.as_list())
                .map(|l| {
                    (
                        l.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                        l.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    )
                })
                .unwrap_or((0.0, 0.0));

            let mass = particle
                .as_map()
                .and_then(|m| m.get("mass"))
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;

            let radius = particle
                .as_map()
                .and_then(|m| m.get("radius"))
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;

            let body = rigid_body_set.insert(
                RigidBodyBuilder::dynamic()
                    .translation(vector![pos.0, pos.1])
                    .linear_damping(self.damping as f32)
                    .build(),
            );

            collider_set.insert_with_parent(
                ColliderBuilder::ball(radius)
                    .restitution(self.elasticity as f32)
                    .density(mass / (std::f32::consts::PI * radius * radius))
                    .build(),
                body,
                &mut rigid_body_set,
            );

            body_map.push((pid.clone(), body));
        }

        // Step physics
        physics_pipeline.step(
            &gravity,
            &integration_parameters,
            &mut island_manager,
            &mut broad_phase,
            &mut narrow_phase,
            &mut rigid_body_set,
            &mut collider_set,
            &mut impulse_joint_set,
            &mut multibody_joint_set,
            &mut ccd_solver,
            None,
            &(),
            &(),
        );

        // Extract updated positions
        let mut result: IndexMap<String, Value> = IndexMap::new();

        for (pid, body_handle) in &body_map {
            let body = &rigid_body_set[*body_handle];
            let pos = body.translation();
            let vel = body.linvel();

            let original = &particles[pid];
            let mut updated = original.clone();
            if let Some(map) = updated.as_map_mut() {
                map.insert(
                    "position".to_string(),
                    Value::List(vec![
                        Value::float(pos.x as f64),
                        Value::float(pos.y as f64),
                    ]),
                );
                map.insert(
                    "velocity".to_string(),
                    Value::List(vec![
                        Value::float(vel.x as f64),
                        Value::float(vel.y as f64),
                    ]),
                );
            }
            result.insert(pid.clone(), updated);
        }

        Update::value(Value::tree([("particles", Value::Map(result))]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Construct from vivarium config.
pub fn newtonian_from_config(config: &Value) -> NewtonianParticles {
    let map = config.as_map().cloned().unwrap_or_default();

    let bounds_val = map.get("bounds");
    let bounds = bounds_val
        .and_then(|v| v.as_list())
        .map(|l| {
            (
                l.first().and_then(|v| v.as_f64()).unwrap_or(50.0),
                l.get(1).and_then(|v| v.as_f64()).unwrap_or(50.0),
            )
        })
        .unwrap_or((50.0, 50.0));

    let gravity = map
        .get("gravity")
        .and_then(|v| v.as_list())
        .map(|l| {
            (
                l.first().and_then(|v| v.as_f64()).unwrap_or(0.0),
                l.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
            )
        })
        .unwrap_or((0.0, -9.8));

    let elasticity = map
        .get("elasticity")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.9);

    let damping = map
        .get("damping_per_second")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1);

    let interval = map
        .get("interval")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1);

    NewtonianParticles {
        bounds,
        gravity,
        elasticity,
        damping,
        interval,
    }
}
