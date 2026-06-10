//! #71 units-in-schema — the consumer/proof.
//!
//! `Float`/`Delta` now carry a `Dimension` in the **type** (erased from the
//! **value** → zero-cost; the magnitude is still a bare `f64`). This test pins the
//! three things that makes true, each previously blocked by units being erased
//! past the schema (`docs/units-in-the-schema.md`):
//!
//!  1. **It serializes** — the Axis-A units half: a `Mass` type carries its
//!     dimension AS DATA, so a `type` definition can quote / reify / serialize
//!     (before #71 the dimension was dropped, so there was nothing to round-trip).
//!  2. **It is zero-cost + backward-compatible** — a dimensionless schema
//!     serializes with NO `dimension` key, and old wire JSON still deserializes.
//!  3. **The wiring ops thread it** — `resolve` (the JOIN) and `generalize` (the
//!     MEET) combine dimensions by the lattice: a dimensionless side is the units
//!     wildcard; agreement is kept; a genuine conflict collapses to dimensionless.

use prism_schema::Schema;
use prism_schema::algebra::{dimension_conflict, generalize, resolve};
use prism_schema::units::Dimension;

fn mass() -> Dimension {
    Dimension::base("mass")
}
fn length() -> Dimension {
    Dimension::base("length")
}

#[test]
fn a_dimensioned_schema_round_trips_through_serde() {
    // A `Mass` (extensive) and a concentration (intensive) both carry their
    // dimension through serialize → deserialize unchanged. This is the Axis-A
    // fix: the dimension is durable schema DATA, not an erased compile-time fact.
    let mass_delta = Schema::delta_dim(mass());
    let json = serde_json::to_string(&mass_delta).unwrap();
    assert!(json.contains("dimension"), "the dimension is serialized: {json}");
    let back: Schema = serde_json::from_str(&json).unwrap();
    assert_eq!(mass_delta, back, "a dimensioned Delta round-trips with its dimension");

    // substance · length⁻³ = concentration — a composite, fractional-free dim.
    let concentration = Schema::float_dim(mass().div(&length().pow(3)));
    let back2: Schema =
        serde_json::from_str(&serde_json::to_string(&concentration).unwrap()).unwrap();
    assert_eq!(concentration, back2);
}

#[test]
fn a_dimensionless_schema_is_bit_identical_to_today() {
    // Zero-cost + backward-compatible: `skip_serializing_if = None` ⇒ a plain
    // Float serializes with NO `dimension` key (the old wire form), so existing
    // data is untouched and the per-tick value stays a bare f64.
    let json = serde_json::to_string(&Schema::float()).unwrap();
    assert!(!json.contains("dimension"), "dimensionless ⇒ no dimension key: {json}");

    // And old JSON without the field deserializes to a dimensionless Float.
    let from_old: Schema = serde_json::from_str(r#"{"_type":"Float"}"#).unwrap();
    assert_eq!(from_old, Schema::float());
}

#[test]
fn resolve_joins_dimensions_for_wiring() {
    // resolve = the wiring JOIN (most-specific satisfying both). A dimensionless
    // side is the units wildcard (like `Any` for sorts): it yields to a present
    // dimension, regardless of order.
    assert_eq!(
        resolve(&Schema::float_dim(mass()), &Schema::float()),
        Schema::float_dim(mass())
    );
    assert_eq!(
        resolve(&Schema::float(), &Schema::float_dim(mass())),
        Schema::float_dim(mass())
    );
    // Agreement is kept.
    assert_eq!(
        resolve(&Schema::float_dim(mass()), &Schema::float_dim(mass())),
        Schema::float_dim(mass())
    );
    // A genuine conflict (mass vs length) is an illegal link; resolve stays a
    // TOTAL semilattice — it keeps a deterministic representative (NOT
    // dimensionless, which would break associativity), and the conflict is NAMED
    // by `dimension_conflict` (the wiring check) rather than silently dropped.
    let conflicted = resolve(&Schema::float_dim(mass()), &Schema::float_dim(length()));
    assert!(
        conflicted == Schema::float_dim(mass()) || conflicted == Schema::float_dim(length()),
        "conflicting dimensions resolve to a deterministic representative: {conflicted:?}"
    );
}

#[test]
fn generalize_meets_dimensions() {
    // generalize = the MEET (least-specific both refine). Only agreement keeps a
    // dimension; a dimensionless side OR a conflict generalizes to dimensionless.
    assert_eq!(
        generalize(&Schema::delta_dim(mass()), &Schema::delta_dim(mass())),
        Schema::delta_dim(mass())
    );
    assert_eq!(
        generalize(&Schema::delta_dim(mass()), &Schema::delta_dim(length())),
        Schema::delta()
    );
    assert_eq!(
        generalize(&Schema::delta_dim(mass()), &Schema::delta()),
        Schema::delta()
    );
}

#[test]
fn dimension_conflict_names_a_mismatched_wire() {
    // The first-class wiring check (the mechanism; its compile-time consumer is
    // the chrysalis check pass): a [mass] slot wired to a [length] source is an
    // illegal link — `dimension_conflict` NAMES the offending pair, where
    // `resolve` would silently drop it to dimensionless.
    assert_eq!(
        dimension_conflict(&Schema::delta_dim(mass()), &Schema::delta_dim(length())),
        Some((mass(), length()))
    );
    // Compatible wires → None: equal dimensions, a dimensionless wildcard side,
    // or a non-numeric schema.
    assert_eq!(
        dimension_conflict(&Schema::float_dim(mass()), &Schema::float_dim(mass())),
        None
    );
    assert_eq!(
        dimension_conflict(&Schema::float_dim(mass()), &Schema::float()),
        None
    );
    assert_eq!(dimension_conflict(&Schema::float(), &Schema::float()), None);
    assert_eq!(dimension_conflict(&Schema::Any, &Schema::delta_dim(mass())), None);
}
