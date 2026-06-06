//! Offline rendering — run a patch through the engine in logical time and
//! collect the output `Signal` blocks. No wall-clock, no device: the
//! deterministic, CI-testable render path. The realtime device (A3) swaps the
//! sink for gorgon's lock-free ring; the patch itself is unchanged
//! (synthesis-bigraphs.md §II move 3, §X A1).
//!
//! Signal slots are the `Signal` type ([`crate::signal::signal_type`]), so the
//! engine is given a [`signal_registry`] — that is what makes a bus mix
//! additively (reconcile) and hold the current frame (apply).

use std::collections::HashMap;

use prism_bigraph::process::ProcessNode;
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Key, Process, Schema, Value};

use crate::envelope::Envelope;
use crate::lowpass::LowPass;
use crate::oscillator::Oscillator;
use crate::signal::{signal_registry, signal_to_vec, signal_type, silence};
use crate::vca::Vca;

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

/// The simplest end-to-end patch — one oscillator into an `out` bus, phase
/// self-wired — rendered `n_blocks` blocks offline.
pub fn render_oscillator(osc: Oscillator, n_blocks: usize) -> Vec<f32> {
    let block = osc.block_size;
    let interval = osc.interval();

    let mut topo = Topology::new();
    topo.initial_state = Value::tree([("phase", Value::float(0.0)), ("out", silence(block))]);
    topo.state_schema = Schema::tree([
        ("phase", Schema::overwrite(Schema::float())),
        ("out", signal_type()),
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
    engine.set_type_registry(signal_registry(block));
    render(&mut engine, "out", n_blocks)
}

/// Render several oscillators **into one `Signal` bus** offline. Every
/// oscillator's `out` is wired to the *same* `mix` path, so the BSP reconcile
/// sums their blocks (additive `Signal` reconcile) and the `Signal` apply
/// keeps only this frame's mix — a summing bus, straight from the algebra.
/// Each oscillator carries its own phase slot.
pub fn render_mix(oscs: Vec<Oscillator>, n_blocks: usize) -> Vec<f32> {
    assert!(!oscs.is_empty(), "render_mix needs at least one oscillator");
    let block = oscs[0].block_size;
    let interval = oscs[0].interval();

    let mut state_entries: Vec<(String, Value)> = vec![("mix".to_string(), silence(block))];
    let mut schema_entries: Vec<(String, Schema)> = vec![("mix".to_string(), signal_type())];
    for i in 0..oscs.len() {
        state_entries.push((format!("phase_{i}"), Value::float(0.0)));
        schema_entries.push((format!("phase_{i}"), Schema::overwrite(Schema::float())));
    }

    let mut topo = Topology::new();
    topo.initial_state = Value::tree(state_entries);
    topo.state_schema = Schema::tree(schema_entries);

    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    for (i, osc) in oscs.into_iter().enumerate() {
        let phase = format!("phase_{i}");
        let name = format!("osc_{i}");
        topo.processes.insert(
            name.clone(),
            ProcessSpec {
                process_type: "Oscillator".to_string(),
                config: Value::None,
                inputs: std::iter::once(("phase".to_string(), vec![Key::from(phase.as_str())]))
                    .collect(),
                outputs: [
                    ("phase".to_string(), vec![Key::from(phase.as_str())]),
                    ("out".to_string(), vec![Key::from("mix")]),
                ]
                .into_iter()
                .collect(),
                interval: Some(interval),
                priority: 0.0,
            },
        );
        instances.insert(name, ProcessNode::Process(Box::new(osc)));
    }

    let mut engine = Engine::new(topo, instances);
    engine.set_type_registry(signal_registry(block));
    render(&mut engine, "mix", n_blocks)
}

/// Render a hand-patched **voice** offline: `osc → LowPass → Vca`, with an
/// `Envelope` (gated by `gate`) driving the Vca's gain. This is the canonical
/// subtractive voice — the place graph is the four modules, the link graph is
/// the wiring below.
///
/// All four are `Process`es, so each reads the pre-tick snapshot: the chain has
/// **~2 blocks of pipeline latency** (osc→filtered→out), an inherent property
/// of BSP signal flow. A within-tick variant (stateless nodes as `Step`s) is a
/// later refinement; the latency is harmless (a fixed delay) and the steady
/// state is exact.
pub fn render_voice(
    osc: Oscillator,
    filt: LowPass,
    env: Envelope,
    vca: Vca,
    gate: f64,
    n_blocks: usize,
) -> Vec<f32> {
    let block = osc.block_size;
    let interval = osc.interval();

    let wire = |pairs: &[(&str, &str)]| -> indexmap::IndexMap<String, Vec<Key>> {
        pairs
            .iter()
            .map(|(p, s)| (p.to_string(), vec![Key::from(*s)]))
            .collect()
    };

    let mut topo = Topology::new();
    topo.initial_state = Value::tree([
        ("phase", Value::float(0.0)),
        ("raw", silence(block)),
        ("z1", Value::float(0.0)),
        ("filtered", silence(block)),
        ("gate", Value::float(gate)),
        ("level", Value::float(0.0)),
        ("out", silence(block)),
    ]);
    topo.state_schema = Schema::tree([
        ("phase", Schema::overwrite(Schema::float())),
        ("raw", signal_type()),
        ("z1", Schema::overwrite(Schema::float())),
        ("filtered", signal_type()),
        ("gate", Schema::float()),
        ("level", Schema::overwrite(Schema::float())),
        ("out", signal_type()),
    ]);

    let spec = |ty: &str, inputs: &[(&str, &str)], outputs: &[(&str, &str)]| ProcessSpec {
        process_type: ty.to_string(),
        config: Value::None,
        inputs: wire(inputs),
        outputs: wire(outputs),
        interval: Some(interval),
        priority: 0.0,
    };

    topo.processes.insert(
        "osc".to_string(),
        spec("Oscillator", &[("phase", "phase")], &[("phase", "phase"), ("out", "raw")]),
    );
    topo.processes.insert(
        "filt".to_string(),
        spec("LowPass", &[("in", "raw"), ("z1", "z1")], &[("out", "filtered"), ("z1", "z1")]),
    );
    topo.processes.insert(
        "env".to_string(),
        spec("Envelope", &[("gate", "gate"), ("level", "level")], &[("level", "level")]),
    );
    topo.processes.insert(
        "amp".to_string(),
        spec("Vca", &[("in", "filtered"), ("gain", "level")], &[("out", "out")]),
    );

    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("osc".to_string(), ProcessNode::Process(Box::new(osc)));
    instances.insert("filt".to_string(), ProcessNode::Process(Box::new(filt)));
    instances.insert("env".to_string(), ProcessNode::Process(Box::new(env)));
    instances.insert("amp".to_string(), ProcessNode::Process(Box::new(vca)));

    let mut engine = Engine::new(topo, instances);
    engine.set_type_registry(signal_registry(block));
    render(&mut engine, "out", n_blocks)
}

/// Render by driving the process directly (no engine), threading phase by
/// hand. The reference the engine path must match — equality proves a `Signal`
/// block crosses the wire and the slot replays each frame faithfully.
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
