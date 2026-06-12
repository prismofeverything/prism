//! prism-audio — the audio / modular-synthesis layer for prism.
//!
//! Design: `docs/synthesis-bigraphs.md`. A synthesizer patch IS a bigraph —
//! a module is a process with signal ports, a patch cable is a link, a voice
//! is a composite — so generative synthesis is a BRS rewriting that bigraph.
//!
//! **A1 (this slice)** is the substrate, proven *offline* in logical time:
//! the [`Signal`](signal) block value, an [`Oscillator`](oscillator) process,
//! and offline [`render`](render)ing to a buffer / WAV — no audio device, no
//! wall-clock, fully deterministic under `cargo test` (synthesis-bigraphs.md
//! §II/§X). Later slices add the realtime device boundary (gorgon's lock-free
//! ring, A3) and the distributed `net:` protocol (A8); the *patch* is
//! unchanged — only the sink differs.

pub mod audioin;
pub mod audioout;
pub mod chaos;
pub mod clock;
pub mod clockdiv;
pub mod comb;
pub mod device;
pub mod envelope;
pub mod factory;
pub mod instrument;
pub mod compare;
pub mod counter;
pub mod delay;
pub mod fold;
pub mod ladder;
pub mod logic;
pub mod lowpass;
pub mod lpg;
pub mod matrix;
pub mod mix;
/// The universal modulation layer — CV ≡ audio; every parameter a `Signal` input.
pub mod modulation;
pub mod noise;
pub mod oscillator;
pub mod pan;
pub mod quantizer;
pub mod ringmod;
pub mod samplehold;
pub mod sequencer;
pub mod shaper;
pub mod slope;
pub mod patch;
/// The audio domain's runnable Core (`audio_core`, chrysalis-free) + the `.ys`
/// language facet (`audio_modules`, behind `ys`).
pub mod prelude;
pub mod reactions;
pub mod render;
pub mod signal;
pub mod svf;
pub mod vca;
pub mod wavetable;
pub mod voice;
pub mod wav;
pub mod wave;

pub use device::interleave;
#[cfg(feature = "realtime")]
pub use device::{run_realtime, RealtimeOpts};
pub use envelope::Envelope;
pub use factory::{stack_factory, stack_voice_node, StackRecipe};
pub use instrument::{cast_instrument, define_instrument, instrument_voice_node, InstrumentRecipe};
pub use lowpass::LowPass;
pub use oscillator::Oscillator;
pub use patch::{module_node, register_audio, render_patch, wire_map};
pub use reactions::{
    audio_engine, detune_voice, patch_brs_node, prune_oscillator, render_patch_brs, spawn_voice,
    PATCH_BRS,
};
pub use render::{
    render, render_kernel, render_mix, render_oscillator, render_path, render_voice, tick_block,
};
pub use voice::voice_composite_node;
pub use signal::{
    register_signal, signal_from_slice, signal_registry, signal_schema, signal_to_vec, signal_type,
    silence, SIGNAL,
};
pub use audioin::AudioIn;
pub use audioout::AudioOut;
pub use chaos::Chaos;
pub use clock::Clock;
pub use clockdiv::ClockDiv;
pub use comb::Comb;
pub use compare::Compare;
pub use counter::Counter;
pub use delay::Delay;
pub use fold::Fold;
pub use ladder::Ladder;
pub use logic::Logic;
pub use lpg::Lpg;
pub use matrix::Matrix;
pub use mix::Mix;
pub use noise::Noise;
pub use pan::Pan;
pub use quantizer::Quantizer;
pub use ringmod::RingMod;
pub use samplehold::SampleHold;
pub use sequencer::Sequencer;
pub use shaper::Shaper;
pub use slope::Slope;
pub use svf::Svf;
pub use vca::Vca;
pub use wavetable::Wavetable;
pub use wav::write_wav_i16;
pub use wave::Wave;
