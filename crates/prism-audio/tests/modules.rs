//! A2 — the standard module library (`Vca`, `LowPass`, `Envelope`) and a
//! hand-patched `Voice`, verified offline: per-module DSP correctness (driving
//! `update()` directly) plus the composed voice rendered through the engine
//! (docs/synthesis-bigraphs.md §V, slice A2).

use prism_audio::{
    render_voice, signal_from_slice, signal_to_vec, Envelope, LowPass, Oscillator, Vca, Wave,
};
use prism_bigraph::{Process, Value};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;

// ── per-module DSP ─────────────────────────────────────────────────────────

#[test]
fn vca_scales_by_gain() {
    let vca = Vca::new(RATE, BLOCK);
    let state = Value::tree([
        ("in", signal_from_slice(&vec![1.0_f32; BLOCK])),
        ("gain", Value::float(0.5)),
    ]);
    let out = vca.update(&state, vca.interval()).into_value().unwrap();
    let block = signal_to_vec(out.get_field("out").unwrap());
    assert_eq!(block.len(), BLOCK);
    assert!(block.iter().all(|&s| (s - 0.5).abs() < 1e-6), "out = in × 0.5");
}

#[test]
fn vca_zero_gain_is_silence() {
    let vca = Vca::new(RATE, BLOCK);
    let state = Value::tree([
        ("in", signal_from_slice(&vec![0.9_f32; BLOCK])),
        ("gain", Value::float(0.0)),
    ]);
    let out = vca.update(&state, vca.interval()).into_value().unwrap();
    let block = signal_to_vec(out.get_field("out").unwrap());
    assert!(block.iter().all(|&s| s == 0.0), "gain 0 → silence");
}

#[test]
fn lowpass_settles_toward_dc_input() {
    let lp = LowPass::new(1000.0, RATE, BLOCK);
    let input = signal_from_slice(&vec![1.0_f32; BLOCK]);
    let state = Value::tree([("in", input.clone()), ("z1", Value::float(0.0))]);
    let out = lp.update(&state, lp.interval()).into_value().unwrap();
    let block = signal_to_vec(out.get_field("out").unwrap());

    assert!(block[0] > 0.0 && block[0] < 1.0, "rises from 0 toward DC");
    assert!(block.windows(2).all(|w| w[1] >= w[0] - 1e-6), "monotone within a block");

    // Carry z1 across blocks → approaches the DC level.
    let mut z = out.get_field("z1").unwrap().as_f64().unwrap();
    for _ in 0..20 {
        let st = Value::tree([("in", input.clone()), ("z1", Value::float(z))]);
        z = lp.update(&st, lp.interval()).into_value().unwrap()
            .get_field("z1").unwrap().as_f64().unwrap();
    }
    assert!(z > 0.99, "settles toward DC: z = {z}");
}

#[test]
fn lowpass_attenuates_highs_more_than_lows() {
    let lp = LowPass::new(1000.0, RATE, BLOCK);
    let rms_through = |freq: f64| -> f64 {
        let osc = Oscillator::new(Wave::Sine, freq, RATE, BLOCK);
        let (mut phase, mut z, mut acc, mut n) = (0.0, 0.0, 0.0_f64, 0usize);
        for _ in 0..50 {
            let ou = osc
                .update(&Value::tree([("phase", Value::float(phase))]), osc.interval())
                .into_value()
                .unwrap();
            phase = ou.get_field("phase").unwrap().as_f64().unwrap();
            let lo = lp
                .update(
                    &Value::tree([
                        ("in", ou.get_field("out").unwrap().clone()),
                        ("z1", Value::float(z)),
                    ]),
                    lp.interval(),
                )
                .into_value()
                .unwrap();
            z = lo.get_field("z1").unwrap().as_f64().unwrap();
            for s in signal_to_vec(lo.get_field("out").unwrap()) {
                acc += (s as f64) * (s as f64);
                n += 1;
            }
        }
        (acc / n as f64).sqrt()
    };
    let low = rms_through(200.0);
    let high = rms_through(8000.0);
    assert!(low > high * 1.5, "low-pass: low {low} ≫ high {high}");
}

#[test]
fn envelope_rises_on_gate_then_falls_off_gate() {
    let env = Envelope::new(0.05, 0.05, RATE, BLOCK);
    let step = |gate: f64, level: f64| -> f64 {
        env.update(
            &Value::tree([("gate", Value::float(gate)), ("level", Value::float(level))]),
            env.interval(),
        )
        .into_value()
        .unwrap()
        .get_field("level")
        .unwrap()
        .as_f64()
        .unwrap()
    };
    let mut level = 0.0;
    for _ in 0..20 {
        level = step(1.0, level);
    }
    assert!(level > 0.95, "attack rises to ~1: {level}");
    for _ in 0..20 {
        level = step(0.0, level);
    }
    assert!(level < 0.05, "release falls to ~0: {level}");
}

// ── the composed voice ─────────────────────────────────────────────────────

#[test]
fn voice_renders_bounded_and_envelope_shaped() {
    let osc = Oscillator::new(Wave::Saw, 110.0, RATE, BLOCK).with_amplitude(0.5);
    let filt = LowPass::new(2000.0, RATE, BLOCK);
    let env = Envelope::new(0.05, 0.05, RATE, BLOCK);
    let vca = Vca::new(RATE, BLOCK);
    let n = 60;
    let buf = render_voice(osc, filt, env, vca, 1.0, n);

    assert_eq!(buf.len(), BLOCK * n);
    // One-pole LP can't exceed the saw's amplitude; envelope ≤ 1.
    assert!(buf.iter().all(|&s| s.abs() <= 0.5 + 1e-3), "bounded by osc amplitude");

    let mean_abs = |k: usize| {
        let s = &buf[k * BLOCK..(k + 1) * BLOCK];
        s.iter().map(|x| x.abs()).sum::<f32>() / BLOCK as f32
    };
    assert!(mean_abs(0) < 1e-4, "block 0 silent (pipeline fill)");
    assert!(mean_abs(3) < mean_abs(50), "envelope shapes the voice (rises)");
    assert!(mean_abs(50) > 0.02, "voice is audible once the envelope opens");
}
