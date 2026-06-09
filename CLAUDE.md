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
- **`docs/NEXT-SESSION.md`** — **READ FIRST on boot.** Lean tracker: the `⏯️ ON BOOT`
  prompt + a single **`⏯️ CURRENT STATUS`** block (latest session) + **Remaining work —
  by category** (A Distribution/Mesh · B Biology · C Quantum · D Synthesizers · E
  Unification/Fundamentals · F Tooling · G Performance; rebuild the ephemeral harness
  panel from the `#N` entries here) + a **`✅ Completed`** archive + a 1-line-per-session
  archive + long-run milestones. The durable record of where we are and what's next.
- **`docs/grand-synthesis.md`** — the **roadmap to the ultimate demo**: one
  distributed/streaming mesh-network simulation running biology + quantum +
  synthesizers at once. The one-engine thesis (three domains = three specializations
  of the same substrate) + the M0→M4 milestone path + which tasks feed each. Read
  with NEXT-SESSION.md for the *why* and the *order*.
- **`docs/execution-model.md`** — how prism runs + parallelizes: the BSP tick and
  the one seam (`invoke() → Defer → flush → collect`) that makes
  `local`/`parallel`/`rest`/`stream`/batched-`ray` all parallelize through the same
  code. Read before any parallelism/scheduling work.
- **`docs/distributed-execution.md`** — the plan for **planet-scale** colonies:
  the fractal/octree vision = SOTA HPC (domain decomposition + halo exchange +
  FMM aggregation + adaptive octrees), how prism's composites/protocols/bridge/
  encapsulation/BSP already encode it, the gaps, a phased plan (#25–#29), and the
  build-vs-adopt decision (cluster = a protocol with a pluggable backend). Read
  before distributed/scaling work.
- **`docs/bigraphs-all-the-way-down.md`** — the thesis that prism's
  "two place graphs" (state-tree *inside* a composite; composites
  *within* composites) are ONE self-similar bigraph; the composite
  boundary is a `fold` we built for distribution; `fold`/`unfurl` is the
  object↔morphism maneuver (ports = where the link graph is cut); and one
  BRS can rewrite **both** levels — topology-rewriting reactions over the
  distributed/peer/mesh streaming bigraph. Also: link-graph scoping = the
  **implicit composite** (a connected component) vs today's place-graph
  *mediating* composite. Read for the distributed-bigraph / program-
  autonomy direction.
- **`docs/schema-algebra.md`** — the schema layer stated as an **algebra**:
  the sorts, the closed set of operations (`default`/`check`/`apply`/
  `reconcile`/`resolve`/`promote`/`merge`/`diff`/`generalize`/`coerce`/
  `divide`/`serialize`/…), their laws, and the **closure invariant**
  (nothing manipulates schema/state outside these ops). Read before any
  schema/engine/composite work. The faithful ports live in
  `bigraph_schema/methods/` (cloned at `../bigraph-schema`).
- **`docs/generative-core.md`** — the design **philosophy + the theory
  behind it**: maximize emergent capability from a minimal basis ("one way
  to do each thing") while staying free to add needed tools. Maps the wish
  to real theory — generating bases / clones (completeness = closure under
  composition), confluence + normal forms (the "auto-merge duplications"
  engine), Felleisen macro-expressibility + conservative extensions, and the
  *one-door build-failing invariant* as the automatable, non-straitjacket
  form (Rice makes full dup-detection undecidable). The standard for the
  unification work; companion to the schema algebra and the wei-qi memory.
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
- **The schema layer is a closed ALGEBRA — stay inside it.** All
  schema/state manipulation goes through the operations in
  `docs/schema-algebra.md` (`default`/`check`/`apply`/`reconcile`/
  `resolve`/`promote`/`merge`/`diff`/`generalize`/`coerce`/`divide`/
  `serialize`/…), each a faithful port of `bigraph_schema/methods/`.
  **Never** hand-roll a merge/diff/apply, stamp `Schema::Any` to dodge a
  type, or inline `_add`/`_remove` munging at a call site — those are
  "extra operations outside the algebra," and they are exactly how prism
  accreted `infer_and_merge`, `overlay_apply_types`, the schemaless
  `Composite`, and a dead `reconcile`. If a behaviour isn't expressible
  in the algebra, **add a named operation with a signature + laws +
  property tests** (executable axioms in `prism-schema`), together with
  the consumer that needs it — do not squish around it. Composite/engine
  logic is *defined in terms of* the algebra (law #10 in the doc). See
  memory `feedback_schema_algebra`.
