//! Mesh-safety of a shared link = its merge is a CRDT (a join-SEMILATTICE).
//!
//! A distributed link is replicated per peer and converges with NO coordinator
//! (the peer-bridge / `mesh:` direction, [[mesh_as_protocol]]). The δ-CRDT JOIN
//! is `apply(local_replica, peer_delta)` — and coordination-free convergence
//! holds iff that join is **commutative + associative + IDEMPOTENT** (the CRDT
//! semilattice law; CALM: monotone ⇒ coordination-free). The link's schema
//! reconcile/apply IS the merge, so this is a property OF THE SCHEMA — these
//! tests pin which reconciles are mesh-safe and which are the classic footgun.
//!
//! Grounded by the 2026-06-08 P2P survey (Shapiro et al. CvRDT/CmRDT;
//! Almeida/Shoker/Baquero delta-state CRDTs; Hellerstein/Alvaro CALM). The
//! discriminator is IDEMPOTENCE: order-independence (commutativity) most merges
//! have; re-delivery safety (idempotence) is what additive lacks.

use prism_schema::{algebra, Schema, Value};

fn apply(schema: &Schema, current: &Value, delta: &Value) -> Value {
    algebra::apply_with(None, schema, current, delta)
}

#[test]
fn grow_only_map_join_is_a_semilattice_so_mesh_safe() {
    // A `map[T]` reconciled by `_add` (key-union, a G-Set / grow-only OR-Map) is
    // the easy, mesh-safe case: each peer contributes distinct keys.
    let schema = Schema::Map { value: Box::new(Schema::Any) };
    let s0 = Value::tree([("alice", Value::Int(1))]);
    let from_bob = Value::tree([("_add", Value::tree([("bob", Value::Int(2))]))]);
    let from_carol = Value::tree([("_add", Value::tree([("carol", Value::Int(3))]))]);

    // IDEMPOTENT — re-delivering bob's contribution changes nothing (robust to
    // retransmit / anti-entropy; no exactly-once needed → fits Tailscale's L3).
    let once = apply(&schema, &s0, &from_bob);
    let twice = apply(&schema, &once, &from_bob);
    assert_eq!(once, twice, "map-union join is IDEMPOTENT → mesh-safe");

    // COMMUTATIVE — two peers' contributions converge regardless of arrival order.
    let bc = apply(&schema, &apply(&schema, &s0, &from_bob), &from_carol);
    let cb = apply(&schema, &apply(&schema, &s0, &from_carol), &from_bob);
    assert_eq!(bc, cb, "map-union join is COMMUTATIVE → order-independent convergence");
    assert_eq!(bc.as_map().map(|m| m.len()), Some(3));
}

#[test]
fn bare_additive_join_is_not_idempotent_so_not_mesh_safe() {
    // THE classic CRDT footgun (survey, unanimous): a plain additive merge
    // DOUBLE-COUNTS on re-delivery — NOT idempotent, NOT a semilattice, NOT a
    // CRDT. A scalar additive link cannot be replicated coordination-free.
    let schema = Schema::float();
    let s0 = Value::float(0.0);
    let delta = Value::float(5.0);

    let once = apply(&schema, &s0, &delta); // 5
    let twice = apply(&schema, &once, &delta); // 10 — double-counted on retry
    assert_ne!(
        once, twice,
        "bare additive join is NOT idempotent (double-counts) → NOT mesh-safe; \
         the mesh-safe form is a per-source PN-counter (each peer a distinct key → \
         the grow-only-map case above)"
    );
}

#[test]
fn per_source_pool_recovers_mesh_safety_for_quantities() {
    // The fix for additive (survey: PN-Counter): give each peer its OWN key; the
    // pool's total is the sum of the per-source keys, but the MERGE across peers
    // is a key-union (idempotent), so the quantity converges coordination-free.
    let schema = Schema::Map { value: Box::new(Schema::Any) };
    let s0 = Value::tree([("alice", Value::float(3.0))]); // alice's contribution
    let bob_says = Value::tree([("_add", Value::tree([("bob", Value::float(7.0))]))]);

    let merged = apply(&schema, &s0, &bob_says);
    // Re-delivery is safe — bob's key just re-asserts, no double count.
    let merged2 = apply(&schema, &merged, &bob_says);
    assert_eq!(merged, merged2, "per-source pool join is idempotent");
    let total: f64 = merged
        .as_map()
        .unwrap()
        .values()
        .filter_map(|v| v.as_f64())
        .sum();
    assert_eq!(total, 10.0, "the pool total is the sum of per-source contributions");
}
