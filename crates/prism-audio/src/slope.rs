//! `Slope` — the **universal function generator** (Serge DUSG / Make Noise Maths /
//! joranalogue Contour). ONE module that is an envelope, an LFO, *or* an audio
//! oscillator depending only on how it is patched — the purest expression of Serge
//! "patch-programmability":
//!
//! - patch `trigger`, leave `cycle` off → a one-shot **AD envelope** (rise then fall);
//! - hold `cycle` on (or `cycle: true`) → it self-retriggers = an **LFO** (and at audio
//!   rate, a **VCO**);
//! - drive `time_cv` at 1V/oct → the cycling slope **tracks pitch**.
//!
//! Every time is a CV: `rise_cv` / `fall_cv` bend the two slopes, `time_cv` scales both
//! (the pitch axis) — all exponential, so positive CV = faster. Outputs (simultaneous):
//! `out` (0…1 unipolar slope), `inv` (its negation), `eoc` (a one-sample gate at the
//! end of each cycle — chain it to clock the next thing). Level + phase live on state
//! slots.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, rising_edge};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Slope {
    /// Seconds for the rising slope (0→1) at unity CV.
    pub rise_time: f64,
    /// Seconds for the falling slope (1→0) at unity CV.
    pub fall_time: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    /// Force self-cycling (an LFO) without a patched `cycle` gate.
    pub cycle: bool,
    /// Exponential depth for `rise_cv` / `fall_cv` (octaves of rate per unit).
    pub rise_depth: f64,
    pub fall_depth: f64,
    /// Exponential depth for `time_cv` — scales BOTH slopes (the pitch axis, V/oct).
    pub time_depth: f64,
    /// The `trigger` / `cycle` gate threshold.
    pub threshold: f64,
}

impl Slope {
    pub fn new(rise_time: f64, fall_time: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            rise_time,
            fall_time,
            sample_rate,
            block_size,
            cycle: false,
            rise_depth: 1.0,
            fall_depth: 1.0,
            time_depth: 1.0,
            threshold: 0.5,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            rise_time: cfg("rise").unwrap_or(0.01),
            fall_time: cfg("fall").unwrap_or(0.2),
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            cycle: config
                .get_field("cycle")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            rise_depth: cfg("rise_depth").unwrap_or(1.0),
            fall_depth: cfg("fall_depth").unwrap_or(1.0),
            time_depth: cfg("time_depth").unwrap_or(1.0),
            threshold: cfg("threshold").unwrap_or(0.5),
        }
    }

    /// Per-sample slope increment for a base time scaled by exponential CV (positive CV
    /// ⇒ shorter time ⇒ faster). Clamped so the slope always advances.
    fn rate(&self, base_time: f64, cv_octaves: f64) -> f64 {
        let t = (base_time * 2.0_f64.powf(-cv_octaves)).max(1e-5);
        1.0 / (t * self.sample_rate)
    }
}

impl Process for Slope {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("level".to_string(), Schema::float()),
            ("rising".to_string(), Schema::float()),
            ("trig_z".to_string(), Schema::float()),
            ("trigger".to_string(), signal_type()),
            ("cycle".to_string(), signal_type()),
            ("rise_cv".to_string(), signal_type()),
            ("fall_cv".to_string(), signal_type()),
            ("time_cv".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),
            ("inv".to_string(), signal_type()),
            ("eoc".to_string(), signal_type()),
            ("level".to_string(), Schema::overwrite(Schema::float())),
            ("rising".to_string(), Schema::overwrite(Schema::float())),
            ("trig_z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut level = state.get_field("level").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut rising = state.get_field("rising").and_then(|v| v.as_f64()).unwrap_or(0.0) != 0.0;
        let mut trig_z = state.get_field("trig_z").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let trigger = cv_in(state, "trigger");
        let cycle_in = cv_in(state, "cycle");
        let rise_cv = cv_in(state, "rise_cv");
        let fall_cv = cv_in(state, "fall_cv");
        let time_cv = cv_in(state, "time_cv");

        let n = self.block_size;
        let (mut out, mut inv, mut eoc) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );

        for i in 0..n {
            // A rising edge on `trigger` (re)starts the rise — the DUSG retrigger.
            let t = at(&trigger, i);
            if rising_edge(trig_z, t, self.threshold) {
                rising = true;
            }
            trig_z = t;
            let cycling = self.cycle || at(&cycle_in, i) > self.threshold;

            let time_mod = at(&time_cv, i) * self.time_depth;
            let mut eoc_now = 0.0;
            if rising {
                level += self.rate(self.rise_time, at(&rise_cv, i) * self.rise_depth + time_mod);
                if level >= 1.0 {
                    level = 1.0;
                    rising = false;
                }
            } else if level > 0.0 {
                level -= self.rate(self.fall_time, at(&fall_cv, i) * self.fall_depth + time_mod);
                if level <= 0.0 {
                    level = 0.0;
                    eoc_now = 1.0; // end-of-cycle pulse
                    if cycling {
                        rising = true;
                    }
                }
            } else if cycling {
                // At rest + cycling ⇒ self-start the next cycle (an LFO needs no trigger).
                rising = true;
            }

            out.push(level as f32);
            inv.push((-level) as f32);
            eoc.push(eoc_now as f32);
        }

        Update::value(Value::tree([
            ("out", signal_from_slice(&out)),
            ("inv", signal_from_slice(&inv)),
            ("eoc", signal_from_slice(&eoc)),
            ("level", Value::float(level)),
            ("rising", Value::float(if rising { 1.0 } else { 0.0 })),
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
