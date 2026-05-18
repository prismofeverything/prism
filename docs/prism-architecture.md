# Prism Architecture

A Rust workspace implementing Milner-style **process bigraphs** for
composable multi-scale simulation, with cellular biology as the
driving application domain. Unifies time-stepped processes, reactive
steps, and bigraphical reaction-rule rewriting under one runtime so
cell-scale phenomena (metabolism, signaling, spatial dynamics,
growth/division) compose without bespoke glue between simulators.

## Crates

- **`prism-schema`** — type system.
  - `Schema` enum covers structural types (`Tree`, `Map`, `List`),
    primitive types, `Schema::Link` (≈ function type for process
    slots), `Schema::Custom` for nominal types via `TypeRegistry`.
  - `reaction::ReactionRule` / `Pattern` for bigraphical reaction
    rules. Patterns support sites, link variables, sort constraints,
    structural maps, `absent` markers.
- **`prism-bigraph`** — the runtime. Two computational primitives:
  - **`Process` trait** — temporal. Declares `inputs()` / `outputs()`
    port schemas; runs on an `interval()`; takes state + interval →
    `Update`.
  - **`Step` trait** — dependency-triggered. No time; reactive
    dataflow.
  - **`BigraphicalReactiveSystem`** — a `Process` that fires Milner-
    style parametric reaction rules. Modes: Deterministic /
    Stochastic / Gillespie SSA τ-leap.
  - **`Engine`** — schedules processes by interval, triggers steps on
    changed paths, applies projections, and **dynamically discovers
    new processes** via `discover_processes` when state writes
    contain `_type` / `address` annotations. State IS code; the
    engine lifts code-shaped state into running computation. This is
    the reflection mechanism that makes higher-order /
    self-modifying computation feasible.
- **`prism-mapk`** — MAPK signaling cascade example (cell biology).
  Source of the seven canonical reaction rules used as chrysalis
  benchmark #2.
- **`spatio-flux`** — spatial physics (rapier2d) + FBA via HiGHS solver.
- **`prism-derive`** — proc macros for ergonomic schema declarations.
- **`prism-viz`** — visualization (plotters, SVG output).
- **`chrysalis`** *(planned)* — surface programming language compiling
  to this runtime. See [chrysalis-design.md](chrysalis-design.md).

## Reflection (the killer feature)

`discover_processes` makes state-as-code real: when a process emits a
`Value::Tree` with a `_type` marker and config, the engine
instantiates a fresh process on the next tick. This enables:

- Composites that emit more composites (cell division).
- Reactions whose reactum spawns running processes.
- Surface-language constructions (chrysalis) where a process can
  generate **new reaction rules** at runtime — the rules are values,
  the BRS picks them up automatically.

## State representation

`Value::Tree(IndexMap<String, Value>)` — keyed parallel-of-nested.
Bigraph reading: keys label parallel siblings, `_type` is the
control, nested subtrees are the `.` operator. `Value::List` covers
anonymous parallel (`B | B | B` populations). Scalars live as
attributes on ions, not place-graph atoms.

See chrysalis-design.md §"Implied bigraph assembly" for the full
decomposition table.
