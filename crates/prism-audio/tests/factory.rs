//! A6 — the homoiconic module factory, proven structurally and audibly.
//!
//! A `StackSeed{ base, spread, voices }` recipe sits on a rack beside a
//! `local:PatchBrs`. The generic `stack_factory` reaction reads that recipe and
//! `_add`s a freshly AUTHORED compound module type — `voices` oscillators detuned
//! across `±spread` cents around `base`, summed into one bus. Discovery
//! instantiates the new subengine and its partials mix in. We probe with a
//! single-bin Goertzel: every authored partial is present (the generated
//! structure RAN), the *count* tracks the recipe (a 2- and a 3-voice seed author
//! DIFFERENT types from the SAME rule), and the sum stays bounded.

use prism_audio::{
    patch_brs_node, render_patch_brs, signal_type, silence, stack_factory, stack_voice_node,
    StackRecipe,
};
use prism_bigraph::{Schema, Value};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;
const N: usize = 80;
const SPREAD: f64 = 700.0; // wide enough that the detuned partials are Goertzel-separable

/// Single-bin Goertzel magnitude — energy at `freq` over `samples`, normalized by
/// length so a pure sine of amplitude A reads ≈ A/2 at its bin and ≈ 0 elsewhere.
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

/// A rack carrying a `StackSeed{ base, spread, voices }` recipe + the factory BRS
/// over `rack`. The seed has EXACTLY the recipe fields (no surplus, so each site
/// captures its bare value — see `stack_factory`).
fn seeded_rack(base: f64, spread: f64, voices: usize) -> (Value, Schema) {
    let state = Value::tree([
        (
            "rack",
            Value::tree([
                ("mix", silence(BLOCK)),
                (
                    "seed",
                    Value::tree([
                        ("_type", Value::String("StackSeed".into())),
                        ("base", Value::float(base)),
                        ("spread", Value::float(spread)),
                        ("voices", Value::float(voices as f64)),
                    ]),
                ),
            ]),
        ),
        ("brs", patch_brs_node("rack", BLOCK as f64 / RATE)),
    ]);
    let schema = Schema::tree([("rack", Schema::tree([("mix", signal_type())]))]);
    (state, schema)
}

/// Render a seeded rack through the factory and return the rack's mix bus.
fn render_stack(base: f64, spread: f64, voices: usize) -> Vec<f32> {
    let (state, schema) = seeded_rack(base, spread, voices);
    render_patch_brs(
        state,
        schema,
        vec![stack_factory("mix", BLOCK, RATE, 0.6)],
        &["rack", "mix"],
        BLOCK,
        N,
    )
}

#[test]
fn factory_authors_one_oscillator_per_voice() {
    // The factory FUNCTION, exercised directly: a 3-voice recipe lowers to a
    // composite with three oscillators + three phase slots + the shared bus —
    // the inner structure is GENERATED from the recipe.
    let recipe = StackRecipe::super_saw(220.0, SPREAD, 3, 0.6);
    let node = stack_voice_node(&recipe, BLOCK, RATE, "mix");

    assert_eq!(
        node.get_field("_type").and_then(|v| v.as_str()),
        Some("composite"),
        "the authored type is a composite spec"
    );
    assert_eq!(
        node.get_field("kind").and_then(|v| v.as_str()),
        Some("Stack")
    );

    let inner = node
        .get_field("config")
        .and_then(|c| c.get_field("state"))
        .expect("inner state");
    let osc_count = inner
        .iter_fields()
        .expect("inner is a map")
        .filter(|(k, _)| k.starts_with("osc_"))
        .count();
    let phase_count = inner
        .iter_fields()
        .expect("inner is a map")
        .filter(|(k, _)| k.starts_with("phase_"))
        .count();
    assert_eq!(osc_count, 3, "one oscillator authored per voice");
    assert_eq!(phase_count, 3, "one phase slot per oscillator");
    assert!(inner.get_field("mix").is_some(), "the shared mix bus");

    // A 5-voice recipe is a DIFFERENT type — more inner nodes — from the same fn.
    let five = stack_voice_node(&StackRecipe::super_saw(220.0, SPREAD, 5, 0.6), BLOCK, RATE, "mix");
    let five_inner = five.get_field("config").and_then(|c| c.get_field("state")).unwrap();
    let five_osc = five_inner.iter_fields().unwrap().filter(|(k, _)| k.starts_with("osc_")).count();
    assert_eq!(five_osc, 5, "the recipe length sets the type's oscillator count");
}

#[test]
fn factory_brings_a_new_stack_type_to_life() {
    // Seed a 3-voice recipe; the reaction authors + installs the type live.
    let out = render_stack(220.0, SPREAD, 3);
    assert_eq!(out.len(), N * BLOCK, "N blocks rendered");

    // super_saw(220, 700, 3) detunes to {-700, 0, +700} cents → three partials.
    let (lo, base) = ((N / 2) * BLOCK, 220.0);
    let f_lo = cents(base, -SPREAD); // ~146.8 Hz
    let f_mid = base; // 220 Hz (the centre voice)
    let f_hi = cents(base, SPREAD); // ~329.6 Hz

    let (e_lo, e_mid, e_hi) = (
        goertzel(&out[lo..], f_lo, RATE),
        goertzel(&out[lo..], f_mid, RATE),
        goertzel(&out[lo..], f_hi, RATE),
    );

    // All three AUTHORED partials sound — the generated structure ran.
    assert!(
        e_lo > 0.05 && e_mid > 0.05 && e_hi > 0.05,
        "all three authored partials present (lo {e_lo}, mid {e_mid}, hi {e_hi})"
    );
    // Bounded: each oscillator is amplitude/3 ≈ 0.2 (Goertzel ≈ 0.1); no runaway.
    assert!(
        e_lo < 0.4 && e_mid < 0.4 && e_hi < 0.4,
        "bounded stack (lo {e_lo}, mid {e_mid}, hi {e_hi})"
    );
}

#[test]
fn distinct_recipes_author_distinct_types_from_one_rule() {
    // The SAME `stack_factory` rule, two different seeds → two different types.
    // The centre frequency is the discriminator: an odd voice count authors a
    // centre oscillator at `base`; an even count does not.
    let base = 220.0;
    let f_lo = cents(base, -SPREAD); // ~146.8 — present in both
    let f_hi = cents(base, SPREAD); // ~329.6 — present in both

    let lo = (N / 2) * BLOCK;
    let three = render_stack(base, SPREAD, 3); // {-700, 0, +700} → has a 220 centre
    let two = render_stack(base, SPREAD, 2); //  {-700,    +700} → NO 220 centre

    // Both authored types carry the spread extremes.
    for (label, buf) in [("3-voice", &three), ("2-voice", &two)] {
        assert!(
            goertzel(&buf[lo..], f_lo, RATE) > 0.05 && goertzel(&buf[lo..], f_hi, RATE) > 0.05,
            "{label}: spread extremes present"
        );
    }

    // The centre partial distinguishes the two authored TYPES.
    let three_centre = goertzel(&three[lo..], base, RATE);
    let two_centre = goertzel(&two[lo..], base, RATE);
    assert!(
        three_centre > 0.05,
        "the 3-voice type authored a centre oscillator at {base} Hz ({three_centre})"
    );
    assert!(
        two_centre < 0.02,
        "the 2-voice type authored NO centre — a structurally different type ({two_centre})"
    );
}
