# Next session — launch prompt & plan

## ⏯️ ON BOOT — read this first

> Resume prism (Rust process-bigraphs + the `.ys` language). Read this prompt +
> the **CURRENT STATUS** block below + [`docs/grand-synthesis.md`](grand-synthesis.md)
> (the roadmap to the ultimate demo) + [`docs/categorical-core.md`](categorical-core.md)
> (the spine: domains = theories, couplings = functors, M0–M5) + the `MEMORY.md`
> index. Then **rebuild the
> harness task list** from "## Remaining work — by category": `TaskCreate` one task
> per `#N` entry (all are pending / in-progress; the `## ✅ Completed` section is
> provenance, not panel tasks). Continue from the **NEXT** items in CURRENT STATUS.
> The harness panel is EPHEMERAL — THIS file + MEMORY + grand-synthesis are the
> durable record; the `#N` IDs are stable so reconstruction stays faithful.
>
> **Multi-agent runs:** this doc is the single-agent boot. For a PARALLEL run, the
> front door is [`coord/unify.next`](../coord/unify.next) (the overall / cross-agent
> index) + [`coord/ROSTER.md`](../coord/ROSTER.md) (roles + protocol). Each agent boots
> from its own `coord/<agent>.next` (durable resume) and lives in `coord/<agent>.ys`
> (the gossiped board). This file remains the deep durable record they navigate into.

---

## ⏯️ CURRENT STATUS (2026-06-08e — task list reorganized + grand-synthesis roadmap; #62 mesh M1 slices 1–6 landed: closure invariant + `mesh:` protocol + live-bridge convergence + `.ys` surface w/ compile gate + the RUNTIME, distributed over the live bridge)

> This file was reorganized (was ~2034 lines): the durable task list is now
> **Remaining work — by category** + a **✅ Completed** archive + a 1-line **session
> archive**; the roadmap to the ultimate demo (distributed streaming mesh
> biology+quantum+synth) moved to **[`grand-synthesis.md`](grand-synthesis.md)**
> (the one-engine thesis + M0→M4). Then dived into the mesh (#62 / M1), 3 slices,
> regression-clean (prism-schema 119+, prism-bigraph + chrysalis green):
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
> (`prism-bigraph/src/protocols/mesh.rs`): the GATE moved into `MeshReplica::new` /
> `::shared` (the only constructors — `mesh_safety` is checked there, so an ungated
> divergent replica cannot exist), and a replica MERGES a peer's contribution STATE
> via `algebra::merge` — the schema's own state-based CRDT join, the SAME algebra
> every boundary uses, NOT slice-1's hand-rolled union ([[boundary_codec_algebra]]).
> Registered in every chrysalis Core (`stream_protocols`). `mesh_protocol.rs`: the
> gate rejects additive/LWW/missing-schema; two replicas converge; idempotent.
>
> **Slice 3 — the live bridge.** Two `mesh:` replicas, each hosted at a rest
> server, exchange contribution STATES over a LIVE HTTP bridge and CONVERGE with no
> coordinator — the first-class generalization of `peer_shared_link.rs`
> (`prism-bigraph/tests/mesh_live_bridge.rs`). `merge` is state-based, so the
> existing boundary codec carries the state (`realize_with`) — no delta-in (#5)
> needed. Deployable by pointing each peer at the other's Tailscale IP.
>
> **Slice 4 — the `.ys` mesh surface + COMPILE-TIME gate.** `link name :: T mesh =
> default` declares a replicated link in chrysalis; `validate_connections` (run in
> `compile.rs`) gates it through `algebra::mesh_safety` — an additive / LWW / untyped
> mesh link is a COMPILE ERROR (the M1 checkpoint: the algebra refuses an unsafe merge
> at compile time). The `mesh` modifier threads parse→AST→eval→unparse (round-trips);
> the `_links` marker records `"mesh"` for the replication wiring (resolve_link reads
> only presence, so it's backward-compatible). A `mesh` link runs identically to a #56
> shared link locally today. Full chrysalis suite green.
> `chrysalis/tests/mesh_link_surface.rs`.
>
> **Slice 5 — the runtime: a `.ys` `mesh` link REPLICATES across engines.** Two
> runtime primitives: `MeshReplica::gossip(peer)` — one push-pull round (push our
> replica to the peer's `contribution`, pull theirs back, `merge`), converging BOTH
> in one round, idempotent (driven EXPLICITLY, never in `update` — a peer's receive
> path *is* its `update`, so in-`update` gossip re-enters across the bridge); and
> `mesh_links(state)` — the reflection scanning `_links` for the `"mesh"` marker.
> Composed: a mesh runtime scans a running engine for mesh links, attaches a gated
> `MeshReplica` to each slot, gossips, and writes the converged value back through
> `Engine::state_mut`. `chrysalis/tests/mesh_link_runtime.rs`: two `.ys` engines
> declaring `link shared :: map[float] mesh` (different per-source keys) CONVERGE to
> the union, no coordinator — the surface→runtime loop closed. (Transport in-process
> here; over a LIVE rest bridge it's `mesh_live_bridge.rs` + its new
> `one_gossip_round_converges_both_peers` test — composing the two is the distributed
> mesh.)
>
> **Slice 6 — the DISTRIBUTED `.ys` mesh (over the live bridge).** Composed the two
> transports: each peer HOSTS its mesh slot as a `MeshReplica` at a rest server,
> gossips over a REAL socket (`gossip` push-pull), and the converged value is written
> back into each `.ys` engine. `chrysalis/tests/mesh_link_distributed.rs`: two
> `.ys`-declared mesh links CONVERGE over the live bridge — no new mechanism, just the
> `.ys` runtime (slice 5) + the rest transport (`mesh_live_bridge.rs`). The mesh is an
> ADDRESS: the `.ys` is byte-identical to the in-process run. Deployable by pointing
> each peer at the other's Tailscale IP.
>
> **NEXT (continue M1):** (a) continuous gossip / anti-entropy as an engine-driven
> step (vs the explicit round) + a long-lived per-engine mesh agent (host once, gossip
> per interval); (b) SWIM membership + discovery; (c) N-peer; then **Demo 2** (Kuramoto
> tiles phase-locking over a `mesh:` link — `categorical-core.md` §9, the CRDT+dynamical
> convergence *stack*) and reactions / AlChemy / quantum over the mesh; the δ-state CRDT
> (#5). **See [`grand-synthesis.md`](grand-synthesis.md) M1.**

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
  survey; **the CRDT law as a CLOSURE INVARIANT** (`algebra::mesh_safety` +
  `crdt_laws.rs` — rejects a non-semilattice merge); **the first-class `mesh:`
  protocol** (`MeshProtocol`/`MeshReplica`) — gated at construction, merges replicas
  via the schema `merge` (state-based CRDT join), registered in every chrysalis
  Core; **live-bridge convergence** (two `mesh:` replicas converge over a LIVE rest
  bridge, no coordinator — `mesh_live_bridge.rs`); **the `.ys` mesh surface +
  compile-time gate** (`link name :: T mesh = d`, gated by `mesh_safety` in
  `validate_connections` — an additive/LWW/untyped replicated link is a COMPILE
  error; `mesh_link_surface.rs`); **the RUNTIME** — `MeshReplica::gossip` (push-pull
  CRDT round) + `mesh_links` reflection compose into a mesh sync that converges a
  `.ys`-declared mesh link across engines, IN-PROCESS (`mesh_link_runtime.rs`) AND
  **DISTRIBUTED over a LIVE rest bridge** (`mesh_link_distributed.rs` — each peer
  hosts its slot as a rest `MeshReplica`, gossips over a real socket, converged value
  written back); **N-PEER** (`MeshAgent` host-once/gossip-per-round; `mesh_npeer.rs`);
  **CONTINUOUS gossip / anti-entropy** (`MeshAgent::start_gossip` background loop +
  `::contribute` — the mesh self-converges + propagates live updates; `mesh_continuous.rs`);
  and a **live coordination form** (`chrysalis coord` serve/push/pull — the agents
  building the mesh coordinate over it). **NEXT:** (a) SWIM membership + discovery (peers
  auto-find, no hardcoded ports); (b) **Demo 2** (Kuramoto tiles phase-lock over a `mesh:`
  link — manifold is building it on this mesh; the carrier is a per-source
  `map[id → array[[2]]] mesh`; `categorical-core.md` §9 — the CRDT+dynamical stack) +
  reactions / AlChemy / quantum over the mesh; the δ-state CRDT (#5); **transport
  generalized** — address+auth = capability refs,
  transport pluggable (QUIC/Noise/relay), **Tailscale one backend, not a dependency**
  (`categorical-core.md`
  §4/§9). (= grand-synthesis M1.) [[mesh_as_protocol]]
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
- **#64 THE CATEGORICAL CORE** (umbrella, `docs/categorical-core.md`) — name the
  prop/SMT spine the other docs each touch a facet of (a `.ys` module = a presented
  theory; `|`=⊗; `fold`/`unfurl`=the compact cup/cap=M/R-closure=reflection). Mostly
  RECOGNITION. Two weight-pulling additions (each *deletes* a special case, Felleisen-
  gated): (a) a first-class **`functor`** definer — unify unit-conversion + the
  protocol codec + `compile` + cross-domain bridges (today bespoke glue); (b)
  **`quote`/`eval`** first-class — `eval`=`discover_processes` promoted, `quote`=
  `Expr::to_value`; dissolves `compile_reaction`, collapses the meta/object
  (definer/constructor) duplication, makes the homoiconic round-trip TOTAL. Plus a
  shared **`Complex`/`Signal[ℂ]`** base sort (quantum=unitary, manifold=dissipative —
  one ℂ-linear dagger category; Selinger CPM). Companion to #59; threads M0–M5.
  [[boundary_codec_algebra]] [[generative_core_doc]]
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
- **#68 fundamental-type catalog** — a first-class catalog of base sorts / a shared
  type vocabulary (incl. the `Complex` / `Signal[ℂ]` sort, `categorical-core.md` §6,
  unifying quantum amplitudes / manifold phase / synth audio). *Surfaced by the
  manifold domain (#66); see `categorical-core.md` §6/§6b — manifold agent to detail.*
- **#69 Vec / array schema-algebra** — complete the algebra (`apply`/`merge`/`divide`/
  `tensor`) for `Array` / vector sorts (the Kuramoto mean-field is `array[[2], float]`;
  the additive-array reduction is the mean-field fold). *Surfaced by #66; manifold
  agent to detail.*
- **#71 units as a schema refinement** (`docs/units-in-the-schema.md`) — units are
  **compile-time-checked then fully erased** today (`Quantity[…,extensive]` → `Delta`/
  `Float`, dimension dropped; *"units never reach the engine"*), which is exactly why a
  `type Mass` can't serialize — **the units half of the Axis-A gap**. Fix: decouple the
  two erasures — keep raw `f64` in the **value** (zero-cost hot path) but carry the
  `Dimension` in the **schema** (an `Option<Dimension>` on `Float`/`Delta`, or a
  `Quantity` sort), so `check` does dimension-mismatch wiring, the boundary codec does
  gram→kg conversion (subsuming `thread_factor_inputs`), and `serialize` round-trips a
  units `type`. Consulted at check/wire/boundary/serialize, *not* per-op. The first
  base-sort refinement of #68; touches core (schema + algebra threading + units serde,
  the gated `Ratio`-serde add) + chrysalis (`lower_schema` keeps the dimension).
  *Surfaced 2026-06-09 with the human; design note written.* [[feedback_zero_cost]]
  [[project_units_cross_boundary]]

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
- **#70 composite intervals** — a composite's inner processes should tick at a
  controllable interval; today a nested composite under-integrates at the default
  rate (the Kuramoto Demo-1 symptom). Generalize #16's overridable-`interval`-default
  wire to the composite boundary (a shared clock driving an inner sub-engine).
  *Surfaced by #66; relates to #16; manifold agent to detail.*
- **#17 enforce `::`=type / `:`=value / `fulfills`-contract** — lenient phase live;
  **REMAINING:** migrate every `.ys` + GUIDE/design to canonical `::`/`fulfills`,
  then tighten the parser.
- **#18 file-as-composite streaming + type-driven outputs** — Trace kernel,
  delta-log/Arrow codec, `--serve-stream`, mandatory schema-header `refines` check,
  `plot(schema, trace)` all DONE. **REMAINING:** `--map out=in` + `--adapt
  adapter.ys`; the 4D structural plot (bigraph-viz per `_add`/`_remove`); apply down
  `CANONICAL_ORDER` validating each family. *(Streaming kernel done; this is the
  tooling/viz polish.)*
- **#67 the dependability trio** (theoretical→dependable DX) — (a) **trace-as-
  time-travel debugger**: the delta-log already records every tick (`state(t)=
  fold(apply,initial,deltas[..t])`), so step / inspect-at-t / breakpoint-on-fire is a
  UI over data we already emit — *near-free, high value*; (b) **chrysalis LSP** over
  the REPL's type/env machinery (hover / go-to-def / completion); (c) a **package
  registry + lockfile** — a registry *of theories* (semver, `chrysalis add`),
  generalizing the `project.ys` + `from … import …` resolver. **✅ DONE (`pkg`, 2026-06-10→12):
  the FULL ecosystem** — manifest-as-data, linking = `Core::colimit`, resolver-as-confluence,
  semver, `project.lock`, the registry (LOCAL dir **+ REMOTE HTTP** via mesh's transport),
  `chrysalis add`/`remove`/`install`/`update`/`publish`, native deps; a remote package links
  identically to a local one (M4 enabler). See `docs/packages-ecosystem.md` + `coord/pkg.next`.
  *Tails:* checksum-in-lock, workspaces, the first published real package. **(a) trace
  debugger · (b) chrysalis LSP remain.** `chrysalis new` already shipped (#31).

### G. Performance — *do LAST, after the feature set*
- **#19 performance sweep** — establish benchmarks; profile the hot paths (eval per
  tick — consider compiling bodies vs re-walking the AST; the engine step loop;
  algebra apply/diff/reconcile + `Value` cloning; the delta-log/Arrow codec +
  streaming; discovery); optimize **without** sacrificing the principled design
  (closed algebra, no half-measures; check once, erase, run raw). Gated on features.

### H. Spatial computing & visualization — *the eye (M5)*
- **#65 SPATIAL DOMAIN** (the 4th specialization, `../parsimony` — a mature ~17.6k-LOC
  cellPACK rewrite). FIT: its `Compartment`=place graph, `Op`-batch (Insert/Remove/
  Replace)=the `_add`/`_remove` delta vocabulary, pipeline-cache=`StepCache`, and
  **`VoxelField`/QBVH = the #27 octree, already built**. Fold-in: (a) the type catalog
  NOW (`Snapshot↔Value` via `Schema::Custom`, `Space`/`Transform`/`Mesh`/`Field`, a
  `Render` method = a functor to the geometry prop, #64); (b) 3D/4D rendering of the
  whole organism + the bigraph topology itself (retires the graphviz viz; folds in
  #11/#18's 4D structural plot); (c) the deep packing-as-BRS adapter AFTER the M1 API
  settles (parsimony's own plan). nD is natural (the `Array` schema). (= grand-synth
  M5.) See `categorical-core.md` §9.

### I. Adaptive networks — *the coupling layer (M5)*
- **#66 MANIFOLD DOMAIN** (the 5th specialization, `../manifold` — ~8.5k-LOC adaptive
  dynamical networks on a complex substrate: real=amplitude/Hebbian, imag=phase/
  Kuramoto/STDP; its two-phase update IS the BSP tick). FIT (recognition, no new
  engine): node=a `process`, synapse=a first-class `link` ([[link_surface]]), plastic
  weight=a link-schema `reconcile`/`apply` of a Δ ([[reaction_delta_basis]]), topology
  learning=a topology-rewriting BRS (#43) — **the network learns its own topology = the
  M/R closure made dynamical**. Shares the `Complex` sort with quantum (#64). Demos:
  **Demo 1** Kuramoto tile as a composite syncing via one `map[id→ℂ]` link (`R→1`,
  **runs today**); **Demo 2** two tiles phase-lock over a `mesh:` link (M1); **Demo 3**
  plastic topology = a self-rewriting BRS (#43, local-first — the closure payoff). The
  natural coupling layer that ties the domains into one *learning* organism. (=
  grand-synth M5.)

---

## ✅ Completed (provenance; full detail in git + memories)

**Distribution / streaming / protocol.** #1 standalone cell.ys + stream
conservation test · #2 serve_stream/serve_process delta-forwarder · #3 grow-divide
over stream == rest (boundary proof) · #4 real parallelism (Defer seam,
ParallelPool, stream-concurrent) · #20 parallel stream cells from env.ys · #21
rest-concurrent + `flush_pending` (batched ray = open #25) · #45 unify process
instantiation through the protocol-aware Core (`Core::instantiate`;
local/rest/parallel/stream drive identically) · #47 unify chrysalis node-spec
(flat envelope everywhere) · #50 unify the import forms (ONE `from … import
Name`; explicit named selection — later flipped to EXPLICIT-ORIGIN: bare = native/
registry, leading-dot `.name` = relative file; std-module-first/#50 precedence retired,
see `project_ys_import_explicit_origin`) · **#40** cross-composite redex
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
- `docs/chrysalis-primer.md` — how to write `.ys`: the executable, canonical
  fluency reference (every block run by a test). `docs/README.md` — the doc index.
- **[`docs/categorical-core.md`](categorical-core.md)** — the spine (domains =
  theories, couplings = functors, `fold`/`unfurl` = cup/cap = M/R-closure =
  reflection, convergence = the fixpoint; adds spatial `../parsimony` + manifold
  `../manifold` as M5). Read with grand-synthesis.
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
