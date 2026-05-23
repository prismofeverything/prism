# Next session — launch prompt

## 🔧 IN PROGRESS (2026-05-22): engine execution-correctness arc

The schema *algebra* is complete (see below). This session found the **engine's
USE of it** (the run loop) had correctness gaps and fixed them in the core — no
workarounds. Started from a RunProcess/process-contract demo (#8) and the
integrator-comparison `.ys`; that uncovered the run-loop bugs. **Suite went 8
failures → 0** — full workspace green (67 suites).

**Landed (prism-bigraph/src/engine.rs, prism-schema/src/{schema.rs,reconcile.rs}):**
1. **invoke/apply separation** — run loop is now `advance_to_next_event`:
   invoke all due processes against ONE snapshot → advance time → apply together
   → trigger/discover. (Was fire=invoke+apply immediately ⇒ a process could see
   another's mid-tick mutation. The core correctness bug.)
2. **`reconcile` wired into apply** (`apply_reconciled`) — a tick's updates are
   reconciled into one combined update, applied once. `reconcile` had 13 laws +
   tests but was **never called by the engine** before.
3. **reconcile remove-wins** (reconcile.rs `reconcile_keyed`) — a `_remove`d key
   voids a concurrent value-update.
4. **apply `_add`/per-key ordering** (schema.rs — Map/Tree/RecursiveTree/Any
   arms) — per-key loop now bases on `result` (post-`_add`), not `cur`, so a
   re-added key composes with a concurrent update instead of reverting. (Was the
   dynamic_structure bug: rewired worker reverted to old config, never
   re-instantiated.)
5. **process front = creation time** (`add_process`: `next_time = self.time`, no
   `+interval` hack); mid-run-created processes land post-advance ⇒ run next step.
6. **step firing = dependency layers** (`run_step_layers`, replaced the
   readiness-wave): producer→consumer layers (pb `wire_step_layers`); each layer
   invokes one snapshot + reconciled apply. settle (init) + trigger (reactive)
   both use it.
7. **`from_state` auto-discovers** (idempotent via `fired_init_steps`) — like
   `Composite`, so callers that don't call `discover_all_processes` still work.
8. **projections → fragments + reconcile** (`apply_reconciled`) — each port
   output becomes a single-path fragment; `reconcile` combines them all
   (overlapping parent/child writes compose schema-aware — no hand-rolled merge).
   `apply_reconciled` also preserves each writer's port schema so additive fields
   promote across a nested composite bridge (the culture fix).
9. **`Engine::new` infers its schema** (`resolve(infer(state), declared)`, same as
   `from_state`) — no engine runs schema-free. This was the monod regression: with
   an `Any` root, `reconcile`/`apply` take the opaque *last-wins* branch and drop
   concurrent partial updates (biomass stayed 0.1). It only surfaced once fragments
   split a process's output into partial per-port updates — deep_merge had hidden
   it by handing reconcile one complete tree. Tasks #20 (unify construction paths)
   and #21 (survey algebra-evasion + schema-free tree ops) capture the follow-up.

**✅ DONE — full workspace green (66 suites, 0 failed).** The last failure
(`culture_imports_nests_and_runs_dish`) is fixed: `apply_reconciled` now takes
projection-sets and preserves each writer's port schema, so the additive `Array`
field promotes and reconstructs across the nested bridge (it had been overwritten
by the zero-sum diffusion delta).

**Plan (remaining — all follow-ups; the core is green + clean):**
- ✅ #16 done: `crates/prism-bigraph/tests/execution_invariants.rs` — 6 executable
  axioms: invoke/apply snapshot isolation, reconcile-by-sum, dependency layering
  (D=B*(A+B)=714), remove-wins, `_add`-compose, and `_remove`+`_add`=replace.
  (Mid-tick-creation-fires-next and removed-store-expiry are already covered by
  the grow_divide / dynamic_structure tests.)
- Resume the demo arc (#9 packages, #11 Plot split + view SVG, #5 KISAO) and the
  surveys/refactors (#19 method survey, #14 Core, #15 Foreign).

**Fast debug loop (build is slow — full workspace links ~16 binaries w/ heavy
deps):** `cargo check -p <crate>` (compile, no link, ~secs) for "does it build";
`cargo test -p <crate> --test <name>` for the touched area; full `cargo test`
only at checkpoints. No `.cargo/config.toml` linker override (rustflags change ⇒
whole-tree rebuild). See memory `feedback_test_timeout`.

**Memories added:** `feedback_no_sleep_polling`, `feedback_test_timeout`,
`feedback_ys_layering`. Pattern: the algebra is ported & faithful; gaps were the
engine's *use* of it. A defined-but-unused op (`reconcile`) was a latent bug ⇒
**#19** surveys upstream methods for more "ported but not wired" gaps.

**Tasks #1–19 in the tracker; key in-flight: #8, #16, #17, #18, #19.**

---

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
     injected methods (`compile_with_methods`) → Engine run → MSE + overlay
     `Figure` produced (`spatio-flux/tests/integrator_comparison.rs`, 3 green —
     the test lives in spatio-flux, not chrysalis, per `feedback_ys_layering`).
     Native integrators are one-shot `Process`es; `Compare` is a ys-native step
     calling the registered methods; contracts flow through wirings to slots
     (`check::collect_slot_contracts`). Gotcha learned:
     `reference_ys_workflow_dag_scheduling`.
   - **Artifacts (2026-05-22):** wrapped as a runnable example,
     `crates/spatio-flux/examples/integrator_comparison.rs` — `cargo run -p
     spatio-flux --example integrator_comparison` writes trajectory CSVs, an MSE
     table, the overlay SVG (the workflow's own `Figure`), and `state.json` to
     `outputs/integrator-comparison/` (gitignored). RK4 tracks `e^{-kt}`, Euler
     lags ⇒ MSE ≈ 3.6e-4/species: warranted comparison, method-induced divergence.
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
