//! The `mesh:` protocol — a value-bearing link REPLICATED across peers,
//! converging coordination-free.
//!
//! Generalizes the parent↔child protocols (`local`/`rest`/`stream`) to
//! **peer↔peer**: a `mesh:` node is a LOCAL REPLICA of a shared link (a bigraph
//! hyperedge). There is no coordinator and no home node — each peer co-owns the
//! link as a replica, and the replicas CONVERGE because the link's merge is a
//! CRDT (the schema's reconcile, an idempotent join-semilattice).
//!
//! Two halves, both already in the algebra — no bolt-on:
//!   1. **The closure invariant.** `instantiate` gates the link's value-schema
//!      through [`algebra::mesh_safety`]; a non-semilattice schema (additive
//!      scalar, last-writer-wins, sequence) is REJECTED before any replica can
//!      diverge.
//!   2. **The join is the schema `apply`.** Merging a peer's δ is
//!      [`algebra::apply_with`]`(schema, replica, δ)` — the same boundary codec
//!      every other protocol uses — NOT a hand-rolled key-union. Convergence is
//!      *guaranteed* by half (1): a mesh-safe schema's apply is idempotent +
//!      commutative, so re-delivery and reordering are harmless.
//!
//! This is the first-class form of `prism-bigraph/tests/peer_shared_link.rs`'s
//! slice 1 (which hand-rolled the union and had no gate). Continuous gossip /
//! anti-entropy, SWIM membership, and N-peer meshes are later slices; the
//! replica + the gate + the algebra-join are the foundation. See
//! `docs/grand-synthesis.md` M1 and memory `mesh_as_protocol`.

use std::any::Any;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use prism_schema::{algebra, value_to_schema, Schema, TypeRegistry, Value};

use crate::core::Core;
use crate::process::{Process, ProcessNode};
use crate::protocol::{string_record, Protocol, ProtocolError};
use crate::update::Update;

/// The shared-link value port a mesh replica publishes (its converged replica).
pub const LINK_PORT: &str = "link";
/// The δ-input port a mesh replica receives a peer's contribution on.
pub const CONTRIBUTION_PORT: &str = "contribution";

/// A peer's local REPLICA of a shared `mesh` link. Co-owns the link as `slot`;
/// there is no central copy. Receiving a peer's `contribution` (a δ) MERGES it
/// via the link's schema-reconcile — the CRDT join — and republishes the merged
/// replica on `link`. Convergence is guaranteed by the schema being mesh-safe
/// (gated at instantiation), so re-delivery and reordering are harmless.
#[derive(Debug)]
pub struct MeshReplica {
    schema: Schema,
    slot: Arc<Mutex<Value>>,
    types: Arc<TypeRegistry>,
}

impl MeshReplica {
    /// The current replica value (the converged link).
    pub fn replica(&self) -> Value {
        self.slot.lock().unwrap().clone()
    }

    /// The shared `Arc` slot — so a test (or an in-process peering harness) can
    /// observe this replica, as `peer_shared_link.rs` does over a rest bridge.
    pub fn slot(&self) -> Arc<Mutex<Value>> {
        Arc::clone(&self.slot)
    }

    /// Merge a peer's contribution δ into this replica via the schema join, and
    /// return the converged replica. The in-process form of receiving a δ over a
    /// bridge — the engine path (`update`) does exactly this.
    pub fn merge(&self, delta: &Value) -> Value {
        let mut slot = self.slot.lock().unwrap();
        *slot = algebra::apply_with(Some(self.types.as_ref()), &self.schema, &slot, delta);
        slot.clone()
    }
}

impl Process for MeshReplica {
    fn inputs(&self) -> IndexMap<String, Schema> {
        // A contribution is a δ in the link's sort.
        IndexMap::from([(CONTRIBUTION_PORT.to_string(), self.schema.clone())])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(LINK_PORT.to_string(), self.schema.clone())])
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        if let Some(delta) = state.get_field(CONTRIBUTION_PORT) {
            // THE JOIN = the schema's reconcile/apply (the CRDT merge). Idempotent
            // + commutative because the schema is mesh-safe (gated at instantiate).
            self.merge(delta);
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

/// The `mesh:` protocol. Instantiates a [`MeshReplica`] for a shared link after
/// gating its value-schema through the CRDT closure invariant.
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

        // ── THE CLOSURE INVARIANT ── refuse a link that cannot converge
        // coordination-free, before any replica exists to diverge.
        algebra::mesh_safety_with(Some(core.types.as_ref()), &schema).map_err(|e| {
            ProtocolError::Other {
                protocol: "mesh".into(),
                message: format!("link is not mesh-safe — {e}"),
            }
        })?;

        // The initial replica: config.state if seeded, else the schema default.
        let initial = match config.get_field("state") {
            Some(v) => algebra::realize_with(Some(core.types.as_ref()), &schema, v),
            None => algebra::default_with(Some(core.types.as_ref()), &schema),
        };

        Ok(ProcessNode::Process(Box::new(MeshReplica {
            schema,
            slot: Arc::new(Mutex::new(initial)),
            types: Arc::clone(&core.types),
        })))
    }

    fn address_type(&self) -> Option<(String, Schema)> {
        Some(("mesh".into(), string_record(&["link"])))
    }
}
