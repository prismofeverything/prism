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

   **First concrete instance: the process-contract demonstration** —
   `docs/process-contracts.md` (design + build sequence). A contract-typed
   COPASI/Tellurium-style comparison: two processes, one interface, each
   `fulfills` a shared contract (target semantics + method class), and a
   `Compare` node typed by that shared contract so an illegitimate comparison
   won't compile. Replaces `biocompose` (which stalled at the interface).
   - **Rung 1 substrate — built + green (2026-05-21):** mass-action ODE
     integrators in `spatio-flux::processes::mass_action` — `Rk4` /
     `ForwardEuler` over a shared `MassActionNetwork`, `TimeSeries` with
     `species_mse`, analytic-solution tests. **Now native processes:** wrapped as
     a one-shot `Step` (`MassActionIntegrator`) and registered in `build_registry`
     as `Rk4` / `ForwardEuler`, so chrysalis `extern Rk4` binds to them
     (`reg.create("Rk4", config)` → Foreign `TimeSeries`, tested). See
     `docs/chrysalis-design.md` "Two kinds of process".
   - **Contract layer — DONE (2026-05-21):** substitutability is
     `prism_schema::algebra::refines` (= `resolve==`, **no new op**); AST
     (`Def::Contract` / `ContractRef` / `PortDecl.contract`), lowering
     (`schema::contract_ref_schema` → `Tree` of nominal axes), enforcement
     (`check::check_contract` — a producer wired into a contract-demanding port
     must refine it, via producer paths like `r.trajectory`), and the **parser**
     (`contract …`, `port :: C`, `fulfills C[…]`). Proven by
     `prism-schema/tests/contract_substitutability.rs` (7) +
     `chrysalis/tests/contract_enforcement.rs` (3, AST) +
     `chrysalis/tests/parse_contract.rs` (2, parse `.ys` → enforce).
   - **Runnable demo COMPLETE (2026-05-22):** `TimeSeries::species_mse`/`overlay`
     are registered methods; `crates/chrysalis/ys/integrator-comparison.ys` runs
     end to end — parse → enforce contracts → compile with the native registry +
     injected methods (`compile_with_methods`) → Engine run → MSE produced
     (`chrysalis/tests/integrator_comparison.rs`, 2 green). Native integrators are
     one-shot `Process`es; `Compare` is a ys-native step calling the registered
     methods; contracts flow through wirings to slots
     (`check::collect_slot_contracts`). Gotcha learned:
     `reference_ys_workflow_dag_scheduling`.
   - **Remaining for the contracts arc:** rung-3 KISAO export (#5); convert other
     examples to `.ys` (#6); the fundamental-type catalog + packages (#1).
   - HiGHS/FBA is the cross-target **negative test** (constraint-based steady
     state ≠ mass-action ODE), not a fulfiller; dFBA is the bridge case.

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
