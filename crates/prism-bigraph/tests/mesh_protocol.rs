//! The `mesh:` protocol (#62, grand-synthesis M1) — the CRDT closure invariant
//! made LOAD-BEARING, and the replica join routed through the schema algebra.
//!
//! Two properties, both leaning on existing mechanism (no bolt-on):
//!   1. `instantiate` GATES the link's value-schema through
//!      `algebra::mesh_safety` — an additive / last-writer-wins / sequence link
//!      is refused before any replica can diverge.
//!   2. The replica MERGES a peer's δ via `algebra::apply_with` (the schema's
//!      reconcile — the same boundary codec every protocol uses), so the join is
//!      idempotent + commutative *because* the schema passed the gate. This is
//!      the first-class form of `peer_shared_link.rs`'s slice 1, whose hand-rolled
//!      union is now the schema apply.

use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{MeshProtocol, MeshReplica};
use prism_bigraph::{Core, Protocol, Schema, Value};
use prism_schema::schema_to_value;

fn core() -> Core {
    Core::from(Arc::new(ProcessRegistry::new()))
}

/// A `mesh:` node config carrying the link's declared value-schema (and an
/// optional seed), exactly as composites carry `config.schema`.
fn mesh_config(schema: &Schema, state: Option<Value>) -> Value {
    let mut m: IndexMap<_, _> = IndexMap::from_iter([("schema".into(), schema_to_value(schema))]);
    if let Some(s) = state {
        m.insert("state".into(), s);
    }
    Value::Map(m)
}

fn instantiate(schema: &Schema, state: Option<Value>) -> Result<ProcessNode, String> {
    MeshProtocol
        .instantiate(&Value::None, mesh_config(schema, state), &core())
        .map_err(|e| e.to_string())
}

fn replica_of(node: ProcessNode) -> Box<dyn Process> {
    match node {
        ProcessNode::Process(p) => p,
        _ => panic!("expected a Process"),
    }
}

fn add(key: &str, v: f64) -> Value {
    Value::tree([("_add", Value::tree([(key, Value::float(v))]))])
}

#[test]
fn unsafe_schema_is_rejected_by_the_closure_invariant() {
    // A bare additive scalar cannot converge coordination-free → REFUSED, with a
    // reason that names the footgun and the fix.
    let err = instantiate(&Schema::float(), None).unwrap_err();
    assert!(err.contains("not mesh-safe"), "got: {err}");
    assert!(err.contains("additive") && err.contains("per-source"), "reason: {err}");

    // Last-writer-wins likewise refused.
    assert!(instantiate(&Schema::overwrite(Schema::float()), None).is_err());
    assert!(instantiate(&Schema::string(), None).is_err());

    // A per-source pool IS admissible — the canonical mesh link.
    assert!(instantiate(&Schema::map(Schema::float()), None).is_ok());
    // …and a missing schema is a clear error (a mesh link must declare its type).
    let no_schema = MeshProtocol.instantiate(&Value::None, Value::None, &core());
    assert!(no_schema.is_err());
}

#[test]
fn replica_merges_through_the_schema_join() {
    // The merge IS the schema's apply (the CRDT join) — idempotent on re-delivery.
    let schema = Schema::map(Schema::float());
    let node = instantiate(&schema, Some(Value::tree([("alice", Value::float(1.0))]))).unwrap();
    let p = replica_of(node);
    let r = p.as_any().downcast_ref::<MeshReplica>().unwrap();

    let once = r.merge(&add("bob", 2.0));
    let twice = r.merge(&add("bob", 2.0)); // re-delivery
    assert_eq!(once, twice, "idempotent (CRDT join via the schema apply)");
    assert_eq!(r.replica().get_field("alice").and_then(|v| v.as_f64()), Some(1.0));
    assert_eq!(r.replica().get_field("bob").and_then(|v| v.as_f64()), Some(2.0));
}

#[test]
fn two_replicas_converge_with_no_coordinator() {
    // Each peer co-owns a replica; they exchange contributions and converge — the
    // first-class `mesh:` form of peer_shared_link.rs, the join now the algebra.
    let schema = Schema::map(Schema::float());
    let alice = replica_of(
        instantiate(&schema, Some(Value::tree([("alice", Value::float(1.0))]))).unwrap(),
    );
    let bob =
        replica_of(instantiate(&schema, Some(Value::tree([("bob", Value::float(2.0))]))).unwrap());
    let a = alice.as_any().downcast_ref::<MeshReplica>().unwrap();
    let b = bob.as_any().downcast_ref::<MeshReplica>().unwrap();

    b.merge(&add("alice", 1.0)); // alice's contribution lands in bob's replica
    a.merge(&add("bob", 2.0)); // bob's contribution lands in alice's replica
    assert_eq!(a.replica(), b.replica(), "converged with no coordinator");

    // Order-independent + idempotent: re-delivery preserves the agreement.
    a.merge(&add("alice", 1.0));
    b.merge(&add("bob", 2.0));
    assert_eq!(a.replica(), b.replica(), "idempotent re-delivery preserves convergence");
}

#[test]
fn replica_update_drives_the_join_through_the_engine_port() {
    // The engine path: a `contribution` input δ flows through `update`, merges via
    // the schema join, and the converged link is republished on `link`.
    let schema = Schema::map(Schema::float());
    let p = replica_of(instantiate(&schema, None).unwrap());
    let state = Value::tree([("contribution", add("a", 5.0))]);
    let out = p.update(&state, 1.0);

    let r = p.as_any().downcast_ref::<MeshReplica>().unwrap();
    assert_eq!(r.replica().get_field("a").and_then(|v| v.as_f64()), Some(5.0));
    // The update republishes the converged link on the `link` port.
    let published = out.into_value().and_then(|v| {
        v.get_field("link").and_then(|l| l.get_field("a")).and_then(|x| x.as_f64())
    });
    assert_eq!(published, Some(5.0));
}
