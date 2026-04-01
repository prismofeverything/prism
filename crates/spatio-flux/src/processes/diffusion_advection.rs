//! 2D diffusion-advection process on a rectangular grid.
//!
//! Uses explicit finite-difference Laplacian with adaptive sub-stepping
//! for stability. Supports boundary conditions: periodic, dirichlet,
//! neumann (zero-gradient), and outflow.

use std::any::Any;

use indexmap::IndexMap;

use prism_bigraph::{Process, Schema, Update, Value};

/// Boundary condition types.
#[derive(Clone, Debug, PartialEq)]
pub enum BoundaryCondition {
    Periodic,
    Dirichlet(f64),
    Neumann, // zero-gradient
    Outflow,
}

impl Default for BoundaryCondition {
    fn default() -> Self {
        Self::Neumann
    }
}

/// Boundary conditions for all four sides.
#[derive(Clone, Debug, Default)]
pub struct Boundaries {
    pub left: BoundaryCondition,
    pub right: BoundaryCondition,
    pub top: BoundaryCondition,
    pub bottom: BoundaryCondition,
}

/// 2D diffusion-advection process.
///
/// Fields are stored as flat Vec<f64> in row-major order (ny rows × nx cols).
/// Convention: n_bins = (nx, ny), array shape = (ny, nx), index = [y * nx + x].
#[derive(Clone, Debug)]
pub struct DiffusionAdvection {
    pub n_bins: (usize, usize), // (nx, ny)
    pub bounds: (f64, f64),     // (width, height) in micrometers
    pub diffusion_coeffs: IndexMap<String, f64>,
    pub advection_coeffs: IndexMap<String, (f64, f64)>, // (vx, vy)
    pub boundary_conditions: IndexMap<String, Boundaries>,
    pub default_diffusion: f64,
    pub interval: f64,
}

impl DiffusionAdvection {
    pub fn new(
        n_bins: (usize, usize),
        bounds: (f64, f64),
        diffusion_coeffs: IndexMap<String, f64>,
        interval: f64,
    ) -> Self {
        Self {
            n_bins,
            bounds,
            diffusion_coeffs,
            advection_coeffs: IndexMap::new(),
            boundary_conditions: IndexMap::new(),
            default_diffusion: 1e-1,
            interval,
        }
    }

    fn dx(&self) -> f64 {
        self.bounds.0 / self.n_bins.0 as f64
    }

    fn dy(&self) -> f64 {
        self.bounds.1 / self.n_bins.1 as f64
    }

    /// Compute one diffusion step on a 2D field.
    fn diffuse(&self, field: &[f64], d: f64, dt: f64, bc: &Boundaries) -> Vec<f64> {
        let (nx, ny) = self.n_bins;
        let dx = self.dx();
        let dy = self.dy();
        let mut result = field.to_vec();

        for y in 0..ny {
            for x in 0..nx {
                let idx = y * nx + x;
                let c = field[idx];

                // Neighbors with boundary conditions
                let left = self.get_neighbor(field, x, y, -1, 0, bc, nx, ny);
                let right = self.get_neighbor(field, x, y, 1, 0, bc, nx, ny);
                let down = self.get_neighbor(field, x, y, 0, -1, bc, nx, ny);
                let up = self.get_neighbor(field, x, y, 0, 1, bc, nx, ny);

                let laplacian =
                    (left - 2.0 * c + right) / (dx * dx) + (down - 2.0 * c + up) / (dy * dy);

                result[idx] = c + d * laplacian * dt;
                if result[idx] < 0.0 {
                    result[idx] = 0.0;
                }
            }
        }

        result
    }

    fn get_neighbor(
        &self,
        field: &[f64],
        x: usize,
        y: usize,
        dx: i32,
        dy: i32,
        bc: &Boundaries,
        nx: usize,
        ny: usize,
    ) -> f64 {
        let new_x = x as i32 + dx;
        let new_y = y as i32 + dy;

        // Check boundaries
        if new_x < 0 {
            return match &bc.left {
                BoundaryCondition::Periodic => field[y * nx + (nx - 1)],
                BoundaryCondition::Dirichlet(v) => *v,
                BoundaryCondition::Neumann | BoundaryCondition::Outflow => field[y * nx + x],
            };
        }
        if new_x >= nx as i32 {
            return match &bc.right {
                BoundaryCondition::Periodic => field[y * nx],
                BoundaryCondition::Dirichlet(v) => *v,
                BoundaryCondition::Neumann | BoundaryCondition::Outflow => field[y * nx + x],
            };
        }
        if new_y < 0 {
            return match &bc.bottom {
                BoundaryCondition::Periodic => field[(ny - 1) * nx + x],
                BoundaryCondition::Dirichlet(v) => *v,
                BoundaryCondition::Neumann | BoundaryCondition::Outflow => field[y * nx + x],
            };
        }
        if new_y >= ny as i32 {
            return match &bc.top {
                BoundaryCondition::Periodic => field[x],
                BoundaryCondition::Dirichlet(v) => *v,
                BoundaryCondition::Neumann | BoundaryCondition::Outflow => field[y * nx + x],
            };
        }

        field[new_y as usize * nx + new_x as usize]
    }

    /// Maximum stable timestep for explicit diffusion.
    fn max_stable_dt(&self, d: f64) -> f64 {
        let dx = self.dx();
        let dy = self.dy();
        let min_dx = dx.min(dy);
        min_dx * min_dx / (4.0 * d)
    }
}

impl Process for DiffusionAdvection {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("fields".to_string(), Schema::map(Schema::list(Schema::float())))])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        // Use Any so the engine uses the state schema (map[array[...]]) for apply
        IndexMap::from([("fields".to_string(), Schema::Any)])
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let fields = match state
            .as_map()
            .and_then(|m| m.get("fields"))
            .and_then(|v| v.as_map())
        {
            Some(f) => f,
            None => return Update::Noop,
        };

        let mut result_fields: IndexMap<String, Value> = IndexMap::new();

        let (nx, ny) = self.n_bins;
        let expected_size = nx * ny;

        for (mol_id, field_val) in fields {
            let field = super::fields::flatten_field(field_val);
            if field.len() < expected_size {
                // Scalar or wrong-sized field — pass through unchanged
                result_fields.insert(mol_id.clone(), field_val.clone());
                continue;
            }

            let d = self
                .diffusion_coeffs
                .get(mol_id)
                .copied()
                .unwrap_or(self.default_diffusion);

            let bc = self
                .boundary_conditions
                .get(mol_id)
                .cloned()
                .unwrap_or_default();

            // Adaptive sub-stepping for stability
            let max_dt = self.max_stable_dt(d);
            let n_steps = (interval / max_dt).ceil() as usize;
            let dt = interval / n_steps as f64;

            let original = field.clone();
            let mut current = field;
            for _ in 0..n_steps {
                current = self.diffuse(&current, d, dt, &bc);
            }

            // Apply advection if configured
            if let Some(&(vx, vy)) = self.advection_coeffs.get(mol_id) {
                current = self.advect(&current, vx, vy, interval);
            }

            // Output DELTA (cur - original), matching Python's diffusion process.
            // Applied element-wise via Array schema.
            let delta: Vec<f64> = current.iter().zip(original.iter())
                .map(|(c, o)| c - o)
                .collect();
            result_fields.insert(
                mol_id.clone(),
                super::fields::rebuild_field(&delta, field_val),
            );
        }

        Update::value(Value::tree([("fields", Value::Map(result_fields))]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DiffusionAdvection {
    /// Simple upwind advection.
    fn advect(&self, field: &[f64], vx: f64, vy: f64, dt: f64) -> Vec<f64> {
        let (nx, ny) = self.n_bins;
        let dx = self.dx();
        let dy = self.dy();
        let mut result = field.to_vec();

        for y in 0..ny {
            for x in 0..nx {
                let idx = y * nx + x;

                // Upwind scheme
                let dc_dx = if vx > 0.0 && x > 0 {
                    (field[idx] - field[y * nx + (x - 1)]) / dx
                } else if vx < 0.0 && x < nx - 1 {
                    (field[y * nx + (x + 1)] - field[idx]) / dx
                } else {
                    0.0
                };

                let dc_dy = if vy > 0.0 && y > 0 {
                    (field[idx] - field[(y - 1) * nx + x]) / dy
                } else if vy < 0.0 && y < ny - 1 {
                    (field[(y + 1) * nx + x] - field[idx]) / dy
                } else {
                    0.0
                };

                result[idx] = field[idx] - dt * (vx * dc_dx + vy * dc_dy);
                if result[idx] < 0.0 {
                    result[idx] = 0.0;
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diffusion_conserves_mass() {
        let (nx, ny) = (10, 10);
        let n = nx * ny;

        // Spike in center
        let mut field = vec![0.0; n];
        field[5 * nx + 5] = 100.0;
        let initial_mass: f64 = field.iter().sum();

        let proc = DiffusionAdvection::new(
            (nx, ny),
            (10.0, 10.0),
            IndexMap::from([("test".into(), 0.1)]),
            1.0,
        );

        let state = Value::tree([(
            "fields",
            Value::tree([(
                "test",
                Value::List(field.into_iter().map(Value::float).collect()),
            )]),
        )]);

        let update = proc.update(&state, 1.0);
        let result = update.into_value().unwrap();
        let result_field: Vec<f64> = result
            .get_path(&["fields".into(), "test".into()])
            .unwrap()
            .as_list()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_f64())
            .collect();

        let final_mass: f64 = result_field.iter().sum();
        // With Neumann BCs, mass should be approximately conserved
        assert!(
            (initial_mass - final_mass).abs() < 1.0,
            "Mass not conserved: {initial_mass} -> {final_mass}"
        );
    }
}
