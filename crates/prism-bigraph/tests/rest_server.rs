//! The `rest` process SERVER end to end: stand up a real `RestProcessServer` over
//! a `ProcessRegistry`, drive processes through it with the `RestProcess` client,
//! and assert the lifecycle — including that `end` DELETES instances so a client
//! that ends what it starts leaves the server with **zero** live processes.
//!
//! The whole point of the protocol abstraction: a process driven over HTTP is
//! indistinguishable from a local one (same `Process` surface), so the boundary
//! is real and enforced — the only channel is the port interface.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{RestProcess, RestProcessServer};
use prism_bigraph::{Schema, Update, Value};

/// mass += mass · rate · interval (a delta) — the same `Grow` as growth_division.
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

fn grow_registry() -> Arc<ProcessRegistry> {
    let mut r = ProcessRegistry::new();
    r.register("Grow", |config| {
        let rate = config.get_field("rate").and_then(|v| v.as_f64()).unwrap_or(0.1);
        ProcessNode::Process(Box::new(Grow { rate }))
    });
    Arc::new(r)
}

/// Start a server, send it a bunch of requests (initialize × N, update × N), then
/// verify the cleanup invariant: ending every process leaves none behind.
#[test]
fn rest_server_lifecycle_creates_runs_and_cleans_up() {
    let server = RestProcessServer::start(grow_registry()).expect("start server");
    std::thread::sleep(Duration::from_millis(50)); // let the listener settle

    let config = Value::tree([("rate", Value::float(2.0))]);

    // Start a bunch of remote processes (initialize → server stores each).
    let procs: Vec<RestProcess> = (0..3)
        .map(|_| {
            RestProcess::initialize(server.base_url(), "Grow", config.clone()).expect("initialize")
        })
        .collect();
    assert_eq!(server.live_count(), 3, "three live processes after initialize");

    // Drive them over HTTP — the real computation runs server-side.
    let state = Value::tree([("mass", Value::float(1.0))]);
    for p in &procs {
        let mass = p
            .update(&state, 1.0)
            .into_value()
            .and_then(|v| v.get_field("mass").and_then(|m| m.as_f64()))
            .expect("update returns mass");
        assert!((mass - 2.0).abs() < 1e-9, "Grow(rate 2)·mass 1·interval 1 = 2, got {mass}");
    }
    assert_eq!(server.live_count(), 3, "updates don't change the live set");

    // Dropping each RestProcess sends `end` → the server DELETES it.
    drop(procs);
    assert_eq!(server.live_count(), 0, "every ended process is cleaned up — no leaks");
}

/// A document referencing a process the server's core lacks is rejected, not run
/// as a partial graph (here: the whole class is unknown).
#[test]
fn rest_server_rejects_an_unregistered_process() {
    let server = RestProcessServer::start(grow_registry()).expect("start server");
    std::thread::sleep(Duration::from_millis(50));

    // `Grow` is registered; `Ghost` is not.
    let result = RestProcess::initialize(server.base_url(), "Ghost", Value::None);
    assert!(result.is_err(), "initializing an unregistered process must error");
    assert_eq!(server.live_count(), 0, "nothing is created for a rejected request");
}
