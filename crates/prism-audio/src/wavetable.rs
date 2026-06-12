//! `Wavetable` — an oscillator that **morphs** continuously through a bank of waveshapes
//! by a `pos` CV: 0 = sine, 1 = triangle, 2 = saw, 3 = pulse, interpolated between. Sweep
//! `pos_cv` for an evolving timbre (the digital complex-oscillator axis). `fm_exp` (V/oct)
//! and `am` modulate it like any [`Oscillator`](crate::oscillator). State: the phase.

use std::any::Any;
use std::f64::consts::TAU;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Wavetable {
    freq_hz: f64,
    sample_rate: f64,
    block_size: usize,
    pos: f64,
    pos_depth: f64,
    fm_depth: f64,
    amplitude: f64,
    am_depth: f64,
}

impl Wavetable {
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str, d: f64| config.get_field(k).and_then(|v| v.as_f64()).unwrap_or(d);
        Self {
            freq_hz: cfg("freq", 220.0),
            sample_rate: cfg("sample_rate", 48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            pos: cfg("pos", 0.0),
            pos_depth: cfg("pos_depth", 1.0),
            fm_depth: cfg("fm_depth", 1.0),
            amplitude: cfg("amplitude", 1.0),
            am_depth: cfg("am_depth", 1.0),
        }
    }
}

/// The four bank waveforms at a phase fraction `f ∈ [0,1)`.
fn bank(f: f64) -> [f64; 4] {
    let sine = (TAU * f).sin();
    let tri = if f < 0.5 { 4.0 * f - 1.0 } else { 3.0 - 4.0 * f };
    let saw = 2.0 * f - 1.0;
    let pulse = if f < 0.5 { 1.0 } else { -1.0 };
    [sine, tri, saw, pulse]
}

impl Process for Wavetable {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("phase".to_string(), Schema::float()),
            ("fm_exp".to_string(), signal_type()),
            ("pos_cv".to_string(), signal_type()),
            ("am".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("out".to_string(), signal_type()),
            ("phase".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut phase = state.get_field("phase").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let fm_exp = cv_in(state, "fm_exp");
        let pos_cv = cv_in(state, "pos_cv");
        let am = cv_in(state, "am");

        let mut out = Vec::with_capacity(self.block_size);
        for i in 0..self.block_size {
            let f = self.freq_hz * 2.0_f64.powf(at(&fm_exp, i) * self.fm_depth);
            let frac = phase - phase.floor();
            let waves = bank(frac);
            let pos = (self.pos + at(&pos_cv, i) * self.pos_depth).clamp(0.0, 3.0);
            let i0 = pos.floor() as usize;
            let i1 = (i0 + 1).min(3);
            let t = pos - i0 as f64;
            let w = waves[i0] * (1.0 - t) + waves[i1] * t;
            let amp = self.amplitude + at(&am, i) * self.am_depth;
            out.push((amp * w) as f32);
            phase += f / self.sample_rate;
        }
        let mut next = phase.fract();
        if next < 0.0 {
            next += 1.0;
        }

        Update::value(Value::tree([
            ("out", signal_from_slice(&out)),
            ("phase", Value::float(next)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
