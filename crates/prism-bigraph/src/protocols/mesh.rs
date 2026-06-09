//! The `mesh:` protocol — a value-bearing link REPLICATED across peers,
//! converging coordination-free.
//!
//! Generalizes the parent↔child protocols (`local`/`rest`/`stream`) to
//! **peer↔peer**: a `mesh:` node is a LOCAL REPLICA of a shared link (a bigraph
//! hyperedge). There is no coordinator and no home node — each peer co-owns the
//! link as a replica, and the replicas CONVERGE because the link's merge is a
//! CRDT (an idempotent join-semilattice).
//!
//! Two halves, both already in the algebra — no bolt-on:
//!   1. **The closure invariant.** A replica is built only through
//!      [`MeshReplica::new`] / [`MeshReplica::shared`], which gate the link's
//!      value-schema through [`algebra::mesh_safety`]; a non-semilattice schema
//!      (additive scalar, last-writer-wins, sequence) is REFUSED, so an ungated
//!      (divergent) replica cannot be constructed.
//!   2. **The join is the schema `merge`.** Absorbing a peer's contribution STATE
//!      is [`algebra::merge`]`(schema, replica, contribution)` — the state-based
//!      CRDT join (key-union for maps), the same algebra every boundary uses, NOT
//!      a hand-rolled union. Convergence is *guaranteed* by half (1): a mesh-safe
//!      schema's merge is idempotent + commutative for the per-source discipline,
//!      so re-delivery and reordering are harmless. A contribution crosses a
//!      bridge as a STATE — decoded by the rest codec's `realize_with` — so the
//!      live bridge needs no delta-in support; the δ-state CRDT refinement that
//!      ships *deltas* (apply, not merge) is a later slice (cf. #5).
//!
//! This is the first-class form of `prism-bigraph/tests/peer_shared_link.rs`'s
//! slice 1 (which hand-rolled the union and had no gate); the live-bridge
//! convergence through this protocol is `tests/mesh_live_bridge.rs`. Continuous
//! gossip / anti-entropy, SWIM membership, and N-peer meshes are later slices.
//! See `docs/grand-synthesis.md` M1 and memory `mesh_as_protocol`.

use std::any::Any;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use prism_schema::{algebra, value_to_schema, MeshUnsafe, Schema, TypeRegistry, Value};

use crate::core::Core;
use crate::process::{Process, ProcessNode};
use crate::protocol::{string_record, Protocol, ProtocolError};
use crate::update::Update;

/// The shared-link value port a mesh replica publishes (its converged replica).
pub const LINK_PORT: &str = "link";
/// The state-contribution port a mesh replica receives a peer's state on.
pub const CONTRIBUTION_PORT: &str = "contribution";

/// A peer's local REPLICA of a shared `mesh` link. Co-owns the link as `slot`;
/// there is no central copy. Receiving a peer's `contribution` STATE MERGES it
/// via the schema's `merge` — the state-based CRDT join — and republishes the
/// merged replica on `link`. Convergence is guaranteed by the schema being
/// mesh-safe (gated at construction), so re-delivery and reordering are harmless.
#[derive(Debug)]
pub struct MeshReplica {
    schema: Schema,
    slot: Arc<Mutex<Value>>,
}

impl MeshReplica {
    /// Build a replica for `schema`, GATED through the closure invariant
    /// ([`algebra::mesh_safety`]) — a non-semilattice schema is refused, so an
    /// ungated (divergent) replica cannot be constructed. `initial` seeds it.
    pub fn new(
        schema: Schema,
        initial: Value,
        types: Arc<TypeRegistry>,
    ) -> Result<Self, MeshUnsafe> {
        Self::shared(schema, Arc::new(Mutex::new(initial)), types)
    }

    /// Like [`MeshReplica::new`] but sharing an existing `slot`, so several
    /// handles co-own the SAME replica — e.g. a peer that hosts the link at a
    /// rest server while another handle observes or seeds it.
    pub fn shared(
        schema: Schema,
        slot: Arc<Mutex<Value>>,
        types: Arc<TypeRegistry>,
    ) -> Result<Self, MeshUnsafe> {
        algebra::mesh_safety_with(Some(types.as_ref()), &schema)?;
        Ok(Self { schema, slot })
    }

    /// The current replica value (the converged link).
    pub fn replica(&self) -> Value {
        self.slot.lock().unwrap().clone()
    }

    /// The shared `Arc` slot — so a peering harness (or a rest server hosting this
    /// link) can co-own this replica, as `peer_shared_link.rs` does.
    pub fn slot(&self) -> Arc<Mutex<Value>> {
        Arc::clone(&self.slot)
    }

    /// Merge a peer's contribution STATE into this replica via the schema's
    /// `merge` (the state-based CRDT join), returning the converged replica. The
    /// engine path (`update`) and a direct caller share this one join.
    pub fn merge(&self, contribution: &Value) -> Value {
        let mut slot = self.slot.lock().unwrap();
        *slot = algebra::merge(&self.schema, &slot, contribution);
        slot.clone()
    }
}

impl Process for MeshReplica {
    fn inputs(&self) -> IndexMap<String, Schema> {
        // A contribution is a peer's STATE in the link's sort.
        IndexMap::from([(CONTRIBUTION_PORT.to_string(), self.schema.clone())])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(LINK_PORT.to_string(), self.schema.clone())])
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        if let Some(contribution) = state.get_field(CONTRIBUTION_PORT) {
            // THE JOIN = the schema's `merge` (the CRDT join). Idempotent +
            // commutative because the schema is mesh-safe (gated at construction).
            self.merge(contribution);
        }
        Update::value(Value::tree([(LINK_PORT, self.replica())]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The `mesh:` protocol. Instantiates a [`MeshReplica`] for a shared link; the
/// constructor gates its value-schema through the CRDT closure invariant.
#[derive(Debug)]
pub struct MeshProtocol;

impl MeshProtocol {
    /// The link's value-schema from the spec — `config.schema` (a schema-value,
    /// the declared link type `T`), exactly as composites carry their schema.
    fn link_schema(config: &Value) -> Result<Schema, ProtocolError> {
        config
            .get_field("schema")
            .and_then(value_to_schema)
            .ok_or_else(|| ProtocolError::Other {
                protocol: "mesh".into(),
                message: "a mesh link requires a declared value-schema (config.schema = T)".into(),
            })
    }
}

impl Protocol for MeshProtocol {
    fn name(&self) -> &str {
        "mesh"
    }

    fn instantiate(
        &self,
        _data: &Value,
        config: Value,
        core: &Core,
    ) -> Result<ProcessNode, ProtocolError> {
        let schema = Self::link_schema(&config)?;

        // The initial replica: config.state if seeded, else the schema default.
        let initial = match config.get_field("state") {
            Some(v) => algebra::realize_with(Some(core.types.as_ref()), &schema, v),
            None => algebra::default_with(Some(core.types.as_ref()), &schema),
        };

        // `MeshReplica::new` GATES the schema through the closure invariant — a
        // link that cannot converge coordination-free is refused here, before any
        // replica exists to diverge.
        MeshReplica::new(schema, initial, Arc::clone(&core.types))
            .map(|r| ProcessNode::Process(Box::new(r)))
            .map_err(|e| ProtocolError::Other {
                protocol: "mesh".into(),
                message: format!("link is not mesh-safe — {e}"),
            })
    }

    fn address_type(&self) -> Option<(String, Schema)> {
        Some(("mesh".into(), string_record(&["link"])))
    }
}
