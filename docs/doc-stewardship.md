# Doc stewardship — the signoff (freshness, built in)

> **Steward:** `primer`. How the docs in `docs/` stay coherent and current — the
> project's own CALM / board pattern, applied to its own documentation.

## The problem

Stewardship used to be *asserted* (a `**Steward:**` line in a header) but never
*verified*. So a doc with an active steward stayed fresh as a side-effect of the
work — but the **exposition** docs (the README, the old GUIDE) had no steward and
rotted, and, worse, there was **no freshness signal**: you couldn't tell a current
doc from a stale one without reading it. Only the **primer** was safe, because its
[doctest](../crates/chrysalis/tests/primer_doctest.rs) is a guarantee that can't lie.

## The mechanism — stewardship is a board signoff

Stewardship rides the **same fractal as everything else** (per-source, monotone, on
the board — see [`../coord/ROSTER.md`](../coord/ROSTER.md)), so no one curates a
central map that itself rots:

- Each agent's `coord/<role>.ys` heartbeat carries a **`stewards`** field: the docs
  it owns → a **current-as-of** date (the *signoff*).

  ```
  stewards: { 'README.md': '2026-06-12', 'docs/chrysalis-primer.md': '2026-06-12 +doctest' }
  ```

- **Session-end ritual:** before winding down, sign off the docs you touched —

  ```sh
  chrysalis coord set <role> stewards='{"docs/your-doc.md":"2026-06-12"}'
  ```

  (like updating `coord/<role>.next`; the codec escapes data, so slash/dot keys are fine).

- The **board renders the whole map for free** — `chrysalis run coord/board.ys
  --time 1` merges every heartbeat's `stewards` field through the mesh link, so the
  live stewardship map *and* its freshness is one view, with no shared file.

- **Staleness becomes visible:** a doc whose signoff date lags its git mtime is
  drifting. (A `chrysalis docs --stale` check that flags the drift automatically is
  the natural next step — turning "visible" into "enforced.")

## Two tiers of freshness

| tier | guarantee | applies to |
|---|---|---|
| **test-net** | automatic — the doc *cannot* drift (CI goes red) | executable docs: the **primer** (`primer_doctest.rs`); extend to any doc with `ys` blocks |
| **signoff** | human judgment — staleness made *visible* on the board | the prose design docs (the other 39) |

The aspiration: make more docs executable, so freshness is *structural*. The primer
is the model — every fenced `ys` block is run by a test.

## The session-end sign-off ceremony

Winding down a session is a ritual — the wind-down dual of the boot command. Run it
for your role (invoke **`/signoff`**, or do the steps by hand):

1. **Update `coord/<role>.next`** — your durable resume (what landed, the immediate
   next step, open threads). You rebuild `coord/<role>.ys` from this on boot.
2. **Sign off your stewarded docs** — `chrysalis coord set <role>
   stewards='{"docs/…":"<today>"}'` for each doc you made current (a doc you own but
   did not review → `"assigned (review pending)"`).
3. **Park your heartbeat** — `chrysalis coord set <role> task='PARKED — …'
   status='…' build.state=idle`, a clean final state on the board.
4. **Hand off open threads** — leave any peer asks in your `to_<peer>` fields.
5. **Confirm** — read back your `.ys` + `.next`; report what the next boot resumes into.

Boot is the mirror — invoke **`/boot <role>`** (`chrysalis coord set <role>
task='booting — reading <role>.next'`, then read ROSTER + the board + your `.next`).
Symmetric open and close.

## The orchestrator — `primer`

`primer` does **not** own the map; it owns the **mechanism and the coherence**:

- **steward** the exposition docs (README, primer, this index) **and any doc outside a
  defined role** — `primer` is the catch-all owner of the orphans,
- assign a steward to any other **unstewarded** doc,
- **ping** a steward whose signoff has gone stale,
- watch **cross-doc drift** — the one thing no single steward sees.

This is the human-facing dual of `unify`: `unify` keeps the *structure* coherent,
`primer` keeps the *exposition* coherent.

## The stewardship map (proposed; each agent confirms by signing off its own)

| steward | docs |
|---|---|
| **primer** — exposition + catch-all (any doc outside a defined role) | `README.md` · `docs/README.md` · `chrysalis-primer.md` · `doc-stewardship.md` · `exploring-the-computational-unknown` |
| **unify** — spine + overall | `categorical-core` · `grand-synthesis` · `generative-core` · `bigraphs-all-the-way-down` · `homoiconic-unification` · `functors` · `organism` · `domain-libraries` · `packages-decomposition` · `packages-ecosystem` · `canonical-run-core` · `NEXT-SESSION` |
| **core** — substrate | `prism-architecture` · `schema-algebra` · `state-schema-unification` · `execution-model` · `upstream-alignment` · `units-in-the-schema` · `delta-traces` |
| **lang** — surface | `chrysalis-design` · `protocols-as-types` · `coord-set-command` · `ys-corpus-health` |
| **mesh** — distribution + viz | `distributed-execution` · `distributed-bigraphs` · `distributed-mesh-survey` · `merge-protocol` · `bigraph-viewer` · `web-bigraphs` |
| **bio** | `cells-and-division` · `process-contracts` · `process-contracts-implementation` · `agreement-demo` · `kisao-export` |
| **quantum** | `quantum-bigraphs` · `effects-and-handlers` |
| **synth** | `synthesis-bigraphs` · `synth-module-library` |

A doc with **no signoff yet** is "assigned, awaiting first signoff" — which is
exactly the visible-staleness the mechanism is for. `primer` reconciles this map
over time (it is not frozen here; the board is the live truth).

---

*Stewarded by `primer`, signed off 2026-06-12. This doc is itself in the map above.*
