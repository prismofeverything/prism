//! `SampleHold` — sample & hold + slew, the Serge **SSG** (Smooth & Stepped
//! Generator). Two faces of one idea: capture `input` on a `trigger` edge and HOLD it
//! (`stepped`), or continuously SLEW toward `input` (`smooth`). Patch noise → `input` +
//! a clock → `trigger` and `stepped` is the classic **random voltage** source; patch a
//! stepped CV → `input` with no trigger and `smooth` is a **slew limiter / glide**.
//!
//! The slew rate is a CV (`slew_cv`, exponential): 0 ⇒ instant (stepped), large ⇒ lazy
//! glides. Outputs `out` (= stepped), `stepped`, `smooth`. State: the held value, the
//! smoothing accumulator, and the trigger edge level.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, rising_edge};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct SampleHold {
    pub sample_rate: f64,
    pub block_size: usize,
    /// Base slew time in seconds (the glide for `smooth`); `slew_cv` scales it.
    pub slew_time: f64,
    pub slew_depth: f64,
    pub threshold: f64,
}

impl SampleHold {
    pub fn new(slew_time: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            sample_rate,
            block_size,
            slew_time,
            slew_depth: 1.0,
            threshold: 0.5,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            slew_time: cfg("slew").unwrap_or(0.02),
            slew_depth: cfg("slew_depth").unwrap_or(1.0),
            threshold: cfg("threshold").unwrap_or(0.5),
        }
    }
}

impl Process for SampleHold {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("held".to_string(), Schema::float()),
            ("smooth_v".to_string(), Schema::float()),
            ("trig_z".to_string(), Schema::float()),
            ("input".to_string(), signal_type()),
            ("trigger".to_string(), signal_type()),
            ("slew_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),
            ("stepped".to_string(), signal_type()),
            ("smooth".to_string(), signal_type()),
            ("held".to_string(), Schema::overwrite(Schema::float())),
            ("smooth_v".to_string(), Schema::overwrite(Schema::float())),
            ("trig_z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut held = state.get_field("held").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut smooth_v = state.get_field("smooth_v").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut trig_z = state.get_field("trig_z").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let input = cv_in(state, "input");
        let trigger = cv_in(state, "trigger");
        let slew_cv = cv_in(state, "slew_cv");

        let n = self.block_size;
        let (mut stepped, mut smooth) = (Vec::with_capacity(n), Vec::with_capacity(n));

        for i in 0..n {
            let x = at(&input, i);
            // Sample on a rising edge of `trigger` (the stepped / S&H face).
            let t = at(&trigger, i);
            if rising_edge(trig_z, t, self.threshold) {
                held = x;
            }
            trig_z = t;

            // Slew the smooth accumulator toward the live input (the smooth face). A
            // one-pole step whose coefficient is set by the (CV-scaled) slew time.
            let slew_t = (self.slew_time * 2.0_f64.powf(-at(&slew_cv, i) * self.slew_depth)).max(1e-5);
            let a = (1.0 / (slew_t * self.sample_rate)).min(1.0);
            smooth_v += a * (x - smooth_v);

            stepped.push(held as f32);
            smooth.push(smooth_v as f32);
        }

        Update::value(Value::tree([
            ("out", signal_from_slice(&stepped)),
            ("stepped", signal_from_slice(&stepped)),
            ("smooth", signal_from_slice(&smooth)),
            ("held", Value::float(held)),
            ("smooth_v", Value::float(smooth_v)),
            ("trig_z", Value::float(trig_z)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
