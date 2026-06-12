//! Hear a `.ys` PATCH — a package's continuous audio engine, compiled and driven to
//! the speakers. The `.ys` → audible seam.
//!
//! ```sh
//! cargo run -p prism-audio --features realtime,ys --example play_ys
//! cargo run -p prism-audio --features realtime,ys --example play_ys -- <patch.ys> <out-bus>
//! ```
//!
//! By default plays `packages/synth/ys/live.ys` (a 220 Hz oscillator as a live composite
//! child, one block per tick) reading its `out` bus. The patch is compiled to a prism
//! [`Engine`](prism_bigraph::Engine) by the SAME path `chrysalis run` uses
//! (`chrysalis::runner::build_engine`) and ticked by the device — one engine tick per
//! audio block. It is the same engine a mesh / streaming / parallel run drives; the
//! device is just another sink on it.

use prism_audio::device::{run_ys_realtime, RealtimeOpts};
use prism_audio::prelude::{audio_core, audio_modules};

fn main() -> anyhow::Result<()> {
    const RATE: f64 = 48_000.0;
    const BLOCK: usize = 256;

    // Default to the synth package's continuous patch; override with argv.
    let default_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/synth/ys/live.ys");
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| default_path.to_string());
    let out_bus = args.next().unwrap_or_else(|| "out".to_string());

    let src = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("read {path}: {e}"))?;

    println!(
        "playing {path} (~5 s, bus `{out_bus}`) — a .ys patch compiled to an engine and\n\
         driven to the audio device, one engine tick per block.\n\
         (no sound? pass a device name / check your default output)"
    );
    run_ys_realtime(
        &src,
        audio_core(BLOCK),
        audio_modules(),
        &[&out_bus],
        RealtimeOpts {
            device: None,
            seconds: 5.0,
            block: BLOCK,
            sample_rate: RATE,
        },
    )
}
