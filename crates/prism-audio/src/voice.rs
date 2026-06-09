//! A `Voice` as a self-contained **composite** — the unit a reaction spawns,
//! and (later) merges/splits (A7).
//!
//! `render_voice` (in [`crate::render`]) hand-wires four flat processes; here a
//! voice is a *composite node* `{_type: "composite", address: "local:Composite",
//! config: {state, bridge, schema, interval}, inputs, outputs}`. The crucial
//! property for A5 (reactions that SPAWN voices): the spec **carries its own
//! inner schema** (`config.schema`), so when a reaction `_add`s a voice the
//! inner `phase`/`z1`/`level` slots land as `overwrite(float)` and the buses as
//! `Signal` — *typed correctly*, with no schema-on-add hole. This is exactly how
//! a reaction spawns a live cell composite
//! (`prism-bigraph/tests/reaction_creates_process.rs::reaction_creates_a_live_composite`),
//! applied to audio. (synthesis-bigraphs.md §V "a voice is a composite", §VIII.)
//!
//! Time: the composite's `interval` is set to the audio block interval so it
//! produces exactly one block per outer tick; per #70 the inner modules keep
//! their own dt and the composite stays time-transparent
//! (`chrysalis/tests/composite_interval.rs`, the manifold invariant).

use prism_bigraph::{Schema, Value};
use prism_schema::schema_to_value;

use crate::patch::module_node;
use crate::signal::{signal_type, silence};

/// A module `config` map: `sample_rate` + `block` + the named float `extras`.
fn mod_cfg(rate: f64, block: usize, extras: &[(&str, f64)]) -> Value {
    let mut v = vec![
        ("sample_rate".to_string(), Value::float(rate)),
        ("block".to_string(), Value::Int(block as i64)),
    ];
    for (k, x) in extras {
        v.push((k.to_string(), Value::float(*x)));
    }
    Value::tree(v)
}

/// A `Voice` composite spec: `osc → LowPass → Vca`, amplitude shaped by an
/// `Envelope` (gated internally at `gate`). The inner `out` is bridged to the
/// composite's `out` port; the node's `outputs` wire that port to `out_slot`
/// (a Rack mix bus — many voices summing there via the additive `Signal`
/// reconcile). Self-contained and schema-carrying, so a reaction can spawn it.
#[allow(clippy::too_many_arguments)]
pub fn voice_composite_node(
    freq: f64,
    cutoff: f64,
    gate: f64,
    amplitude: f64,
    block: usize,
    rate: f64,
    out_slot: &str,
) -> Value {
    let osc_cfg = Value::tree([
        ("wave", Value::String("Sine".into())),
        ("freq", Value::float(freq)),
        ("amplitude", Value::float(amplitude)),
        ("sample_rate", Value::float(rate)),
        ("block", Value::Int(block as i64)),
    ]);
    let inner_state = Value::tree([
        ("phase", Value::float(0.0)),
        ("raw", silence(block)),
        ("z1", Value::float(0.0)),
        ("filtered", silence(block)),
        ("gate", Value::float(gate)),
        ("level", Value::float(0.0)),
        ("out", silence(block)),
        (
            "osc",
            module_node(
                "local:Oscillator",
                osc_cfg,
                &[("phase", "phase")],
                &[("phase", "phase"), ("out", "raw")],
            ),
        ),
        (
            "filt",
            module_node(
                "local:LowPass",
                mod_cfg(rate, block, &[("cutoff", cutoff)]),
                &[("in", "raw"), ("z1", "z1")],
                &[("out", "filtered"), ("z1", "z1")],
            ),
        ),
        (
            "env",
            module_node(
                "local:Envelope",
                mod_cfg(rate, block, &[("attack", 0.02), ("release", 0.02)]),
                &[("gate", "gate"), ("level", "level")],
                &[("level", "level")],
            ),
        ),
        (
            "amp",
            module_node(
                "local:Vca",
                mod_cfg(rate, block, &[]),
                &[("in", "filtered"), ("gain", "level")],
                &[("out", "out")],
            ),
        ),
    ]);

    // The apply-critical inner schema (carried with the spec, not inferred):
    // state slots overwrite/replace; buses are `Signal`.
    let inner_schema = Schema::tree([
        ("phase", Schema::overwrite(Schema::float())),
        ("raw", signal_type()),
        ("z1", Schema::overwrite(Schema::float())),
        ("filtered", signal_type()),
        ("gate", Schema::float()),
        ("level", Schema::overwrite(Schema::float())),
        ("out", signal_type()),
    ]);

    // The composite exposes one `out` port, sourced from the inner `out` slot.
    let bridge = Value::tree([
        ("inputs", Value::map()),
        (
            "outputs",
            Value::tree([("out", Value::List(vec![Value::String("out".into())]))]),
        ),
    ]);
    let config = Value::tree([
        ("state", inner_state),
        ("bridge", bridge),
        ("schema", schema_to_value(&inner_schema)),
        ("interval", Value::float(block as f64 / rate)),
    ]);

    Value::tree([
        ("_type", Value::String("composite".into())),
        ("address", Value::String("local:Composite".into())),
        ("config", config),
        ("inputs", Value::map()),
        (
            "outputs",
            Value::tree([("out", Value::List(vec![Value::String(out_slot.into())]))]),
        ),
    ])
}
