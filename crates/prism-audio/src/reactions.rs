//! A5 — reactions rewriting a **live** patch.
//!
//! A patch is a bigraph held as data ([`crate::patch`]); a
//! [`BigraphicalReactiveSystem`] rewrites it. This module supplies the audio
//! reaction **rules** (as plain `prism_schema::reaction` data) and a runner that
//! wires a BRS over a patch sub-bigraph *inside the engine*, so a reaction fires
//! DURING the render and you hear the patch change (synthesis-bigraphs.md
//! §VI/§X A5 — "you hear a reaction fire").
//!
//! Nothing here re-implements matching/firing (`CLAUDE.md`, the thin-layer
//! rule): the rules are ordinary `ReactionRule`s and the BRS is prism's. The
//! engine applies the localized `_add`/`_remove` delta the BRS emits and
//! `discover_processes` brings the rewritten patch to life — the *exact*
//! mechanism a cell colony grows and divides by
//! (`prism-bigraph/tests/reaction_creates_process.rs`). The synth is a new
//! consumer of that substrate, not new substrate.

use std::sync::{Arc, OnceLock};

use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::ProcessNode;
use prism_bigraph::{BigraphicalReactiveSystem, Core, Engine, Key, Schema, StateMap, Value};
use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};

use crate::patch::register_audio;
use crate::render::render_path;
use crate::signal::signal_registry;
use crate::voice::voice_composite_node;

/// The registered address of the patch-BRS factory. A patch node addressed
/// `local:PatchBrs` is instantiated as a [`BigraphicalReactiveSystem`] carrying
/// the rules the factory closes over (see [`render_patch_brs`]).
pub const PATCH_BRS: &str = "PatchBrs";

/// **Prune** — a reaction that removes the oscillator running at frequency
/// `freq` from a patch.
///
/// The redex selects an `Oscillator` module node (`_type: "process"`, `address:
/// "local:Oscillator"`) whose `config.freq` equals `freq`; the placeholder key
/// `osc` matches combinatorially so the match lands at the *parent* with
/// `key_map["osc"]` bound to the node's actual key. The computed reactum emits
/// `{_remove: [<that key>]}`. Firing deletes the module node: the engine drops
/// it, `discover_processes` stops running it, and the bus it fed loses that
/// partial — the live patch is rewired, audibly. (The exact-`freq` form is the
/// simplest correct selector; a guard-driven `prune_above(threshold)` is the
/// next refinement.)
pub fn prune_oscillator(freq: f64) -> ReactionRule {
    let redex = Pattern::map([(
        "osc",
        Pattern::map([
            ("_type", Pattern::atom(Value::String("process".into()))),
            (
                "address",
                Pattern::atom(Value::String("local:Oscillator".into())),
            ),
            (
                "config",
                Pattern::map([("freq", Pattern::atom(Value::float(freq)))]),
            ),
        ]),
    )]);
    let reactum_fn: ReactumFn = Arc::new(|b: &Bindings| {
        let key = b
            .key_map
            .get("osc")
            .map(|k| k.to_string())
            .unwrap_or_default();
        Value::tree([("_remove", Value::List(vec![Value::String(key)]))])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("prune")
        .with_reactum_fn(reactum_fn)
}

/// A patch node wiring a BRS over the sub-bigraph at `patch_slot`
/// (`~{state: [patch_slot]} ->{state: [patch_slot]}`) — the same shape the
/// colony BRS uses over `world` in `reaction_creates_process.rs`. `interval` is
/// the BRS's logical tick; set it to the audio block interval
/// (`block / sample_rate`) so the reaction is scheduled alongside the modules
/// rather than once per logical second.
pub fn patch_brs_node(patch_slot: &str, interval: f64) -> Value {
    let wire = Value::List(vec![Value::String(patch_slot.into())]);
    Value::tree([
        ("_type", Value::String("process".into())),
        ("address", Value::String(format!("local:{PATCH_BRS}"))),
        (
            "config",
            Value::tree([
                ("mode", Value::String("deterministic".into())),
                ("interval", Value::float(interval)),
            ]),
        ),
        ("inputs", Value::tree([("state", wire.clone())])),
        ("outputs", Value::tree([("state", wire)])),
    ])
}

/// Render a patch that contains a `local:PatchBrs` node, supplying `rules` to the
/// BRS factory. Rules carry `Pattern`s / closures, so they can't live in
/// serializable config — the factory closes over them (mirrors the `SpawnBrs`
/// factory in `reaction_creates_process.rs`). The audio modules and the `Signal`
/// type are registered exactly as in [`crate::render_patch`]; the engine fires
/// the BRS, applies its delta schema-aware, and discovers the rewritten patch —
/// all real prism mechanism. The output `Signal` is read at the nested
/// `out_path` (e.g. `["patch", "mix"]`).
pub fn render_patch_brs(
    state: Value,
    schema: Schema,
    rules: Vec<ReactionRule>,
    out_path: &[&str],
    block: usize,
    n_blocks: usize,
) -> Vec<f32> {
    let mut engine = audio_engine(state, schema, rules, block);
    render_path(&mut engine, out_path, n_blocks)
}

/// Build the audio patch engine and return it ready to tick: the standard module
/// registry + a `Composite` factory (for Voice composites) + a `PatchBrs` factory
/// carrying `rules`, the `Signal` type registry, and the patch `state`/`schema`
/// discovered. The shared builder behind both offline render ([`render_patch_brs`])
/// and live playback ([`crate::device::run_realtime`]).
pub fn audio_engine(state: Value, schema: Schema, rules: Vec<ReactionRule>, block: usize) -> Engine {
    // The Composite factory needs the whole Core (a spawned voice subengine
    // inherits types/processes); the Core holds the registry that holds this
    // factory — the cycle is resolved by a OnceLock set once everything is built
    // (the spatio-flux / chrysalis prelude pattern).
    let handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());
    let mut reg = ProcessRegistry::new();
    register_audio(&mut reg);
    {
        let handle = Arc::clone(&handle);
        reg.register("Composite", move |config| {
            let core = handle.get().expect("core handle initialized");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("Composite::from_config"),
            ))
        });
    }
    let rules = Arc::new(rules);
    reg.register(PATCH_BRS, move |c| {
        ProcessNode::Process(Box::new(BigraphicalReactiveSystem::from_config(
            (*rules).clone(),
            &c,
        )))
    });
    let core = Core::new()
        .with_processes(Arc::new(reg))
        .with_types(signal_registry(block));
    let _ = handle.set(core.clone());
    let mut engine =
        Engine::from_state(schema, state, core).expect("build patch+brs engine from state");
    engine.discover_all_processes();
    engine
}

/// **Spawn a voice** — a reaction that brings a new [`voice_composite_node`] to
/// life on a running rack. The redex consumes a `VoiceSeed` marker (the
/// fire-once trigger, à la `reaction_creates_process.rs`); the computed reactum
/// `_remove`s the seed and `_add`s a Voice composite at `freq`, its `out` wired
/// to `out_slot` (the rack mix bus). Discovery instantiates the voice subengine
/// next tick — its inner `phase`/`level`/buses correctly typed because the spec
/// carries its own schema — and the new partial mixes in. The audio image of a
/// cell colony adding a daughter; the spawn half of **Detune** (`Detune` =
/// spawn at a pitch offset of an existing voice).
pub fn spawn_voice(freq: f64, out_slot: &str, block: usize, rate: f64) -> ReactionRule {
    let no_fields: [(&str, Pattern); 0] = [];
    let redex = Pattern::map([("seed", Pattern::sort("VoiceSeed", no_fields))]);
    let out_slot = out_slot.to_string();
    let reactum_fn: ReactumFn = Arc::new(move |b: &Bindings| {
        let seed_key = b
            .key_map
            .get("seed")
            .map(|k| k.to_string())
            .unwrap_or_default();
        let voice = voice_composite_node(freq, 1200.0, 1.0, 0.5, block, rate, &out_slot);
        let mut add = StateMap::new();
        add.insert(Key::from("voice_spawn"), voice);
        Value::tree([
            ("_remove", Value::List(vec![Value::String(seed_key)])),
            ("_add", Value::Map(add)),
        ])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("spawn_voice")
        .with_reactum_fn(reactum_fn)
}

/// Insert `field: value` into a node `Value` (a map), returning it. Used to flag a
/// spawned/matched voice as `detuned` so the Detune redex's NAC won't re-fire on it.
fn with_flag(mut node: Value, field: &str, value: Value) -> Value {
    if let Some(m) = node.as_map_mut() {
        m.insert(Key::from(field), value);
    }
    node
}

/// **Detune** — a reaction that, for each running voice, spawns a copy shifted by
/// `cents` and detunes-once.
///
/// The redex matches a `Voice` composite (its shallow `kind`/`freq` tags), binds
/// the pitch as a site, and carries a **negative application condition**
/// (`detuned: absent`) so it only fires on a voice not yet detuned. The reactum
/// (a) `_add`s a copy at `freq · 2^(cents/1200)`, flagged `detuned`, and (b) flags
/// the MATCHED voice `detuned` in place — so the NAC blocks any re-fire and the
/// rule is **bounded** (each voice detunes exactly once; no runaway spawning).
/// This is the #43 consume/produce-AND-modify-in-place pattern, on audio: the
/// "Detune" capstone of synthesis-bigraphs.md §VI, made audible.
pub fn detune_voice(cents: f64, out_slot: &str, block: usize, rate: f64) -> ReactionRule {
    let redex = Pattern::map([(
        "voice",
        Pattern::map([
            ("_type", Pattern::atom(Value::String("composite".into()))),
            ("kind", Pattern::atom(Value::String("Voice".into()))),
            ("freq", Pattern::site()),
            ("detuned", Pattern::absent()),
        ]),
    )]);
    let out_slot = out_slot.to_string();
    let reactum_fn: ReactumFn = Arc::new(move |b: &Bindings| {
        let voice_key = b
            .key_map
            .get("voice")
            .map(|k| k.to_string())
            .unwrap_or_default();
        let freq = b.sites.get("freq").and_then(|v| v.as_f64()).unwrap_or(220.0);
        let detuned_freq = freq * 2.0_f64.powf(cents / 1200.0);
        let copy = with_flag(
            voice_composite_node(detuned_freq, 1200.0, 1.0, 0.5, block, rate, &out_slot),
            "detuned",
            Value::Bool(true),
        );
        let mut add = StateMap::new();
        add.insert(Key::from(format!("{voice_key}_dt").as_str()), copy);
        // Localized delta at the rack: add the detuned copy + flag the source voice
        // in place (so the NAC blocks any further fire on it).
        let mut delta = StateMap::new();
        delta.insert(Key::from("_add"), Value::Map(add));
        delta.insert(
            Key::from(voice_key.as_str()),
            Value::tree([("detuned", Value::Bool(true))]),
        );
        Value::Map(delta)
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("detune")
        .with_reactum_fn(reactum_fn)
}
