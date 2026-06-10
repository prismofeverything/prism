//! A6 — the homoiconic module **factory**: a reaction whose reactum AUTHORS a
//! new compound module *type* as data, which the engine then brings to life.
//!
//! ## Why this is a level above A5
//!
//! [`crate::reactions::spawn_voice`] (A5) `_add`s a
//! [`voice_composite_node`](crate::voice::voice_composite_node) — an INSTANCE of
//! a *fixed* structure (one `osc → filt → vca → env` chain wired in Rust); only
//! `freq` varies. A6 generates the **structure itself**: [`stack_voice_node`]
//! lowers a [`StackRecipe`] into a composite holding *one oscillator per voice*,
//! detuned across a spread and summed into a shared inner `Signal` mix bus. A
//! 2-voice recipe and a 3-voice recipe are therefore DIFFERENT module types —
//! different inner node count, different inner schema — both authored from data.
//! That is the body-level "homoiconic module factory" capstone
//! (`synthesis-bigraphs.md` §VII): *the synth writes a synth — a new module
//! type, not a new instance of a known one.*
//!
//! ## A generic factory: the recipe lives in state
//!
//! [`stack_factory`] is ONE reaction that authors ANY stack: its redex reads a
//! `StackSeed{ base, spread, voices }` marker and the reactum lowers *that*
//! recipe. So the structure produced is a function of the data in the live
//! bigraph, not baked into the rule — seed a 3-voice recipe and a 5-voice recipe
//! and the same rule authors two different types.
//!
//! ## Same substrate, zero clone
//!
//! The authored type is just a `{_type: "composite", …}` spec — a sub-bigraph
//! held as data. The BRS emits only the `_add` delta; the engine applies it
//! schema-aware and `discover_processes` instantiates the new subengine — the
//! EXACT #61 mechanism a cell colony divides by
//! (`prism-bigraph/tests/reaction_creates_process.rs::reaction_creates_a_live_composite`).
//! Nothing here re-implements matching / firing / instantiation (the thin-layer
//! rule); the recipe → spec lowering is the only new code, and it is pure data
//! construction.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{Key, Schema, StateMap, Value};
use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};
use prism_schema::schema_to_value;

use crate::patch::module_node;
use crate::signal::{signal_type, silence};

/// A recipe for a **detuned stack** module type — the DATA a factory lowers into
/// a new compound module. One oscillator is authored per voice; the recipe's
/// `intervals` (cents relative to `base`) set both the inner node count *and* the
/// detune of each oscillator, so a recipe of length *k* authors a *k*-oscillator
/// type. Each oscillator runs at `amplitude / k` so the summed bus stays bounded
/// regardless of voice count. A narrow spread is a "super-saw"; a chord of
/// intervals is a stacked-interval voice — one factory, a family of types.
#[derive(Clone, Debug)]
pub struct StackRecipe {
    /// Centre frequency in Hz; an interval of 0 cents sounds exactly here.
    pub base: f64,
    /// One oscillator per entry, detuned `cents` from `base` (`2^(cents/1200)`).
    pub intervals: Vec<f64>,
    /// Total stack amplitude; split evenly across the oscillators.
    pub amplitude: f64,
}

impl StackRecipe {
    /// A symmetric stack: `voices` oscillators spread linearly across
    /// `±spread_cents` around `base` (the centre voice at `base` when `voices` is
    /// odd). `voices = 1` collapses to a single oscillator at `base`.
    pub fn super_saw(base: f64, spread_cents: f64, voices: usize, amplitude: f64) -> Self {
        let voices = voices.max(1);
        let intervals = (0..voices)
            .map(|i| {
                let t = if voices == 1 {
                    0.0
                } else {
                    (i as f64) / ((voices - 1) as f64) * 2.0 - 1.0 // i: 0..voices-1 → t: -1..1
                };
                t * spread_cents
            })
            .collect();
        Self {
            base,
            intervals,
            amplitude,
        }
    }

    /// The authored type's voice count = its inner oscillator count (≥ 1).
    pub fn voices(&self) -> usize {
        self.intervals.len().max(1)
    }
}

/// `base · 2^(cents/1200)` — equal-tempered detune.
fn detune(base: f64, cents: f64) -> f64 {
    base * 2.0_f64.powf(cents / 1200.0)
}

/// An `Oscillator` `config` map at `freq` / `amplitude`.
fn osc_cfg(freq: f64, amplitude: f64, block: usize, rate: f64) -> Value {
    Value::tree([
        ("wave", Value::String("Sine".into())),
        ("freq", Value::float(freq)),
        ("amplitude", Value::float(amplitude)),
        ("sample_rate", Value::float(rate)),
        ("block", Value::Int(block as i64)),
    ])
}

/// **The factory.** Lower a [`StackRecipe`] into a NEW compound module type: a
/// `local:Composite` whose inner state holds one `Oscillator` per interval (each
/// with its own `phase_i` slot), all summing into a shared `mix` `Signal` bus
/// (the additive reconcile *is* the mixer), bridged to a single `out` port wired
/// to `out_slot`. Self-contained and schema-carrying — the apply-critical inner
/// schema travels WITH the spec (`mix` is `Signal`, each `phase_i` overwrites) —
/// so a reaction can `_add` it and discovery realizes the subengine with every
/// slot correctly typed. The inner node set + schema are GENERATED from the
/// recipe: *this* is the "writes a new type" move (vs `voice_composite_node`'s
/// fixed chain).
pub fn stack_voice_node(recipe: &StackRecipe, block: usize, rate: f64, out_slot: &str) -> Value {
    // An empty recipe still authors one (centre) oscillator, so a Stack is never
    // a silent composite.
    let fallback = [0.0];
    let intervals: &[f64] = if recipe.intervals.is_empty() {
        &fallback
    } else {
        &recipe.intervals
    };
    let voices = intervals.len();
    let per_voice_amp = recipe.amplitude / voices as f64;

    let mut inner: Vec<(String, Value)> = vec![("mix".to_string(), silence(block))];
    let mut branches: IndexMap<Key, Schema> = IndexMap::new();
    branches.insert(Key::from("mix"), signal_type());

    for (i, &cents) in intervals.iter().enumerate() {
        let phase_key = format!("phase_{i}");
        let freq = detune(recipe.base, cents);
        inner.push((phase_key.clone(), Value::float(0.0)));
        let osc = module_node(
            "local:Oscillator",
            osc_cfg(freq, per_voice_amp, block, rate),
            &[("phase", phase_key.as_str())],
            // every oscillator writes its `out` into the SHARED `mix` bus; the
            // `Signal` reconcile sums them (the mix-bus-as-reconcile, A2).
            &[("phase", phase_key.as_str()), ("out", "mix")],
        );
        inner.push((format!("osc_{i}"), osc));
        branches.insert(
            Key::from(phase_key.as_str()),
            Schema::overwrite(Schema::float()),
        );
    }

    // One `out` port, sourced from the inner summed `mix` bus.
    let bridge = Value::tree([
        ("inputs", Value::map()),
        (
            "outputs",
            Value::tree([("out", Value::List(vec![Value::String("mix".into())]))]),
        ),
    ]);
    let inner_schema = Schema::Tree { branches };
    let config = Value::tree([
        ("state", Value::tree(inner)),
        ("bridge", bridge),
        ("schema", schema_to_value(&inner_schema)),
        ("interval", Value::float(block as f64 / rate)),
    ]);

    Value::tree([
        ("_type", Value::String("composite".into())),
        ("address", Value::String("local:Composite".into())),
        // Shallow self-describing tags (ignored by discovery / `from_config`):
        // `kind` marks a stack and `base`/`voices` expose the recipe at the top
        // level, so a later reaction can MATCH a stack without descending.
        ("kind", Value::String("Stack".into())),
        ("base", Value::float(recipe.base)),
        ("voices", Value::Int(voices as i64)),
        ("config", config),
        ("inputs", Value::map()),
        (
            "outputs",
            Value::tree([("out", Value::List(vec![Value::String(out_slot.into())]))]),
        ),
    ])
}

/// **The factory reaction** — a generic rule that authors a new stack module type
/// from a `StackSeed` recipe found in the live bigraph.
///
/// The redex matches a `StackSeed{ base, spread, voices }` marker (the recipe as
/// data, à la `reaction_creates_process.rs`'s `Seed`), binding the three fields
/// as sites. The computed reactum reads them, builds a [`StackRecipe::super_saw`],
/// lowers it with [`stack_voice_node`], `_remove`s the seed (fire-once, so it is
/// **bounded** — one stack per seed, no runaway) and `_add`s the authored stack
/// wired to `out_slot`. Discovery instantiates the new subengine next tick and
/// its detuned partials mix in: the synth has written — and installed — a module
/// type that did not exist in the program, from a recipe in its own state.
///
/// The seed must carry *exactly* `{ _type: StackSeed, base, spread, voices }`
/// (extra fields would be absorbed into the last site by Milner rest-capture).
pub fn stack_factory(out_slot: &str, block: usize, rate: f64, amplitude: f64) -> ReactionRule {
    let redex = Pattern::map([(
        "seed",
        Pattern::sort(
            "StackSeed",
            [
                ("base", Pattern::site()),
                ("spread", Pattern::site()),
                ("voices", Pattern::site()),
            ],
        ),
    )]);
    let out_slot = out_slot.to_string();
    let reactum_fn: ReactumFn = Arc::new(move |b: &Bindings| {
        let seed_key = b
            .key_map
            .get("seed")
            .map(|k| k.to_string())
            .unwrap_or_default();
        let base = b.sites.get("base").and_then(|v| v.as_f64()).unwrap_or(220.0);
        let spread = b.sites.get("spread").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let voices = b
            .sites
            .get("voices")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0)
            .round()
            .max(1.0) as usize;
        let recipe = StackRecipe::super_saw(base, spread, voices, amplitude);
        let stack = stack_voice_node(&recipe, block, rate, &out_slot);
        let mut add = StateMap::new();
        add.insert(Key::from("stack"), stack);
        Value::tree([
            ("_remove", Value::List(vec![Value::String(seed_key)])),
            ("_add", Value::Map(add)),
        ])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("stack_factory")
        .with_reactum_fn(reactum_fn)
}
