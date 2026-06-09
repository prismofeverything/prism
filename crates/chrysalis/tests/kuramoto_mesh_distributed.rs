//! Demo 2 — DISTRIBUTED slice. Two Kuramoto tiles on two ENGINES, coupled only
//! through a `mesh` link replicated over a LIVE rest bridge — they phase-lock
//! across the machine boundary with no coordinator (categorical-core.md §5: the
//! CRDT replication of the mean-field map + the dynamical attractor reading it).
//!
//! Built on the mesh agent's runtime (`MeshReplica`/`mesh_links`, mirroring
//! `prism-bigraph/tests/mesh_link_distributed.rs`). Each peer owns DISJOINT
//! oscillator keys (a0..a3 / b0..b3), so the field map is a per-source key-union
//! (each key single-writer) = CRDT-safe. The `.ys` is unchanged from the local
//! run — only the transport between the field replicas differs (the mesh is an
//! ADDRESS, not a mode).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::protocols::{mesh_links, MeshAgent, MeshReplica, RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Engine, ProcessNode, ProcessRegistry, Protocol};
use prism_bigraph::process::Process;
use prism_schema::{Key, Schema, TypeRegistry, Value};

const KURAMOTO_MESH: &str = include_str!("../ys/kuramoto-mesh.ys");

fn types() -> Arc<TypeRegistry> {
    Arc::new(TypeRegistry::new())
}

// the field link's value schema: map[id -> array[[2], float]]
fn field_schema() -> Schema {
    Schema::map(Schema::array(vec![2], Schema::float()))
}

fn run_peer(src: &str) -> Engine {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile_with_methods(
        &program,
        chrysalis::prelude::std_registry(),
        chrysalis::prelude::std_methods(),
    )
    .expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine");
    engine.discover_all_processes();
    engine
}

/// Host a peer's mesh-link slot as a `MeshReplica` at its own rest server.
fn host_replica(seed: Value) -> (RestProcessServer, Arc<Mutex<Value>>) {
    let slot = Arc::new(Mutex::new(seed));
    let s = Arc::clone(&slot);
    let mut processes = ProcessRegistry::new();
    processes.register("Link", move |_| {
        ProcessNode::Process(Box::new(
            MeshReplica::shared(field_schema(), Arc::clone(&s), types()).expect("mesh-safe"),
        ))
    });
    let server = RestProcessServer::start(Core::from(Arc::new(processes))).expect("server");
    (server, slot)
}

/// A rest client onto a peer's hosted replica (the live-bridge handle).
fn client_to(port: u16) -> Box<dyn Process> {
    let core = Core::from(Arc::new(ProcessRegistry::new()));
    let addr = Value::Map(IndexMap::from_iter([
        (Key::from("process"), Value::String("Link".into())),
        (Key::from("host"), Value::String("127.0.0.1".into())),
        (Key::from("port"), Value::Int(port as i64)),
    ]));
    match RestProtocol.instantiate(&addr, Value::None, &core).expect("rest Link") {
        ProcessNode::Process(p) => p,
        _ => panic!("expected a Process"),
    }
}

fn write_slot(e: &mut Engine, name: &str, value: Value) {
    if let Value::Map(m) = e.state_mut() {
        m.insert(Key::from(name), value);
    }
}

/// R = |Σ field| / N over the (gossiped) field map.
fn order_parameter(state: &Value, n: f64) -> f64 {
    let (mut sx, mut sy) = (0.0, 0.0);
    if let Some(m) = state.get_field("field").and_then(|v| v.as_map()) {
        for (key, v) in m.iter() {
            if key.starts_with('_') {
                continue;
            }
            if let Some(arr) = v.as_list() {
                sx += arr.first().and_then(Value::as_f64).unwrap_or(0.0);
                sy += arr.get(1).and_then(Value::as_f64).unwrap_or(0.0);
            }
        }
    }
    (sx * sx + sy * sy).sqrt() / n
}

#[test]
fn two_tiles_phase_lock_over_a_live_mesh_boundary() {
    // Disjoint oscillator key-sets per peer; n=8 is the GLOBAL count.
    let src_a = format!(
        "{KURAMOTO_MESH}\nTileMesh[omegas: {{'a0': 0.6, 'a1': 0.8, 'a2': 1.2, 'a3': 1.4}}, n: 8.0, k: 4.0]\n"
    );
    let src_b = format!(
        "{KURAMOTO_MESH}\nTileMesh[omegas: {{'b0': 0.7, 'b1': 0.9, 'b2': 1.1, 'b3': 1.3}}, n: 8.0, k: 4.0]\n"
    );

    let mut a = run_peer(&src_a);
    let mut b = run_peer(&src_b);
    a.run(0.05);
    b.run(0.05);

    let name = mesh_links(a.state())
        .into_iter()
        .next()
        .expect("a mesh link named `field`");

    let (server_a, slot_a) = host_replica(a.state().get_field(&name).cloned().unwrap());
    let (server_b, slot_b) = host_replica(b.state().get_field(&name).cloned().unwrap());
    std::thread::sleep(Duration::from_millis(50));

    for round in 0..400 {
        // gossip the field both ways over the live bridge (key-union merge)
        let la = MeshReplica::shared(field_schema(), Arc::clone(&slot_a), types()).unwrap();
        let lb = MeshReplica::shared(field_schema(), Arc::clone(&slot_b), types()).unwrap();
        la.gossip(client_to(server_b.port()).as_ref());
        lb.gossip(client_to(server_a.port()).as_ref());

        // write the converged field back into each engine, then tick its oscillators
        write_slot(&mut a, &name, slot_a.lock().unwrap().clone());
        write_slot(&mut b, &name, slot_b.lock().unwrap().clone());
        a.run(0.5);
        b.run(0.5);
        // re-host the updated local field for the next round's gossip
        *slot_a.lock().unwrap() = a.state().get_field(&name).cloned().unwrap();
        *slot_b.lock().unwrap() = b.state().get_field(&name).cloned().unwrap();

        if round % 80 == 0 {
            eprintln!(
                "round {round}: R_a={:.3} R_b={:.3} keys_a={} keys_b={}",
                order_parameter(a.state(), 8.0),
                order_parameter(b.state(), 8.0),
                a.state().get_field(&name).and_then(|v| v.as_map()).map(|m| m.len()).unwrap_or(0),
                b.state().get_field(&name).and_then(|v| v.as_map()).map(|m| m.len()).unwrap_or(0),
            );
        }
    }

    let r_a = order_parameter(a.state(), 8.0);
    let r_b = order_parameter(b.state(), 8.0);
    eprintln!("DISTRIBUTED Kuramoto:  R_a={r_a:.3}  R_b={r_b:.3}");
    assert!(r_a > 0.8, "peer A should phase-lock over the mesh: R={r_a:.3}");
    assert!(r_b > 0.8, "peer B should phase-lock over the mesh: R={r_b:.3}");
}

/// The LIVE-STREAM form (the mesh agent's follow-up): instead of 400 explicit
/// gossip rounds, each tile hosts its field via a `MeshAgent` whose background
/// `start_gossip` loop replicates CONTINUOUSLY while the engines tick — the
/// phase-lock happens live, not in a batch. The engine stays authoritative for
/// its own keys (re-host each tick); gossip refreshes the remote keys.
#[test]
fn two_tiles_phase_lock_as_a_live_mesh_stream() {
    let src_a = format!(
        "{KURAMOTO_MESH}\nTileMesh[omegas: {{'a0': 0.6, 'a1': 0.8, 'a2': 1.2, 'a3': 1.4}}, n: 8.0, k: 4.0]\n"
    );
    let src_b = format!(
        "{KURAMOTO_MESH}\nTileMesh[omegas: {{'b0': 0.7, 'b1': 0.9, 'b2': 1.1, 'b3': 1.3}}, n: 8.0, k: 4.0]\n"
    );
    let mut a = run_peer(&src_a);
    let mut b = run_peer(&src_b);
    a.run(0.05);
    b.run(0.05);
    let name = mesh_links(a.state()).into_iter().next().expect("a mesh link `field`");

    // Host each tile's field; CONTINUOUS background gossip keeps them converged live.
    let agent_a = MeshAgent::host(field_schema(), a.state().get_field(&name).cloned().unwrap(), types())
        .expect("host a");
    let agent_b = MeshAgent::host(field_schema(), b.state().get_field(&name).cloned().unwrap(), types())
        .expect("host b");
    let _ga = agent_a.start_gossip(vec![agent_b.port()], Duration::from_millis(3));
    let _gb = agent_b.start_gossip(vec![agent_a.port()], Duration::from_millis(3));
    std::thread::sleep(Duration::from_millis(50));

    for round in 0..300 {
        // pull the live-converged field, tick the oscillators, push the authoritative field
        write_slot(&mut a, &name, agent_a.replica());
        write_slot(&mut b, &name, agent_b.replica());
        a.run(0.5);
        b.run(0.5);
        *agent_a.slot_arc().lock().unwrap() = a.state().get_field(&name).cloned().unwrap();
        *agent_b.slot_arc().lock().unwrap() = b.state().get_field(&name).cloned().unwrap();
        std::thread::sleep(Duration::from_millis(2)); // let the background gossip run
        if round % 60 == 0 {
            eprintln!(
                "stream round {round}: R_a={:.3} R_b={:.3}",
                order_parameter(a.state(), 8.0),
                order_parameter(b.state(), 8.0)
            );
        }
    }
    let r_a = order_parameter(a.state(), 8.0);
    let r_b = order_parameter(b.state(), 8.0);
    eprintln!("LIVE-STREAM Kuramoto:  R_a={r_a:.3}  R_b={r_b:.3}");
    assert!(r_a > 0.8, "peer A phase-locks on the live stream: R={r_a:.3}");
    assert!(r_b > 0.8, "peer B phase-locks on the live stream: R={r_b:.3}");
}
