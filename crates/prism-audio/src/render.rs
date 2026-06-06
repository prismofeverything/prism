//! Offline rendering — run a patch through the engine in logical time and
//! collect the output `Signal` blocks. No wall-clock, no device: this is the
//! deterministic, CI-testable render path. The realtime device (A3) swaps the
//! sink for gorgon's lock-free ring; the patch itself is unchanged
//! (synthesis-bigraphs.md §II move 3, §X A1).

use std::collections::HashMap;

use prism_bigraph::process::ProcessNode;
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Key, Process, Schema, Value};

use crate::oscillator::Oscillator;
use crate::signal::{signal_schema, signal_to_vec, silence};

/// Step the engine `n_blocks` ticks, concatenating the `Signal` block found at
/// state path `out_key` after each tick. The render is `block_size *
/// n_blocks` samples.
pub fn render(engine: &mut Engine, out_key: &str, n_blocks: usize) -> Vec<f32> {
    let mut out = Vec::new();
    for _ in 0..n_blocks {
        engine.tick();
        if let Some(block) = engine.state().get_field(out_key) {
            out.extend_from_slice(&signal_to_vec(block));
        }
    }
    out
}

/// Build the simplest end-to-end audio patch — a single oscillator wired to an
/// `out` bus, its phase self-wired — and render `n_blocks` blocks offline.
pub fn render_oscillator(osc: Oscillator, n_blocks: usize) -> Vec<f32> {
    let block = osc.block_size;
    let interval = osc.interval();

    let mut topo = Topology::new();
    topo.initial_state = Value::tree([("phase", Value::float(0.0)), ("out", silence(block))]);
    topo.state_schema = Schema::tree([
        ("phase", Schema::overwrite(Schema::float())),
        ("out", Schema::overwrite(signal_schema(block))),
    ]);
    topo.processes.insert(
        "osc".to_string(),
        ProcessSpec {
            process_type: "Oscillator".to_string(),
            config: Value::None,
            inputs: [("phase", "phase")]
                .iter()
                .map(|(p, s)| (p.to_string(), vec![Key::from(*s)]))
                .collect(),
            outputs: [("phase", "phase"), ("out", "out")]
                .iter()
                .map(|(p, s)| (p.to_string(), vec![Key::from(*s)]))
                .collect(),
            interval: Some(interval),
            priority: 0.0,
        },
    );

    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("osc".to_string(), ProcessNode::Process(Box::new(osc)));

    let mut engine = Engine::new(topo, instances);
    render(&mut engine, "out", n_blocks)
}

/// Render by driving the process directly (no engine), threading phase by
/// hand. This is the reference the engine path must match — equality proves a
/// `Signal` block crosses the wire and the `overwrite` slot replays it
/// faithfully (`engine_render_matches_kernel_render`).
pub fn render_kernel(osc: &Oscillator, n_blocks: usize) -> Vec<f32> {
    let mut out = Vec::new();
    let mut phase = 0.0_f64;
    for _ in 0..n_blocks {
        let state = Value::tree([("phase", Value::float(phase))]);
        let upd = osc
            .update(&state, osc.interval())
            .into_value()
            .expect("oscillator emits an update");
        out.extend_from_slice(&signal_to_vec(
            upd.get_field("out").expect("out port present"),
        ));
        phase = upd
            .get_field("phase")
            .and_then(|v| v.as_f64())
            .expect("phase port present");
    }
    out
}
