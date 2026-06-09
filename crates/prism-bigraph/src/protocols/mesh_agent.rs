//! The mesh AGENT — the host-once / gossip-per-round runtime over a shared link.
//!
//! [`MeshReplica::gossip`](super::MeshReplica::gossip) is one push-pull round; a
//! [`MeshAgent`] is the long-lived peer that wraps it: it HOSTS its replica at a
//! rest server (so peer agents reach it over the live bridge) and gossips with the
//! known peers each round. The explicit `sync_round` is the engine-driven gossip
//! step a continuous mesh repeats per interval (anti-entropy).
//!
//! N-peer convergence holds with NO coordinator because the join is a CRDT
//! semilattice (the schema `merge`, gated by [`mesh_safety`](prism_schema::algebra::mesh_safety)):
//! gossiping with each peer in any order, repeatedly, drives every replica to the
//! union — order-, duplication-, and partition-independent. Three agents are the
//! mesh at the scale of `coordination.ys`'s three peers.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use prism_schema::{algebra, Schema, TypeRegistry, Value};

use crate::core::Core;
use crate::factory::ProcessRegistry;
use crate::process::{Process, ProcessNode};
use crate::protocol::Protocol;
use crate::protocols::{MeshReplica, RestProcessServer, RestProtocol};

/// A long-lived mesh agent for one peer's shared link.
pub struct MeshAgent {
    schema: Schema,
    slot: Arc<Mutex<Value>>,
    types: Arc<TypeRegistry>,
    server: RestProcessServer,
}

impl MeshAgent {
    /// Host a replica for `schema` (seeded from `seed`) at a fresh rest server, so
    /// peer agents can gossip with it over the live bridge. Gated: errors if
    /// `schema` is not mesh-safe (or the server can't bind).
    pub fn host(schema: Schema, seed: Value, types: Arc<TypeRegistry>) -> Result<Self, String> {
        algebra::mesh_safety_with(Some(types.as_ref()), &schema).map_err(|e| e.to_string())?;
        let slot = Arc::new(Mutex::new(seed));
        let (s, sch, ty) = (Arc::clone(&slot), schema.clone(), Arc::clone(&types));
        let mut processes = ProcessRegistry::new();
        processes.register("Link", move |_| {
            ProcessNode::Process(Box::new(
                MeshReplica::shared(sch.clone(), Arc::clone(&s), Arc::clone(&ty))
                    .expect("schema gated mesh-safe at host()"),
            ))
        });
        let server = RestProcessServer::start(Core::from(Arc::new(processes)))
            .map_err(|e| format!("mesh agent server: {e}"))?;
        Ok(Self { schema, slot, types, server })
    }

    /// The bound port — a peer agent reaches this one here (its rest address).
    pub fn port(&self) -> u16 {
        self.server.port()
    }

    /// This agent's current replica value (the converged link).
    pub fn replica(&self) -> Value {
        self.slot.lock().unwrap().clone()
    }

    /// One anti-entropy round: gossip (push-pull) with each peer agent at `ports`.
    /// After every agent has run a round against the others, all replicas hold the
    /// union — order- and duplication-independent (the CRDT property). Repeat per
    /// interval for liveness; repetition is idempotent.
    pub fn sync_round(&self, peer_ports: &[u16]) {
        let local =
            MeshReplica::shared(self.schema.clone(), Arc::clone(&self.slot), Arc::clone(&self.types))
                .expect("schema gated mesh-safe at host()");
        for &port in peer_ports {
            local.gossip(self.peer_client(port).as_ref());
        }
    }

    /// A rest client onto a peer agent's hosted `Link` replica. (Built per round
    /// here; a continuous agent would cache one client per known peer.)
    fn peer_client(&self, port: u16) -> Box<dyn Process> {
        let (sch, ty) = (self.schema.clone(), Arc::clone(&self.types));
        let mut processes = ProcessRegistry::new();
        processes.register("Link", move |_| {
            ProcessNode::Process(Box::new(
                MeshReplica::shared(sch.clone(), Arc::new(Mutex::new(Value::None)), Arc::clone(&ty))
                    .expect("mesh-safe"),
            ))
        });
        let core = Core::from(Arc::new(processes));
        let addr = Value::Map(IndexMap::from_iter([
            ("process".into(), Value::String("Link".into())),
            ("host".into(), Value::String("127.0.0.1".into())),
            ("port".into(), Value::Int(port as i64)),
        ]));
        match RestProtocol.instantiate(&addr, Value::None, &core).expect("rest Link") {
            ProcessNode::Process(p) => p,
            _ => panic!("expected a Process"),
        }
    }
}
