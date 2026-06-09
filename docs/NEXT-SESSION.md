# Next session — launch prompt & plan

## ⏯️ ON BOOT — read this first

> Resume prism (Rust process-bigraphs + the `.ys` language). Read this prompt +
> the **CURRENT STATUS** block below + [`docs/grand-synthesis.md`](grand-synthesis.md)
> (the roadmap to the ultimate demo) + the `MEMORY.md` index. Then **rebuild the
> harness task list** from "## Remaining work — by category": `TaskCreate` one task
> per `#N` entry (all are pending / in-progress; the `## ✅ Completed` section is
> provenance, not panel tasks). Continue from the **NEXT** items in CURRENT STATUS.
> The harness panel is EPHEMERAL — THIS file + MEMORY + grand-synthesis are the
> durable record; the `#N` IDs are stable so reconstruction stays faithful.

---

## ⏯️ CURRENT STATUS (2026-06-08e — task list reorganized + the grand-synthesis roadmap; #62 mesh M1 slices 1–2 landed: the CRDT closure invariant + the first-class `mesh:` protocol)

> This file was reorganized (was ~2034 lines): the durable task list is now
> **Remaining work — by category** + a **✅ Completed** archive + a 1-line **session
> archive**; the roadmap to the ultimate demo (distributed streaming mesh
> biology+quantum+synth) moved to **[`grand-synthesis.md`](grand-synthesis.md)**
> (the one-engine thesis + M0→M4). Then dived into the mesh (#62 / M1), 2 slices,
> full workspace assumptions intact (prism-schema 118+, mesh+peer tests green):
>
> **Slice 1 — the CRDT law is now a CLOSURE INVARIANT.** `algebra::mesh_safety`
> (new op, `prism-schema/src/mesh.rs`, re-exported through the algebra surface): a
> structural recursion classifying whether a schema's `reconcile` is a
> join-SEMILATTICE (commutative + associative + IDEMPOTENT — CALM). Key-union
> (`Map`/`RecursiveTree`) + immutable (`Const`) safe; `Tree`/`Tuple` recurse per
> field; additive (`Float`/`Delta`/`Array`) + LWW (`Overwrite`/`Bool`/`String`/
> `Any`) + sequence (`List`) rejected with the fix in the message. The structural
> DUAL of `divide`'s extensivity; `Custom` delegates to its representation.
> Executable laws in `prism-schema/tests/crdt_laws.rs` — each verdict
> cross-checked against actual `apply` idempotence/commutativity. In
> `schema-algebra.md`'s op table.
>
> **Slice 2 — the first-class `mesh:` protocol.** `MeshProtocol` / `MeshReplica`
> (`prism-bigraph/src/protocols/mesh.rs`): `instantiate` GATES the link's
> value-schema through `mesh_safety` (a non-convergent link is REFUSED — illegal
> distributed states unrepresentable), and a replica MERGES a peer's δ via
> `algebra::apply_with` — the schema's own reconcile, the SAME boundary codec
> every protocol uses, NOT slice-1's hand-rolled key-union ([[boundary_codec_algebra]]).
> Registered in every chrysalis Core (`stream_protocols`). `prism-bigraph/tests/
> mesh_protocol.rs`: the gate rejects additive/LWW/missing-schema; two replicas
> converge with no coordinator; re-delivery is idempotent.
>
> **NEXT (continue M1):** (a) the δ-CRDT shared-link over a LIVE rest/stream bridge
> — two `mesh:` replicas converging across a socket (generalize `peer_shared_link.rs`
> to the protocol; the replica's `update` already speaks the δ-in/link-out port
> contract); (b) the **`link :: T mesh`** chrysalis surface (declare a mesh link in
> `.ys`, lower to a `mesh:` node, gate at compile); (c) continuous gossip /
> anti-entropy (vs one-shot). Then SWIM membership; N-peer; reactions / AlChemy /
> quantum over the mesh (≈ free given the uniform codec); transport over Tailscale.
> **See [`grand-synthesis.md`](grand-synthesis.md) M1.**

---

## 🎯 The ultimate demo

The north star: **one** continuously-running, mesh-distributed, streamed
simulation in which **biological** colonies, **quantum** subsystems, and audio
**synthesizers** all tick the same engine at once — "distributed streaming
mesh-network biological quantum synthesizers." The full motivation, the
one-engine thesis, the M0–M4 milestone path, and which tasks below feed each
milestone live in **[`docs/grand-synthesis.md`](grand-synthesis.md)**. In short:
the two *real* builds are **M1 = the mesh (#62)** and **M3 = the synth domain
(#63)**; the domains compose onto the mesh almost for free because every boundary
is the same schema-algebra codec.

---

## Remaining work — by category

### A. Distribution & Mesh — *the active front*
- **#62 NATIVE MESH** — the protocol abstraction generalized parent↔child →
  **peer↔peer** (`peer:`/`mesh:`). DONE: peer shared-link slice 1, deep P2P
  survey, executable CRDT law; **the CRDT law as a CLOSURE INVARIANT**
  (`algebra::mesh_safety` + `crdt_laws.rs` — rejects a non-semilattice merge);
  **the first-class `mesh:` protocol** (`MeshProtocol`/`MeshReplica`) — gates
  instantiation through `mesh_safety` and merges replicas via the schema `apply`
  (the CRDT join, not a hand-rolled union), registered in every chrysalis Core.
  **NEXT:** (a) the δ-CRDT shared-link over a LIVE rest/stream bridge (two `mesh:`
  replicas converging across a socket); (b) the **`link :: T mesh`** chrysalis
  surface (declare + gate at compile); (c) continuous gossip / anti-entropy; then
  SWIM membership; N-peer; reactions / AlChemy / quantum over the mesh. Transport
  over Tailscale. (= grand-synthesis M1.) [[mesh_as_protocol]]
- **#25 distributed phase 1** — batched `ray:` protocol (`flush_pending`, one
  packet/shard). Also the last open bit of #21. *Low-effort; proves the batching
  seam.*
- **#26 distributed phase 2** — halo / neighbor exchange + a 2-environment
  diffusion-across-a-boundary demo. *The neighbor↔neighbor gap the octree needs.*
- **#27 distributed phase 3** — static spatial-partition protocol (the octree;
  environment-of-environments across machines).
- **#28 distributed phase 4** — dynamic load balancing (split/migrate overflowing
  subdomains — the one exponential colony growth makes this non-optional).
- **#29 distributed phase 5** — adopt a cluster backend (Charm++ / Ray / MPI)
  behind the `Protocol` interface. *cluster = a protocol with a pluggable backend;
  do not rebuild a cluster runtime.*
- **#48 conformance suite (TCK)** for "process-bigraph server" — one suite, driven
  by prism's `RestProcess`, run against every backend (prism `rest_server`, our
  python sidecar, upstream FastAPI). All green ⇒ the Python↔Rust unification proof.
- **#49 promote sidecar wrappers to full Processes** — duck-typed today; the
  `Process` base unlocks `reconfigure` (loaded-model reuse), python-side
  composites, the shared `sbml` type registry + `/type-packages`,
  `discover_packages`.

### B. Biology
- **#8 SBML→CRN importer / repressilator** — *the original science goal.* Native
  subset SBML/MathML reader → a `CRN`; generalize the integrators to an arbitrary
  ODE RHS (an `OdeSystem` trait); repressilator BIOMD0000000012 is Hill kinetics.
  Reference at `../biocompose/`. *Large.*
- **#9 dt-refinement convergence sweep** — MSE → 0 as dt shrinks (divergence is
  pure discretization); a convergence plot/CSV. Also #44's remaining. *Small.*
- **#13 spatio-flux as a `.ys` project** — codegen MVP done; **REMAINING:** 12/18
  of `CANONICAL_ORDER` not yet ported (the rest of the dFBA family, comets
  variants, `spatioflux_reference_demo`).
- **#33 KISAO IDs + SED-ML/OMEX export** (contract rung 3) — slices (A) `kisao:`
  field on contracts; (B) `bigraph export --as sedml`; (C) `import-sedml` → match
  a prism integrator; (D) OMEX round-trip. A contracts-aware advisor over SED-ML.
  See `docs/kisao-export.md`.
- **#44 auto-fan-out over `fulfillers(C)`** — the three-notions-of-agreement demo
  shipped (real COPASI/Tellurium engines as contracted fulfillers); **REMAINING:**
  a comprehension over `fulfillers(C)` that GENERATES the `RunProcess` children
  ("run every fulfiller of C").

### C. Quantum
- **#35 algebraic effects + handlers** — *goal: quantum bigraphs through effects.*
  DONE: eval-time `handle` + quantum interference / Bell / measurement /
  engine-level / GHZ demos. **REMAINING:** slices 2–6 (wrap protocol/method/apply
  as effects; a `log` effect); slices 7–8 (engine-time: state-representation
  parameter on effects; categorical-constraint typing); slices 9–10 (a quantum
  handler bundle + a `measure` handler with state-collapse). `docs/effects-and-handlers.md`.
- **#36 quantum bigraphs** — DONE: Q1 independent subprocesses, Q2 LOCC classical
  wire, Q3 `tensor`, Q6 teleportation. **REMAINING:** Q4 — auto-merge on a
  cross-composite gate (compile-time form; runtime lifecycle form done); Q5 — a
  Divider step that turns a `factorize` observation into a structural `_divide`.
  `docs/quantum-bigraphs.md`.

### D. Synthesizers
- **#63 STREAMING SYNTHESIZERS** — the third domain (NOT STARTED). Build order
  **A1–A9** (`docs/synthesis-bigraphs.md`): offline `Signal` + module library
  (A1–A2) → **realtime device boundary = FIRST AUDIBLE MILESTONE (A3)** → control
  rate (A4) → reactions rewiring a live patch (A5) → homoiconic module factory
  (A6) → merge/split voices (A7) → `net:` distributed synths (A8) → the dream
  (A9). Offline-first; gorgon → library. Independent of the mesh until A8 — a good
  parallel front. (= grand-synthesis M3.) [[synthesizer_project]]

### E. Unification & Fundamentals — *the generative core*
- **#59 the generative-core unification PROGRAM** (umbrella, `docs/generative-core.md`)
  — reduce to the essential core, ONE way to do each thing. DONE: the Core-threading
  RULE; the boundary codec unified through the algebra; the 4-registry conformance
  test. **REMAINING facets:** survey duplicate paths across prism+chrysalis;
  one-door BUILD-FAILING guards (non-duplication as an automatable invariant);
  confluence / normal-forms (the algebra as canonicalizer); core + desugaring
  (Felleisen conservative extensions); unify method definition (composites get a
  `with { methods }` block too).
- **#30 unified entity registry** — "a type is a control with extras." DONE:
  slices 1A/1B/2/2b (reactions & bare controls as values; `EntityView`; owning
  `EntityDef` + AST-as-value). **REMAINING:** Slice 3 — methods on sited cells +
  `?c.divide()` (`?c :: Cell` binds a typed value; method dispatch on it; unblocks
  `grow-divide-glucose.ys`'s method-call reactum); Slice 4 — a `control Foo`
  declarative form (typo detection, declared ports). *Pre-req for tier-2 M/R.*
- **#15 schema-as-state: a meta-schema** — make the schema itself first-class
  operable STATE governed by a meta-schema; mutating the schema **generates/fills
  the state to match** (new field → its `default`, retyped → `coerce`),
  transactionally, via the algebra. Paired with #30. *Deep/foundational; a new
  named algebra op + laws, not ad-hoc munging.*
- **#5 law generators over node sorts** — complete the property-test generators
  over node sorts + fix the flaky law + uniform node-data.
- **#46 audit for unification opportunities** — first pass done (`infer_and_merge`
  / `overlay_apply_types` / `ChrysalisBrs` gone). **REMAINING:** verify schema↔string
  (`render`/`parse_type_expression`), `trace_of`, `divide_by_schema`,
  `MethodRegistry`; spin off a task per confirmed multiplicity.
- **#61 AlChemy cleanup** — built end-to-end (local → shared-link → outer-link →
  distributed; #61a link schema first-class + #61b real transport over a live rest
  bridge). Only a small cleanup tail remains.
- **#34 stream over a hand-built Cell** — the mechanism is COMPLETE (slices A–E:
  `from_value`, `compile_value`, the AST-builder `.ys` library, and the hand-built
  `Cell` that conserves mass identically to a parsed one). **REMAINING:** run the
  streaming env over the hand-built Cell (same `_program` mechanism, same protocol
  layer — "exercise the mechanism more").
- **#32 programs-as-data** — DONE: `load()`, `Document.run`, `eval(ast)`,
  Expr↔Value round-trip. **REMAINING:** (a) a `Document.run` variant returning only
  the OUTPUT FACE; (b) the bin's entry-rule for a trailing scalar/method-chain
  expression.
- **#6 explicit bridge conduits** — make conduits explicit (today inferred by
  inner-slot absence). *Cleanup.*

### F. Surface language & tooling — *chrysalis DX*
- **#10 comment-preserving parse/unparse** — extern fully retired; **REMAINING:**
  retain comments through `parse → unparse` (~190 comment lines) so every `.ys`
  regenerates losslessly. Then `chrysalis format -w` can run on commented files.
- **#11 prism-svg** — typed SVG nodes done (`prism_viz::plot` emits typed SVG);
  **REMAINING:** retire `render_timeseries_svg`'s plotters-string path.
- **#14 chrysalis diagnostics** — a first-class diagnostics subsystem so every way
  a `.ys` program can fail gives a clear, located, actionable, *educational* error
  (rustc/Elm-quality): a failure-mode taxonomy + a structured diagnostic threaded
  parse→eval→compile→check→run + a regression suite of deliberately-wrong `.ys`.
  *Medium-large; high DX value.*
- **#16 surface-language defaults** — input defaults + harness + dynamic-τ done;
  **REMAINING:** an inner-wire-typed `interval` whose `default` is `%.port`,
  retiring the engine's `[name,"interval"]` side-channel (so a process can wire its
  interval to a shared clock).
- **#17 enforce `::`=type / `:`=value / `fulfills`-contract** — lenient phase live;
  **REMAINING:** migrate every `.ys` + GUIDE/design to canonical `::`/`fulfills`,
  then tighten the parser.
- **#18 file-as-composite streaming + type-driven outputs** — Trace kernel,
  delta-log/Arrow codec, `--serve-stream`, mandatory schema-header `refines` check,
  `plot(schema, trace)` all DONE. **REMAINING:** `--map out=in` + `--adapt
  adapter.ys`; the 4D structural plot (bigraph-viz per `_add`/`_remove`); apply down
  `CANONICAL_ORDER` validating each family. *(Streaming kernel done; this is the
  tooling/viz polish.)*

### G. Performance — *do LAST, after the feature set*
- **#19 performance sweep** — establish benchmarks; profile the hot paths (eval per
  tick — consider compiling bodies vs re-walking the AST; the engine step loop;
  algebra apply/diff/reconcile + `Value` cloning; the delta-log/Arrow codec +
  streaming; discovery); optimize **without** sacrificing the principled design
  (closed algebra, no half-measures; check once, erase, run raw). Gated on features.

---

## ✅ Completed (provenance; full detail in git + memories)

**Distribution / streaming / protocol.** #1 standalone cell.ys + stream
conservation test · #2 serve_stream/serve_process delta-forwarder · #3 grow-divide
over stream == rest (boundary proof) · #4 real parallelism (Defer seam,
ParallelPool, stream-concurrent) · #20 parallel stream cells from env.ys · #21
rest-concurrent + `flush_pending` (batched ray = open #25) · #45 unify process
instantiation through the protocol-aware Core (`Core::instantiate`;
local/rest/parallel/stream drive identically) · #47 unify chrysalis node-spec
(flat envelope everywhere) · #50 unify the import forms (ONE `from dotted import
Name`; explicit named selection, std-module-first) · **#40** cross-composite redex
syntax resolved as a LINK-GRAPH op (`?west ~{edge:~e} | ?east ~{edge:~e}`) ·
**#43** cross-composite reactor (place-graph delta reactor + link-graph matcher +
`|` unification + in-place modify + DISTRIBUTED over a live rest bridge + LIVE via
the bridge) · **S1** fold/unfurl as schema-algebra ops · **S2** BRS-by-unfurl
(`fire_across_composites`).

**Biology.** #12 real contract enforcement through `RunProcess` (a contract
survives a generic wrapper) · #44 the three-notions-of-agreement demo + REAL
engines (COPASI/Tellurium) as contracted fulfillers + agreement-across-engines +
Schlögl bistable showcase (auto-fan-out = open) · #52 rate-as-expression
(`reaction … ) rate ( expr )`) · #53 `pattern` definer + MAPK de-dup (5 patterns
replace 14 blocks) · #57 reaction-driven division, CONSERVING (`?c.divide()`;
`divide_by_schema` late; mass conserved across the dividing tick).

**Quantum.** #35 algebraic effects — eval-time `handle` + interference / Bell /
measurement / engine-level / GHZ demos (slices 2–10 = open) · #36 quantum bigraphs
Q1 (independent) / Q2 (LOCC) / Q3 (tensor) / Q6 (teleportation) (Q4/Q5 = open) ·
#39 `tensor_by_schema` (schema-driven dual of `divide_by_schema`; Qubits
cross-product) · #42 `Bigraph` port type (reactions as typed updates that fire
inside via the algebra).

**Unification & fundamentals.** #7 schema-first discovery (`scan_for_processes`;
address-fallback expunged) · #22 protocols as Custom types + composite IS-A type ·
#24 retire `branch_schema` (inner schema via the algebra) · #54 subtractions
(reactum-by-structure; records/maps unified) · #55 brand/subtype type system
(Cardelli F₁&lt;:; `_type` brand; `is_a` over `inherits`) · #56 link surface
(first-class `link :: T = default`; `resolve_link` depth-independent — one
primitive for pool/entanglement/halo) · #58 `apply_fire` unified onto the algebra
(BRS returns reconciled deltas; engine applies; Core threaded) · #60
reactions-vs-`_add`/`_remove` resolved (sentinels = the delta vocabulary;
reactions = the dynamical generator; rules-as-state proven) · #61a/#61b AlChemy
(link schema + real transport) · #30 entity-registry slices 1A/1B/2/2b
(reactions/bare-controls as values; `EntityView`; `EntityDef` + AST-as-value) ·
#34 hand-built Cell slices A–E (`from_value`/`compile_value`/AST-builder lib/the
conserving Cell) · #32 programs-as-data (`load`/`Document.run`/`eval(ast)`/Expr↔Value
round-trip) · #59 the Core-threading RULE + boundary-codec-through-the-algebra +
the 4-registry conformance test.

**Tooling.** #23 modernize `.ys` (width-aware unparser; `chrysalis format`) · #31
`chrysalis new` scaffolder · #51 executable primer + doctest net
(`chrysalis-primer.md` pinned by tests) · the chrysalis CLI toolchain — `bigraph
export/import`, `server`, `repl` (all shipped earlier).

---

## 🗂️ Session archive (1 line per session; full prompts in git history)

- **2026-06-08d** — #43 cross-composite reactor COMPLETE; NATIVE-MESH keystone
  started; deep P2P survey grounds the design.
- **2026-06-08c** — #43 link-graph matcher + FIRING + `|` parser unification +
  reaction-firing KEYING unified to one `localize_fire`.
- **2026-06-08b** — #43 first slice: the delta-returning cross-composite reactor
  (the crux) + engine per-tick reactor + auto-detect.
- **2026-06-08** — Core threaded through the protocol layer + the all-four-registries
  conformance test + the method matrix made consistent.
- **2026-06-07d** — boundary codec UNIFIED through the algebra.
- **2026-06-07c** — #61a link schema first-class.
- **2026-06-07b** — AlChemy arc COMPLETE through distributed.
- **2026-06-07** — #60 resolved: reaction/delta basis + rules-as-state.
- **2026-06-06d** — Core-threading RULE formalized + applied.
- **2026-06-06c** — link surface + conserving division + algebra unification.
- **2026-06-06b** — wei-qi program: executable primer + dir-1/2 subtractions.
- **2026-06-06** — BATWD fold/unfurl complete; S2 BRS-by-unfurl shipped.
- **2026-05-28** — symmetric bridge apply + quantum lifecycle on real composites.
- **2026-05-27** — quantum bigraphs Q2/Q3/Q5/Q6; #36 nearly complete.
- **2026-05-25** — schema-first discovery + canonical-list audit.
- **2026-05-24 (parts 2–6)** — division proven over rest → stream → in parallel;
  bridge unified; rest-concurrency + the defaults/harness fix; distributed plan
  drafted; the spatio-flux-report-as-`.ys` + delta-motion + `.ys`-modules arc.
- **2026-05-22** — post runtime + toolchain session (codegen path, REPL, server).

---

## Long-run milestones (beyond the tracker)
- **Distributed execution at scale** (`docs/distributed-execution.md`) — giant
  colonies growing exponentially across machines. The fractal/octree vision = SOTA
  HPC (domain decomposition + halo exchange + Barnes-Hut/FMM aggregation + adaptive
  octrees). prism's composite-of-composites / protocol boundary / bridge /
  encapsulation / BSP tick already encode the skeleton; gaps = #25–#29. The cluster
  is a **protocol with a pluggable backend** (Charm++ / Ray / MPI) — adopt, don't
  rebuild. *(Now the spine of grand-synthesis M1/M2; see `grand-synthesis.md`.)*
- **First-class `Custom` types** — `type Name = <repr> with { op = …, method(args)
  = … }` (the algebraic-effects handler model): make a `Custom` indistinguishable
  from a built-in sort by threading a registry through the algebra ops. (Ties to
  #59's "unify method definition" facet.)
- **Tier-2: evolving M/R** — process bodies as first-class values (after Fontana's
  AlChemy); schema-driven typed construction so illegal programs are
  unrepresentable. (Pre-req: #30 entity registry + #15 schema-as-state.)
- **A packages ecosystem** — #13 makes spatio-flux the first non-std `.ys` package;
  generalize so any native crate ships importable `.ys` modules (a `project.ys`
  manifest + a resolver/registry) + a fundamental-type catalog.
- **dFBA cross-target bridge** — comparing a kinetic integrator vs dFBA needs an
  explicit, *named* cross-target contract (name the approximation, don't silently
  MSE incomparable things).

---

## Build / test loop
Build is slow (links many binaries). Use `cargo check -p <crate>` for "does it
build", `cargo test -p <crate> --test <name>` for the touched area, full
`cargo test --workspace` only at checkpoints. Run in the background and act on the
completion notification — a slow run is a BUG (infinite loop / structural
explosion), not patience ([[feedback_run_and_notify]]). Detect failures with
`grep -vE '0 failed;'` (empty = pass), NEVER `grep -iE 'FAILED'` (matches "0
failed" on passing lines — [[feedback_clean_test_check]]). No `.cargo/config.toml`
linker override (rustflags change ⇒ whole-tree rebuild).

## Pointers
- **[`docs/grand-synthesis.md`](grand-synthesis.md)** — the roadmap to the ultimate
  demo (read with this file).
- `README.md` — the port + chrysalis + commands.
- `crates/chrysalis/ys/GUIDE.md` — how to write `.ys`; `docs/chrysalis-primer.md` —
  the executable fluency reference.
- `docs/prism-architecture.md`, `docs/schema-algebra.md`,
  `docs/chrysalis-design.md`, `docs/process-contracts.md`,
  `docs/execution-model.md`, `docs/distributed-execution.md`.
- Memories: `feedback_ys_layering`, `feedback_chrysalis_thin_layer`,
  `feedback_no_half_measures`, `feedback_schema_algebra`, `mesh_as_protocol`,
  `boundary_codec_algebra`, `synthesizer_project`.
- A side-quest survey of the broader landscape (reflective towers, Futamura,
  staging, algebraic effects, probabilistic / differentiable / reversible /
  quantum / unconventional computing):
  `docs/exploring-the-computational-unknown.md`.

## Historical (done — provenance in git)
- The fresh-core **schema-algebra rebuild** — closure achieved; `prism_schema::
  algebra` is the single door; 13 laws + closure-guard green.
- The **engine execution-correctness arc** — invoke/apply separation, `reconcile`
  wired into apply, dependency-layered step firing, schema always inferred.
- The **process-contract demo**, the **codegen path** (`chrysalis run` on non-std
  packages), and the **CLI/REPL toolchain** — all built and running.
- The older **task tracker** (a parallel #1–#26 numbering from the 2026-05-22/24
  era) has been folded into the canonical `#N` scheme above and retired; its prose
  is in git history.
