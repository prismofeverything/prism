//! The protocol boundary made real: a process addressed `rest:` *in state* is
//! discovered and driven over HTTP exactly as if it were local — same `Process`
//! surface, same engine loop. Proves discovery resolves remote addresses (not
//! just `local:`), and that teardown ends the remote process (no leaks).

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocol::ProtocolRegistry;
use prism_bigraph::protocols::{RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Engine, Schema, Update, Value};

/// mass += mass · rate · interval — the growth_division `Grow`.
#[derive(Debug)]
struct Grow {
    rate: f64,
}
impl Process for Grow {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("mass".to_string(), Schema::float())])
    }
    fn update(&self, state: &Value, interval: f64) -> Update {
        let mass = state.get_field("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
        Update::value(Value::tree([("mass", Value::float(mass * self.rate * interval))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The SERVER's core: it owns the real `Grow`.
fn server_registry() -> Arc<ProcessRegistry> {
    let mut r = ProcessRegistry::new();
    r.register("Grow", |c| {
        let rate = c.get_field("rate").and_then(|v| v.as_f64()).unwrap_or(0.1);
        ProcessNode::Process(Box::new(Grow { rate }))
    });
    Arc::new(r)
}

/// The CLIENT's core: no local processes (the work is remote) — just `rest`.
fn rest_client_core() -> Core {
    let mut protocols = ProtocolRegistry::new(); // already contains `local`
    protocols.register(Arc::new(RestProtocol));
    Core::new().with_protocols(Arc::new(protocols))
}

fn rest_addr(process: &str, port: u16) -> Value {
    Value::Map(IndexMap::from([
        ("protocol".into(), Value::String("rest".into())),
        (
            "data".into(),
            Value::Map(IndexMap::from([
                ("process".into(), Value::String(process.into())),
                ("host".into(), Value::String("127.0.0.1".into())),
                ("port".into(), Value::String(port.to_string())),
            ])),
        ),
    ]))
}

fn wire(seg: &str) -> Value {
    Value::List(vec![Value::String(seg.into())])
}

#[test]
fn a_remote_process_is_discovered_and_driven_over_rest() {
    let server = RestProcessServer::start(server_registry()).expect("start server");
    std::thread::sleep(Duration::from_millis(50));

    // A `Grow` addressed `rest:` — discovery must resolve it and instantiate a
    // RestProcess pointing at the server.
    let state = Value::tree([
        ("mass", Value::float(1.0)),
        (
            "grower",
            Value::tree([
                ("address", rest_addr("Grow", server.port())),
                ("config", Value::tree([("rate", Value::float(2.0))])),
                ("interval", Value::float(1.0)),
                ("inputs", Value::tree([("mass", wire("mass"))])),
                ("outputs", Value::tree([("mass", wire("mass"))])),
            ]),
        ),
    ]);

    let mut engine = Engine::from_state(Schema::Any, state, rest_client_core()).expect("engine");
    assert_eq!(server.live_count(), 1, "discovery initialized the remote Grow on the server");

    // Drive it: each tick the engine calls update() over HTTP; the server runs
    // Grow and returns the delta. Mass climbs.
    engine.run(3.0);
    let mass = engine.state().get_field("mass").and_then(|v| v.as_f64()).unwrap();
    assert!(mass > 1.0, "remote Grow drove mass up over HTTP (got {mass})");

    // Teardown drops the RestProcess → its `end` deletes the server instance.
    drop(engine);
    assert_eq!(server.live_count(), 0, "engine teardown ended the remote process — no leak");
}

/// A bare remote-`Grow` node (no wiring needed — this exercises lifecycle, not
/// dataflow).
fn rest_grow_node(port: u16) -> Value {
    Value::tree([
        ("address", rest_addr("Grow", port)),
        ("config", Value::tree([("rate", Value::float(1.0))])),
        ("inputs", Value::map()),
        ("outputs", Value::map()),
    ])
}

/// "It cleans them up when they are deleted": initialize several remote cells,
/// delete one, and confirm exactly that one's server instance is ended.
#[test]
fn deleting_a_remote_cell_ends_its_server_instance() {
    let server = RestProcessServer::start(server_registry()).expect("start server");
    std::thread::sleep(Duration::from_millis(50));

    let state = Value::tree([
        ("c0", rest_grow_node(server.port())),
        ("c1", rest_grow_node(server.port())),
        ("c2", rest_grow_node(server.port())),
    ]);
    let mut engine = Engine::from_state(Schema::Any, state, rest_client_core()).expect("engine");
    assert_eq!(server.live_count(), 3, "three remote cells initialized on the server");

    engine.remove_process("c1");
    assert_eq!(server.live_count(), 2, "deleting a cell ends exactly its remote instance");

    drop(engine);
    assert_eq!(server.live_count(), 0, "teardown ends the remaining cells — no leaks");
}
