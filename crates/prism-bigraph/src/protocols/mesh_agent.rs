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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

    /// The shared replica slot — so a reader (e.g. SWIM's dynamic-peer closure)
    /// can observe the live replica each gossip round.
    pub fn slot_arc(&self) -> Arc<Mutex<Value>> {
        Arc::clone(&self.slot)
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
            if let Some(client) = peer_client(port, &self.schema, &self.types) {
                local.gossip(client.as_ref());
            }
        }
    }

    /// Apply this peer's OWN local update to its replica (its per-source key), for
    /// the next gossip round to propagate. The mesh-safe `merge` keeps it
    /// conflict-free — a peer only ever writes its own key, so updates never clash.
    pub fn contribute(&self, contribution: &Value) {
        let mut slot = self.slot.lock().unwrap();
        *slot = algebra::merge(&self.schema, &slot, contribution);
    }

    /// Spawn a CONTINUOUS gossip loop with a DYNAMIC peer set: every `interval`,
    /// `peers()` is re-evaluated and a `sync_round` runs against it. The mesh
    /// converges and STAYS converged with no explicit calls — anti-entropy: a
    /// [`MeshAgent::contribute`] on ANY peer propagates to all within a few rounds.
    /// Because the peer set is re-read each round, it can GROW as membership is
    /// discovered (SWIM — see [`crate::protocols::swim`]). Dropping the returned
    /// [`GossipHandle`] stops + joins the loop.
    pub fn start_gossip_dynamic(
        &self,
        peers: impl Fn() -> Vec<u16> + Send + 'static,
        interval: Duration,
    ) -> GossipHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let (schema, slot, types, run) = (
            self.schema.clone(),
            Arc::clone(&self.slot),
            Arc::clone(&self.types),
            Arc::clone(&stop),
        );
        let thread = std::thread::spawn(move || {
            while !run.load(Ordering::SeqCst) {
                let local =
                    MeshReplica::shared(schema.clone(), Arc::clone(&slot), Arc::clone(&types))
                        .expect("mesh-safe");
                for port in peers() {
                    if run.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Some(client) = peer_client(port, &schema, &types) {
                        local.gossip(client.as_ref());
                    }
                }
                std::thread::sleep(interval);
            }
        });
        GossipHandle { stop, thread: Some(thread) }
    }

    /// Continuous gossip over a FIXED peer set (the common case;
    /// [`start_gossip_dynamic`](MeshAgent::start_gossip_dynamic) for SWIM's growing set).
    pub fn start_gossip(&self, peers: Vec<u16>, interval: Duration) -> GossipHandle {
        self.start_gossip_dynamic(move || peers.clone(), interval)
    }
}

/// A rest client onto a peer agent's hosted `Link` replica, for the given link
/// `schema`/`types`. Built per round; a continuous agent could cache one per peer.
fn peer_client(port: u16, schema: &Schema, types: &Arc<TypeRegistry>) -> Option<Box<dyn Process>> {
    let (sch, ty) = (schema.clone(), Arc::clone(types));
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
    // A peer that is down (or not yet up) fails to connect — SKIP it this round, never
    // panic (the failure detector reaps a peer that stays unreachable). `rest:`'s
    // `update` is itself a no-op on a dead socket, but `instantiate` eagerly POSTs
    // `initialize`, so the connection error surfaces here; swallow it gracefully.
    match RestProtocol.instantiate(&addr, Value::None, &core) {
        Ok(ProcessNode::Process(p)) => Some(p),
        _ => None,
    }
}

/// Handle to a running [`MeshAgent::start_gossip`] loop; dropping it stops + joins
/// the background gossip thread (so the mesh quiesces cleanly).
pub struct GossipHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for GossipHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A LIVE mesh field — the engine-side bridge for a `mesh:` link. A running engine
/// reads + writes a shared field each tick; a [`LiveField`] wraps a [`MeshAgent`]
/// that gossips that field CONTINUOUSLY in the background, so the field stays
/// converged with peers WHILE the engine runs — the streaming form of the batched
/// `mesh_link_distributed` (e.g. Kuramoto tiles phase-locking live, or synth voices
/// over `net:`).
///
/// Per tick the engine calls [`sync`](LiveField::sync) with its OWN keys (single-
/// writer-per-key); it returns the converged field (its keys + peers', kept fresh by
/// the background gossip) to read back. With an `overwrite`-per-value schema both the
/// publish and the gossip overwrite, so re-delivery never accumulates.
pub struct LiveField {
    agent: MeshAgent,
    gossip: Option<GossipHandle>,
}

impl LiveField {
    /// Host the field replica (gated mesh-safe); not yet gossiping — call
    /// [`go_live`](LiveField::go_live) once peer ports are known.
    pub fn host(schema: Schema, seed: Value, types: Arc<TypeRegistry>) -> Result<Self, String> {
        Ok(Self { agent: MeshAgent::host(schema, seed, types)?, gossip: None })
    }

    /// This field's rest port (a peer address).
    pub fn port(&self) -> u16 {
        self.agent.port()
    }

    /// Begin CONTINUOUSLY gossiping `peers` in the background (the live stream). For
    /// a growing/auto-discovered peer set use the agent directly with SWIM.
    pub fn go_live(&mut self, peers: Vec<u16>, interval: Duration) {
        self.gossip = Some(self.agent.start_gossip(peers, interval));
    }

    /// Each engine tick: PUBLISH the engine's own field keys, and return the
    /// CONVERGED field (its keys + peers', merged in live by the background gossip).
    pub fn sync(&self, local: &Value) -> Value {
        self.agent.contribute(local);
        self.agent.replica()
    }

    /// The current converged field, without publishing (a read-only peek).
    pub fn field(&self) -> Value {
        self.agent.replica()
    }
}
