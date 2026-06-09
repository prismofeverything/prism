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

use std::sync::Arc;

use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::ProcessNode;
use prism_bigraph::{BigraphicalReactiveSystem, Core, Engine, Schema, Value};
use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};

use crate::patch::register_audio;
use crate::render::render_path;
use crate::signal::signal_registry;

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
    let mut reg = ProcessRegistry::new();
    register_audio(&mut reg);
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
    let mut engine =
        Engine::from_state(schema, state, core).expect("build patch+brs engine from state");
    engine.discover_all_processes();
    render_path(&mut engine, out_path, n_blocks)
}
