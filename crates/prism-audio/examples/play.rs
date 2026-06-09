//! Hear a reaction fire. Run with the realtime feature + your audio device:
//!
//! ```sh
//! cargo run -p prism-audio --features realtime --example play
//! ```
//!
//! Builds a live rack — one 220 Hz voice — and a Detune reaction wired over it on
//! a ~2 s interval. You hear the 220 Hz voice play; then, when the reaction fires,
//! a copy a fifth up joins it on the same mix bus. The patch rewrote *itself*, in
//! real time, through prism's BRS — the audible form of A5 (synthesis-bigraphs.md
//! §VI/§X). The realtime path is the *same* patch as the offline render, only the
//! sink differs (the gorgon ring instead of a buffer).

use prism_audio::device::{run_realtime, RealtimeOpts};
use prism_audio::{
    audio_engine, detune_voice, patch_brs_node, signal_type, silence, voice_composite_node,
};
use prism_bigraph::{Schema, Value};

fn main() -> anyhow::Result<()> {
    const RATE: f64 = 48_000.0;
    const BLOCK: usize = 256;

    // A rack: one 220 Hz voice into the mix bus, plus a Detune BRS over the rack.
    // The BRS ticks every ~2 s, so it fires once a couple of seconds in — you hear
    // the change rather than it being there from the first sample.
    let state = Value::tree([
        (
            "rack",
            Value::tree([
                ("mix", silence(BLOCK)),
                (
                    "voice_a",
                    voice_composite_node(220.0, 1200.0, 1.0, 0.4, BLOCK, RATE, "mix"),
                ),
            ]),
        ),
        ("brs", patch_brs_node("rack", 2.0)),
    ]);
    let schema = Schema::tree([("rack", Schema::tree([("mix", signal_type())]))]);

    // A fifth up (700 cents). The Voice composite carries its own schema, so the
    // engine spawns it correctly typed and it mixes straight in.
    let rules = vec![detune_voice(700.0, "mix", BLOCK, RATE)];
    let engine = audio_engine(state, schema, rules, BLOCK);

    println!(
        "playing ~6 s — a 220 Hz voice; ~2 s in, a Detune reaction forks it up a fifth.\n\
         (no sound? pass a device name and check your default output)"
    );
    run_realtime(
        engine,
        &["rack", "mix"],
        RealtimeOpts {
            device: None,
            seconds: 6.0,
            block: BLOCK,
            sample_rate: RATE,
        },
    )
}
