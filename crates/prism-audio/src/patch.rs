//! A patch as a **value** — modules as discoverable state nodes.
//!
//! [`crate::render::render_voice`] hand-wires a `Topology` in Rust. Here a patch
//! is *data*: a `Value::Tree` of data slots + module nodes of the form
//! `{_type: "process", address, config, inputs, outputs}`. `Engine::from_state`
//! walks it, finds those nodes by their `_type`, and instantiates them through
//! the [`ProcessRegistry`] (`discover_processes`).
//!
//! This is the pivot toward the generative capstones: once a patch is a value,
//! it is a **bigraph held as data** — the thing a reaction (A5) rewrites and a
//! factory (A6) authors (synthesis-bigraphs.md §I, §VI–VII). The proof is that
//! a patch built as data renders *identically* to the hand-wired one
//! (`tests/patch.rs`).

use std::sync::Arc;

use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::ProcessNode;
use prism_bigraph::{Core, Engine, Schema, Value};

use crate::envelope::Envelope;
use crate::lowpass::LowPass;
use crate::oscillator::Oscillator;
use crate::render::render;
use crate::signal::signal_registry;
use crate::svf::Svf;
use crate::vca::Vca;

/// Register the standard audio modules as config-driven process factories, so a
/// patch value's `{address: "local:Oscillator", config: …}` nodes can be
/// instantiated by `discover_processes`.
pub fn register_audio(reg: &mut ProcessRegistry) {
    reg.register("Oscillator", |c| {
        ProcessNode::Process(Box::new(Oscillator::from_config(&c)))
    });
    reg.register("LowPass", |c| {
        ProcessNode::Process(Box::new(LowPass::from_config(&c)))
    });
    reg.register("Svf", |c| ProcessNode::Process(Box::new(Svf::from_config(&c))));
    reg.register("Vca", |c| ProcessNode::Process(Box::new(Vca::from_config(&c))));
    reg.register("Envelope", |c| {
        ProcessNode::Process(Box::new(Envelope::from_config(&c)))
    });
}

/// A port→path wiring map `{port: [slot]}` for a module node's `inputs` /
/// `outputs`. Each target is a single top-level slot name.
pub fn wire_map(pairs: &[(&str, &str)]) -> Value {
    Value::tree(pairs.iter().map(|(port, slot)| {
        (
            port.to_string(),
            Value::List(vec![Value::String(slot.to_string())]),
        )
    }))
}

/// A module node — the homoiconic spec `discover_processes` lifts into a running
/// module: `{_type: "process", address, config, inputs, outputs}`.
pub fn module_node(
    address: &str,
    config: Value,
    inputs: &[(&str, &str)],
    outputs: &[(&str, &str)],
) -> Value {
    Value::tree([
        ("_type", Value::String("process".into())),
        ("address", Value::String(address.into())),
        ("config", config),
        ("inputs", wire_map(inputs)),
        ("outputs", wire_map(outputs)),
    ])
}

/// Build a patch engine from a patch value + schema — data slots typed (the
/// declared schema carries the apply-critical `Signal` type), module nodes
/// discovered from their `_type` — wired with the audio module registry + the
/// `Signal` type registry, and render `n_blocks` from `out_key`.
pub fn render_patch(
    state: Value,
    schema: Schema,
    out_key: &str,
    block: usize,
    n_blocks: usize,
) -> Vec<f32> {
    let mut reg = ProcessRegistry::new();
    register_audio(&mut reg);
    let core = Core::new()
        .with_processes(Arc::new(reg))
        .with_types(signal_registry(block));
    let mut engine =
        Engine::from_state(schema, state, core).expect("build patch engine from state value");
    render(&mut engine, out_key, n_blocks)
}
