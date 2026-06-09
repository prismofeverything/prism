//! Mesh-safety — is a schema's merge a join-SEMILATTICE (a CRDT)?
//!
//! A `mesh`/`peer` link is REPLICATED per peer and must converge with **no
//! coordinator**. Coordination-free convergence holds iff the link's
//! schema-reconcile (its δ-CRDT join) is commutative + associative +
//! **IDEMPOTENT** — the join-semilattice law (CALM: monotone ⇒
//! coordination-free). The discriminator is IDEMPOTENCE: additive merges
//! double-count on re-delivery; last-writer-wins isn't commutative without a
//! causal clock.
//!
//! [`mesh_safety`] is the algebra's **closure invariant** for the mesh: a
//! `mesh`-declared link whose value-schema is not mesh-safe is REJECTED at
//! declaration, before any replica can diverge. It is a structural recursion
//! over [`Schema`] — the dual of [`crate::registry::divide_by_schema`]'s
//! extensivity question — classifying each reconcile strategy (the strategies
//! are exactly those in [`crate::reconcile`]):
//!
//! | reconcile strategy | sorts | semilattice? |
//! |---|---|---|
//! | key union (per-source, dynamic keys) | `Map`, `RecursiveTree` | ✅ |
//! | immutable / stateless | `Const`, `Site`/`InnerName`/`OuterName`/`Interface` | ✅ (degenerate) |
//! | delegate to inner | `Maybe` | ✅ iff inner |
//! | per-field record | `Tree`, `Tuple` | ✅ iff every field |
//! | additive sum | `Float`, `Integer`, `Delta`, `Array` | ❌ not idempotent |
//! | last-writer-wins | `Overwrite`, `Quote`, `Bool`, `String`, `Enum`, `Any`, `Bridge` | ❌ not commutative |
//! | sequence append | `List` | ❌ not idempotent on re-delivery |
//! | process node | `Link`/`StepLink`/`ProcessLink`/`CompositeLink` | ❌ not data |
//! | rich type | `Custom` | delegate to its representation |
//!
//! The mesh-safe forms map to the survey's CRDTs: a `Map` keyed by SOURCE is a
//! G-Set / PN-Counter — each peer writes only its own key, so the key-union join
//! is idempotent + commutative (single-writer-per-key ⇒ no concurrent inner
//! conflict; that per-source discipline is the realization's contract, which the
//! schema *admits*). A bare additive scalar is THE footgun; the fix for an
//! additive quantity is exactly the per-source pool (`map[peer -> qty]`). A
//! `Tree` branch, by contrast, is a fixed SHARED slot (not per-source), so each
//! branch must independently be a semilattice.
//!
//! Grounded by the 2026-06-08 P2P survey (Shapiro et al. CRDTs; Almeida/Shoker/
//! Baquero δ-state CRDTs; Hellerstein/Alvaro CALM) — see memory
//! `mesh_as_protocol`. The executable laws live in
//! `prism-schema/tests/crdt_laws.rs`.

use crate::registry::TypeRegistry;
use crate::schema::Schema;

/// Why a schema's merge is not a join-semilattice — so a replicated
/// `mesh`/`peer` link over it cannot converge coordination-free. The `reason`
/// names the offending (sub-)schema's reconcile strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshUnsafe {
    pub reason: String,
}

impl std::fmt::Display for MeshUnsafe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not mesh-safe: {}", self.reason)
    }
}
impl std::error::Error for MeshUnsafe {}

fn unsafe_(reason: impl Into<String>) -> Result<(), MeshUnsafe> {
    Err(MeshUnsafe {
        reason: reason.into(),
    })
}

/// `mesh_safety(s)` — is sort `s`'s reconcile a join-SEMILATTICE (a CRDT), so a
/// replicated `mesh` link over it converges with no coordinator? `Ok(())` ⇒ safe
/// to replicate; `Err` carries the reason (the offending reconcile strategy).
/// The algebra's closure invariant for the mesh.
///
/// Registryless form — a `Custom` slot with no resolvable representation is
/// conservatively rejected; use [`mesh_safety_with`] to classify `Custom` types.
///
/// Law (executable in `tests/crdt_laws.rs`): for every `s` with `mesh_safety(s)
/// == Ok`, the δ-CRDT join `apply(s, ·, δ)` is idempotent + commutative; for the
/// canonical `Err` cases it is not.
pub fn mesh_safety(schema: &Schema) -> Result<(), MeshUnsafe> {
    mesh_safety_with(None, schema)
}

/// [`mesh_safety`] consulting a [`TypeRegistry`] so a `Schema::Custom` slot is
/// classified by its REPRESENTATION — mirroring `reconcile_with` / `apply_with`,
/// whose `Custom` arm also delegates to the representation (so the classifier
/// agrees with the reconcile it is classifying).
pub fn mesh_safety_with(
    registry: Option<&TypeRegistry>,
    schema: &Schema,
) -> Result<(), MeshUnsafe> {
    match schema {
        // ── Key-union containers (per-source / dynamic keys): the canonical
        //    CRDT. A `Map` keyed by source is a G-Set / PN-Counter; the key-union
        //    join is idempotent + commutative. `RecursiveTree` is the nested form.
        Schema::Map { .. } | Schema::RecursiveTree { .. } => Ok(()),

        // ── Immutable / stateless: trivially a degenerate semilattice (the join
        //    never moves the replica).
        Schema::Const { .. }
        | Schema::Site { .. }
        | Schema::InnerName { .. }
        | Schema::OuterName { .. }
        | Schema::Interface { .. } => Ok(()),

        // ── Delegating wrapper: safe iff its inner is.
        Schema::Maybe { inner } => mesh_safety_with(registry, inner),

        // ── Fixed-field records: each branch is a SHARED mutable slot (not
        //    per-source), so each must independently be a semilattice.
        Schema::Tree { branches } => {
            for (k, b) in branches {
                mesh_safety_with(registry, b).map_err(|e| MeshUnsafe {
                    reason: format!("branch `{k}`: {}", e.reason),
                })?;
            }
            Ok(())
        }
        Schema::Tuple { elements } => {
            for (i, e) in elements.iter().enumerate() {
                mesh_safety_with(registry, e).map_err(|err| MeshUnsafe {
                    reason: format!("element {i}: {}", err.reason),
                })?;
            }
            Ok(())
        }

        // ── Additive: NOT idempotent — double-counts on re-delivery (the
        //    footgun). The mesh-safe form for a quantity is a per-source pool.
        Schema::Float { .. } | Schema::Integer { .. } | Schema::Delta { .. } => unsafe_(
            "additive scalar double-counts on re-delivery (not idempotent); \
             replicate as a per-source pool `map[peer -> qty]` instead",
        ),
        Schema::Array { .. } => {
            unsafe_("element-wise additive array double-counts on re-delivery (not idempotent)")
        }

        // ── Last-writer-wins: NOT commutative without a causal clock.
        Schema::Overwrite { .. } => {
            unsafe_("overwrite is last-writer-wins (not commutative without a causal clock)")
        }
        Schema::Bool { .. } | Schema::String { .. } | Schema::Enum { .. } => unsafe_(
            "atomic last-writer-wins (not commutative); wrap in a per-source `map`, \
             a `const`, or a registered CRDT type",
        ),
        Schema::Quote { .. } | Schema::Bridge { .. } | Schema::Any => unsafe_(
            "opaque last-writer-wins (not commutative); give it a concrete CRDT schema",
        ),

        // ── Sequence: append/concat isn't idempotent on re-delivery (sequence
        //    CRDTs need per-element unique ids). Use a keyed `map`.
        Schema::List { .. } => {
            unsafe_("list append/concat isn't idempotent on re-delivery; use a keyed `map`")
        }

        // ── A process/step/composite node is not replicable data.
        Schema::Link { .. }
        | Schema::StepLink { .. }
        | Schema::ProcessLink { .. }
        | Schema::CompositeLink { .. } => {
            unsafe_("a process/step/composite node is not a replicable data semilattice")
        }

        // ── Rich type: classify by its representation (no registry ⇒ reject),
        //    matching reconcile's Custom→representation delegation.
        Schema::Custom { name, .. } => match registry.and_then(|r| r.schema(name)) {
            Some(repr) => mesh_safety_with(registry, repr).map_err(|e| MeshUnsafe {
                reason: format!("type `{name}`: {}", e.reason),
            }),
            None => unsafe_(format!(
                "custom type `{name}` has no resolvable representation; register it \
                 (or declare it mesh-safe) before replicating"
            )),
        },
    }
}

/// `is_mesh_safe(s)` — convenience boolean form of [`mesh_safety`].
pub fn is_mesh_safe(schema: &Schema) -> bool {
    mesh_safety(schema).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use indexmap::IndexMap;

    #[test]
    fn map_and_recursive_tree_are_safe() {
        assert!(is_mesh_safe(&Schema::map(Schema::float())));
        assert!(is_mesh_safe(&Schema::map(Schema::Any)));
        assert!(is_mesh_safe(&Schema::RecursiveTree {
            leaf: Box::new(Schema::float())
        }));
    }

    #[test]
    fn additive_and_lww_scalars_are_unsafe() {
        for s in [
            Schema::float(),
            Schema::integer(),
            Schema::delta(),
            Schema::bool(),
            Schema::string(),
            Schema::Any,
            Schema::overwrite(Schema::float()),
        ] {
            assert!(mesh_safety(&s).is_err(), "{s:?} should be unsafe");
        }
    }

    #[test]
    fn tree_recurses_into_branches() {
        // A fixed branch that is additive makes the whole record unsafe…
        let bad = Schema::Tree {
            branches: IndexMap::from([("mass".into(), Schema::float())]),
        };
        let err = mesh_safety(&bad).unwrap_err();
        assert!(err.reason.contains("branch `mass`"), "{}", err.reason);

        // …but a record of per-source pools is safe.
        let good = Schema::Tree {
            branches: IndexMap::from([
                ("peers".into(), Schema::map(Schema::float())),
                ("pinned".into(), Schema::const_of(Schema::string())),
            ]),
        };
        assert!(is_mesh_safe(&good));
    }

    #[test]
    fn custom_classified_by_representation() {
        let mut reg = TypeRegistry::new();
        reg.register("Pool", Schema::map(Schema::float()), None);
        reg.register("Count", Schema::float(), None);
        let pool = Schema::Custom {
            name: "Pool".into(),
            parameters: Default::default(),
        };
        let count = Schema::Custom {
            name: "Count".into(),
            parameters: Default::default(),
        };
        assert!(mesh_safety_with(Some(&reg), &pool).is_ok());
        assert!(mesh_safety_with(Some(&reg), &count).is_err());
        // Unregistered / registryless ⇒ conservatively rejected.
        assert!(mesh_safety(&pool).is_err());
    }

    /// The op's correctness axiom, in miniature: a Safe schema's `_add` join is
    /// idempotent; an Unsafe additive schema's join is not. (Full laws in
    /// `tests/crdt_laws.rs`.)
    #[test]
    fn verdict_matches_idempotence() {
        use crate::algebra::apply_with;
        // Safe: map key-union.
        let safe = Schema::map(Schema::Any);
        let s0 = Value::tree([("a", Value::Int(1))]);
        let d = Value::tree([("_add", Value::tree([("b", Value::Int(2))]))]);
        let once = apply_with(None, &safe, &s0, &d);
        let twice = apply_with(None, &safe, &once, &d);
        assert!(is_mesh_safe(&safe) && once == twice);

        // Unsafe: additive float.
        let bad = Schema::float();
        let v1 = apply_with(None, &bad, &Value::float(0.0), &Value::float(5.0));
        let v2 = apply_with(None, &bad, &v1, &Value::float(5.0));
        assert!(mesh_safety(&bad).is_err() && v1 != v2);
    }
}
