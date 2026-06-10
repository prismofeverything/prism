//! A6 (deepened) — the synth writes a *full instrument*, then REUSES it.
//!
//! [`crate::factory`] authors a detuned oscillator STACK. This module goes richer
//! in the two ways A6 asks for (`synthesis-bigraphs.md` §VII):
//!
//! 1. **A full subtractive INSTRUMENT type.** [`instrument_voice_node`] composes
//!    ALL four native kernels into one authored type: a generated `k`-oscillator
//!    bank (detuned, like the stack) → an inner `mix` `Signal` bus → a `LowPass`
//!    → a `Vca` whose gain is shaped by an `Envelope`. The factory authors a
//!    complete voice (bank → filter → amp+envelope), and its inner node set is
//!    still GENERATED from the recipe.
//!
//! 2. **Author-then-REUSE (AlChemy).** The authored type is stored in state as a
//!    plain-data **blueprint** ([`define_instrument`]); a SECOND reaction
//!    ([`cast_instrument`]) reads that blueprint and instances it at the pitch
//!    each `CastSeed` carries. The product of the first reaction becomes a
//!    building block the later reactions consume — the synth writes an instrument,
//!    then plays a whole ENSEMBLE of it. (The audio image of Fontana AlChemy: a
//!    reaction's product feeds further reactions.)
//!
//! Zero clone, same substrate as the stack: an instance is a `{_type:
//! "composite"}` spec brought to life by `discover_processes` (the #61 mechanism).
//! The blueprint is plain data (no `_type`), so discovery never instantiates the
//! template — only the casts.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{Key, Schema, StateMap, Value};
use prism_schema::reaction::{Bindings, Pattern, ReactionRule, ReactumFn};
use prism_schema::schema_to_value;

use crate::factory::{detune, osc_cfg, spread_intervals};
use crate::patch::module_node;
use crate::signal::{signal_type, silence};
use crate::voice::mod_cfg;

/// A recipe for a full subtractive INSTRUMENT type — richer than a
/// [`StackRecipe`](crate::factory::StackRecipe): the oscillator bank PLUS a filter
/// cutoff and an amplitude envelope. Round-trips through [`to_value`](Self::to_value)
/// / [`from_value`](Self::from_value) as plain data, so it can be stored in state
/// as a blueprint a later reaction reads back (the type-as-data, homoiconic).
#[derive(Clone, Debug)]
pub struct InstrumentRecipe {
    /// Centre frequency in Hz.
    pub base: f64,
    /// One oscillator per entry, detuned `cents` from `base`.
    pub intervals: Vec<f64>,
    /// Total amplitude, split across the bank.
    pub amplitude: f64,
    /// `LowPass` cutoff (Hz).
    pub cutoff: f64,
    /// `Envelope` attack / release (seconds).
    pub attack: f64,
    pub release: f64,
    /// Sustained gate level driving the envelope.
    pub gate: f64,
}

impl InstrumentRecipe {
    /// A subtractive voice preset: a `voices`-wide detuned bank around `base`,
    /// filtered at `cutoff`, with a short envelope. The four salient params
    /// `define_instrument` reads from a seed; the rest are sensible defaults.
    pub fn voice(base: f64, spread_cents: f64, voices: usize, cutoff: f64) -> Self {
        Self {
            base,
            intervals: spread_intervals(spread_cents, voices),
            amplitude: 0.8,
            cutoff,
            attack: 0.02,
            release: 0.2,
            gate: 1.0,
        }
    }

    /// The authored type's oscillator count (≥ 1).
    pub fn voices(&self) -> usize {
        self.intervals.len().max(1)
    }

    /// Serialize the recipe as plain DATA — the **blueprint** stored in state.
    pub fn to_value(&self) -> Value {
        Value::tree([
            ("base", Value::float(self.base)),
            (
                "intervals",
                Value::List(self.intervals.iter().map(|c| Value::float(*c)).collect()),
            ),
            ("amplitude", Value::float(self.amplitude)),
            ("cutoff", Value::float(self.cutoff)),
            ("attack", Value::float(self.attack)),
            ("release", Value::float(self.release)),
            ("gate", Value::float(self.gate)),
        ])
    }

    /// Reconstruct a recipe from its blueprint data (inverse of [`to_value`](Self::to_value));
    /// missing fields fall back to sane defaults.
    pub fn from_value(v: &Value) -> Self {
        let f = |k: &str, d: f64| v.get_field(k).and_then(|x| x.as_f64()).unwrap_or(d);
        let intervals = v
            .get_field("intervals")
            .and_then(|x| x.as_list())
            .map(|l| l.iter().filter_map(|c| c.as_f64()).collect::<Vec<_>>())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| vec![0.0]);
        Self {
            base: f("base", 220.0),
            intervals,
            amplitude: f("amplitude", 0.8),
            cutoff: f("cutoff", 1500.0),
            attack: f("attack", 0.02),
            release: f("release", 0.2),
            gate: f("gate", 1.0),
        }
    }
}

/// **The instrument factory.** Lower an [`InstrumentRecipe`] into a NEW full-voice
/// compound type: a generated oscillator bank summed into an inner `mix` bus, a
/// `LowPass` at `cutoff`, and a `Vca` whose gain follows an `Envelope` — bridged
/// to one `out` wired to `out_slot`. Self-contained and schema-carrying (`mix` /
/// `filtered` / `out` are `Signal`; `phase_i` / `z1` / `level` overwrite). The
/// inner node set is GENERATED from the recipe — the rich-type analogue of
/// [`stack_voice_node`](crate::factory::stack_voice_node).
pub fn instrument_voice_node(
    recipe: &InstrumentRecipe,
    block: usize,
    rate: f64,
    out_slot: &str,
) -> Value {
    let fallback = [0.0];
    let intervals: &[f64] = if recipe.intervals.is_empty() {
        &fallback
    } else {
        &recipe.intervals
    };
    let voices = intervals.len();
    let per_voice_amp = recipe.amplitude / voices as f64;

    // The oscillator bank → shared `mix` bus (the stack geometry).
    let mut inner: Vec<(String, Value)> = vec![("mix".to_string(), silence(block))];
    let mut branches: IndexMap<Key, Schema> = IndexMap::new();
    branches.insert(Key::from("mix"), signal_type());
    for (i, &cents) in intervals.iter().enumerate() {
        let phase_key = format!("phase_{i}");
        let freq = detune(recipe.base, cents);
        inner.push((phase_key.clone(), Value::float(0.0)));
        inner.push((
            format!("osc_{i}"),
            module_node(
                "local:Oscillator",
                osc_cfg(freq, per_voice_amp, block, rate),
                &[("phase", phase_key.as_str())],
                &[("phase", phase_key.as_str()), ("out", "mix")],
            ),
        ));
        branches.insert(
            Key::from(phase_key.as_str()),
            Schema::overwrite(Schema::float()),
        );
    }

    // The subtractive tail: LowPass(mix) → filtered → Vca(filtered, level) → out,
    // level driven by Envelope(gate). The `voice_composite_node` chain, but fed by
    // the generated bank instead of a single oscillator.
    for (k, v) in [
        ("z1", Value::float(0.0)),
        ("filtered", silence(block)),
        ("gate", Value::float(recipe.gate)),
        ("level", Value::float(0.0)),
        ("out", silence(block)),
    ] {
        inner.push((k.to_string(), v));
    }
    inner.push((
        "filt".to_string(),
        module_node(
            "local:LowPass",
            mod_cfg(rate, block, &[("cutoff", recipe.cutoff)]),
            &[("in", "mix"), ("z1", "z1")],
            &[("out", "filtered"), ("z1", "z1")],
        ),
    ));
    inner.push((
        "env".to_string(),
        module_node(
            "local:Envelope",
            mod_cfg(rate, block, &[("attack", recipe.attack), ("release", recipe.release)]),
            &[("gate", "gate"), ("level", "level")],
            &[("level", "level")],
        ),
    ));
    inner.push((
        "amp".to_string(),
        module_node(
            "local:Vca",
            mod_cfg(rate, block, &[]),
            &[("in", "filtered"), ("gain", "level")],
            &[("out", "out")],
        ),
    ));
    for (k, s) in [
        ("z1", Schema::overwrite(Schema::float())),
        ("filtered", signal_type()),
        ("gate", Schema::float()),
        ("level", Schema::overwrite(Schema::float())),
        ("out", signal_type()),
    ] {
        branches.insert(Key::from(k), s);
    }

    let bridge = Value::tree([
        ("inputs", Value::map()),
        (
            "outputs",
            Value::tree([("out", Value::List(vec![Value::String("out".into())]))]),
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
        ("kind", Value::String("Instrument".into())),
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

/// **define** — author an instrument type and store it as a blueprint in state.
/// Consumes a `DefineSeed{ base, spread, voices, cutoff }` and `_add`s the lowered
/// recipe under a `blueprint` slot (plain data — discovery ignores it: no
/// `_type`). The synth has written a new instrument *type*; it does not play yet.
pub fn define_instrument() -> ReactionRule {
    let redex = Pattern::map([(
        "def",
        Pattern::sort(
            "DefineSeed",
            [
                ("base", Pattern::site()),
                ("spread", Pattern::site()),
                ("voices", Pattern::site()),
                ("cutoff", Pattern::site()),
            ],
        ),
    )]);
    let reactum_fn: ReactumFn = Arc::new(move |b: &Bindings| {
        let def_key = b
            .key_map
            .get("def")
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
        let cutoff = b
            .sites
            .get("cutoff")
            .and_then(|v| v.as_f64())
            .unwrap_or(1500.0);
        let recipe = InstrumentRecipe::voice(base, spread, voices, cutoff);
        let mut add = StateMap::new();
        add.insert(Key::from("blueprint"), recipe.to_value());
        Value::tree([
            ("_remove", Value::List(vec![Value::String(def_key)])),
            ("_add", Value::Map(add)),
        ])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("define_instrument")
        .with_reactum_fn(reactum_fn)
}

/// **cast** — REUSE the authored blueprint: instance the stored instrument type at
/// the pitch a `CastSeed{ freq }` carries.
///
/// The redex binds the whole `blueprint` node via an as-pattern (`Pattern::Bind`,
/// so it is NOT a surplus-absorbing `Site` at the rack level — a bare site would
/// swallow the sibling slots by Milner rest-capture) and each `CastSeed`
/// combinatorially. The reactum reads the blueprint, overrides its `base` to the
/// cast pitch, lowers it with [`instrument_voice_node`], removes the seed, and
/// `_add`s a live instance wired to `out_slot`. Fire it for several seeds → an
/// ENSEMBLE of the one authored type: the product became a building block.
pub fn cast_instrument(out_slot: &str, block: usize, rate: f64) -> ReactionRule {
    let no_fields: [(&str, Pattern); 0] = [];
    let redex = Pattern::map([
        (
            "cast",
            Pattern::sort("CastSeed", [("freq", Pattern::site())]),
        ),
        (
            "blueprint",
            Pattern::Bind {
                name: Key::from("bp"),
                inner: Box::new(Pattern::map(no_fields)),
            },
        ),
    ]);
    let out_slot = out_slot.to_string();
    let reactum_fn: ReactumFn = Arc::new(move |b: &Bindings| {
        let cast_key = b
            .key_map
            .get("cast")
            .map(|k| k.to_string())
            .unwrap_or_default();
        let freq = b.sites.get("freq").and_then(|v| v.as_f64()).unwrap_or(220.0);
        let blueprint = b.sites.get("bp").cloned().unwrap_or(Value::None);
        let mut recipe = InstrumentRecipe::from_value(&blueprint);
        recipe.base = freq; // cast the stored type at the seed's pitch
        let inst = instrument_voice_node(&recipe, block, rate, &out_slot);
        let mut add = StateMap::new();
        add.insert(Key::from(format!("voice_{cast_key}").as_str()), inst);
        Value::tree([
            ("_remove", Value::List(vec![Value::String(cast_key)])),
            ("_add", Value::Map(add)),
        ])
    });
    ReactionRule::new(redex, Pattern::Site)
        .with_label("cast_instrument")
        .with_reactum_fn(reactum_fn)
}
