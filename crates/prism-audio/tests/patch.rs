//! A2 (patch-as-data) — a patch is a **value** the engine discovers and runs.
//! The load-bearing proof: a patch authored as DATA (module nodes in a
//! `Value::Tree`) renders *identically* to the hand-wired `Topology`. That
//! equivalence is the bridge to the generative capstones — once the patch is a
//! value, a reaction can rewrite it (A5) and a factory can author it (A6).
//! (docs/synthesis-bigraphs.md §I, §VI–VII.)

use prism_audio::{
    module_node, render_mix, render_patch, render_voice, signal_type, silence, Envelope, LowPass,
    Oscillator, Vca, Wave,
};
use prism_bigraph::{Schema, Value};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;
const N: usize = 60;

fn osc_config(wave: &str, freq: f64, amp: f64) -> Value {
    Value::tree([
        ("wave", Value::String(wave.into())),
        ("freq", Value::float(freq)),
        ("amplitude", Value::float(amp)),
        ("sample_rate", Value::float(RATE)),
        ("block", Value::Int(BLOCK as i64)),
    ])
}

#[test]
fn data_patch_voice_equals_hand_wired_voice() {
    // Hand-wired (the A2 baseline).
    let hand = render_voice(
        Oscillator::new(Wave::Saw, 110.0, RATE, BLOCK).with_amplitude(0.5),
        LowPass::new(2000.0, RATE, BLOCK),
        Envelope::new(0.05, 0.05, RATE, BLOCK),
        Vca::new(RATE, BLOCK),
        1.0,
        N,
    );

    // The same voice, authored as a PATCH VALUE — modules are data nodes.
    let cfg = |pairs: &[(&str, f64)]| {
        let mut v = vec![
            ("sample_rate".to_string(), Value::float(RATE)),
            ("block".to_string(), Value::Int(BLOCK as i64)),
        ];
        for (k, x) in pairs {
            v.push((k.to_string(), Value::float(*x)));
        }
        Value::tree(v)
    };

    let state = Value::tree([
        ("phase", Value::float(0.0)),
        ("raw", silence(BLOCK)),
        ("z1", Value::float(0.0)),
        ("filtered", silence(BLOCK)),
        ("gate", Value::float(1.0)),
        ("level", Value::float(0.0)),
        ("out", silence(BLOCK)),
        (
            "osc",
            module_node(
                "local:Oscillator",
                osc_config("Saw", 110.0, 0.5),
                &[("phase", "phase")],
                &[("phase", "phase"), ("out", "raw")],
            ),
        ),
        (
            "filt",
            module_node(
                "local:LowPass",
                cfg(&[("cutoff", 2000.0)]),
                &[("in", "raw"), ("z1", "z1")],
                &[("out", "filtered"), ("z1", "z1")],
            ),
        ),
        (
            "env",
            module_node(
                "local:Envelope",
                cfg(&[("attack", 0.05), ("release", 0.05)]),
                &[("gate", "gate"), ("level", "level")],
                &[("level", "level")],
            ),
        ),
        (
            "amp",
            module_node(
                "local:Vca",
                cfg(&[]),
                &[("in", "filtered"), ("gain", "level")],
                &[("out", "out")],
            ),
        ),
    ]);
    let schema = Schema::tree([
        ("phase", Schema::overwrite(Schema::float())),
        ("raw", signal_type()),
        ("z1", Schema::overwrite(Schema::float())),
        ("filtered", signal_type()),
        ("gate", Schema::float()),
        ("level", Schema::overwrite(Schema::float())),
        ("out", signal_type()),
    ]);

    let data = render_patch(state, schema, "out", BLOCK, N);

    assert_eq!(hand.len(), data.len());
    for i in 0..hand.len() {
        assert!(
            (hand[i] - data[i]).abs() < 1e-6,
            "sample {i}: data-patch {} != hand-wired {} — discovery diverged from hand-wiring",
            data[i],
            hand[i]
        );
    }
}

#[test]
fn data_patch_mix_equals_hand_wired_mix() {
    // Two oscillators into one Signal bus, hand-wired …
    let hand = render_mix(
        vec![
            Oscillator::new(Wave::Sine, 220.0, RATE, BLOCK).with_amplitude(0.3),
            Oscillator::new(Wave::Sine, 330.0, RATE, BLOCK).with_amplitude(0.4),
        ],
        N,
    );

    // … and as data: both `out`s wired to the shared `mix` slot (Signal type),
    // so discovery + the additive Signal reconcile mix them.
    let state = Value::tree([
        ("mix", silence(BLOCK)),
        ("phase_0", Value::float(0.0)),
        ("phase_1", Value::float(0.0)),
        (
            "a",
            module_node(
                "local:Oscillator",
                osc_config("Sine", 220.0, 0.3),
                &[("phase", "phase_0")],
                &[("phase", "phase_0"), ("out", "mix")],
            ),
        ),
        (
            "b",
            module_node(
                "local:Oscillator",
                osc_config("Sine", 330.0, 0.4),
                &[("phase", "phase_1")],
                &[("phase", "phase_1"), ("out", "mix")],
            ),
        ),
    ]);
    let schema = Schema::tree([
        ("mix", signal_type()),
        ("phase_0", Schema::overwrite(Schema::float())),
        ("phase_1", Schema::overwrite(Schema::float())),
    ]);

    let data = render_patch(state, schema, "mix", BLOCK, N);

    assert_eq!(hand.len(), data.len());
    for i in 0..hand.len() {
        assert!(
            (hand[i] - data[i]).abs() < 1e-6,
            "sample {i}: data-mix {} != hand mix {}",
            data[i],
            hand[i]
        );
    }
}
