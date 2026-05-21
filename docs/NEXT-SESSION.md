# Next session — launch prompt

## ✅ The fresh-core schema-algebra rebuild is COMPLETE (2026-05-21)

The rebuild described below (tasks #20→#18→#19→#21 + cutover) is **done**.
Closure is achieved and enforced. Full workspace green (375 passed, 2 ignored =
doctests). Do **not** re-do it. State of the core:

- **`prism_schema::algebra`** is the single public door for all schema/state
  transformation: `apply`/`apply_with`, `reconcile`, `merge`, `diff`,
  `resolve`, `promote`, `generalize`, plus `default`/`check`/`infer`/`realize`/
  `serialize`/`deserialize`. The raw mutators (`apply_update`,
  `apply_update_with`, `apply_add_remove`) are `pub(crate)` — external crates
  must use `algebra::*`, so a shortcut won't compile.
- **Laws** (`prism-schema/tests/algebra_laws.rs`, `proptest`): 13 executable
  axioms over generated `(schema, value)` pairs — apply-identity / preserves-
  sort, reconcile-coherence, diff↔apply inverse, resolve idempotent /
  commutative / associative / `Any`-identity, promote ≤ resolve, generalize
  idempotent / `Any`-identity, codec round-trip, check-default.
- **Closure guard** (`prism-bigraph/tests/closure_guard.rs`): ratchet over
  engine/composite/chrysalis; `KNOWN_REMAINING = &[]` (empty = closed). Keep it
  empty.
- **Stand-ins deleted**: `Schema::infer_and_merge`, the `Schema::resolve` stub,
  chrysalis `overlay_apply_types`, composite `compute_delta`/`merge_value_maps`,
  dead engine helpers. `from_state` = `resolve(infer(state), declared)`;
  `apply_projections_to` = `promote(slot, port)` + `algebra::apply` (fixed the
  #14 diffusion gotcha — un-ignored + passing); Composite output bridge =
  `algebra::diff` on its real inner schema (law #10).

See memory `schema-algebra-rebuild-done` and `composite-typing-and-resolve`.

## What's next (follow-up milestones, not yet done)

1. **First-class `Custom` types** — make a `Custom` indistinguishable from a
   built-in sort: a type = (representation `Schema`, op-handler overrides,
   value-methods). Thread a `Core`/registry through the algebra ops so each
   resolves a `Custom` to its repr+handlers (with-no-override = delegate to the
   representation; first-class by delegation). Add a chrysalis `type Name =
   <repr> with { op = <expr>, method(args) = <expr> }` surface — the
   algebraic-effects *handler* model. The closure rebuild is the prerequisite
   (one algebra fn per op to make `Custom`-aware, not scattered sites).
2. **`.ys` workflow files as task-graph composites** — an `.ys` file is itself a
   composite; a *workflow* style (init → simulate → analyze DAG, edges inferred
   from data dependencies = the step-trigger graph) compiles to a Composite of
   `Step` instances. A path toward replacing SED-ML. See task list +
   `docs/chrysalis-design.md`.

---

## Historical: the rebuild launch prompt (for reference)

The block below was the original launch prompt. Kept for provenance.

```
We're doing the fresh-core rebuild of prism's schema algebra. [...]
GOAL: faithfully port the schema algebra and rebuild Composite *in terms of it*,
deleting every ad-hoc stand-in, so the core has real algebraic closure.
ORDER: #20 scaffolding → #18 lattice → #19 value/update ops → #21 Composite →
cutover. DONE = every transform via the algebra; laws green; closure-guard
green; Composite via the algebra; stand-ins deleted; workspace green; diffusion
un-ignored. (Full text in git history.)
```
