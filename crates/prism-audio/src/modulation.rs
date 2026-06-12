//! The universal modulation layer — **CV ≡ audio**. There is ONE signal language
//! (`Signal`, a block of `f32`); a control voltage and an audio sample are the same
//! thing, so anything can modulate anything (the modular-synth ethos — joranalogue
//! patches at audio rate, Serge is "patch-programmable", Schlappi cross-modulates
//! everything). A module exposes a **plethora of modulation inputs**, each a `Signal`
//! port; a parameter is `base (knob) + Σ cv·depth (attenuverted CV in)`.
//!
//! An UNPATCHED input reads as silence (`0`), so `base + cv·depth` falls back to the
//! base — exactly a hardware jack normalled to 0 (knob-only when nothing is patched).
//! Modules therefore declare rich inputs for free: they cost nothing until patched.
//!
//! Conventions for the bipolar `[-1, 1]`-ish DC-coupled `Signal`:
//! - **pitch CV is exponential, 1.0 = +1 octave** (V/oct), summed in the exponent;
//! - **linear / through-zero FM** is added in Hz after the exponential;
//! - **level / offset CV** is added linearly around the base;
//! - **gates / triggers / sync** are rising edges through a threshold (default 0.5).

use prism_bigraph::Value;

use crate::signal::signal_to_vec;

/// Read a modulation input port as a sample block. An unpatched (absent) input reads
/// as an empty block ≡ silence, so [`at`] returns `0` for it — the "normalled to 0"
/// fallback that lets a module carry many inputs at no cost when none are patched.
pub fn cv_in(state: &Value, port: &str) -> Vec<f32> {
    state
        .get_field(port)
        .map(signal_to_vec)
        .unwrap_or_default()
}

/// Sample a CV block at index `i`, or `0` past its end (an unpatched / shorter input).
/// The single point where "unpatched = no modulation" is realized.
#[inline]
pub fn at(block: &[f32], i: usize) -> f64 {
    block.get(i).copied().unwrap_or(0.0) as f64
}

/// A rising-edge detector across a sample and the carried previous level — the basis
/// of gates / triggers / hard-sync. Returns whether `cur` crossed UP through
/// `threshold` since `prev`. The caller carries `prev` (the last sample) on a state
/// slot so edges are detected across block boundaries too.
#[inline]
pub fn rising_edge(prev: f64, cur: f64, threshold: f64) -> bool {
    prev < threshold && cur >= threshold
}

/// The exponential (V/oct) + linear contribution to a frequency from its modulation
/// inputs at sample `i`: `base · 2^(Σ exp·depth) + Σ lin·depth`. `exp` are octave-wise
/// pitch CVs, `lin` are Hz (through-zero) FM CVs — the standard complex-oscillator
/// frequency law (joranalogue Generate, Schlappi Three Body).
#[inline]
pub fn modulated_freq(base: f64, exp_octaves: f64, lin_hz: f64) -> f64 {
    base * 2.0_f64.powf(exp_octaves) + lin_hz
}
