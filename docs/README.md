# prism docs — the map

40 design docs live here. This is the index. They are **living design notes**:
some describe built mechanisms, some are RFCs or roadmaps. Two anchors keep you
oriented —

- **[`chrysalis-primer.md`](chrysalis-primer.md)** is the source of truth for what
  the *language* actually does **today** (every example is executed by a test);
- **[`NEXT-SESSION.md`](NEXT-SESSION.md)** is the source of truth for what is
  *built* and what is *next*.

If a doc and the primer disagree, the primer wins. For the project overview and a
30-second run, start at the [top-level README](../README.md).

> Many docs name a **Steward** — the agent role that owns them (see
> [`../coord/ROSTER.md`](../coord/ROSTER.md)). prism is built by a team of agents
> that partition the work into roles and coordinate over the mesh; the docs are
> partitioned the same way.

---

## Start here

- **[`chrysalis-primer.md`](chrysalis-primer.md)** — the executable, canonical
  **language reference**. Every block is run by a test, so it can't drift. **Read first.**
- **[`NEXT-SESSION.md`](NEXT-SESSION.md)** — the live status + task tracker (the
  ON-BOOT prompt, the remaining-work categories, the completed archive).
- **[`../README.md`](../README.md)** — the project front door: what prism is, the
  domains, how to run something.

## The spine — why it's all one thing

The categorical through-line. Read these for the *why* and the order of everything else.

- **[`categorical-core.md`](categorical-core.md)** — **the spine.** One substrate,
  many theories, the functors between them; `fold`/`unfurl` = entanglement =
  M/R-closure = reflection. Every other doc is a facet of this.
- **[`grand-synthesis.md`](grand-synthesis.md)** — **the north star.** One
  continuously-running, mesh-distributed simulation of every domain at once; the
  M0→M5 milestone path.
- **[`generative-core.md`](generative-core.md)** — the design philosophy: maximize
  emergent capability from a minimal basis — "one way to do each thing" + the
  one-door build-failing discipline.
- **[`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md)** — the
  self-similar bigraph: the composite boundary, the outer link graph, and
  `fold`/`unfurl` as a single object↔morphism move.

## The substrate — the engine

- **[`prism-architecture.md`](prism-architecture.md)** — crates, the
  Process/Step/BRS/Engine primitives, and the `discover_processes` reflection.
  **Read before engine work.**
- **[`schema-algebra.md`](schema-algebra.md)** — the schema layer as a *closed
  algebra*: the operations, their laws, and the closure invariant.
- **[`state-schema-unification.md`](state-schema-unification.md)** — the plan to
  unify the (historically inconsistent) state / schema / node representations into
  one schema-always-present core.
- **[`execution-model.md`](execution-model.md)** — how prism runs and parallelizes:
  the BSP tick and the one seam that makes local / parallel / rest / stream all
  parallelize alike.
- **[`upstream-alignment.md`](upstream-alignment.md)** — what is ported, extended,
  and intentionally divergent vs the `process-bigraph` / `bigraph-schema` Python
  originals.

## The surface — chrysalis

(The **[primer](chrysalis-primer.md)** is the tested reference; these are the
design depth behind it.)

- **[`chrysalis-design.md`](chrysalis-design.md)** — the full language design: the
  syntactic kernel, the categorical structure, units, contracts, the benchmark tiers.
- **[`homoiconic-unification.md`](homoiconic-unification.md)** — one `quote`, one
  `reify`, one `eval`: programs as data; the metacircular round-trip.
- **[`functors.md`](functors.md)** — the first-class `functor` definer:
  structure-preserving maps (render is the first), realizing categorical-core §4.
- **[`protocols-as-types.md`](protocols-as-types.md)** — addresses as first-class
  typed values: bind a protocol (`stream:` / `rest:` / `parallel:`) to a type and
  use it like any control.
- **[`units-in-the-schema.md`](units-in-the-schema.md)** — units as a *dimension
  refinement* of the schema (checked, then erased to raw `f64`), not a
  compile-time-only erasure.
- **[`canonical-run-core.md`](canonical-run-core.md)** — one `Core` threaded through
  the `run()` boundary; the rule that *a package is a Core*.

## The domains — the verticals

- **[`cells-and-division.md`](cells-and-division.md)** — **biology:** the one cell
  representation, the bridge-bounded composite, and schema-first
  division-through-reaction.
- **[`process-contracts.md`](process-contracts.md)** — what SED-ML / KISAO are
  actually doing, and the typed alternative: a process declares the mathematical
  object it approximates.
- **[`process-contracts-implementation.md`](process-contracts-implementation.md)** —
  each line of a contract mapped to its prism mechanism.
- **[`agreement-demo.md`](agreement-demo.md)** — three notions of "agree": the
  flagship process-contract demo, with real COPASI / Tellurium engines as fulfillers.
- **[`kisao-export.md`](kisao-export.md)** — KISAO identifiers in contracts + SED-ML
  / OMEX export (the reproducibility-standard bridge).
- **[`quantum-bigraphs.md`](quantum-bigraphs.md)** — **quantum:** unifying quantum
  computation with the bigraph substrate (entanglement, teleportation, measurement).
- **[`effects-and-handlers.md`](effects-and-handlers.md)** — algebraic effects &
  handlers: the lens that reframes quantum (and more) as one mechanism.
- **[`synthesis-bigraphs.md`](synthesis-bigraphs.md)** — **synthesizers:** a
  generative modular synth on the substrate — the `Signal` sort and the A1→A9 build
  order.
- **[`synth-module-library.md`](synth-module-library.md)** — the patch-programmable
  "Eurorack in prism": the audio domain's curated generator modules.
- **[`organism.md`](organism.md)** — **the living demo:** the spine become one
  self-producing, audible, visible object — M/R metabolism, sonified and seen (M5).

## Distribution & the mesh

- **[`distributed-execution.md`](distributed-execution.md)** — planet-scale colonies:
  domain decomposition + halo exchange + FMM + adaptive octrees, and how prism's
  composites/protocols/bridge already encode the skeleton.
- **[`distributed-bigraphs.md`](distributed-bigraphs.md)** — prism across many nodes:
  topology-rewriting reactions over the distributed / peer / mesh bigraph.
- **[`distributed-mesh-survey.md`](distributed-mesh-survey.md)** — a cited survey of
  P2P / mesh computing on a Tailscale mesh — the background grounding the native
  mesh design.
- **[`merge-protocol.md`](merge-protocol.md)** — the merge protocol: composite
  unification across the bridge, the structural dual of `divide`.

## Visualization & traces — the eye

- **[`bigraph-viewer.md`](bigraph-viewer.md)** — the viewer: render as a functor,
  navigation by `fold`/`unfurl`, side↔top-down transitions as natural transformations.
- **[`web-bigraphs.md`](web-bigraphs.md)** — HTML *is* a bigraph (nesting = place,
  id/href = link); render = a functor to the markup prop; the self-displaying system.
- **[`delta-traces.md`](delta-traces.md)** — a simulation as a procession of
  schema-deltas in time — the trace kernel behind replay, plotting, and time-travel
  debugging.

## Packages & ecosystem

- **[`packages-ecosystem.md`](packages-ecosystem.md)** — dependency management as
  theory composition (#67): a manifest + resolver + registry; a package is a `Core`.
- **[`packages-decomposition.md`](packages-decomposition.md)** — the domains as a
  category of theories — breaking the monolith into `packages/<domain>/`.
- **[`domain-libraries.md`](domain-libraries.md)** — each domain = a foundational
  library + rich examples.

## Process & meta

- **[`ys-corpus-health.md`](ys-corpus-health.md)** — keeping the `.ys` corpus
  *provably correct*, not just non-crashing (the diagnostics / DX angle).
- **[`doc-stewardship.md`](doc-stewardship.md)** — how the docs stay fresh: the
  per-heartbeat `stewards` signoff, the two-tier (test-net / signoff) model, and the
  stewardship map. (Steward: `primer`.)
- **[`coord-set-command.md`](coord-set-command.md)** — `chrysalis coord set`:
  updating a coordination heartbeat from DATA (the board's remain-parsable invariant).
- **[`exploring-the-computational-unknown.md`](exploring-the-computational-unknown.md)**
  — a side-quest survey: programs that operate on programs (reflective towers,
  staging, effects, probabilistic / reversible / quantum computing).
