//! A1 — the offline render proves the audio substrate *deterministically*,
//! under `cargo test`, with no device and no wall-clock: a `Signal` block
//! flows through the prism engine and renders a correct sine
//! (docs/synthesis-bigraphs.md §X, slice A1).

use prism_audio::{render::render_kernel, render_oscillator, write_wav_i16, Oscillator, Wave};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 512;
const FREQ: f64 = 440.0;
const N_BLOCKS: usize = 94; // ~1.003 s
const AMP: f64 = 0.5;

fn osc() -> Oscillator {
    Oscillator::new(Wave::Sine, FREQ, RATE, BLOCK).with_amplitude(AMP)
}

#[test]
fn renders_expected_length() {
    let buf = render_oscillator(osc(), N_BLOCKS);
    assert_eq!(buf.len(), BLOCK * N_BLOCKS, "one block of samples per tick");
}

#[test]
fn samples_bounded_by_amplitude_and_nonzero() {
    let buf = render_oscillator(osc(), N_BLOCKS);
    assert!(
        buf.iter().all(|&s| s.abs() <= AMP as f32 + 1e-4),
        "a sine is bounded by its amplitude"
    );
    assert!(
        buf.iter().any(|&s| s.abs() > 0.1),
        "the render is not silence"
    );
}

#[test]
fn frequency_matches_via_zero_crossings() {
    let buf = render_oscillator(osc(), N_BLOCKS);
    let duration = (BLOCK * N_BLOCKS) as f64 / RATE;
    let zc = buf
        .windows(2)
        .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
        .count();
    let expected = 2.0 * FREQ * duration; // two zero crossings per cycle
    let ratio = zc as f64 / expected;
    assert!(
        (ratio - 1.0).abs() < 0.02,
        "zero crossings {zc} ≈ expected {expected:.1} (ratio {ratio:.4}) ⇒ ~{FREQ} Hz"
    );
}

#[test]
fn phase_continuous_across_block_boundaries() {
    let buf = render_oscillator(osc(), N_BLOCKS);
    // The largest per-sample step of an amp·sin(2π·f·t) is ≈ amp·2π·f/rate;
    // allow 2× slack. A phase discontinuity at a block seam would blow past it.
    let max_step = AMP * std::f64::consts::TAU * FREQ / RATE * 2.0;
    let worst = buf
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        (worst as f64) < max_step,
        "no block-boundary discontinuity: worst step {worst} < {max_step}"
    );
}

#[test]
fn engine_render_matches_kernel_render() {
    // The engine path (Topology → invoke → reconcile → apply → state) must
    // equal the direct kernel render (Process::update threaded by hand). This
    // is the load-bearing proof: a `Signal` block survives the wire and the
    // `overwrite` slot replays each frame exactly.
    let kernel = render_kernel(&osc(), N_BLOCKS);
    let engine = render_oscillator(osc(), N_BLOCKS);
    assert_eq!(kernel.len(), engine.len(), "same number of samples");
    for (i, (k, e)) in kernel.iter().zip(engine.iter()).enumerate() {
        assert!(
            (k - e).abs() < 1e-6,
            "sample {i}: engine {e} == kernel {k}"
        );
    }
}

#[test]
fn writes_a_wav_artifact() {
    let buf = render_oscillator(osc(), N_BLOCKS);
    let path = std::env::temp_dir().join("prism_audio_a1_sine440.wav");
    write_wav_i16(&path, &buf, RATE as u32).expect("write wav");
    let meta = std::fs::metadata(&path).expect("wav file exists");
    assert_eq!(
        meta.len() as usize,
        44 + 2 * buf.len(),
        "44-byte header + 2 bytes per mono 16-bit sample"
    );
}
