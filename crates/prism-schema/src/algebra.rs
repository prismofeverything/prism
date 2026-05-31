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
/// of the update monoid): additive for numeric/`Delta`, replacing for
/// `Overwrite`, structural `_add`/`_remove` for collections, and `Custom`
/// dispatch + the `_divide` sentinel when a registry is threaded.
///
/// THE single apply door — there is no registryless `apply`. `registry`
/// is `Option` only so a genuinely-untyped context (a pure-schema unit test)
/// can pass `None`; every path where a `Custom` value can flow passes `Some`.
///
/// Law: `apply(s, v, default-update(s)) ≡ v` (empty update is the identity).
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

/// `default(s)` — the canonical inhabitant (unit) of a sort; dispatches a
/// `Custom` type's default when a registry is threaded. THE single default door.
///
/// Law: `check(s, default(s))`.
#[inline]
pub fn default_with(registry: Option<&TypeRegistry>, schema: &Schema) -> Value {
    schema.default_with_reg(registry)
}

/// `check(s, v)` — membership: is `v` a value of sort `s`; dispatches a
/// `Custom` type's check when a registry is threaded. THE single check door.
#[inline]
pub fn check_with(registry: Option<&TypeRegistry>, schema: &Schema, value: &Value) -> bool {
    schema.check_with_reg(registry, value)
}

/// `infer(v)` — the least sort `v` inhabits (structural, honoring `_type`).
#[inline]
pub fn infer(value: &Value) -> Schema {
    Schema::infer(value)
}

/// `realize(s, v)` — fill defaults / decode an encoded value to a complete
/// value of sort `s` (codec inverse of [`serialize_with`]); dispatches a
/// `Custom` type's realize at any depth when a registry is threaded, so a
/// declared type (`:: Qubits`) auto-promotes a bare literal to a tagged
/// instance. THE single realize door.
#[inline]
pub fn realize_with(
    registry: Option<&TypeRegistry>,
    schema: &Schema,
    encoded: &Value,
) -> Value {
    schema.realize_with_reg(registry, encoded)
}

// ── Codec ──────────────────────────────────────────────────────────────

/// `serialize(s, v)` — encode a typed value to a JSON-compatible value;
/// dispatches a `Custom` type's canonical wire form when a registry is
/// threaded. THE single serialize door (decode via [`realize_with`]).
///
/// Law: `realize_with(r, s, serialize_with(r, s, v)) ≡ v`.
#[inline]
pub fn serialize_with(registry: Option<&TypeRegistry>, schema: &Schema, value: &Value) -> Value {
    schema.serialize_with_reg(registry, value)
}
