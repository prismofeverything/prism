//! A6 (deepened) — the synth writes a full INSTRUMENT type, then REUSES it.
//!
//! Two things beyond the bare stack (`tests/factory.rs`): (1) the factory authors
//! a complete subtractive voice (a generated oscillator bank → LowPass → Vca shaped
//! by an Envelope), and (2) the authored type is stored as a blueprint and a SECOND
//! reaction casts it into an ensemble — the product becomes a building block.

use prism_audio::{
    cast_instrument, define_instrument, instrument_voice_node, patch_brs_node, render_patch_brs,
    signal_type, silence, InstrumentRecipe,
};
use prism_bigraph::{Schema, Value};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;
const N: usize = 80;

fn goertzel(samples: &[f32], freq: f64, rate: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq / rate;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
    for &x in samples {
        let s0 = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    power.max(0.0).sqrt() / (samples.len().max(1) as f64)
}

fn cents(base: f64, c: f64) -> f64 {
    base * 2.0_f64.powf(c / 1200.0)
}

fn cast_seed(freq: f64) -> Value {
    Value::tree([
        ("_type", Value::String("CastSeed".into())),
        ("freq", Value::float(freq)),
    ])
}

#[test]
fn instrument_authors_a_full_subtractive_voice() {
    // A 3-voice instrument: the factory composes the bank AND the filter/env/vca
    // tail — all four kernels — into one authored type.
    let recipe = InstrumentRecipe::voice(220.0, 600.0, 3, 3000.0);
    let node = instrument_voice_node(&recipe, BLOCK, RATE, "mix");

    assert_eq!(
        node.get_field("kind").and_then(|v| v.as_str()),
        Some("Instrument")
    );
    let inner = node
        .get_field("config")
        .and_then(|c| c.get_field("state"))
        .expect("inner state");
    let osc_count = inner
        .iter_fields()
        .expect("map")
        .filter(|(k, _)| k.starts_with("osc_"))
        .count();
    assert_eq!(osc_count, 3, "one oscillator per voice in the bank");
    // The subtractive tail + its buses are present (vs the stack's bare bank).
    for slot in ["filt", "env", "amp", "filtered", "level", "out", "z1"] {
        assert!(
            inner.get_field(slot).is_some(),
            "the instrument has the `{slot}` slot/node"
        );
    }
}

#[test]
fn recipe_round_trips_through_blueprint_data() {
    // The type-as-data: to_value/from_value is the homoiconic blueprint a reaction
    // stores and another reads back.
    let r = InstrumentRecipe::voice(330.0, 500.0, 4, 2200.0);
    let back = InstrumentRecipe::from_value(&r.to_value());
    assert_eq!(back.voices(), 4, "voice count survives");
    assert!((back.base - 330.0).abs() < 1e-9);
    assert!((back.cutoff - 2200.0).abs() < 1e-9);
    assert_eq!(back.intervals.len(), r.intervals.len());
}

/// A rack with a `blueprint` preseeded + one `CastSeed` per pitch. Isolates the
/// cast reaction's reuse from the define step.
fn rack_with_blueprint(freqs: &[f64]) -> (Value, Schema) {
    // A single-oscillator instrument blueprint (clean, Goertzel-separable casts).
    let blueprint = InstrumentRecipe::voice(220.0, 0.0, 1, 3000.0).to_value();
    let mut entries = vec![
        ("mix".to_string(), silence(BLOCK)),
        ("blueprint".to_string(), blueprint),
    ];
    for (i, f) in freqs.iter().enumerate() {
        entries.push((format!("cast_{i}"), cast_seed(*f)));
    }
    let state = Value::tree([
        ("rack", Value::tree(entries)),
        ("brs", patch_brs_node("rack", BLOCK as f64 / RATE)),
    ]);
    let schema = Schema::tree([("rack", Schema::tree([("mix", signal_type())]))]);
    (state, schema)
}

#[test]
fn cast_reuses_a_blueprint_into_an_ensemble() {
    // The authored type, instanced at three pitches (a triad) — the product reused.
    let freqs = [220.0, cents(220.0, 400.0), cents(220.0, 700.0)]; // 220, ~277, ~330
    let (state, schema) = rack_with_blueprint(&freqs);
    let out = render_patch_brs(
        state,
        schema,
        vec![cast_instrument("mix", BLOCK, RATE)],
        &["rack", "mix"],
        BLOCK,
        N,
    );

    let lo = (N / 2) * BLOCK;
    for f in freqs {
        let e = goertzel(&out[lo..], f, RATE);
        // present (the reused instance sounds) and individually bounded (no runaway).
        assert!(e > 0.02, "cast instance at {f:.0} Hz sounds ({e})");
        assert!(e < 0.7, "instance at {f:.0} Hz bounded ({e})");
    }
}

#[test]
fn define_then_cast_writes_a_type_and_plays_it() {
    // The full AlChemy loop: `define` authors the blueprint (tick 1), then `cast`
    // reuses it at each pitch (tick 2+, once the blueprint is present). The casts
    // sounding is proof the blueprint was authored — cast cannot match without it.
    let freqs = [220.0, cents(220.0, 400.0), cents(220.0, 700.0)];
    let mut entries = vec![
        ("mix".to_string(), silence(BLOCK)),
        (
            "def".to_string(),
            Value::tree([
                ("_type", Value::String("DefineSeed".into())),
                ("base", Value::float(220.0)),
                ("spread", Value::float(0.0)),
                ("voices", Value::float(1.0)),
                ("cutoff", Value::float(3000.0)),
            ]),
        ),
    ];
    for (i, f) in freqs.iter().enumerate() {
        entries.push((format!("cast_{i}"), cast_seed(*f)));
    }
    let state = Value::tree([
        ("rack", Value::tree(entries)),
        ("brs", patch_brs_node("rack", BLOCK as f64 / RATE)),
    ]);
    let schema = Schema::tree([("rack", Schema::tree([("mix", signal_type())]))]);

    let out = render_patch_brs(
        state,
        schema,
        vec![define_instrument(), cast_instrument("mix", BLOCK, RATE)],
        &["rack", "mix"],
        BLOCK,
        N,
    );

    let lo = (N / 2) * BLOCK;
    for f in freqs {
        let e = goertzel(&out[lo..], f, RATE);
        assert!(
            e > 0.02,
            "the defined-then-cast instrument sounds at {f:.0} Hz ({e})"
        );
    }
}
