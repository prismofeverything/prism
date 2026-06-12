//! `Chaos` — a **Lorenz strange attractor**: three coupled nonlinear ODEs whose state
//! `(x, y, z)` orbits forever without repeating (deterministic chaos). A chaos SOURCE in
//! the Schlappi Three Body spirit: at a slow `rate` it is organic, never-looping CV
//! modulation; cranked up (audio rate) it is a gnarly drone. Three simultaneous outputs
//! `x` / `y` / `z` (scaled to ~`[-1, 1]`); `rate` (× `rate_cv`, exponential) sets the
//! integration speed. State: the raw attractor coordinates.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Chaos {
    /// Speed knob (1.0 ≈ a fast-ish CV wander; raise for audio-rate chaos).
    pub rate: f64,
    pub sigma: f64,
    pub rho: f64,
    pub beta: f64,
    pub block_size: usize,
    pub rate_depth: f64,
}

impl Chaos {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        Self {
            rate: cfg("rate", 1.0),
            sigma: cfg("sigma", 10.0),
            rho: cfg("rho", 28.0),
            beta: cfg("beta", 8.0 / 3.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            rate_depth: cfg("rate_depth", 1.0),
        }
    }
}

impl Process for Chaos {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("cx".to_string(), Schema::float()),
            ("cy".to_string(), Schema::float()),
            ("cz".to_string(), Schema::float()),
            ("rate_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("x".to_string(), signal_type()),
            ("y".to_string(), signal_type()),
            ("z".to_string(), signal_type()),
            ("cx".to_string(), Schema::overwrite(Schema::float())),
            ("cy".to_string(), Schema::overwrite(Schema::float())),
            ("cz".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let f = |k: &str| state.get_field(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let (mut cx, mut cy, mut cz) = (f("cx"), f("cy"), f("cz"));
        // Kick off the origin (the attractor is fixed at 0,0,0).
        if cx == 0.0 && cy == 0.0 && cz == 0.0 {
            cx = 0.1;
        }
        let rate_cv = cv_in(state, "rate_cv");

        let n = self.block_size;
        let (mut xs, mut ys, mut zs) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        for i in 0..n {
            // dt per sample (Euler); clamped for stability.
            let dt = (0.0002 * self.rate * 2.0_f64.powf(at(&rate_cv, i) * self.rate_depth))
                .clamp(1e-7, 0.02);
            let dx = self.sigma * (cy - cx);
            let dy = cx * (self.rho - cz) - cy;
            let dz = cx * cy - self.beta * cz;
            cx += dx * dt;
            cy += dy * dt;
            cz += dz * dt;
            xs.push((cx / 20.0) as f32);
            ys.push((cy / 25.0) as f32);
            zs.push(((cz - 25.0) / 25.0) as f32);
        }

        Update::value(Value::tree([
            ("x", signal_from_slice(&xs)),
            ("y", signal_from_slice(&ys)),
            ("z", signal_from_slice(&zs)),
            ("cx", Value::float(cx)),
            ("cy", Value::float(cy)),
            ("cz", Value::float(cz)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
