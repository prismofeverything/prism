# `.ys` corpus health — provably-correct demos, not just non-crashing

> **Owner:** `lang` (diagnostics / DX — folds into #14 + the dependability trio #67a).
> **Steward:** `unify`. **Surfaced:** 2026-06-09 (the human: "how are tests green if a `.ys`
> file doesn't work?"). The specific files turned out to be a stale binary, but the concern
> is real: the `.ys` corpus is **smoke-tested, not behavior-tested**, and the `# Expected:`
> goldens the authors already wrote are never checked.

## The gap

`ys_files_run::all_ys_examples_run_clean` runs every `.ys` (at `--time 1`) and asserts ONE
thing: **it did not crash** (exit success; no `error`/`panic` on stderr/stdout). Two files
are `SKIP`'d (documented integration tests). It does **not** check that a file produces the
right or complete output. So a stub, an aspirational demo, or a reaction that silently never
fires all **pass** — as long as they don't throw. Concretely:

- **15 / 50** `.ys` files carry an `# Expected: {…}` golden comment — and **nothing asserts
  any of them.** They are documentation the engine never checks.
- The **keys-only `chrysalis run` display** (`cli.rs:358` prints the final-state *keys*, not
  values) means even a human can't easily see whether a demo produced the right result —
  `alchemy.ys` runs, but you can't see the `Sprout` that proves the reaction-made-a-reaction
  actually fired.
- The smoke runs at `--time 1` only, so a demo needing more ticks to demonstrate (a
  division, a convergence) is never exercised doing its thing.

Net: a `.ys` file can drift out of sync with the engine — in *completeness* or *correctness*
— with **no test signal**, because we only test that it doesn't throw.

## The fix — three parts, one enabling primitive

**1. `chrysalis run --json` — the enabling primitive (small).** Dump the full final state as
JSON, not just the keys. `emit_json` already exists for the batch path (`cli.rs:343`); the
script path (`:358`) just prints keys — so this is *reusing* what's there, not new
machinery. **One primitive, three wins:** assertable demo output (below), the
converged-board / orchestrator view (the keys-only gap `coord/orchestrator.ys` keeps
hitting), and human-inspectable demos.

**2. Executable `# Expected:` goldens — the behavioral test.** Formalize the demo header
convention (already mostly present across the corpus):

```
# Run:      chrysalis run <file> --time <T>
# Expected: { …json… }
```

A golden test runs each demo as its header specifies (with `--json`) and **diffs the output
against `# Expected:`**, failing the build on mismatch. This turns the smoke test into a
behavioral test — drift in completeness/correctness now fails. (Keep the no-crash floor for
demos that have no golden yet.)

**3. Backfill + mark aspirational.** Write the golden for the ~35/50 that lack one. For demos
blocked on a future feature (e.g. a `?c.divide()` method-call form gated on #30), add an
explicit `# STATUS: aspirational (blocked on #N)` directive + a tracked list (the `SKIP`
pattern, but for *runs-but-incomplete-by-design*), so the gap is **documented, not hidden** —
and the demo moves off the list when the blocker lands.

## Why this matters

This is the counterpart to the build-coordination + test-consolidation work: those hardened
*how fast* we test; this hardens *what the tests prove*. A green suite should mean "the demos
demonstrate what they claim," not just "the demos don't throw." And `--json` independently
closes the keys-only display gap the board/orchestrator thread keeps running into.

## Sequencing
1. **`chrysalis run --json`** first — tiny, and it independently unblocks the board view.
2. The **golden harness** (executable `# Expected:`) — convert `ys_files_run` to assert output.
3. **Backfill** goldens + the **aspirational** marker/list — a focused quality sweep.
