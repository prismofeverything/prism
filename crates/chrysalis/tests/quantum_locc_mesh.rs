//! grand-synthesis M2 — LOCC over a `mesh:` link. Alice measures her qubit and
//! writes the classical bit to HER key in a shared `map[String] mesh` channel;
//! one gossip round (the mesh M1 runtime: `MeshReplica`/`gossip`) replicates it to
//! Bob, who reads it and prepares |bit>. Only the classical String crosses the
//! mesh link — the quantum states stay local (entanglement is never split, the
//! `docs/quantum-bigraphs.md` §VI constraint). The `.ys` peers are
//! `packages/quantum/ys/locc-{alice,bob}.ys`; this drives the gossip.

use std::sync::Arc;

use prism_bigraph::protocols::{mesh_links, MeshReplica};
use prism_bigraph::Engine;
use prism_schema::{Key, Schema, TypeRegistry, Value};

fn ys(file: &str) -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    std::fs::read_to_string(format!("{manifest}/../../packages/quantum/ys/{file}"))
        .unwrap_or_else(|e| panic!("read {file}: {e}"))
}

fn run_peer(src: &str) -> Engine {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    // The full std core (incl. the `Qubits` methods); a bare `compile()` core has
    // the type but not the methods, so `.measure` wouldn't dispatch.
    let result = chrysalis::compile::compile_with_core(
        &program,
        chrysalis::prelude::std_core(),
        chrysalis::prelude::std_modules(),
    )
    .expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine");
    engine.discover_all_processes();
    engine.run(1.0);
    engine
}

/// One gossip round syncing every shared `mesh` link between two engines (the M1
/// runtime: read each slot → `gossip` the replicas → write the converged union back).
fn sync_mesh(a: &mut Engine, b: &mut Engine, schema: &Schema) {
    let types = Arc::new(TypeRegistry::new());
    for name in mesh_links(a.state()) {
        let va = a.state().get_field(&name).cloned().unwrap_or(Value::None);
        let vb = b.state().get_field(&name).cloned().unwrap_or(Value::None);
        let ra = MeshReplica::new(schema.clone(), va, Arc::clone(&types)).expect("mesh-safe");
        let rb = MeshReplica::new(schema.clone(), vb, Arc::clone(&types)).expect("mesh-safe");
        ra.gossip(&rb);
        if let Value::Map(m) = a.state_mut() {
            m.insert(Key::from(name.as_str()), ra.replica());
        }
        if let Value::Map(m) = b.state_mut() {
            m.insert(Key::from(name.as_str()), rb.replica());
        }
    }
}

fn channel_alice(e: &Engine) -> Option<String> {
    e.state()
        .get_field("channel")
        .and_then(|c| c.get_field("alice"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[test]
fn locc_classical_bit_crosses_a_mesh_link() {
    let mut alice = run_peer(&ys("locc-alice.ys"));
    let mut bob = run_peer(&ys("locc-bob.ys"));

    // Alice has measured + written her bit to her key; Bob hasn't received it.
    let bit = channel_alice(&alice).expect("Alice wrote her measured bit");
    assert!(bit == "0" || bit == "1", "a classical bit, got {bit:?}");
    assert_eq!(channel_alice(&bob), None, "Bob has no bit before gossip");

    // One gossip round replicates the channel (the classical String) Alice → Bob.
    sync_mesh(&mut alice, &mut bob, &Schema::map(Schema::string()));
    assert_eq!(
        channel_alice(&bob).as_deref(),
        Some(bit.as_str()),
        "Bob received Alice's bit over the mesh link"
    );

    // Re-run Bob: now he reads the bit and prepares |bit> (only the classical bit
    // crossed; Bob's quantum prep is entirely local).
    bob.discover_all_processes();
    bob.run(1.0);
    let prepared = bob.state().get_field("prepared").cloned().expect("bob prepared a state");
    assert_eq!(
        prepared.get_field(&bit).and_then(|v| v.as_f64()),
        Some(1.0),
        "Bob prepared |{bit}> matching Alice's bit; got {prepared:?}"
    );
}
