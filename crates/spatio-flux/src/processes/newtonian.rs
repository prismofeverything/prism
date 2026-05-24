//! Newtonian particle movement using rapier2d rigid-body physics.
//!
//! Simulates 2D particles with gravity, collisions, elasticity,
//! and boundary walls. Port of pymunk_particles.py.
//!
//! The rapier2d world is persistent across ticks — only new/removed
//! particles are synced, avoiding the cost of rebuilding collision
//! structures every step.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use indexmap::IndexMap;
use rapier2d::prelude::*;

use prism_bigraph::{Process, Schema, Update, Value};

use super::particles::{DEFAULT_DENSITY, radius_from_mass};

/// Persistent rapier2d world state.
struct RapierWorld {
    rigid_body_set: RigidBodySet,
    collider_set: ColliderSet,
    physics_pipeline: PhysicsPipeline,
    island_manager: IslandManager,
    broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
    /// Particle ID → (RigidBodyHandle, ColliderHandle)
    body_map: HashMap<String, (RigidBodyHandle, ColliderHandle)>,
}

impl RapierWorld {
    fn new(bounds: (f32, f32), elasticity: f32) -> Self {
        let mut rigid_body_set = RigidBodySet::new();
        let mut collider_set = ColliderSet::new();

        // Create boundary walls
        let (w, h) = bounds;
        let wall_body = rigid_body_set.insert(RigidBodyBuilder::fixed().build());
        // Bottom
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(w, 0.1)
                .translation(vector![w / 2.0, -0.1])
                .restitution(elasticity)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );
        // Top
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(w, 0.1)
                .translation(vector![w / 2.0, h + 0.1])
                .restitution(elasticity)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );
        // Left
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(0.1, h)
                .translation(vector![-0.1, h / 2.0])
                .restitution(elasticity)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );
        // Right
        collider_set.insert_with_parent(
            ColliderBuilder::cuboid(0.1, h)
                .translation(vector![w + 0.1, h / 2.0])
                .restitution(elasticity)
                .build(),
            wall_body,
            &mut rigid_body_set,
        );

        RapierWorld {
            rigid_body_set,
            collider_set,
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            body_map: HashMap::new(),
        }
    }

    /// Add a particle to the world. Returns the body/collider handles.
    fn add_particle(
        &mut self,
        pid: &str,
        pos: (f32, f32),
        vel: (f32, f32),
        mass: f32,
        radius: f32,
        elasticity: f32,
        damping: f32,
    ) {
        let body = self.rigid_body_set.insert(
            RigidBodyBuilder::dynamic()
                .translation(vector![pos.0, pos.1])
                .linvel(vector![vel.0, vel.1])
                .linear_damping(damping)
                .build(),
        );
        let collider = self.collider_set.insert_with_parent(
            ColliderBuilder::ball(radius)
                .restitution(elasticity)
                .density(mass / (std::f32::consts::PI * radius * radius))
                .build(),
            body,
            &mut self.rigid_body_set,
        );
        self.body_map.insert(pid.to_string(), (body, collider));
    }

    /// Remove a particle from the world.
    fn remove_particle(&mut self, pid: &str) {
        if let Some((body_handle, _)) = self.body_map.remove(pid) {
            self.rigid_body_set.remove(
                body_handle,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
    }

    /// Update an existing particle's position, velocity, and collision radius.
    fn update_particle(
        &mut self,
        pid: &str,
        pos: (f32, f32),
        vel: (f32, f32),
        mass: f32,
        radius: f32,
    ) {
        if let Some(&(body_handle, collider_handle)) = self.body_map.get(pid) {
            if let Some(body) = self.rigid_body_set.get_mut(body_handle) {
                body.set_translation(vector![pos.0, pos.1], true);
                body.set_linvel(vector![vel.0, vel.1], true);
            }
            // Update collider radius and density if mass changed
            if let Some(collider) = self.collider_set.get_mut(collider_handle) {
                collider.set_shape(SharedShape::ball(radius));
                collider.set_density(mass / (std::f32::consts::PI * radius * radius));
            }
        }
    }

    fn step(&mut self, gravity: &Vector<f32>, dt: f32, n_substeps: usize) {
        // Wake all bodies — we reset state from external source each tick,
        // so rapier's sleep heuristic is unreliable.
        for (_, body) in self.rigid_body_set.iter_mut() {
            body.wake_up(true);
        }
        let sub_dt = dt / n_substeps as f32;
        let integration_parameters = IntegrationParameters {
            dt: sub_dt,
            ..Default::default()
        };
        for _ in 0..n_substeps {
            self.physics_pipeline.step(
                gravity,
                &integration_parameters,
                &mut self.island_manager,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.rigid_body_set,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                &mut self.ccd_solver,
                None,
                &(),
                &(),
            );
        }
    }
}

/// 2D rigid-body particle physics using rapier2d.
///
/// Maintains a persistent rapier2d world across ticks, syncing
/// particle additions/removals/mass changes each step.
pub struct NewtonianParticles {
    pub bounds: (f64, f64),
    pub gravity: (f64, f64),
    pub elasticity: f64,
    pub damping: f64,
    pub substeps: usize,
    pub interval: f64,
    /// Persistent physics world (lazy-initialized on first update).
    world: Mutex<Option<RapierWorld>>,
}

impl Clone for NewtonianParticles {
    fn clone(&self) -> Self {
        // Clone config only; world will be rebuilt on first use.
        NewtonianParticles {
            bounds: self.bounds,
            gravity: self.gravity,
            elasticity: self.elasticity,
            damping: self.damping,
            substeps: self.substeps,
            interval: self.interval,
            world: Mutex::new(None),
        }
    }
}

impl fmt::Debug for NewtonianParticles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewtonianParticles")
            .field("bounds", &self.bounds)
            .field("gravity", &self.gravity)
            .field("elasticity", &self.elasticity)
            .field("damping", &self.damping)
            .field("substeps", &self.substeps)
            .field("interval", &self.interval)
            .finish()
    }
}

/// Extract mass from particle value (sub_masses sum or mass field).
fn particle_mass(particle: &Value) -> f32 {
    let sub_total: f64 = particle
        .get_field("sub_masses")
        .and_then(|v| v.as_map())
        .map(|sm| sm.values().filter_map(|v| v.as_f64()).sum())
        .unwrap_or(0.0);
    if sub_total > 0.0 {
        sub_total as f32
    } else {
        particle
            .get_field("mass")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32
    }
}

/// Extract position from particle value.
fn particle_pos(particle: &Value) -> (f32, f32) {
    particle
        .get_field("position")
        .and_then(|v| v.as_list())
        .map(|l| {
            (
                l.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                l.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            )
        })
        .unwrap_or((0.0, 0.0))
}

/// Extract velocity from particle value.
fn particle_vel(particle: &Value) -> (f32, f32) {
    particle
        .get_field("velocity")
        .and_then(|v| v.as_list())
        .map(|l| {
            (
                l.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                l.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            )
        })
        .unwrap_or((0.0, 0.0))
}

impl Process for NewtonianParticles {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("particles".into(), Schema::map(Schema::Any))])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        // Additive position + velocity (Δ each step, summed by apply) — Newtonian
        // motion in the same delta model as Brownian. (radius is already a Δ.)
        let vec2 = || Schema::Array {
            shape: vec![2],
            element: Box::new(Schema::float()),
        };
        IndexMap::from([(
            "particles".into(),
            Schema::map(Schema::Tree {
                branches: IndexMap::from([
                    ("position".into(), vec2()),
                    ("velocity".into(), vec2()),
                ]),
            }),
        )])
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let particles = match state.get_field("particles").and_then(|v| v.as_map()) {
            Some(p) => p,
            None => return Update::Noop,
        };

        if particles.is_empty() {
            return Update::Noop;
        }

        let mut world_guard = self.world.lock().unwrap();

        // Lazy-initialize the world on first call
        let world = world_guard.get_or_insert_with(|| {
            RapierWorld::new(
                (self.bounds.0 as f32, self.bounds.1 as f32),
                self.elasticity as f32,
            )
        });

        let rapier_damping = if self.damping > 0.0 && self.damping < 1.0 {
            -(self.damping as f32).ln()
        } else {
            0.0
        };

        // Sync particles: detect additions, removals, and changes
        let current_ids: std::collections::HashSet<&str> =
            particles.keys().map(|s| s.as_str()).collect();
        let world_ids: std::collections::HashSet<String> = world.body_map.keys().cloned().collect();

        // Remove particles no longer in state
        let to_remove: Vec<String> = world_ids
            .iter()
            .filter(|id| !current_ids.contains(id.as_str()))
            .cloned()
            .collect();
        for pid in &to_remove {
            world.remove_particle(pid);
        }

        // Add new particles / update existing ones
        for (pid, particle) in particles {
            let mass = particle_mass(particle);
            let radius = radius_from_mass(mass as f64, DEFAULT_DENSITY) as f32;
            let pos = particle_pos(particle);
            let vel = particle_vel(particle);

            if world.body_map.contains_key(pid.as_str()) {
                world.update_particle(pid, pos, vel, mass, radius);
            } else {
                world.add_particle(
                    pid,
                    pos,
                    vel,
                    mass,
                    radius,
                    self.elasticity as f32,
                    rapier_damping,
                );
            }
        }

        // Step physics
        let gravity = vector![self.gravity.0 as f32, self.gravity.1 as f32];
        world.step(&gravity, interval as f32, self.substeps);

        // Extract updated positions and velocities
        let mut result: IndexMap<prism_schema::Key, Value> = IndexMap::new();

        for (pid, particle) in particles {
            let &(body_handle, _) = match world.body_map.get(pid.as_str()) {
                Some(h) => h,
                None => continue,
            };
            let body = &world.rigid_body_set[body_handle];
            let pos = body.translation();
            let vel = body.linvel();

            let pid_mass = particle_mass(particle) as f64;
            let new_radius = radius_from_mass(pid_mass, DEFAULT_DENSITY);

            let old_radius = particle
                .get_field("radius")
                .and_then(|v| v.as_f64())
                .unwrap_or(new_radius);
            let radius_delta = new_radius - old_radius;

            // Emit DELTAS (new − old) for position + velocity — apply SUMS them
            // (additive Array), so Newtonian motion composes like Brownian and the
            // boundary correction.
            let (ox, oy) = particle_pos(particle);
            let (ovx, ovy) = particle_vel(particle);
            let mut update: IndexMap<prism_schema::Key, Value> = IndexMap::new();
            update.insert(
                prism_schema::Key::from("position"),
                Value::List(vec![
                    Value::float((pos.x - ox) as f64),
                    Value::float((pos.y - oy) as f64),
                ]),
            );
            update.insert(
                prism_schema::Key::from("velocity"),
                Value::List(vec![
                    Value::float((vel.x - ovx) as f64),
                    Value::float((vel.y - ovy) as f64),
                ]),
            );
            if radius_delta.abs() > 1e-12 {
                update.insert(
                    prism_schema::Key::from("radius"),
                    Value::float(radius_delta),
                );
            }

            result.insert(pid.clone(), Value::Map(update));
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

    let bounds = map
        .get("bounds")
        .and_then(|v| v.as_list())
        .map(|l| {
            (
                l.first().and_then(|v| v.as_f64()).unwrap_or(50.0),
                l.get(1).and_then(|v| v.as_f64()).unwrap_or(50.0),
            )
        })
        .unwrap_or((50.0, 50.0));

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

    let substeps = map.get("substeps").and_then(|v| v.as_f64()).unwrap_or(10.0) as usize;

    let interval = map.get("interval").and_then(|v| v.as_f64()).unwrap_or(0.1);

    NewtonianParticles {
        bounds,
        gravity,
        elasticity,
        damping,
        substeps,
        interval,
        world: Mutex::new(None),
    }
}
