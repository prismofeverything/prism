//! Brownian particle movement, boundary management, exchange, and division.

use std::any::Any;

use indexmap::IndexMap;
use rand::Rng;

use prism_bigraph::{Process, Schema, Step, Update, Value};

// ── Particle State Helpers ──

/// Generate a unique short ID for a particle.
pub fn short_id() -> String {
    use rand::distributions::Alphanumeric;
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect()
}

/// Create a single particle state value.
/// Compute circle radius from mass using pymunk convention: r = sqrt(m / (density * pi))
pub fn radius_from_mass(mass: f64, density: f64) -> f64 {
    (mass / (density * std::f64::consts::PI)).sqrt()
}

/// Default 2D mass density (pg/µm²) matching pymunk_particles.py
pub const DEFAULT_DENSITY: f64 = 0.015;

pub fn make_particle(
    position: (f64, f64),
    mass: f64,
    exchange: &IndexMap<String, f64>,
) -> Value {
    let exchange_val = Value::Map(
        exchange
            .iter()
            .map(|(k, v)| (k.clone(), Value::float(*v)))
            .collect(),
    );
    let local_val = Value::Map(
        exchange
            .iter()
            .map(|(k, _)| (k.clone(), Value::float(0.0)))
            .collect(),
    );

    let radius = radius_from_mass(mass, DEFAULT_DENSITY);

    Value::tree([
        ("id", Value::String(short_id())),
        (
            "position",
            Value::List(vec![Value::float(position.0), Value::float(position.1)]),
        ),
        ("mass", Value::float(mass)),
        ("radius", Value::float(radius)),
        ("local", local_val),
        ("exchange", exchange_val),
    ])
}

/// Extract position from a particle value.
fn get_position(particle: &Value) -> Option<(f64, f64)> {
    let map = particle.as_map()?;
    let pos = map.get("position")?.as_list()?;
    Some((pos.first()?.as_f64()?, pos.get(1)?.as_f64()?))
}

/// Extract mass from a particle value.
/// If sub_masses exists and has numeric values, use their sum as the total mass.
/// Otherwise fall back to the `mass` field.
fn get_mass(particle: &Value) -> f64 {
    if let Some(map) = particle.as_map() {
        if let Some(sub_masses) = map.get("sub_masses").and_then(|v| v.as_map()) {
            let total: f64 = sub_masses.values().filter_map(|v| v.as_f64()).sum();
            if total > 0.0 {
                return total;
            }
        }
        map.get("mass").and_then(|v| v.as_f64()).unwrap_or(0.0)
    } else {
        0.0
    }
}

// ── Brownian Movement Process ──

/// Stochastic 2D Brownian motion with optional advection.
#[derive(Clone, Debug)]
pub struct BrownianMovement {
    pub bounds: (f64, f64),
    pub diffusion_rate: f64,
    pub advection_rate: (f64, f64),
    pub interval: f64,
}

impl Process for BrownianMovement {
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

        let mut rng = rand::thread_rng();
        let sigma = (2.0 * self.diffusion_rate * interval).sqrt();
        let mut result: IndexMap<String, Value> = IndexMap::new();

        for (pid, particle) in particles {
            let (x, y) = match get_position(particle) {
                Some(p) => p,
                None => continue,
            };

            // Brownian step + advection
            let nx = (x + normal(&mut rng, sigma) + self.advection_rate.0 * interval)
                .clamp(0.0, self.bounds.0);
            let ny = (y + normal(&mut rng, sigma) + self.advection_rate.1 * interval)
                .clamp(0.0, self.bounds.1);

            // Output ONLY the changed field (position). Map merge leaves other fields unchanged.
            // Position is a List, so apply_update replaces it (not additive).
            let update = Value::tree([
                ("position", Value::List(vec![Value::float(nx), Value::float(ny)])),
            ]);
            result.insert(pid.clone(), update);
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

// ── Particle Exchange Step ──

/// Bidirectional exchange between particles and spatial fields.
/// Particles sample local field values and deposit exchange deltas.
#[derive(Clone, Debug)]
pub struct ParticleExchange {
    pub n_bins: (usize, usize),
    pub bounds: (f64, f64),
    pub depth: f64,
}

impl Step for ParticleExchange {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("particles".into(), Schema::map(Schema::Any)),
            ("fields".into(), Schema::Any),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("particles".into(), Schema::map(Schema::Any)),
            ("fields".into(), Schema::Any),
        ])
    }

    fn update(&self, state: &Value) -> Update {
        let map = match state.as_map() {
            Some(m) => m,
            None => return Update::Noop,
        };

        let particles = match map.get("particles").and_then(|v| v.as_map()) {
            Some(p) => p,
            None => return Update::Noop,
        };

        let fields = match map.get("fields").and_then(|v| v.as_map()) {
            Some(f) => f,
            None => return Update::Noop,
        };

        let (nx, ny) = self.n_bins;
        let mut result_particles: IndexMap<String, Value> = IndexMap::new();

        // Build mutable field arrays for accumulating exchange deltas.
        // Start with ZEROS — we accumulate only the deltas, matching the Python.
        let mut field_deltas: IndexMap<String, Vec<f64>> = IndexMap::new();
        let mut field_values: IndexMap<String, Vec<f64>> = IndexMap::new();
        for (mol_id, field_val) in fields {
            let arr = super::fields::flatten_field(field_val);
            let arr = if arr.is_empty() {
                vec![0.0; nx * ny]
            } else {
                arr
            };
            field_deltas.insert(mol_id.clone(), vec![0.0; arr.len()]);
            field_values.insert(mol_id.clone(), arr);
        }

        for (pid, particle) in particles {
            let (x, y) = match get_position(particle) {
                Some(p) => p,
                None => continue,
            };

            // Convert continuous position to bin index
            let x_bin = ((x / self.bounds.0) * nx as f64).floor() as usize;
            let y_bin = ((y / self.bounds.1) * ny as f64).floor() as usize;
            let x_bin = x_bin.min(nx - 1);
            let y_bin = y_bin.min(ny - 1);
            let bin_idx = y_bin * nx + x_bin;

            // 1. Apply particle's exchange to field FIRST.
            // Exchange is in counts, field is concentration.
            // Divide by cell volume to convert correctly.
            // Cell volume for count→concentration conversion.
            // depth is from config (default 1.0).
            let cell_w = self.bounds.0 / nx as f64;
            let cell_h = self.bounds.1 / ny as f64;
            let cell_volume = (cell_w * cell_h * self.depth).max(1e-10);

            // 1. Accumulate exchange deltas into field delta arrays
            if let Some(exchange) = particle.as_map().and_then(|m| m.get("exchange")).and_then(|v| v.as_map()) {
                for (mol_id, delta) in exchange {
                    if let (Some(darr), Some(d)) = (field_deltas.get_mut(mol_id), delta.as_f64()) {
                        if bin_idx < darr.len() {
                            darr[bin_idx] += d / cell_volume;
                        }
                    }
                }
            }

            // 2. Sample local from the field (current values, not deltas).
            // Output delta: new_local - old_local
            let old_local = particle.as_map()
                .and_then(|m| m.get("local"))
                .and_then(|v| v.as_map());
            let mut local: IndexMap<String, Value> = IndexMap::new();
            for (mol_id, arr) in &field_values {
                let field_val = if bin_idx < arr.len() {
                    arr[bin_idx]
                } else if !arr.is_empty() {
                    arr[0]
                } else {
                    0.0
                };
                let old_val = old_local
                    .and_then(|m| m.get(mol_id))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                local.insert(mol_id.clone(), Value::float(field_val - old_val));
            }

            // 3. Output local delta AND zero exchange.
            let mut update_map: IndexMap<String, Value> = IndexMap::new();
            update_map.insert("local".to_string(), Value::Map(local));
            // Zero exchange: negate current values (additive apply → net zero)
            if let Some(exchange) = particle.as_map().and_then(|m| m.get("exchange")).and_then(|v| v.as_map()) {
                let negated: IndexMap<String, Value> = exchange
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::float(-v.as_f64().unwrap_or(0.0))))
                    .collect();
                update_map.insert("exchange".to_string(), Value::Map(negated));
            }
            let updated = Value::Map(update_map);
            result_particles.insert(pid.clone(), updated);
        }

        // Convert field DELTA arrays back to Values, preserving original 2D structure
        let result_fields: IndexMap<String, Value> = field_deltas
            .into_iter()
            .map(|(k, delta_arr)| {
                let original = fields.get(&k).unwrap_or(&Value::None);
                (k, super::fields::rebuild_field(&delta_arr, original))
            })
            .collect();

        Update::value(Value::tree([
            ("particles", Value::Map(result_particles)),
            ("fields", Value::Map(result_fields)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Particle Division Step ──

/// Binary cell division when mass exceeds threshold.
#[derive(Clone, Debug)]
pub struct ParticleDivision {
    pub division_mass_threshold: f64,
    pub jitter: f64,
    pub max_particles: usize,
}

impl Step for ParticleDivision {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("particles".into(), Schema::map(Schema::Any))])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("particles".into(), Schema::map(Schema::Any))])
    }

    fn update(&self, state: &Value) -> Update {
        let particles = match state
            .as_map()
            .and_then(|m| m.get("particles"))
            .and_then(|v| v.as_map())
        {
            Some(p) => p,
            None => return Update::Noop,
        };

        let mut rng = rand::thread_rng();
        let mut result: IndexMap<String, Value> = IndexMap::new();
        let mut any_divided = false;

        let current_count = particles.len();

        let mut to_remove: Vec<Value> = Vec::new();
        let mut to_add: IndexMap<String, Value> = IndexMap::new();

        for (pid, particle) in particles {
            let mass = get_mass(particle);

            let net_count = particles.len() + to_add.len() - to_remove.len();
            if mass >= self.division_mass_threshold {
                any_divided = true;
                let (x, y) = get_position(particle).unwrap_or((0.0, 0.0));

                // Remove parent
                to_remove.push(Value::String(pid.clone()));

                // At cap: create one daughter (halve mass, keep count stable).
                // Below cap: create two daughters (normal division).
                let n_daughters = if net_count >= self.max_particles { 1 } else { 2 };
                for _ in 0..n_daughters {
                    let dx = normal(&mut rng, self.jitter);
                    let dy = normal(&mut rng, self.jitter);
                    let mut daughter = particle.clone();
                    if let Some(dmap) = daughter.as_map_mut() {
                        dmap.insert("id".to_string(), Value::String(short_id()));
                        dmap.insert("mass".to_string(), Value::float(mass / 2.0));
                        dmap.insert(
                            "position".to_string(),
                            Value::List(vec![Value::float(x + dx), Value::float(y + dy)]),
                        );
                        // Halve sub_masses so ParticleTotalMass doesn't
                        // immediately restore the pre-division total and
                        // re-trigger division on the next tick.
                        if let Some(Value::Map(sm)) = dmap.get("sub_masses").cloned() {
                            let halved: IndexMap<String, Value> = sm.iter()
                                .map(|(k, v)| {
                                    let half = v.as_f64().unwrap_or(0.0) / 2.0;
                                    (k.clone(), Value::float(half))
                                })
                                .collect();
                            dmap.insert("sub_masses".to_string(), Value::Map(halved));
                        }
                    }
                    to_add.insert(short_id(), daughter);
                }
            }
        }

        if any_divided {
            let mut particles_update: IndexMap<String, Value> = IndexMap::new();
            if !to_remove.is_empty() {
                particles_update.insert("_remove".to_string(), Value::List(to_remove));
            }
            if !to_add.is_empty() {
                particles_update.insert("_add".to_string(), Value::Map(to_add));
            }
            Update::value(Value::tree([("particles", Value::Map(particles_update))]))
        } else {
            Update::Noop
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Manage Boundaries Step ──

/// Boundary enforcement: reflect, remove, and spawn particles.
#[derive(Clone, Debug)]
pub struct ManageBoundaries {
    pub bounds: (f64, f64),
    pub buffer: f64,
    pub add_rate: f64,
    pub boundary_to_add: Vec<String>,
    pub boundary_to_remove: Vec<String>,
    pub mass_range: (f64, f64),
}

impl ManageBoundaries {
    /// Generate a random position on a boundary edge.
    fn boundary_position(&self, boundary: &str, rng: &mut impl Rng) -> (f64, f64) {
        let (w, h) = self.bounds;
        match boundary {
            "top" => (rng.r#gen::<f64>() * w, h - self.buffer),
            "bottom" => (rng.r#gen::<f64>() * w, self.buffer),
            "left" => (self.buffer, rng.r#gen::<f64>() * h),
            "right" => (w - self.buffer, rng.r#gen::<f64>() * h),
            _ => (w / 2.0, h / 2.0),
        }
    }
}

impl Step for ManageBoundaries {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("particles".into(), Schema::map(Schema::Any)),
            ("process_interval".into(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("particles".into(), Schema::map(Schema::Any))])
    }

    fn update(&self, state: &Value) -> Update {
        let particles = match state
            .as_map()
            .and_then(|m| m.get("particles"))
            .and_then(|v| v.as_map())
        {
            Some(p) => p,
            None => return Update::Noop,
        };

        // Read process_interval (dt) for Poisson rate→probability conversion.
        // Python: p_birth = 1 - exp(-add_rate * dt)
        let dt = state
            .as_map()
            .and_then(|m| m.get("process_interval"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        let mut rng = rand::thread_rng();
        let mut updates: IndexMap<String, Value> = IndexMap::new();
        let mut to_remove: Vec<Value> = Vec::new();
        let mut to_add: IndexMap<String, Value> = IndexMap::new();
        let mut any_changed = false;

        for (pid, particle) in particles {
            let (x, y) = match get_position(particle) {
                Some(p) => p,
                None => continue,
            };

            // Check removal boundaries
            let should_remove = self.boundary_to_remove.iter().any(|side| match side.as_str() {
                "left" => x < self.buffer,
                "right" => x > self.bounds.0 - self.buffer,
                "bottom" => y < self.buffer,
                "top" => y > self.bounds.1 - self.buffer,
                _ => false,
            });

            if should_remove {
                any_changed = true;
                to_remove.push(Value::String(pid.clone()));
                continue;
            }

            // Reflect out-of-bounds positions
            let clamped_x = x.clamp(self.buffer, self.bounds.0 - self.buffer);
            let clamped_y = y.clamp(self.buffer, self.bounds.1 - self.buffer);

            if (clamped_x - x).abs() > 1e-10 || (clamped_y - y).abs() > 1e-10 {
                any_changed = true;
                updates.insert(pid.clone(), Value::tree([
                    ("position", Value::List(vec![Value::float(clamped_x), Value::float(clamped_y)])),
                ]));
            }
        }

        // Spawn new particles at boundaries (Poisson process).
        // Convert rate (events/second) to per-step probability: p = 1 - exp(-rate * dt)
        // Clone an existing particle as template to preserve process specs
        // (e.g. dFBA) needed for dynamic process discovery.
        if self.add_rate > 0.0 {
            let p_birth = 1.0 - (-self.add_rate * dt).exp();
            let template = particles.values().next().cloned();
            for boundary in &self.boundary_to_add {
                if rng.r#gen::<f64>() < p_birth {
                    any_changed = true;
                    let pos = self.boundary_position(boundary, &mut rng);
                    let mass = self.mass_range.0
                        + rng.r#gen::<f64>() * (self.mass_range.1 - self.mass_range.0);

                    let new_particle = if let Some(tmpl) = &template {
                        // Clone template, override position/mass/exchange/local/id
                        let mut p = tmpl.clone();
                        if let Some(pmap) = p.as_map_mut() {
                            pmap.insert("id".to_string(), Value::String(short_id()));
                            pmap.insert("mass".to_string(), Value::float(mass));
                            // Update radius to match new mass
                            pmap.insert("radius".to_string(),
                                Value::float(radius_from_mass(mass, DEFAULT_DENSITY)));
                            pmap.insert("position".to_string(),
                                Value::List(vec![Value::float(pos.0), Value::float(pos.1)]));
                            // Zero out exchange and local
                            if let Some(Value::Map(ex)) = pmap.get("exchange").cloned() {
                                let zeroed: IndexMap<String, Value> = ex.keys()
                                    .map(|k| (k.clone(), Value::float(0.0))).collect();
                                pmap.insert("exchange".to_string(), Value::Map(zeroed));
                            }
                            if let Some(Value::Map(loc)) = pmap.get("local").cloned() {
                                let zeroed: IndexMap<String, Value> = loc.keys()
                                    .map(|k| (k.clone(), Value::float(0.0))).collect();
                                pmap.insert("local".to_string(), Value::Map(zeroed));
                            }
                            // Zero out sub_masses if present
                            if let Some(Value::Map(sm)) = pmap.get("sub_masses").cloned() {
                                let zeroed: IndexMap<String, Value> = sm.keys()
                                    .map(|k| (k.clone(), Value::float(0.0))).collect();
                                pmap.insert("sub_masses".to_string(), Value::Map(zeroed));
                            }
                        }
                        p
                    } else {
                        make_particle(pos, mass, &IndexMap::new())
                    };
                    to_add.insert(short_id(), new_particle);
                }
            }
        }

        if any_changed {
            let mut particles_update = updates;
            if !to_remove.is_empty() {
                particles_update.insert("_remove".to_string(), Value::List(to_remove));
            }
            if !to_add.is_empty() {
                particles_update.insert("_add".to_string(), Value::Map(to_add));
            }
            Update::value(Value::tree([("particles", Value::Map(particles_update))]))
        } else {
            Update::Noop
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Particle Total Mass Step ──

/// Sums sub_masses into total mass.
#[derive(Clone, Debug)]
pub struct ParticleTotalMass;

impl Step for ParticleTotalMass {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("particles".into(), Schema::map(Schema::Any)),
            ("sub_masses".into(), Schema::map(Schema::float())),
            ("total_mass".into(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("particles".into(), Schema::map(Schema::Any)),
            // Overwrite: total mass replaces, not accumulates
            ("total_mass".into(), Schema::overwrite(Schema::float())),
        ])
    }

    fn update(&self, state: &Value) -> Update {
        let map = match state.as_map() {
            Some(m) => m,
            None => return Update::Noop,
        };

        // Per-particle mode: wired inside a particle with sub_masses → sub_masses
        if let Some(sub_masses) = map.get("sub_masses").and_then(|v| v.as_map()) {
            if !sub_masses.is_empty() {
                let total: f64 = sub_masses.values().filter_map(|v| v.as_f64()).sum();
                // Output ABSOLUTE total — Overwrite schema means replacement
                return Update::value(Value::tree([
                    ("total_mass", Value::float(total)),
                ]));
            }
            return Update::Noop;
        }

        // Top-level mode: wired to the particles map
        let particles = match map.get("particles").and_then(|v| v.as_map()) {
            Some(p) => p,
            None => return Update::Noop,
        };

        let mut result: IndexMap<String, Value> = IndexMap::new();
        let mut any_changed = false;

        for (pid, particle) in particles {
            if let Some(sub_masses) = particle
                .as_map()
                .and_then(|m| m.get("sub_masses"))
                .and_then(|v| v.as_map())
            {
                if !sub_masses.is_empty() {
                    let total: f64 = sub_masses.values().filter_map(|v| v.as_f64()).sum();
                    let mut updated = particle.clone();
                    if let Some(map) = updated.as_map_mut() {
                        map.insert("mass".to_string(), Value::float(total));
                    }
                    result.insert(pid.clone(), updated);
                    any_changed = true;
                    continue;
                }
            }
            result.insert(pid.clone(), particle.clone());
        }

        if any_changed {
            Update::value(Value::tree([("particles", Value::Map(result))]))
        } else {
            Update::Noop
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Simple Box-Muller normal random number.
fn normal(rng: &mut impl Rng, sigma: f64) -> f64 {
    let u1: f64 = rng.r#gen::<f64>().max(1e-10);
    let u2: f64 = rng.r#gen::<f64>();
    sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}
