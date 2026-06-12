//! `Lpg` — a **low-pass gate** (Buchla 292 / Serge VCFS): the quintessential West-Coast
//! module. A `ping` (a trigger or gate) lights a modeled **vactrol** whose resistance
//! opens BOTH a low-pass filter AND a VCA together — and the vactrol's *asymmetric lag*
//! (fast attack, slow release) gives the natural, organic "bongo/pluck" decay for free,
//! no envelope needed. Strike it with a trigger and it sings; hold a gate and it opens.
//!
//! One control, two coupled effects (filter brightness ∝ amplitude ∝ vactrol level) is
//! exactly why an LPG sounds *acoustic* — louder is brighter, like a real resonator.
//! `input` = audio; `ping` = the control. State: the vactrol level + the filter memory.

use std::any::Any;
use std::f64::consts::TAU;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

#[derive(Clone, Debug)]
pub struct Lpg {
    /// Cutoff (Hz) when the vactrol is dark (closed) — the residual leak.
    pub base_cutoff: f64,
    /// How far the cutoff opens at full brightness (Hz added on top of base).
    pub range: f64,
    /// Vactrol attack time (s) — fast (the LED lights quickly).
    pub rise: f64,
    /// Vactrol release time (s) — slow (the LDR decays lazily = the bongo tail).
    pub fall: f64,
    pub sample_rate: f64,
    pub block_size: usize,
}

impl Lpg {
    pub fn new(sample_rate: f64, block_size: usize) -> Self {
        Self {
            base_cutoff: 40.0,
            range: 6_000.0,
            rise: 0.002,
            fall: 0.25,
            sample_rate,
            block_size,
        }
    }

    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        Self {
            base_cutoff: cfg("base_cutoff").unwrap_or(40.0),
            range: cfg("range").unwrap_or(6_000.0),
            rise: cfg("rise").unwrap_or(0.002),
            fall: cfg("fall").unwrap_or(0.25),
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
        }
    }

    /// One-pole coefficient for a time constant.
    fn coeff(&self, time: f64) -> f64 {
        1.0 - (-1.0 / (time.max(1e-5) * self.sample_rate)).exp()
    }
}

impl Process for Lpg {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("input".to_string(), signal_type()),
            ("ping".to_string(), signal_type()),
            ("vactrol".to_string(), Schema::float()),
            ("z".to_string(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),
            ("vactrol".to_string(), Schema::overwrite(Schema::float())),
            ("z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let input = state.get_field("input").map(signal_to_vec).unwrap_or_default();
        let mut vactrol = state.get_field("vactrol").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut z = state.get_field("z").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let ping = cv_in(state, "ping");

        let rise_c = self.coeff(self.rise);
        let fall_c = self.coeff(self.fall);

        let n = self.block_size;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let ctrl = at(&ping, i).clamp(0.0, 1.0);
            // Asymmetric vactrol lag: chase up fast, fall away slow — the bongo decay.
            let c = if ctrl > vactrol { rise_c } else { fall_c };
            vactrol += c * (ctrl - vactrol);

            // The vactrol opens the low-pass AND scales the VCA — coupled, the LPG signature.
            let fc = self.base_cutoff + vactrol * self.range;
            let g = 1.0 - (-TAU * fc / self.sample_rate).exp();
            let x = input.get(i).copied().unwrap_or(0.0) as f64;
            z += g * (x - z);
            out.push((z * vactrol) as f32);
        }

        Update::value(Value::tree([
            ("out", signal_from_slice(&out)),
            ("vactrol", Value::float(vactrol)),
            ("z", Value::float(z)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
