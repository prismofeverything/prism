# Units in the schema — a dimension refinement, not a compile-time-only erasure

> **Design note** (owner: `core`; surfaced 2026-06-09 reviewing the Axis-A
> "unit/type not serialized" gap with the human). Companion to **#68** (the
> fundamental-type catalog) and the homoiconic-unification Axis-A completeness.
> Captures *where units live today*, why that blocks reflection / serialization /
> first-class dimension checking, and the principled fix.

## 1. Where units are today — compile-time, then erased

prism does dimensional analysis as a **compile-time** pass and then **erases** it:

- `crates/chrysalis/src/units.rs` is, in its own words, "the 'check' half of *check
  once, erase, run raw*." `UnitEnv::infer` walks each expression assigning a `Unit`,
  checks `+`/`-`/comparisons agree dimensionally (`DimensionMismatch`), resolves
  conversions (consulting `Context`s for cross-dimension coercions) — using the units
  algebra in `prism-schema/src/units.rs` (`Dimension`/`Unit`/`Conversion`/`Context`).
- Once the check succeeds, *"magnitudes are bare `f64` and units never reach the
  engine"* (units.rs header). A conversion (gram→kg, or concentration↔count via a
  volume `Context`) is **baked as a constant factor at the boundary**, threaded from
  state where the factor is dynamic (`thread_factor_inputs`; memory
  `project_units_cross_boundary`).
- What survives into the runtime `Schema` is **only the extensive/intensive bit**
  (`crates/chrysalis/src/schema.rs:149`): `Quantity[…, extensive]` → `Schema::Delta`
  (additive, halves on `divide`), intensive → `Schema::Float` (shares). **The dimension
  is dropped.**

So a `type Mass = Quantity[M, extensive]` is, at runtime, just a `Delta`. The `Mass`
name and the `[M]` dimension are gone.

## 2. Why this *is* the Axis-A "units not serialized" gap

The homoiconic unification (`homoiconic-unification.md`) needs every *definition* to be
data (`quote ↔ reify ↔ run`). A `type` carrying units can't round-trip: its
representation erases to `Delta`, so `EntityDef::to_value`/`from_value` has nothing to
serialize for the dimension. That is lang's "unit/type not serialized" finding seen from
the type side — **units were designed to be erased, so there is no durable schema
representation to serialize.** The erasure was the right call for the numeric hot path
when units were built, *before* reflection / homoiconicity became central; it conflicts
with them now.

## 3. The fix — decouple two erasures

"Check once, erase, run raw" quietly couples two erasures that needn't be:

| erase from… | keep? | why |
|---|---|---|
| the **value** (raw `f64` at runtime) | ✅ KEEP | zero-cost; the engine's per-tick arithmetic never touches units (`feedback_zero_cost`) |
| the **type / schema** (the `Dimension`) | ❌ STOP | this is what blocks serialization, reflection, and first-class dimension-checked wiring |

**Carry the `Dimension` in the schema** — a dimension-refined numeric sort. Cleanest
shape: `Float`/`Delta` gain an optional `dimension: Option<Dimension>`, or a dedicated
`Quantity { dimension, extensive }` sort the algebra treats as `Delta`/`Float` for
arithmetic. The dimension is present in the **type**, absent from the **value**. Then:

- **`check`** consults it for **wiring**: a dimension mismatch on a link is a
  schema-algebra `check` / `resolve` failure — first-class, not a bespoke chrysalis pass.
- the **boundary codec** consults it for **conversion**: gram→kg becomes schema-driven
  `apply` at the bridge, subsuming the baked-factor / `thread_factor_inputs` workaround.
  A cross-dimension `Context` is a typed coercion the codec applies.
- **`serialize`** carries it → a `type Mass` round-trips → **Axis-A's units half is
  fixed** (the dimension is data).
- **`divide` / `tensor`** already do the right thing via the extensive/`Delta` mapping;
  the dimension rides along unchanged.

All consulted at **check / wire / boundary / serialize** time — *rare, not per-op* — so
the numeric hot path stays bare `f64`. The chrysalis units checker stays (it still
infers + rejects illegal programs early); it now **feeds the dimension into the schema**
instead of erasing past it.

## 4. Why this is the principled home

- `feedback_schema_algebra` — "schema drives all operations." A dimension check that
  lives in the schema and runs through `check`/`resolve`/codec is more consistent than a
  compile-time side-channel.
- `project_schema_as_state` — schema is first-class operable state; a `type` with units
  is then genuinely data.
- It is the substance of **#68** (the fundamental-type catalog / shared base-sort
  vocabulary): units are the first base-sort *refinement* to make first-class, the model
  for `Complex` / `Signal[ℂ]` and the rest.

## 5. Scope (a real but bounded piece)

- **core**: the schema refinement (`Option<Dimension>` on `Float`/`Delta`, or a
  `Quantity` sort) + thread `Dimension` through `check` (wiring), the boundary codec
  (`apply`/conversion), and `serialize`/`deserialize`; serde on the
  `prism-schema::units` types (the gated add — needs `Ratio` serde). `divide`/`tensor`
  unchanged.
- **chrysalis**: `lower_schema` keeps the dimension on `Quantity` instead of collapsing
  to `Delta`/`Float`; the units checker feeds the schema; `EntityDef` serializes the
  `type` arm through the codec.
- **proof / consumers**: a `type Mass = Quantity[…]` round-trips through `quote ↔ reify`;
  a dimension-mismatched wire is a `check` error; a gram→kg bridge converts via the codec.

## 6. References

- `prism-schema/src/units.rs` (the algebra) · `crates/chrysalis/src/units.rs` (the
  check/erase) · `crates/chrysalis/src/schema.rs:149` (`Quantity` → `Delta`/`Float`).
- `docs/homoiconic-unification.md` (Axis-A) · `docs/schema-algebra.md` (the ops a
  dimension would thread through) · `docs/categorical-core.md` §6 (#68) · memories
  `feedback_zero_cost`, `project_units_cross_boundary`, `feedback_schema_algebra`.
