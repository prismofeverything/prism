//! SWIM-style membership — peers AUTO-DISCOVER and DETECT FAILURES via a gossiped
//! membership MESH LINK.
//!
//! The membership is a per-source `map[id -> {port, inc}]` **mesh** link: each peer
//! owns its own entry (CRDT-safe by construction — the survey's per-source rule), and
//! the existing [`MeshAgent`] gossip disseminates it INFECTION-STYLE (SWIM's piggyback
//! dissemination). A peer JOINs knowing only a SEED; the membership spreads so every
//! peer learns every peer — **no hardcoded port list** — the gossip set DERIVED from
//! the membership each round ([`MeshAgent::start_gossip_dynamic`]), growing as
//! discovery happens.
//!
//! **Failure detection** rides the same link: `inc` is an incarnation HEARTBEAT each
//! peer bumps every round. A peer that stops heartbeating (its `inc` frozen for
//! `DEAD_AFTER` rounds) is declared dead and dropped from the live gossip set — the
//! heartbeat-over-gossip form of SWIM's failure detector (the ping/indirect-ping
//! variant + incarnation refutation of a false suspicion is the scaling refinement;
//! tombstone-GC of a dead entry is budgeted, per the survey). Gossiping a dead peer
//! is safe — the rest call fails to `Update::Noop`, no crash.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use prism_schema::{algebra, Schema, TypeRegistry, Value};

use super::{GossipHandle, MeshAgent};

/// Rounds without a heartbeat advance before a member is declared dead.
pub const DEAD_AFTER: u32 = 5;

/// A peer in a self-discovering, failure-detecting mesh: a [`MeshAgent`] whose link is
/// the membership `map[id -> {port, inc}]`, gossiped continuously over the LIVE members.
pub struct Membership {
    id: String,
    agent: MeshAgent,
    _gossip: GossipHandle,
    /// Per-peer (last incarnation seen, rounds stale) — the failure detector's state.
    seen: Arc<Mutex<HashMap<String, (i64, u32)>>>,
}

fn membership_schema() -> Schema {
    Schema::map(Schema::Any)
}

impl Membership {
    /// JOIN the mesh as `id`, knowing only `seeds` (peer membership ports — may be
    /// empty for the first peer). The continuous gossip loop drives both the HEARTBEAT
    /// (bump our incarnation each round) and the FAILURE DETECTOR (reap peers whose
    /// incarnation has stalled), and gossips only the LIVE peers.
    pub fn join(id: &str, seeds: &[u16], interval: Duration) -> Result<Self, String> {
        let agent = MeshAgent::host(
            membership_schema(),
            Value::Map(IndexMap::new()),
            Arc::new(TypeRegistry::new()),
        )?;
        let port = agent.port();
        agent.contribute(&entry(id, port, 1)); // assert self, incarnation 1
        agent.sync_round(seeds); // learn the seeds

        let seen = Arc::new(Mutex::new(HashMap::new()));
        let slot = agent.slot_arc();
        let (cid, cseen, inc) = (id.to_string(), Arc::clone(&seen), AtomicI64::new(1));
        let gossip = agent.start_gossip_dynamic(
            move || {
                // Heartbeat: bump our own incarnation and re-assert (per-source).
                let beat = inc.fetch_add(1, Ordering::SeqCst) + 1;
                {
                    let mut s = slot.lock().unwrap();
                    *s = algebra::merge(&membership_schema(), &s, &entry(&cid, port, beat));
                }
                // Reap + return the LIVE peer ports (skip the dead — don't gossip them).
                live_ports(&slot, &cseen, &cid)
            },
            interval,
        );
        Ok(Self { id: id.into(), agent, _gossip: gossip, seen })
    }

    /// This peer's membership port (a seed address for others).
    pub fn port(&self) -> u16 {
        self.agent.port()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Every member this peer knows: `(id, port)` — including dead-but-not-yet-GC'd.
    pub fn members(&self) -> Vec<(String, u16)> {
        read_members(&self.agent.replica()).into_iter().map(|(id, p, _)| (id, p)).collect()
    }

    /// The members currently considered ALIVE: heartbeat advancing within `DEAD_AFTER`
    /// rounds (the detector drops a peer that stopped heartbeating). Self is always live.
    pub fn live_members(&self) -> Vec<(String, u16)> {
        let seen = self.seen.lock().unwrap();
        read_members(&self.agent.replica())
            .into_iter()
            .filter(|(id, _, _)| {
                id == &self.id || seen.get(id).map_or(true, |(_, stale)| *stale < DEAD_AFTER)
            })
            .map(|(id, p, _)| (id, p))
            .collect()
    }
}

/// A membership entry `{ id: { port, inc } }`. `inc` is the heartbeat incarnation.
fn entry(id: &str, port: u16, inc: i64) -> Value {
    Value::tree([(
        id,
        Value::tree([("port", Value::Int(port as i64)), ("inc", Value::Int(inc))]),
    )])
}

/// Read `(id, port, inc)` for every member of a membership map value.
fn read_members(membership: &Value) -> Vec<(String, u16, i64)> {
    membership
        .as_map()
        .map(|m| {
            m.iter()
                .filter_map(|(id, v)| {
                    let port = v.get_field("port").and_then(|p| p.as_i64())? as u16;
                    let inc = v.get_field("inc").and_then(|i| i.as_i64()).unwrap_or(0);
                    Some((id.to_string(), port, inc))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Advance each peer's staleness (reset on a heartbeat bump, else +1) and return the
/// LIVE peers' ports (excluding self and the dead) — the gossip set.
fn live_ports(
    slot: &Arc<Mutex<Value>>,
    seen: &Arc<Mutex<HashMap<String, (i64, u32)>>>,
    self_id: &str,
) -> Vec<u16> {
    let members = read_members(&slot.lock().unwrap());
    let mut s = seen.lock().unwrap();
    let mut live = Vec::new();
    for (id, port, inc) in members {
        if id == self_id {
            continue;
        }
        let e = s.entry(id).or_insert((inc, 0));
        if inc > e.0 {
            *e = (inc, 0); // heartbeat advanced → alive
        } else {
            e.1 += 1; // stalled this round
        }
        if e.1 < DEAD_AFTER {
            live.push(port);
        }
    }
    live
}
