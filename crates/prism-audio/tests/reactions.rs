//! A5.1 — a reaction rewrites a **live** patch, audibly.
//!
//! A `BigraphicalReactiveSystem` is wired over a patch sub-bigraph (two
//! oscillators mixing into one `Signal` bus). A `prune_oscillator(330)` rule
//! fires on the first tick and `_remove`s the 330 Hz oscillator node; the engine
//! drops it and `discover_processes` stops running it, so from the next block the
//! bus carries only the 220 Hz partial. We render the patch and probe the late
//! window with a single-bin Goertzel: the pruned partial is gone, the surviving
//! one untouched — proof that prism's BRS rewired a running patch, with no audio
//! machinery of its own (the cell-colony mechanism, applied to sound).

use prism_audio::{
    module_node, prune_oscillator, render_patch_brs, signal_type, silence, spawn_voice,
    voice_composite_node,
};
use prism_bigraph::{Schema, Value};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;
const N: usize = 80;

fn osc_config(freq: f64) -> Value {
    Value::tree([
        ("wave", Value::String("Sine".into())),
        ("freq", Value::float(freq)),
        ("amplitude", Value::float(0.5)),
        ("sample_rate", Value::float(RATE)),
        ("block", Value::Int(BLOCK as i64)),
    ])
}

/// Single-bin Goertzel magnitude — the energy at `freq` over `samples`,
/// normalized by length so a pure sine of amplitude A reads ≈ A/2 at its bin and
/// ≈ 0 elsewhere. Dependency-free and deterministic.
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

/// A patch: two sine oscillators (220, 330) summed into a `Signal` bus `mix`,
/// nested under a `patch` slot. With `brs`, a `local:PatchBrs` node sits beside
/// it, wired over `patch` (the sub-bigraph it rewrites).
fn two_osc_patch(with_brs: bool) -> (Value, Schema) {
    let osc = |freq: f64, phase: &str| {
        module_node(
            "local:Oscillator",
            osc_config(freq),
            &[("phase", phase)],
            &[("phase", phase), ("out", "mix")],
        )
    };
    let patch = Value::tree([
        ("mix", silence(BLOCK)),
        ("phase_a", Value::float(0.0)),
        ("phase_b", Value::float(0.0)),
        ("osc_a", osc(220.0, "phase_a")),
        ("osc_b", osc(330.0, "phase_b")),
    ]);

    let mut root = vec![("patch".to_string(), patch)];
    if with_brs {
        root.push((
            "brs".to_string(),
            prism_audio::patch_brs_node("patch", BLOCK as f64 / RATE),
        ));
    }
    let state = Value::tree(root);

    // Only the data slots need declared types (mix = Signal, the apply-critical
    // bus; phases overwrite). The module / BRS nodes are discovered.
    let schema = Schema::tree([(
        "patch",
        Schema::tree([
            ("mix", signal_type()),
            ("phase_a", Schema::overwrite(Schema::float())),
            ("phase_b", Schema::overwrite(Schema::float())),
        ]),
    )]);
    (state, schema)
}

#[test]
fn prune_removes_a_running_partial() {
    // With the BRS: the 330 Hz oscillator is pruned after the first tick.
    let (state, schema) = two_osc_patch(true);
    let pruned = render_patch_brs(
        state,
        schema,
        vec![prune_oscillator(330.0)],
        &["patch", "mix"],
        BLOCK,
        N,
    );

    // Control: the same two oscillators with no BRS node — both partials persist
    // the whole render (the rules are irrelevant: nothing addresses PatchBrs).
    let (cstate, cschema) = two_osc_patch(false);
    let both = render_patch_brs(cstate, cschema, vec![], &["patch", "mix"], BLOCK, N);

    assert_eq!(pruned.len(), both.len(), "same render length");
    assert_eq!(pruned.len(), N * BLOCK, "N blocks rendered");

    // Probe the late half — well past the firing tick and any pipeline transient.
    let lo = (N / 2) * BLOCK;
    let (e220_both, e330_both) = (
        goertzel(&both[lo..], 220.0, RATE),
        goertzel(&both[lo..], 330.0, RATE),
    );
    let (e220_pruned, e330_pruned) = (
        goertzel(&pruned[lo..], 220.0, RATE),
        goertzel(&pruned[lo..], 330.0, RATE),
    );

    // Control carries both partials.
    assert!(e220_both > 0.1, "control: 220 present (got {e220_both})");
    assert!(e330_both > 0.1, "control: 330 present (got {e330_both})");

    // After the reaction fires: 220 survives ~unchanged, 330 is gone.
    assert!(
        e220_pruned > 0.8 * e220_both,
        "pruned: 220 survives ({e220_pruned} vs control {e220_both})"
    );
    assert!(
        e330_pruned < 0.1 * e330_both,
        "pruned: 330 removed ({e330_pruned} vs control {e330_both}) — the reaction rewired the live patch"
    );
}

/// A rack of `Voice` composites + a `VoiceSeed`. The reaction `spawn_voice(330)`
/// consumes the seed and `_add`s a self-contained 330 Hz Voice composite — its
/// inner phase/level/buses correctly typed because the spec carries its own
/// schema. The new voice spins up and mixes in: a partial APPEARS in the live
/// rack. (A5.2 — the audio image of a cell colony adding a daughter; the spawn
/// half of Detune.)
fn rack(with_seed: bool) -> (Value, Schema) {
    let mut rack_entries = vec![
        ("mix".to_string(), silence(BLOCK)),
        (
            "voice_a".to_string(),
            voice_composite_node(220.0, 1200.0, 1.0, 0.5, BLOCK, RATE, "mix"),
        ),
    ];
    if with_seed {
        rack_entries.push((
            "seed".to_string(),
            Value::tree([("_type", Value::String("VoiceSeed".into()))]),
        ));
    }

    let mut root = vec![("rack".to_string(), Value::tree(rack_entries))];
    if with_seed {
        root.push((
            "brs".to_string(),
            prism_audio::patch_brs_node("rack", BLOCK as f64 / RATE),
        ));
    }
    let schema = Schema::tree([("rack", Schema::tree([("mix", signal_type())]))]);
    (Value::tree(root), schema)
}

#[test]
fn spawn_voice_adds_an_audible_partial() {
    // With the seed + BRS: a 330 Hz voice is spawned onto the running rack.
    let (sstate, sschema) = rack(true);
    let spawned = render_patch_brs(
        sstate,
        sschema,
        vec![spawn_voice(330.0, "mix", BLOCK, RATE)],
        &["rack", "mix"],
        BLOCK,
        N,
    );

    // Control: the same rack with no seed — only the original 220 Hz voice.
    let (cstate, cschema) = rack(false);
    let control = render_patch_brs(cstate, cschema, vec![], &["rack", "mix"], BLOCK, N);

    assert_eq!(spawned.len(), N * BLOCK, "N blocks rendered");

    let lo = (N / 2) * BLOCK;
    let (e220_c, e330_c) = (
        goertzel(&control[lo..], 220.0, RATE),
        goertzel(&control[lo..], 330.0, RATE),
    );
    let (e220_s, e330_s) = (
        goertzel(&spawned[lo..], 220.0, RATE),
        goertzel(&spawned[lo..], 330.0, RATE),
    );

    // The original voice plays in both; only the control LACKS the 330 voice.
    assert!(e220_c > 0.05, "control: original 220 voice present ({e220_c})");
    assert!(e330_c < 0.02, "control: no 330 voice ({e330_c})");

    // After the reaction fires: 220 still plays, and a 330 voice has appeared.
    assert!(e220_s > 0.05, "spawned: 220 still present ({e220_s})");
    assert!(
        e330_s > 0.05 && e330_s > 5.0 * e330_c.max(1e-6),
        "spawned: a 330 Hz voice was added to the live rack ({e330_s} vs control {e330_c})"
    );
}
