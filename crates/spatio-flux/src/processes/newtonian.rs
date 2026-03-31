//! Newtonian particle movement using rapier2d rigid-body physics.
//!
//! Simulates 2D particles with gravity, collisions, elasticity,
//! and boundary walls. Port of pymunk_particles.py.

use std::any::Any;

use indexmap::IndexMap;
use rapier2d::prelude::*;

use prism_bigraph::{Process, Schema, Update, Value};

use super::particles::{radius_from_mass, DEFAULT_DENSITY};

/// 2D rigid-body particle physics using rapier2d.
#[derive(Clone, Debug)]
pub struct NewtonianParticles {
    pub bounds: (f64, f64),
    pub gravity: (f64, f64),
    pub elasticity: f64,
    pub damping: f64,
    pub substeps: usize,
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

        // Build rapier world with sub-stepping for stability.
        // Since we rebuild the world each update (no persistent contacts),
        // sub-stepping helps the solver converge for resting stacks.
        let n_substeps = self.substeps;
        let sub_dt = interval as f32 / n_substeps as f32;

        let gravity = vector![self.gravity.0 as f32, self.gravity.1 as f32];
        let mut rigid_body_set = RigidBodySet::new();
        let mut collider_set = ColliderSet::new();
        let integration_parameters = IntegrationParameters {
            dt: sub_dt,
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

            // Use sub_masses sum if available, else fall back to mass field
            let mass = {
                let m = particle.as_map();
                let sub_total: f64 = m
                    .and_then(|m| m.get("sub_masses"))
                    .and_then(|v| v.as_map())
                    .map(|sm| sm.values().filter_map(|v| v.as_f64()).sum())
                    .unwrap_or(0.0);
                if sub_total > 0.0 {
                    sub_total as f32
                } else {
                    m.and_then(|m| m.get("mass"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32
                }
            };

            let radius = radius_from_mass(mass as f64, DEFAULT_DENSITY) as f32;

            // Read velocity from state (preserved across steps)
            let vel = particle
                .as_map()
                .and_then(|m| m.get("velocity"))
                .and_then(|v| v.as_list())
                .map(|l| {
                    (
                        l.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                        l.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    )
                })
                .unwrap_or((0.0, 0.0));

            // Convert pymunk-style damping (fraction retained per second)
            // to rapier linear_damping: v *= 1/(1 + dt*d), so for
            // pymunk damping=0.998 → retain=0.998/s → rapier_d = (1/retain - 1)/dt ≈ (1-retain)/dt
            // But rapier applies per-step: we want 1/(1+d*dt)^(1/dt) = retain
            // Simplify: rapier_damping = -ln(pymunk_damping)
            let rapier_damping = if self.damping > 0.0 && self.damping < 1.0 {
                -(self.damping as f32).ln()
            } else {
                0.0 // damping >= 1 means no damping in pymunk convention
            };

            let body = rigid_body_set.insert(
                RigidBodyBuilder::dynamic()
                    .translation(vector![pos.0, pos.1])
                    .linvel(vector![vel.0, vel.1])
                    .linear_damping(rapier_damping)
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

        // Step physics with sub-stepping
        for _ in 0..n_substeps {
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
        }

        // Extract updated positions and velocities only.
        // IMPORTANT: output only position/velocity (List → replace semantics).
        // Do NOT clone the full particle — Float fields like mass/radius
        // would be treated as deltas and doubled each step.
        let mut result: IndexMap<String, Value> = IndexMap::new();

        for (pid, body_handle) in &body_map {
            let body = &rigid_body_set[*body_handle];
            let pos = body.translation();
            let vel = body.linvel();

            result.insert(pid.clone(), Value::tree([
                (
                    "position",
                    Value::List(vec![
                        Value::float(pos.x as f64),
                        Value::float(pos.y as f64),
                    ]),
                ),
                (
                    "velocity",
                    Value::List(vec![
                        Value::float(vel.x as f64),
                        Value::float(vel.y as f64),
                    ]),
                ),
            ]));
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

    // Gravity can be a scalar (y-component only) or a [gx, gy] list
    let gravity = match map.get("gravity") {
        Some(v) if v.as_list().is_some() => {
            let l = v.as_list().unwrap();
            (
                l.first().and_then(|v| v.as_f64()).unwrap_or(0.0),
                l.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
            )
        }
        Some(v) if v.as_f64().is_some() => (0.0, v.as_f64().unwrap()),
        _ => (0.0, -9.8),
    };

    let elasticity = map
        .get("elasticity")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.9);

    let damping = map
        .get("damping_per_second")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1);

    let substeps = map
        .get("substeps")
        .and_then(|v| v.as_f64())
        .unwrap_or(10.0) as usize;

    let interval = map
        .get("interval")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1);

    NewtonianParticles {
        bounds,
        gravity,
        elasticity,
        damping,
        substeps,
        interval,
    }
}
