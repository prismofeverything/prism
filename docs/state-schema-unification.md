# State / Schema Unification

> The core must be rock-solid. Today it isn't, because there are several
> different ways to represent "a node in state" and several places schema
> can live. This document surveys the incoherence and proposes a single,
> schema-always-present, algebraic representation to converge on.

## Principle

**Schema and state are inseparable.** Every piece of state has a
corresponding schema, and the schema is *entirely how we decide to
operate on it* — `apply`, `divide`, `serialize`, `discover`, `dispatch`,
wire-resolution. There is no state without a schema, and `Schema::Any`
is not an acceptable default. Schema is itself state and may change at
runtime (schema-as-state).

Corollaries:
- **One representation** for a node (process / step / composite). A
  composite *is* a process (Milner; design doc "Composite-as-process is
  a fact"), so it must have the *same shape*, not a special one.
- **One path**: discovery / instantiation / apply are schema-driven.
  No structural guessing, no address-scanning fallback.
- **Algebraic operations**: the core ops compose by definition (the
  s-category: composition `∘`, tensor `⊗`, projection). Our delta from
  that ideal is the work.

## Survey: the current incoherence

### A. Four representations of "a node in state"

| Site | Process | Composite |
|---|---|---|
| `chrysalis::eval::build_pure_spec` | bare `{address, config, inputs, outputs}` | — |
| `chrysalis::eval::build_composite_outer` | — | `{_type, <data slots>, _process: {spec}}` |
| engine typed (`engine::extract_processes`) | `Schema::Link`/`ProcessLink` at the slot, state = bare spec | `Schema::CompositeLink` |
| engine fallback (`engine::scan_for_processes`) | map containing key `"address"`, found at any depth | recurse until an `"address"` is found |
| `vivarium::is_process_node` / `is_composite_container` | has an address | "a map where **all** children are process nodes" |

A process is a *bare spec*; a composite buries its spec under `_process`
and adds `_type` + sibling data slots. So the two are **not** the same
shape — the design's "composite is a process" is false at the
representation level. This is the root inconsistency.

Why the asymmetry exists (history, not principle): a composite has
*observable data* (its output-bridged slots, e.g. `mass`) that must sit
in the state tree as siblings so a parent / BRS can read them; a leaf
process has no data of its own. So composites grew an outer wrapper.

### B. Two-plus discovery / instantiation paths

- **Typed** (`extract_processes`, engine.rs ~500): walks `self.schema`;
  at a `Link`/`ProcessLink`/`CompositeLink` node, reads the spec from
  state and instantiates. Used at construction *iff* `self.schema` is a
  `Tree`.
- **Address-scanning** (`scan_for_processes`, engine.rs ~1383): recurses
  state, instantiates any map with an `"address"`. Used by
  `discover_all_processes` / `discover_processes`. Chrysalis relies on
  this (its schema is `Any`).
- **Vivarium** has its own extraction (`extract_process` /
  `extract_nested_processes`).

These agree *mostly*, which is worse than disagreeing loudly: behavior
depends on which path runs (e.g. interval/scheduling is read from the
node type in one and the schema in another).

### C. Schema lives in three places

- `Engine::self.schema: Schema` — a field *beside* `self.state`, updated
  separately (`self.schema.resolve(...)`, engine.rs:449).
- Inline `Value::Foreign(Foreign::new("schema", …))` — the schema-as-
  state primitive (registry.rs, RT.3). Schemas *can* be values.
- Derived on the fly — `chrysalis::schema` (from AST), `vivarium` (from
  JSON), or defaulted to `Any`.

So "the schema for this state" is ambiguous: which source wins, and when?

### Consequences (the bugs we already found)
- **Nested-composite execution gap** (`project_composite_execution_gap`):
  the `_process`/outer-map form + multi-path discovery means deeply
  nested composites tick but don't propagate.
- **Address-scanning fallback** exists only because chrysalis schema is
  `Any` — remove the `Any`, the fallback becomes dead weight.
- **No clean method dispatch / no compile-time validation** — both need
  the schema to be present and authoritative, which it isn't.

## The unified design

### 1. One node shape

A node is always:

```
Node = { address, config, inputs, outputs, content? }
```

- `address` / `config` — its behavior (the "spec"), identical for leaf
  and composite.
- `inputs` / `outputs` — its interface (the morphism's domain/codomain).
- `content` — *optional* inner sub-state (a map of named child `Node`s
  and data). A **leaf process** has no `content`; a **composite** is
  simply a node *with* `content`. Observable data lives in `content` and
  is exposed through the interface — the same mechanism for both.

No `_process` burial, no `_type`-vs-`address` split, no bare-vs-wrapped
fork. "Composite is a process" becomes literally true: same shape, plus
content.

### 2. Schema always attached, one source of truth

Every node and every data slot has a schema, and it is the authority for
every operation. Leaf → `Link`/`ProcessLink`/`StepLink`; composite →
`CompositeLink { inner_schema }`; data → its value schema; rich type →
`Custom { name }` (→ `TypeRegistry`). Schema travels *with* state
(schema-as-state is the one mechanism, not a side field that drifts), so
it can change as state changes and is never inferred after the fact.

### 3. One schema-driven path

Discovery, instantiation, apply, divide, dispatch, wire-resolution all
read the schema. The schema says `CompositeLink` → instantiate a
sub-engine over `content` with `inner_schema`; says `Float` → additive
apply; says `Custom` → dispatch `TypeMethods`. Delete
`scan_for_processes`' structural guessing and
`is_composite_container`'s all-children heuristic — they become
unnecessary once schema is authoritative.

### 4. Algebraic core operations

Define the operations on `(schema, value)` pairs so they compose by
construction (the s-category already in `prism_schema::assembly`):
`compose`, `tensor`, `project`, `apply`, `divide`. Every higher-level
behavior (a tick, a reaction firing, a composite bridging) is then a
composition of these — no special-cased control flow.

## The delta → migration (test-guarded, core surgery)

1. **Pick the canonical node shape** (above) and make `chrysalis::eval`
   emit it *uniformly* for `process`, `step`, and `composite` — collapse
   `build_pure_spec` and `build_composite_outer` into one builder.
2. **Always attach schema**: `chrysalis::compile` sets a complete
   `Schema::Tree` (using `chrysalis::schema`) on the topology and on each
   composite's `content`; no `Any`.
3. **Collapse discovery** onto the schema-driven path; make
   `scan_for_processes` a thin shim over it (or delete it). Verify
   `grow_divide` + the spatio-flux suite stay green at each step — they
   are the guardrails.
4. **Route every op through schema** (apply/divide/dispatch), removing
   the parallel non-schema paths.
5. The nested-composite execution gap and the cross-boundary units
   routing should both fall out once there is one representation and one
   path.

## Sequencing note

This is core-engine surgery; do it in small, test-guarded steps with
`grow_divide` (chrysalis e2e) and the spatio-flux fixture/suite as
guardrails.

**Done — the guardrail is in place.** `crate::check::validate_connections`
checks every port wiring for **structural** (scalar can't wire to a map)
and **dimensional** (`[mass]` can't wire to `[time]`) compatibility, and
`compile` is gated on it — illegal connections are now compile errors,
not silent runtime nonsense. (Today it resolves local `Var` targets;
non-local paths like `cytoplasm.tf` get validated once the one-node
representation lands below.) This lets us *see* representation mismatches
as errors while we unify the runtime.

See memory `project_schema_state_unity`, and `project_schema_dispatch` /
`project_schema_as_state` for the longer-standing intent.
