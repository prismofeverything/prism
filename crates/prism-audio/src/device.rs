//! The audio device boundary (A3) — the one wall-clock ↔ logical-time seam.
//!
//! prism's engine is logical-time (one BSP tick = one audio block); a cpal device
//! is hard-realtime. They meet at a lock-free ring (gorgon's seam): the engine
//! runs AHEAD on a normal-priority thread, topping up an output ring; the cpal
//! callback, on the realtime thread, only drains it, filling silence on underrun
//! (`synthesis-bigraphs.md` §II). The ring is the only shared state — never put a
//! lock on the callback's path.
//!
//! The offline render path ([`crate::render`]) is unchanged — this is a second,
//! additive sink for the *same* patch. [`interleave`] and the ring round-trip are
//! pure Rust (always built + tested in `tests/device.rs`); the cpal driver
//! [`run_realtime`] is behind the `realtime` feature, which pulls gorgon + cpal
//! (system audio libs).

/// Interleave a mono `Signal` block to `channels` — the same sample on every
/// channel, the layout a cpal output callback drains. (Per-voice multichannel
/// routing — the ES-9's 16 outs — is a later refinement, A8.)
pub fn interleave(mono: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let mut out = Vec::with_capacity(mono.len() * ch);
    for &s in mono {
        for _ in 0..ch {
            out.push(s);
        }
    }
    out
}

#[cfg(feature = "realtime")]
mod realtime {
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, SampleRate, StreamConfig};
    use ringbuf::traits::{Observer, Producer, Split};
    use ringbuf::HeapRb;

    use prism_bigraph::Engine;

    use crate::render::tick_block;

    /// How to play: which output device, for how long, at what block / rate.
    pub struct RealtimeOpts {
        /// Output device name substring (`None` = system default).
        pub device: Option<String>,
        /// How many seconds of audio to produce.
        pub seconds: f64,
        /// Samples per block (one engine tick).
        pub block: usize,
        /// Sample rate — must match the patch's oscillators.
        pub sample_rate: f64,
    }

    /// Play a live patch through the speakers: open the output device via gorgon,
    /// run `engine` ahead of the device on this (normal-priority) thread, and top
    /// up a lock-free ring that the cpal callback drains. Back-pressured by the
    /// ring — when it is full we yield and the device paces us; underrun →
    /// silence (graceful). `out_path` is the patch's output bus (e.g.
    /// `["rack", "mix"]`). Returns once `opts.seconds` of audio has been produced
    /// and the ring has drained.
    pub fn run_realtime(
        mut engine: Engine,
        out_path: &[&str],
        opts: RealtimeOpts,
    ) -> anyhow::Result<()> {
        let device = gorgon::audio::find_output_device(opts.device.as_deref())?;
        // Mono source → up to 2 channels (the ES-9 fan-out is A8).
        let channels = gorgon::audio::max_output_channels(&device)?.clamp(1, 2);
        let config = StreamConfig {
            channels,
            sample_rate: SampleRate(opts.sample_rate as u32),
            buffer_size: BufferSize::Default,
        };

        // ~200 ms of ring slack so the engine can run ahead of the device.
        let ring_frames = (opts.sample_rate * 0.2) as usize;
        let (mut prod, cons) = HeapRb::<f32>::new(ring_frames * channels as usize).split();

        let stream = gorgon::audio::build_output_stream(&device, &config, cons)?;
        stream.play()?;

        let total_blocks = (opts.seconds * opts.sample_rate / opts.block as f64).ceil() as usize;
        let need = opts.block * channels as usize;
        let mut produced = 0usize;
        while produced < total_blocks {
            if prod.vacant_len() >= need {
                let block = super::interleave(&tick_block(&mut engine, out_path), channels);
                prod.push_slice(&block);
                produced += 1;
            } else {
                // Ring full — the device drains at its own rate; wait for room.
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
        // Let the tail of the ring play out before the stream is dropped.
        std::thread::sleep(std::time::Duration::from_millis(250));
        Ok(())
    }
}

#[cfg(feature = "realtime")]
pub use realtime::{run_realtime, RealtimeOpts};

/// Play a `.ys` patch through the speakers — the `.ys` → audible seam. Compile the
/// source to an [`Engine`](prism_bigraph::Engine) via chrysalis's `build_engine` (the
/// SAME engine `chrysalis run` builds — thin layer, no cloned compile), then drive it
/// through [`run_realtime`], routing the bus at `out_path` to the device.
///
/// The patch must be a **continuously-ticking** engine — one engine tick = one audio
/// block: an oscillator (or voice composite) as a live child writing a `Signal` port
/// each tick (e.g. `packages/synth/ys/live.ys`), NOT a one-shot `instantiate(…)` render
/// (which produces a single block and stops). `out_path` is that bus in the engine
/// state (e.g. `["out"]`). Behind both `ys` (the language facet) and `realtime` (the
/// device). The same engine a mesh / streaming / parallel run drives — the device is
/// just another sink on it.
#[cfg(all(feature = "realtime", feature = "ys"))]
pub fn run_ys_realtime(
    src: &str,
    core: prism_bigraph::Core,
    modules: chrysalis::compile::ModuleRegistry,
    out_path: &[&str],
    opts: RealtimeOpts,
) -> anyhow::Result<()> {
    let program = chrysalis::parse::parse_program(src)
        .map_err(|e| anyhow::anyhow!("parse .ys patch: {e}"))?;
    let engine = chrysalis::runner::build_engine(
        &program,
        core,
        modules,
        &std::collections::BTreeMap::new(),
    )
    .map_err(|e| anyhow::anyhow!("build engine from .ys: {e}"))?;
    run_realtime(engine, out_path, opts)
}
