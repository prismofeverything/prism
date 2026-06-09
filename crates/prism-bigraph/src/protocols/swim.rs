//! SWIM-style membership — peers AUTO-DISCOVER via a gossiped membership MESH LINK.
//!
//! The membership is a per-source `map[id -> {port}]` **mesh** link: each peer owns
//! its own entry (CRDT-safe by construction — the survey's per-source rule), and the
//! existing [`MeshAgent`] gossip disseminates it INFECTION-STYLE (SWIM's piggyback
//! dissemination). A peer JOINs knowing only a SEED; the membership then spreads so
//! every peer learns every peer — **no hardcoded port list**. The gossip set is
//! DERIVED from the membership each round ([`MeshAgent::start_gossip_dynamic`]), so it
//! grows as discovery happens.
//!
//! This is the membership + discovery half of SWIM, built on the mesh's own per-source
//! CRDT (membership IS a mesh link). Failure detection — ping / ack / indirect-ping →
//! suspect → dead, with incarnation refutation — is the second slice; `status` and
//! `incarnation` are per-member fields this same map will carry.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use prism_schema::{Schema, TypeRegistry, Value};

use super::{GossipHandle, MeshAgent};

/// A peer in a self-discovering mesh: a [`MeshAgent`] whose link is the membership
/// `map[id -> {port}]`, gossiped continuously over the DISCOVERED members.
pub struct Membership {
    id: String,
    agent: MeshAgent,
    _gossip: GossipHandle,
}

impl Membership {
    /// JOIN the mesh as `id`, knowing only `seeds` (peer membership ports — may be
    /// empty for the first peer). Hosts a membership replica, asserts self, gossips
    /// the seeds once, then continuously gossips the GROWING discovered set.
    pub fn join(id: &str, seeds: &[u16], interval: Duration) -> Result<Self, String> {
        let agent = MeshAgent::host(
            Schema::map(Schema::Any),
            Value::Map(IndexMap::new()),
            Arc::new(TypeRegistry::new()),
        )?;
        let port = agent.port();
        // Assert our OWN entry (per-source: we only ever write our own key).
        agent.contribute(&entry(id, port));
        // Learn the seeds' membership now, so the first dynamic round already has peers.
        agent.sync_round(seeds);

        // Continuously gossip the DISCOVERED members (re-read each round → it grows
        // as new peers are learned transitively through the seed).
        let slot = agent.slot_arc();
        let gossip = agent.start_gossip_dynamic(move || member_ports(&slot, port), interval);
        Ok(Self { id: id.into(), agent, _gossip: gossip })
    }

    /// This peer's membership port (a seed address for others).
    pub fn port(&self) -> u16 {
        self.agent.port()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The members this peer currently knows: `(id, port)`, including itself.
    pub fn members(&self) -> Vec<(String, u16)> {
        read_members(&self.agent.replica())
    }
}

/// A membership entry for `id` at `port` (the per-source key we own).
fn entry(id: &str, port: u16) -> Value {
    Value::tree([(id, Value::tree([("port", Value::Int(port as i64))]))])
}

/// Read `(id, port)` for every member of a membership map value.
fn read_members(membership: &Value) -> Vec<(String, u16)> {
    membership
        .as_map()
        .map(|m| {
            m.iter()
                .filter_map(|(id, v)| {
                    v.get_field("port")
                        .and_then(|p| p.as_i64())
                        .map(|p| (id.to_string(), p as u16))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The gossip set: every known member's port EXCEPT our own.
fn member_ports(slot: &Arc<Mutex<Value>>, self_port: u16) -> Vec<u16> {
    read_members(&slot.lock().unwrap())
        .into_iter()
        .map(|(_, p)| p)
        .filter(|&p| p != self_port)
        .collect()
}
