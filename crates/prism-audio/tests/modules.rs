//! A2 — the standard module library (`Vca`, `LowPass`, `Envelope`) and a
//! hand-patched `Voice`, verified offline: per-module DSP correctness (driving
//! `update()` directly) plus the composed voice rendered through the engine
//! (docs/synthesis-bigraphs.md §V, slice A2).

use prism_audio::{
    render_voice, signal_from_slice, signal_to_vec, AudioOut, Compare, Counter, Envelope, Fold,
    LowPass, Noise, Oscillator, RingMod, SampleHold, Sequencer, Slope, Svf, Vca, Wave,
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

// ── the fully-modulatable oscillator (CV ≡ audio) ────────────────────────────

fn goertzel(samples: &[f32], freq: f64, rate: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq / rate;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
    for &x in samples {
        let s0 = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt() / samples.len().max(1) as f64
}

#[test]
fn oscillator_exp_fm_shifts_a_full_octave() {
    // A constant +1.0 on the exponential (V/oct) FM input lifts a 220 Hz oscillator to
    // 440 Hz — pitch CV is just another Signal patched in.
    let osc = Oscillator::new(Wave::Sine, 220.0, RATE, BLOCK);
    let cv = signal_from_slice(&vec![1.0_f32; BLOCK]); // +1 octave, constant
    let mut phase = 0.0;
    let mut buf: Vec<f32> = Vec::new();
    for _ in 0..40 {
        let out = osc
            .update(
                &Value::tree([("phase", Value::float(phase)), ("fm_exp", cv.clone())]),
                osc.interval(),
            )
            .into_value()
            .unwrap();
        phase = out.get_field("phase").unwrap().as_f64().unwrap();
        buf.extend(signal_to_vec(out.get_field("sine").unwrap()));
    }
    let e220 = goertzel(&buf, 220.0, RATE);
    let e440 = goertzel(&buf, 440.0, RATE);
    assert!(e440 > e220 * 4.0, "exp FM +1 oct: 440 ({e440}) ≫ 220 ({e220})");
}

#[test]
fn oscillator_presents_all_shapes_at_once() {
    // Like joranalogue Generate / Schlappi Three Body: every waveshape on its own
    // output, simultaneously, from one phase.
    let osc = Oscillator::new(Wave::Sine, 440.0, RATE, BLOCK);
    let out = osc
        .update(&Value::tree([("phase", Value::float(0.1))]), osc.interval())
        .into_value()
        .unwrap();
    for port in ["sine", "saw", "square", "triangle", "sub", "out"] {
        assert_eq!(
            signal_to_vec(out.get_field(port).expect(port)).len(),
            BLOCK,
            "{port} is a full block"
        );
    }
    let saw = signal_to_vec(out.get_field("saw").unwrap());
    let square = signal_to_vec(out.get_field("square").unwrap());
    assert!(saw != square, "distinct simultaneous shapes");
    // `out` follows the `wave`-selected shape (Sine here) — single-output patches see
    // exactly the old behaviour.
    assert_eq!(
        signal_to_vec(out.get_field("sine").unwrap()),
        signal_to_vec(out.get_field("out").unwrap()),
        "out follows the wave-selected shape"
    );
}

#[test]
fn oscillator_hard_sync_resets_phase() {
    // A rising edge on `sync` resets the phase mid-block — the sync timbre.
    let osc = Oscillator::new(Wave::Saw, 100.0, RATE, BLOCK);
    let mut sync = vec![0.0_f32; BLOCK];
    sync[BLOCK / 2] = 1.0; // a gate halfway through
    let out = osc
        .update(
            &Value::tree([
                ("phase", Value::float(0.0)),
                ("sync", signal_from_slice(&sync)),
            ]),
            osc.interval(),
        )
        .into_value()
        .unwrap();
    // The saw climbs from -1; at the sync edge it snaps back toward -1 (phase reset).
    let saw = signal_to_vec(out.get_field("saw").unwrap());
    assert!(
        saw[BLOCK / 2] < saw[BLOCK / 2 - 1],
        "phase reset drops the saw at the sync edge"
    );
}

// ── the multimode state-variable filter / resonator ──────────────────────────

#[test]
fn svf_cutoff_cv_opens_the_lowpass() {
    // An 8 kHz tone is rejected by a 500 Hz low-pass; lifting cutoff +5 octaves via CV
    // (≈16 kHz) lets it through — audio-rate cutoff modulation, CV ≡ audio.
    let drive = |cutoff_cv: f32| -> f64 {
        let svf = Svf::new(500.0, 0.0, RATE, BLOCK);
        let osc = Oscillator::new(Wave::Sine, 8000.0, RATE, BLOCK);
        let cv = signal_from_slice(&vec![cutoff_cv; BLOCK]);
        let (mut phase, mut ic1, mut ic2, mut acc, mut n) = (0.0, 0.0, 0.0, 0.0_f64, 0usize);
        for _ in 0..40 {
            let ou = osc
                .update(&Value::tree([("phase", Value::float(phase))]), osc.interval())
                .into_value()
                .unwrap();
            phase = ou.get_field("phase").unwrap().as_f64().unwrap();
            let fo = svf
                .update(
                    &Value::tree([
                        ("input", ou.get_field("sine").unwrap().clone()),
                        ("ic1", Value::float(ic1)),
                        ("ic2", Value::float(ic2)),
                        ("cutoff_cv", cv.clone()),
                    ]),
                    svf.interval(),
                )
                .into_value()
                .unwrap();
            ic1 = fo.get_field("ic1").unwrap().as_f64().unwrap();
            ic2 = fo.get_field("ic2").unwrap().as_f64().unwrap();
            for s in signal_to_vec(fo.get_field("lp").unwrap()) {
                acc += (s as f64) * (s as f64);
                n += 1;
            }
        }
        (acc / n as f64).sqrt()
    };
    let closed = drive(0.0);
    let open = drive(5.0);
    assert!(open > closed * 3.0, "cutoff CV opens the LP: open {open} ≫ closed {closed}");
}

#[test]
fn svf_presents_four_distinct_modes() {
    let svf = Svf::new(1000.0, 0.3, RATE, BLOCK);
    let osc = Oscillator::new(Wave::Saw, 300.0, RATE, BLOCK);
    let ou = osc
        .update(&Value::tree([("phase", Value::float(0.2))]), osc.interval())
        .into_value()
        .unwrap();
    let fo = svf
        .update(
            &Value::tree([
                ("input", ou.get_field("saw").unwrap().clone()),
                ("ic1", Value::float(0.1)),
                ("ic2", Value::float(0.05)),
            ]),
            svf.interval(),
        )
        .into_value()
        .unwrap();
    let lp = signal_to_vec(fo.get_field("lp").unwrap());
    let hp = signal_to_vec(fo.get_field("hp").unwrap());
    let bp = signal_to_vec(fo.get_field("bp").unwrap());
    let notch = signal_to_vec(fo.get_field("notch").unwrap());
    assert!(lp != hp && lp != bp && bp != notch, "four distinct filter modes");
    // The SVF identity: notch = lp + hp.
    for i in 0..BLOCK {
        assert!(
            (notch[i] - (lp[i] + hp[i])).abs() < 1e-4,
            "notch = lp + hp at {i}"
        );
    }
}

// ── the universal slope (Serge DUSG / Maths) ─────────────────────────────────

#[test]
fn slope_one_shot_rises_then_falls() {
    // Patched `trigger` (held high → one rising edge), no cycle ⇒ one AD envelope.
    let slope = Slope::new(0.002, 0.002, RATE, BLOCK); // ~96 samples per slope
    let trig = signal_from_slice(&vec![1.0_f32; BLOCK]);
    let (mut level, mut rising, mut tz) = (0.0, 0.0, 0.0);
    let (mut peak, mut eoc_count) = (0.0_f64, 0usize);
    for _ in 0..3 {
        let out = slope
            .update(
                &Value::tree([
                    ("level", Value::float(level)),
                    ("rising", Value::float(rising)),
                    ("trig_z", Value::float(tz)),
                    ("trigger", trig.clone()),
                ]),
                slope.interval(),
            )
            .into_value()
            .unwrap();
        level = out.get_field("level").unwrap().as_f64().unwrap();
        rising = out.get_field("rising").unwrap().as_f64().unwrap();
        tz = out.get_field("trig_z").unwrap().as_f64().unwrap();
        for s in signal_to_vec(out.get_field("out").unwrap()) {
            peak = peak.max(s as f64);
        }
        for e in signal_to_vec(out.get_field("eoc").unwrap()) {
            if e > 0.5 {
                eoc_count += 1;
            }
        }
    }
    assert!(peak > 0.95, "the slope rose to ~1 (peak {peak})");
    assert!(level < 0.05, "and fell back to ~0 (final {level})");
    assert_eq!(eoc_count, 1, "exactly one end-of-cycle pulse for a one-shot");
}

#[test]
fn slope_cycles_as_an_lfo() {
    // `cycle: true` ⇒ self-retriggering = an LFO, no trigger needed.
    let mut slope = Slope::new(0.002, 0.002, RATE, BLOCK);
    slope.cycle = true;
    let (mut level, mut rising, mut tz) = (0.0, 0.0, 0.0);
    let (mut eoc_count, mut lo, mut hi) = (0usize, 1.0_f64, 0.0_f64);
    for _ in 0..12 {
        let out = slope
            .update(
                &Value::tree([
                    ("level", Value::float(level)),
                    ("rising", Value::float(rising)),
                    ("trig_z", Value::float(tz)),
                ]),
                slope.interval(),
            )
            .into_value()
            .unwrap();
        level = out.get_field("level").unwrap().as_f64().unwrap();
        rising = out.get_field("rising").unwrap().as_f64().unwrap();
        tz = out.get_field("trig_z").unwrap().as_f64().unwrap();
        for s in signal_to_vec(out.get_field("out").unwrap()) {
            lo = lo.min(s as f64);
            hi = hi.max(s as f64);
        }
        for e in signal_to_vec(out.get_field("eoc").unwrap()) {
            if e > 0.5 {
                eoc_count += 1;
            }
        }
    }
    assert!(eoc_count >= 5, "cycles repeatedly ({eoc_count} eoc pulses)");
    assert!(hi > 0.9 && lo < 0.1, "the LFO sweeps the full range ({lo}..{hi})");
}

// ── comparator + analog logic ────────────────────────────────────────────────

#[test]
fn compare_gates_and_analog_logic() {
    let cmp = Compare::new(0.0, BLOCK); // threshold 0
    let input: Vec<f32> = (0..BLOCK)
        .map(|i| (i as f32 / BLOCK as f32) * 2.0 - 1.0) // ramp -1 → +1
        .collect();
    let out = cmp
        .update(
            &Value::tree([
                ("input", signal_from_slice(&input)),
                ("b", signal_from_slice(&vec![0.5_f32; BLOCK])),
            ]),
            cmp.interval(),
        )
        .into_value()
        .unwrap();
    let gate = signal_to_vec(out.get_field("gate").unwrap());
    let max = signal_to_vec(out.get_field("max").unwrap());
    let rect = signal_to_vec(out.get_field("rect").unwrap());
    assert!(gate[0] == 0.0 && gate[BLOCK - 1] == 1.0, "gates at the threshold");
    assert!(max.iter().all(|&m| m >= 0.5 - 1e-6), "max(input, 0.5) ≥ 0.5");
    assert!((rect[0] - 1.0).abs() < 0.02, "full-wave rectify: |−1| = 1");
}

// ── sample & hold / slew (Serge SSG) ─────────────────────────────────────────

#[test]
fn samplehold_holds_the_value_at_the_trigger() {
    let sh = SampleHold::new(0.0001, RATE, BLOCK); // ~instant slew (stepped)
    let input: Vec<f32> = (0..BLOCK).map(|i| i as f32 / BLOCK as f32).collect();
    let mut trig = vec![0.0_f32; BLOCK];
    trig[100] = 1.0; // a trigger pulse
    let out = sh
        .update(
            &Value::tree([
                ("held", Value::float(0.0)),
                ("smooth_v", Value::float(0.0)),
                ("trig_z", Value::float(0.0)),
                ("input", signal_from_slice(&input)),
                ("trigger", signal_from_slice(&trig)),
            ]),
            sh.interval(),
        )
        .into_value()
        .unwrap();
    let stepped = signal_to_vec(out.get_field("stepped").unwrap());
    assert!(stepped[50] == 0.0, "holds the initial value before the trigger");
    let expected = 100.0 / BLOCK as f32;
    assert!(
        (stepped[BLOCK - 1] - expected).abs() < 0.01,
        "holds input sampled at the trigger ({} ≈ {expected})",
        stepped[BLOCK - 1]
    );
}

// ── noise / random source ────────────────────────────────────────────────────

#[test]
fn noise_is_bounded_spread_and_deterministic() {
    let noise = Noise::new(BLOCK);
    let run = || {
        signal_to_vec(
            noise
                .update(&Value::tree([("seed", Value::Int(12345))]), noise.interval())
                .into_value()
                .unwrap()
                .get_field("out")
                .unwrap(),
        )
    };
    let a = run();
    assert_eq!(a.len(), BLOCK);
    assert!(a.iter().all(|&s| (-1.0..=1.0).contains(&s)), "bounded [-1, 1]");
    let mean = a.iter().sum::<f32>() / BLOCK as f32;
    let var = a.iter().map(|&x| (x - mean).powi(2)).sum::<f32>() / BLOCK as f32;
    assert!(var > 0.1, "full-band noise has spread (var {var})");
    assert_eq!(a, run(), "seeded noise is reproducible");
}

// ── wavefolder ───────────────────────────────────────────────────────────────

#[test]
fn fold_passes_small_signals_and_folds_large_ones() {
    // |x| ≤ 1 at unity drive ⇒ identity.
    let fold = Fold::new(1.0, BLOCK);
    let mild: Vec<f32> = (0..BLOCK).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
    let out = signal_to_vec(
        fold.update(&Value::tree([("input", signal_from_slice(&mild))]), fold.interval())
            .into_value()
            .unwrap()
            .get_field("out")
            .unwrap(),
    );
    for i in 0..BLOCK {
        assert!((out[i] - mild[i]).abs() < 1e-4, "|x|<1 passes through");
    }
    // Hard drive folds: bounded, and more zero-crossings (= added harmonics).
    let hard = Fold::new(3.0, BLOCK);
    let sine: Vec<f32> = (0..BLOCK)
        .map(|i| (i as f32 / BLOCK as f32 * std::f32::consts::TAU * 4.0).sin())
        .collect();
    let folded = signal_to_vec(
        hard.update(&Value::tree([("input", signal_from_slice(&sine))]), hard.interval())
            .into_value()
            .unwrap()
            .get_field("out")
            .unwrap(),
    );
    assert!(folded.iter().all(|&s| s.abs() <= 1.0 + 1e-5), "folded output is bounded");
    let zc = |b: &[f32]| b.windows(2).filter(|w| w[0].signum() != w[1].signum()).count();
    assert!(zc(&folded) > zc(&sine), "folding adds zero-crossings (harmonics)");
}

// ── ring modulator ───────────────────────────────────────────────────────────

#[test]
fn ringmod_multiplies_four_quadrant() {
    let rm = RingMod::new(BLOCK);
    let a = vec![0.5_f32; BLOCK];
    let b: Vec<f32> = (0..BLOCK).map(|i| if i % 2 == 0 { 0.4 } else { -0.4 }).collect();
    let out = signal_to_vec(
        rm.update(
            &Value::tree([("a", signal_from_slice(&a)), ("b", signal_from_slice(&b))]),
            rm.interval(),
        )
        .into_value()
        .unwrap()
        .get_field("out")
        .unwrap(),
    );
    for i in 0..BLOCK {
        assert!((out[i] - 0.5 * b[i]).abs() < 1e-5, "out = a·b");
    }
    assert!(out[0] > 0.0 && out[1] < 0.0, "four-quadrant: sign follows b");
}

// ── binary counter (Schlappi Nibbler) ────────────────────────────────────────

#[test]
fn counter_counts_clock_edges_in_binary() {
    let counter = Counter::new(BLOCK);
    let mut clock = vec![0.0_f32; BLOCK];
    for &i in &[10usize, 11, 40, 41, 70, 71] {
        clock[i] = 1.0; // 3 pulses → 3 rising edges
    }
    let out = counter
        .update(
            &Value::tree([
                ("count", Value::Int(0)),
                ("clk_z", Value::float(0.0)),
                ("rst_z", Value::float(0.0)),
                ("clock", signal_from_slice(&clock)),
            ]),
            counter.interval(),
        )
        .into_value()
        .unwrap();
    assert_eq!(out.get_field("count").unwrap().as_f64().unwrap() as i64, 3, "3 clocks → count 3");
    let cv = signal_to_vec(out.get_field("cv").unwrap());
    assert!((cv[BLOCK - 1] - 3.0 / 15.0).abs() < 1e-4, "cv = count/15 staircase");
    // 3 = 0b0011 → b0=1, b1=1, b2=0.
    assert_eq!(signal_to_vec(out.get_field("b0").unwrap())[BLOCK - 1], 1.0);
    assert_eq!(signal_to_vec(out.get_field("b2").unwrap())[BLOCK - 1], 0.0);
}

// ── step sequencer ───────────────────────────────────────────────────────────

#[test]
fn sequencer_steps_through_the_list() {
    let seq = Sequencer::new(vec![0.1, 0.2, 0.3], BLOCK);
    let mut clock = vec![0.0_f32; BLOCK];
    for &i in &[20usize, 21, 60, 61] {
        clock[i] = 1.0; // 2 clocks → 0 → 1 → 2
    }
    let out = seq
        .update(
            &Value::tree([
                ("index", Value::Int(0)),
                ("clk_z", Value::float(0.0)),
                ("rst_z", Value::float(0.0)),
                ("clock", signal_from_slice(&clock)),
            ]),
            seq.interval(),
        )
        .into_value()
        .unwrap();
    let cv = signal_to_vec(out.get_field("cv").unwrap());
    assert!((cv[0] - 0.1).abs() < 1e-4, "starts on step 0");
    assert!((cv[30] - 0.2).abs() < 1e-4, "step 1 after the first clock");
    assert!((cv[BLOCK - 1] - 0.3).abs() < 1e-4, "step 2 after the second clock");
    let trig = signal_to_vec(out.get_field("trig").unwrap());
    assert_eq!(trig[20], 1.0, "a trigger pulse on advance");
    assert_eq!(trig[21], 0.0, "the trigger is one sample wide");
}

// ── the audio device sink (the world-boundary face) ──────────────────────────

#[test]
fn audioout_passes_the_signal_through() {
    // In the default (no-realtime) build, AudioOut is a transparent passthrough, so a
    // patch that wires the device sink still RENDERS the Signal offline (`out = input`).
    // The realtime build adds the side effect of driving the device + the back-pressure
    // pacing; the passthrough contract is the same.
    let ao = AudioOut::from_config(&Value::tree([
        ("block", Value::Int(BLOCK as i64)),
        ("sample_rate", Value::float(RATE)),
    ]));
    let input: Vec<f32> = (0..BLOCK).map(|i| (i as f32 * 0.05).sin()).collect();
    let out = ao
        .update(
            &Value::tree([("input", signal_from_slice(&input))]),
            ao.interval(),
        )
        .into_value()
        .unwrap();
    assert_eq!(
        signal_to_vec(out.get_field("out").unwrap()),
        input,
        "AudioOut passes the Signal through (so a patch renders offline + plays with realtime)"
    );
}
