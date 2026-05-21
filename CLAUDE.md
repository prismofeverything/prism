# Prism

Rust workspace implementing Milner-style process bigraphs for
composable multi-scale simulation. Cellular biology is the driving
application domain.

## Where to look

- **`docs/prism-architecture.md`** — crate roles, the `Process` /
  `Step` / `BRS` / `Engine` primitives, the `discover_processes`
  reflection mechanism. Read this first.
- **`docs/state-schema-unification.md`** — survey of the (currently
  inconsistent) state / schema / node representations and the plan to
  unify them into one schema-always-present, algebraic core. Read before
  any engine/core work; schema and state are inseparable.
- **`docs/chrysalis-design.md`** — design for **chrysalis**, the
  surface PL compiling to this runtime. Homoiconicity goal, the
  bigraph atoms to canonize, the implied-assembly decision (trees
  are bigraphs sugared), the categorical structure (bigraphs as
  morphisms in an s-category — wiring is composition), the
  syntactic kernel (`K[args](body)` terms, `~{} ->{}` port-graph
  interface, `|` parallel composition, lowercase definers /
  capitalized controls), value methods via `MethodRegistry`, and
  the tiered benchmark examples: tier 1 (grow/divide, MAPK, static
  M/R) for patterns + reactions as first-class values; tier 2
  (evolving M/R, after Fontana's AlChemy) for process bodies as
  first-class values with schema-driven typed construction (illegal
  programs are unrepresentable, not "caught at runtime").
- **`crates/prism-mapk/src/rules.rs`** — canonical example of
  hand-written reaction rules in the Rust API. Source for chrysalis
  benchmark #2.
- **`crates/chrysalis/ys/`** — `.ys` files (chrYSalis surface
  syntax). The first,
  [`ys/grow-divide-unbounded.ys`](crates/chrysalis/ys/grow-divide-unbounded.ys),
  is the surface form of tier-1 benchmark #1;
  [`ys/grow-divide-glucose.ys`](crates/chrysalis/ys/grow-divide-glucose.ys)
  is the resource-limited variant (#1b);
  [`ys/nuclear-shuttle.ys`](crates/chrysalis/ys/nuclear-shuttle.ys)
  motivates units *contexts* (concentration↔counts across nested
  compartments). Parser is task #12; until then, AST equivalents live in
  `crates/chrysalis/src/fixtures/`.
- **`chrysalis-example`** (file at repo root) — older sketch in
  surface syntax. Superseded by `crates/chrysalis/ys/`.

## Conventions

- Work from the repo root — cargo finds the workspace from any subdir,
  and paths in these docs are written relative to it (never absolute,
  never machine-specific).
- prism is load-bearing infrastructure; design **on top** of it, not
  around it. Reflection via `discover_processes` is the substrate
  feature that makes higher-order computation already feasible.
- Bigraph state is `Value::Tree` (named parallel-of-nested) or
  `Value::List` (anonymous parallel). The tree-of-maps has an exact
  bigraph reading — see docs/chrysalis-design.md.
- For chrysalis design changes, update `docs/chrysalis-design.md` in
  the same commit as code changes.
- **Build the principled solution, not a workaround.** For core /
  foundational work, implement the full design — and port the upstream
  (bigraph-schema / process-bigraph) mechanism faithfully — from the
  start. Do not propose or ship a bounded shortcut as "good enough":
  half-measures calcify, get fixated as "right", and force costly
  redesigns of everything built on top. `docs/state-schema-unification.md`
  is the standard. Small, test-guarded steps are encouraged, but every
  step is *toward* the full design, never a load-bearing workaround. If
  there's nothing that consumes a new capability yet, build the consumer
  so we know it works. See memory `feedback_no_half_measures`.
- **chrysalis is a THIN layer; never reimplement prism inside it.**
  Before writing any matching, firing, scheduling, structural-diff, or
  bigraph-rewrite logic in `crates/chrysalis/`, grep prism first — it
  almost certainly exists already: `prism_schema::reaction`
  (`Pattern`, `find_matches`, `fire_rule_at`, `instantiate`, `apply_fire`),
  `prism_bigraph::BigraphicalReactiveSystem` (det / stochastic /
  Gillespie BRS `Process`), `Engine`, `Composite::from_config`,
  `discover_processes`, `MethodRegistry`, `divide_by_schema`,
  `prism_schema::units`. **Call these — do not clone them.** If prism's
  mechanism is broken or missing a capability you need, **fix or extend
  prism in place** (add a prism-side test) and then call it — existing
  prism code is changeable, not sacred. Routing around prism with a
  chrysalis copy is what produced `ChrysalisBrs` (a weaker duplicate of
  `BigraphicalReactiveSystem`) and trapped the project in a rebuild
  loop. chrysalis's legitimate surface area is: `parse` → `ast` →
  `eval` (Expr→Value / Expr→Pattern) → `compile` (build prism artifacts).
  Anything that looks like a runtime *algorithm* belongs in prism. See
  memory `feedback_chrysalis_thin_layer`.
