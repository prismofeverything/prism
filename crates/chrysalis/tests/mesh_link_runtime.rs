//! #62 / grand-synthesis M1 — a `.ys`-declared `mesh` link REPLICATES across two
//! running engines, converging via the schema `merge` (the CRDT join) written back
//! through each engine's root state. The surface→runtime connection: `link :: T
//! mesh` isn't just compile-gated — it replicates.
//!
//! The mesh runtime scans `_links` for the `"mesh"` marker (`mesh_links`), attaches
//! a gated `MeshReplica` to each link slot, and drives one `gossip` round. Transport
//! is in-process here; over a LIVE rest bridge it's `mesh_live_bridge.rs` — composing
//! the two (the `.ys` surface + the rest transport) is the distributed mesh.

use std::sync::Arc;

use prism_bigraph::protocols::{mesh_links, MeshReplica};
use prism_bigraph::Engine;
use prism_schema::{Key, Schema, TypeRegistry, Value};

fn run_peer(src: &str) -> Engine {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
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

/// One symmetric gossip round syncing every shared `mesh` link between two running
/// engines: read each peer's slot → gossip the replicas (the CRDT `merge`) → write
/// the converged value back through each engine's root state. The runtime driver
/// (in-process transport here; `rest:`/`mesh:` is `mesh_live_bridge.rs`).
fn sync_mesh(a: &mut Engine, b: &mut Engine, schema: &Schema) {
    let types = Arc::new(TypeRegistry::new());
    for name in mesh_links(a.state()) {
        let va = a.state().get_field(&name).cloned().unwrap_or(Value::None);
        let vb = b.state().get_field(&name).cloned().unwrap_or(Value::None);
        let ra = MeshReplica::new(schema.clone(), va, Arc::clone(&types)).expect("mesh-safe");
        let rb = MeshReplica::new(schema.clone(), vb, Arc::clone(&types)).expect("mesh-safe");
        ra.gossip(&rb); // one round → both replicas hold the union
        if let Value::Map(m) = a.state_mut() {
            m.insert(Key::from(name.as_str()), ra.replica());
        }
        if let Value::Map(m) = b.state_mut() {
            m.insert(Key::from(name.as_str()), rb.replica());
        }
    }
}

const PEER_A: &str = r#"
composite PeerA ->{ pool :: map[float] @ shared } (
  link shared :: map[float] mesh = { a: 1.0 }
)
PeerA[]
"#;

const PEER_B: &str = r#"
composite PeerB ->{ pool :: map[float] @ shared } (
  link shared :: map[float] mesh = { b: 2.0 }
)
PeerB[]
"#;

#[test]
fn a_ys_mesh_link_replicates_across_two_engines() {
    let mut a = run_peer(PEER_A);
    let mut b = run_peer(PEER_B);

    // The runtime SEES the mesh link by reflection (the `"mesh"` marker the surface
    // recorded), without knowing the program — this is what a real mesh agent scans.
    assert_eq!(mesh_links(a.state()), vec!["shared".to_string()]);

    // Before sync: each peer holds only its own per-source key.
    let len = |e: &Engine| e.state().get_field("shared").and_then(|v| v.as_map()).map(|m| m.len());
    assert_eq!(len(&a), Some(1));
    assert_eq!(len(&b), Some(1));

    // Sync: one gossip round (the CRDT merge), the converged value written back.
    sync_mesh(&mut a, &mut b, &Schema::map(Schema::float()));

    // Both engines' `.ys` `mesh` link converged to the union — no coordinator.
    let sa = a.state().get_field("shared").cloned().unwrap();
    let sb = b.state().get_field("shared").cloned().unwrap();
    assert_eq!(sa, sb, "the .ys mesh link converged across engines: a={sa:?} b={sb:?}");
    assert_eq!(sa.get_field("a").and_then(|v| v.as_f64()), Some(1.0));
    assert_eq!(sa.get_field("b").and_then(|v| v.as_f64()), Some(2.0));
}
