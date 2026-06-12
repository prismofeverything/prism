//! `Oscillator` — a **complex, fully-modulatable VCO** (joranalogue Generate /
//! Schlappi Three Body lineage). One audio block per tick (block-rate scheduling:
//! `interval = block / sample_rate`, so logical time advances at audio time).
//!
//! Every parameter is a **modulation input** (a `Signal` port — CV ≡ audio), so
//! anything patches anything. Inputs (all optional; unpatched = no modulation):
//! - `fm_exp` — exponential / V-oct pitch CV (1.0 = +1 octave), summed in the exponent
//! - `fm_lin` — linear / through-zero FM (Hz)
//! - `pm` — phase modulation (cycles) — for PM/FM timbres
//! - `sync` — hard sync (a rising edge resets phase)
//! - `pwm` — pulse-width modulation of the `square` output
//! - `am` — amplitude CV, summed around the base level
//!
//! …and it presents ALL waveshapes at once (like Generate's simultaneous outputs):
//! `sine` / `saw` / `square` / `triangle` / `sub` (an octave-down square), plus the
//! `wave`-selected `out` (so existing single-output patches are unchanged). Phase (and
//! the sync edge-detector's carried level) live on state slots — a `Process` owns its
//! own memory.

use std::any::Any;
use std::f64::consts::TAU;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, modulated_freq, rising_edge};
use crate::signal::{signal_from_slice, signal_type};
use crate::wave::Wave;

/// A complex oscillator. `wave`/`freq`/`amplitude` are the BASE (the knob); the rest
/// are attenuverter depths for the CV inputs. All construction-time config; the
/// evolving `phase` is state.
#[derive(Clone, Debug)]
pub struct Oscillator {
    pub wave: Wave,
    pub freq_hz: f64,
    pub sample_rate: f64,
    pub block_size: usize,
    pub amplitude: f64,
    /// Exponential FM depth — octaves per unit of `fm_exp` CV (default 1.0 = V/oct).
    pub fm_exp_depth: f64,
    /// Linear/through-zero FM depth — Hz per unit of `fm_lin` CV (default = `freq`).
    pub fm_lin_depth: f64,
    /// Phase-modulation depth — cycles per unit of `pm` CV.
    pub pm_depth: f64,
    /// Amplitude-CV depth — level per unit of `am` CV.
    pub am_depth: f64,
    /// Pulse-width-modulation depth for the `square` output (around 50%).
    pub pwm_depth: f64,
    /// The `sync` rising-edge threshold (a gate must cross this to reset phase).
    pub sync_threshold: f64,
}

impl Oscillator {
    pub fn new(wave: Wave, freq_hz: f64, sample_rate: f64, block_size: usize) -> Self {
        Self {
            wave,
            freq_hz,
            sample_rate,
            block_size,
            amplitude: 1.0,
            fm_exp_depth: 1.0,
            fm_lin_depth: freq_hz,
            pm_depth: 1.0,
            am_depth: 1.0,
            pwm_depth: 0.49,
            sync_threshold: 0.5,
        }
    }

    pub fn with_amplitude(mut self, amplitude: f64) -> Self {
        self.amplitude = amplitude;
        self
    }

    /// Build from a patch node's `config` map (the homoiconic / discovery path). Bases
    /// match the historical names (`wave`/`freq`/`amplitude`); the `*_depth` knobs take
    /// musical defaults so a patched CV input does something sensible unset.
    pub fn from_config(config: &Value) -> Self {
        let cfg = |k: &str| config.get_field(k).and_then(|v| v.as_f64());
        let freq = cfg("freq").unwrap_or(440.0);
        Self {
            wave: config
                .get_field("wave")
                .and_then(|v| v.as_str())
                .map(Wave::parse)
                .unwrap_or(Wave::Sine),
            freq_hz: freq,
            sample_rate: cfg("sample_rate").unwrap_or(48_000.0),
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            amplitude: cfg("amplitude").unwrap_or(1.0),
            fm_exp_depth: cfg("fm_depth").unwrap_or(1.0),
            fm_lin_depth: cfg("fm_lin_depth").unwrap_or(freq),
            pm_depth: cfg("pm_depth").unwrap_or(1.0),
            am_depth: cfg("am_depth").unwrap_or(1.0),
            pwm_depth: cfg("pwm_depth").unwrap_or(0.49),
            sync_threshold: cfg("sync_threshold").unwrap_or(0.5),
        }
    }
}

impl Process for Oscillator {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("phase".to_string(), Schema::float()),
            ("sync_z".to_string(), Schema::float()),
            // The modulation inputs — CV ≡ audio. Unpatched = silence = no modulation.
            ("fm_exp".to_string(), signal_type()),
            ("fm_lin".to_string(), signal_type()),
            ("pm".to_string(), signal_type()),
            ("sync".to_string(), signal_type()),
            ("pwm".to_string(), signal_type()),
            ("am".to_string(), signal_type()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("phase".to_string(), Schema::overwrite(Schema::float())),
            ("sync_z".to_string(), Schema::overwrite(Schema::float())),
            // Every shape, simultaneously (joranalogue Generate / Three Body) + the
            // `wave`-selected `out` (single-output patches unchanged).
            ("out".to_string(), signal_type()),
            ("sine".to_string(), signal_type()),
            ("saw".to_string(), signal_type()),
            ("square".to_string(), signal_type()),
            ("triangle".to_string(), signal_type()),
            ("sub".to_string(), signal_type()),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / self.sample_rate
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let mut phase = state.get_field("phase").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut sync_z = state.get_field("sync_z").and_then(|v| v.as_f64()).unwrap_or(0.0);

        // Pull the modulation blocks once (each is silence if unpatched).
        let fm_exp = cv_in(state, "fm_exp");
        let fm_lin = cv_in(state, "fm_lin");
        let pm = cv_in(state, "pm");
        let sync = cv_in(state, "sync");
        let pwm = cv_in(state, "pwm");
        let am = cv_in(state, "am");

        let n = self.block_size;
        let (mut sine, mut saw, mut square, mut triangle, mut sub, mut out) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );

        for i in 0..n {
            // Hard sync — a rising edge on `sync` resets the phase (the classic
            // sync-oscillator timbre; Three Body cross-modulates with this).
            let s = at(&sync, i);
            if rising_edge(sync_z, s, self.sync_threshold) {
                phase = 0.0;
            }
            sync_z = s;

            // Frequency: exponential pitch CV (octaves) + linear/through-zero FM (Hz).
            let f = modulated_freq(
                self.freq_hz,
                at(&fm_exp, i) * self.fm_exp_depth,
                at(&fm_lin, i) * self.fm_lin_depth,
            );
            let inc = f / self.sample_rate;

            // The read phase is phase-modulated; the integrator phase is not (so PM
            // does not accumulate — true phase modulation).
            let read = phase + at(&pm, i) * self.pm_depth;
            let frac = read - read.floor();
            let amp = self.amplitude + at(&am, i) * self.am_depth;
            let width = (0.5 + at(&pwm, i) * self.pwm_depth).clamp(0.01, 0.99);
            let sub_frac = (read * 0.5) - (read * 0.5).floor();

            let s_sine = amp * (TAU * frac).sin();
            let s_saw = amp * (2.0 * frac - 1.0);
            let s_square = amp * if frac < width { 1.0 } else { -1.0 };
            let s_tri = amp * if frac < 0.5 { 4.0 * frac - 1.0 } else { 3.0 - 4.0 * frac };
            let s_sub = amp * if sub_frac < 0.5 { 1.0 } else { -1.0 };

            out.push(match self.wave {
                Wave::Sine => s_sine,
                Wave::Saw => s_saw,
                Wave::Square => s_square,
                Wave::Triangle => s_tri,
            } as f32);
            sine.push(s_sine as f32);
            saw.push(s_saw as f32);
            square.push(s_square as f32);
            triangle.push(s_tri as f32);
            sub.push(s_sub as f32);

            phase += inc; // per-sample integration (so FM tracks within the block)
        }

        // Keep the integrator phase bounded over long renders.
        let mut next_phase = phase.fract();
        if next_phase < 0.0 {
            next_phase += 1.0;
        }

        Update::value(Value::tree([
            ("phase", Value::float(next_phase)),
            ("sync_z", Value::float(sync_z)),
            ("out", signal_from_slice(&out)),
            ("sine", signal_from_slice(&sine)),
            ("saw", signal_from_slice(&saw)),
            ("square", signal_from_slice(&square)),
            ("triangle", signal_from_slice(&triangle)),
            ("sub", signal_from_slice(&sub)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
