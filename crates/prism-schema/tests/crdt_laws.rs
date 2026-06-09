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
    // The CLOSURE INVARIANT agrees up-front: a map-union link is admissible.
    assert!(algebra::mesh_safety(&schema).is_ok(), "map-union is a semilattice");
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
    // The closure invariant REJECTS it up-front — the algebra refuses to
    // replicate a bare additive scalar (it cannot converge coordination-free).
    assert!(algebra::mesh_safety(&schema).is_err(), "additive scalar is rejected");
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
    assert!(algebra::mesh_safety(&schema).is_ok(), "per-source pool is admissible");
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

#[test]
fn the_closure_invariant_classifies_every_sort_by_its_reconcile() {
    // `mesh_safety` is SOUND over the reconcile strategy: it accepts exactly the
    // join-semilattices and rejects additive / last-writer-wins / sequence /
    // process-node merges. This is the table the `mesh:` protocol gates on — a
    // link declared `mesh` is admissible iff its value-schema passes here.
    use indexmap::IndexMap;

    // Safe — key-union, immutable, or records/options built from safe parts.
    for s in [
        Schema::map(Schema::float()), // per-source pool
        Schema::RecursiveTree { leaf: Box::new(Schema::float()) },
        Schema::const_of(Schema::string()), // immutable
        Schema::maybe(Schema::map(Schema::float())), // option of a pool
        Schema::Tree {
            branches: IndexMap::from([
                ("peers".into(), Schema::map(Schema::float())),
                ("pinned".into(), Schema::const_of(Schema::float())),
            ]),
        },
    ] {
        assert!(algebra::is_mesh_safe(&s), "{s:?} should be mesh-safe");
    }

    // Unsafe — additive, last-writer-wins, sequence, or a record with such a field.
    for s in [
        Schema::float(),
        Schema::integer(),
        Schema::delta(), // additive
        Schema::Array { shape: vec![4], element: Box::new(Schema::float()) },
        Schema::overwrite(Schema::float()), // LWW
        Schema::bool(),
        Schema::string(), // atomic LWW
        Schema::Any,       // opaque LWW
        Schema::List { element: Box::new(Schema::float()) }, // sequence
        Schema::maybe(Schema::float()),                      // option of additive
        Schema::Tree { branches: IndexMap::from([("mass".into(), Schema::float())]) },
    ] {
        assert!(algebra::mesh_safety(&s).is_err(), "{s:?} should be rejected");
    }
}

#[test]
fn last_writer_wins_is_rejected_because_it_is_not_commutative() {
    // The OTHER failure mode (besides additive's non-idempotence): an Overwrite
    // link is idempotent but NOT commutative — concurrent writes from two peers
    // converge to whichever arrived last, so replicas DIVERGE by arrival order.
    let schema = Schema::overwrite(Schema::Any);
    let s0 = Value::String("init".into());
    let from_a = Value::String("a".into());
    let from_b = Value::String("b".into());

    let ab = apply(&schema, &apply(&schema, &s0, &from_a), &from_b); // "b"
    let ba = apply(&schema, &apply(&schema, &s0, &from_b), &from_a); // "a"
    assert_ne!(ab, ba, "overwrite is order-dependent → replicas diverge");

    // …so the closure invariant refuses it as a mesh link.
    assert!(algebra::mesh_safety(&schema).is_err());
}

#[test]
fn the_classifier_governs_the_merge_join_too() {
    // `mesh_safety` is derived from `reconcile`, but the `mesh:` protocol joins
    // replicas STATE-based via `algebra::merge`. The verdict governs both: for a
    // mesh-safe per-source map, `merge` is idempotent + commutative; for an unsafe
    // additive scalar, `merge` is order-dependent (last-writer-wins) — exactly
    // what the classifier already says.
    let map = Schema::map(Schema::float());
    let a = Value::tree([("alice", Value::float(1.0))]);
    let b = Value::tree([("bob", Value::float(2.0))]);
    let ab = algebra::merge(&map, &a, &b);
    assert_eq!(ab, algebra::merge(&map, &ab, &b), "per-source map merge is IDEMPOTENT");
    assert_eq!(ab, algebra::merge(&map, &b, &a), "…and COMMUTATIVE (disjoint keys)");
    assert!(algebra::is_mesh_safe(&map));

    let f = Schema::float();
    let z = Value::float(0.0);
    let one_two = algebra::merge(&f, &algebra::merge(&f, &z, &Value::float(1.0)), &Value::float(2.0));
    let two_one = algebra::merge(&f, &algebra::merge(&f, &z, &Value::float(2.0)), &Value::float(1.0));
    assert_ne!(one_two, two_one, "scalar merge is order-dependent (LWW)");
    assert!(algebra::mesh_safety(&f).is_err(), "…so the classifier rejects it");
}
