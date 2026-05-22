//! The schema **algebra** — the single public door for all schema and
//! state transformation.
//!
//! This module is the closed set of operations from `docs/schema-algebra.md`,
//! each a faithful port of `bigraph_schema/methods/<op>.py`. The engine, the
//! Composite, and chrysalis are *defined in terms of* these operations and
//! perform **no** schema/state transformation outside them — no
//! `Schema::Any.apply` shortcut, no hand-rolled `_add`/`_remove`, no per-call
//! merge/diff/overlay. A behaviour the algebra can't express becomes a **new
//! named operation with laws** (property tests in `tests/algebra_laws.rs`),
//! never an inline munge.
//!
//! The carrier is a *typed value* `(schema, value)`. An *update* is a value in
//! the same sort interpreted as a change. The operations:
//!
//! | op | signature | meaning |
//! |---|---|---|
//! | [`default`]   | `s → v`        | canonical inhabitant (unit) of a sort |
//! | [`check`]     | `(s, v) → bool`| membership |
//! | [`infer`]     | `v → s`        | least sort `v` inhabits |
//! | [`realize`]   | `(s, v) → v`   | fill defaults to a complete value |
//! | [`apply`]     | `(s, v, u) → v`| act an update on a value |
//! | [`reconcile`] | `(s, [u]) → u` | combine many updates to one |
//! | [`merge`]     | `(s, v, v) → v`| combine two values |
//! | [`diff`]      | `(s, v, v) → u`| update taking the first value to the second |
//! | [`resolve`]   | `(s, s) → s`   | schema **join** (least sort refining both) |
//! | [`promote`]   | `(lib, sparse) → s` | **local** resolve over sparse's paths |
//! | [`generalize`]| `(s, s) → s`   | schema **meet** (greatest sort both refine) |
//! | [`serialize`] / [`deserialize`] | `(s, v) ↔ json` | codec across boundaries |
//!
//! The laws these satisfy are the executable axioms in
//! `prism-schema/tests/algebra_laws.rs`; they double as a
//! cross-implementation conformance suite (any *model* of the theory is
//! faithful iff it passes them).

use crate::registry::TypeRegistry;
use crate::schema::Schema;
use crate::value::Value;

// ── Schema lattice (join / meet / local join) ──────────────────────────
//
// `resolve` (join) and `generalize` (meet) and `promote` (local resolve)
// live in `crate::resolve`; re-exported here so the lattice is reached only
// through the algebra surface.
pub use crate::resolve::{generalize, promote, refines, resolve};

// ── Update combination ─────────────────────────────────────────────────
//
// `reconcile` combines a batch of updates aimed at one path into one update;
// `reconcile_with` is the registry-aware form (Custom→representation).
pub use crate::reconcile::{reconcile, reconcile_with};

// ── Value combination ──────────────────────────────────────────────────
//
// `merge` folds two values; `diff` is apply's inverse (the update a→b);
// `diff_with` is the registry-aware form (Custom→representation).
pub use crate::diff::{diff, diff_with};
pub use crate::merge::merge;

// ── Value/update ops ───────────────────────────────────────────────────

/// `apply(s, v, u)` — act update `u` on value `v` under sort `s` (the action
/// of the update monoid). Additive for numeric/`Delta`, replacing for
/// `Overwrite`, structural `_add`/`_remove` for collections.
///
/// Law: `apply(s, v, default-update(s)) ≡ v` (empty update is the identity).
#[inline]
pub fn apply(schema: &Schema, current: &Value, update: &Value) -> Value {
    schema.apply_update(current, update)
}

/// `apply` consulting a [`TypeRegistry`] for `Schema::Custom` dispatch and the
/// `_divide` sentinel. The registry-aware entry point the engine uses.
#[inline]
pub fn apply_with(
    registry: Option<&TypeRegistry>,
    schema: &Schema,
    current: &Value,
    update: &Value,
) -> Value {
    schema.apply_update_with(registry, current, update)
}

// ── Value queries / construction ───────────────────────────────────────

/// `default(s)` — the canonical inhabitant (unit) of a sort.
///
/// Law: `check(s, default(s))`.
#[inline]
pub fn default(schema: &Schema) -> Value {
    schema.default_value()
}

/// `check(s, v)` — membership: is `v` a value of sort `s`.
#[inline]
pub fn check(schema: &Schema, value: &Value) -> bool {
    schema.check(value)
}

/// `infer(v)` — the least sort `v` inhabits (structural, honoring `_type`).
#[inline]
pub fn infer(value: &Value) -> Schema {
    Schema::infer(value)
}

/// `realize(s, v)` — fill defaults / decode an encoded value to a complete
/// value of sort `s`. (Codec inverse of [`serialize`].)
#[inline]
pub fn realize(schema: &Schema, encoded: &Value) -> Value {
    schema.realize(encoded)
}

// ── Codec ──────────────────────────────────────────────────────────────

/// `serialize(s, v)` — encode a typed value to a JSON-compatible value.
///
/// Law: `deserialize(s, serialize(s, v)) ≡ v`.
#[inline]
pub fn serialize(schema: &Schema, value: &Value) -> Value {
    schema.encode(value)
}

/// `deserialize(s, v)` — decode a JSON-compatible value back to a typed value
/// (the inverse of [`serialize`]; same as [`realize`]).
#[inline]
pub fn deserialize(schema: &Schema, encoded: &Value) -> Value {
    schema.realize(encoded)
}
