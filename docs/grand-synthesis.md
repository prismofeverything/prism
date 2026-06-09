# The grand synthesis — one ultimate demo

> **North star.** A single, continuously-running simulation, distributed across
> a **mesh** of machines and **streamed** in real time, in which **biological**
> colonies, **quantum** subsystems, and audio **synthesizers** all tick the same
> engine at once — coupled through shared links, rewired live by reactions, and
> converging with no coordinator. *"Distributed streaming mesh-network biological
> quantum synthesizers."*

This doc is the **roadmap** to that demo: why it is already mostly reachable, the
milestone path (M0–M4), and exactly which open tasks feed each milestone. The
durable task list + status live in [`NEXT-SESSION.md`](NEXT-SESSION.md); this is
the *why* and the *order*.

---

## 1. Why this is (mostly) already possible — the one-engine thesis

prism is **one engine**. The three application domains are not three systems —
they are **three specializations of the same substrate**, each a witness that the
core is general:

| Aspect | Biology | Quantum | Synthesis |
|---|---|---|---|
| **node** (place graph) | cell | qubit | module (osc / filter / VCA) |
| **link** (link graph) | diffusion / signalling | entanglement | patch cable / CV bus |
| **couple** (`tensor`) | cells merge; diffuse across a boundary | states merge; qubits entangle | voices merge; share a mod bus |
| **split** (`divide` / `unfurl`) | cell division (mass factorizes) | measurement (separable states factorize) | voices decouple when a cable is cut |
| **distribute** (protocol boundary) | environments on different machines | separable qubits in different composites | voices on different machines |
| **generate** (BRS) | growth / division rules | gate sequences; entangle/decouple | patch rewiring; module insertion |

Every column runs on: the schema algebra (the **codec at every boundary**), the
composite/link bigraph, the BSP tick, and **one BRS** that rewrites topology at
every level (inside a composite, between composites, across the mesh). `fold` /
`unfurl` and `tensor` / `divide` are the object↔morphism and couple↔split
maneuvers — *identical* in `quantum-bigraphs.md` §VII and `synthesis-bigraphs.md`
§VIII. This is the payoff of the unification work: **the demo is a composition of
specializations, not a new mechanism.**

**What this buys us.** Because every protocol boundary is the same
schema-algebra codec ([[boundary_codec_algebra]]), anything that works *locally*
works *over a wire* unchanged — already proven for the cross-composite reactor
(local → rest → live bridge, #43), AlChemy (local → shared-link → distributed,
#61), and quantum (Bell/GHZ/teleportation on the engine, #36). So once the
**mesh is an address** (#62), the domains ride it for free — modulo each domain's
merge being **CRDT-safe** (the one real correctness condition; see M1).

**What is genuinely new** (not free): the **mesh protocol** itself (M1) and the
**synthesizer domain** (M3 — a `Signal` type, an audio device boundary, the
gorgon library). Everything else is "exercise the substrate harder."

### 1b. The spine — the categorical core (and two more specializations)

Why each column is a *specialization* rather than a separate system has a name,
stated in [`categorical-core.md`](categorical-core.md): **every domain is a
presentation (generators + equations) of one symmetric monoidal theory; every
cross-domain coupling and protocol boundary is a functor; the closed/compact
structure gives reflection, entanglement, and self-production as one thing.** It is
the **spine** — mostly *recognition* (`|`=⊗, `~{}->{}`=a morphism, `fold`/`unfurl`=
the compact cup/cap, the schema algebra=a Lawvere theory, a `.ys` module=a presented
theory) — not a milestone; it threads through all of M0–M4 and makes the demo *more
coherent*, not bigger. The spine reveals **two further specializations** of the same
substrate (table columns 4 and 5):

- **Spatial (`../parsimony`)** — node = placement/compartment; link = adjacency;
  couple = pack; split = partition; distribute = **the octree (parsimony's
  `VoxelField`/QBVH *is* #27, already built)**; generate = a BRS over geometry. It
  contributes the **eye**: 3D/4D rendering of the whole organism (render = a functor
  to the geometry prop).
- **Adaptive networks (`../manifold`)** — node = oscillator/adaptive unit; link =
  a (plastic) synapse; couple = Kuramoto phase-coupling; split = decouple; generate
  = Hebbian/STDP firing **topology reactions — the network learns its own topology,
  the M/R closure made dynamical.** Plastic weight = a link-schema `reconcile`;
  two-phase update = the BSP tick. The natural **coupling layer** binding the domains
  into one learning organism.

---

## 2. The milestone path

### M0 — The substrate (DONE) ✅

The "everything is ready" baseline already shipped:

- **One engine, one codec.** Schema algebra closed; `apply`/`diff`/`reconcile`/
  `resolve`/`merge`/`tensor`/`divide` faithful; the codec is the algebra at every
  boundary; the Core threaded through the protocol layer (no registry subsets).
- **Topology-rewriting BRS across composites** — local, distributed over a live
  rest bridge, and on live running composites via the bridge (#43).
- **Quantum** — Bell / GHZ / teleportation run on the engine (#35/#36).
- **AlChemy** — reactions that generate reactions, local → distributed (#61).
- **Peer shared-link, slice 1** — a hyperedge replicated across two peer engines,
  no coordinator, converging over a live bridge (#62). **CRDT law** executable
  (`crdt_laws.rs`). **Deep P2P/mesh survey** done — the design is grounded on
  proven work (CALM, δ-CRDTs, SWIM+gossip over Tailscale, ship-the-rule).

### M1 — The mesh is real (the ACTIVE front) — #62

Generalize the protocol abstraction from parent↔child (`local`/`rest`/`stream`)
to **peer↔peer** (`peer:` / `mesh:` first-class). The mesh is an *address*, not a
mode. Slices (in order):

1. ✅ **CRDT law as a closure invariant** *(done 2026-06-08)* — `algebra::mesh_safety`
   *rejects* a `mesh` link whose reconcile isn't a join-semilattice (commutative +
   associative + **idempotent**). Idempotence is the discriminator: `map[T]` ∪ and
   per-source pools are safe; naive additive + LWW are not (CALM). Laws in
   `crdt_laws.rs`; in the `schema-algebra.md` op table.
2. 🚧 **The `peer:` / `mesh:` protocol + a CRDT shared-link realization** —
   *landed:* the first-class `mesh:` protocol (`MeshProtocol`/`MeshReplica`) gates
   instantiation through `mesh_safety` and merges replicas via the schema `merge`
   (the state-based CRDT join); two replicas **converge over a LIVE rest bridge**
   with no coordinator (`mesh_live_bridge.rs`). **Remaining:** the `link name :: T
   mesh` `.ys` surface (declare a mesh link in chrysalis), continuous gossip /
   anti-entropy (vs one-shot exchange), and the δ-state CRDT (ship *deltas* via
   `apply`, needs rest delta-in #5) as a bandwidth refinement.
3. **Continuous gossip / anti-entropy** — per-tick convergence (vs slice-1's
   one-shot), with tombstone/metadata GC budgeted (no auto-DGC — bigraph links
   cycle → explicit lease/epoch).
4. **SWIM membership + discovery** — peers join/leave; failure detection.
5. **N-peer** — beyond two; the convergence holds at N.
6. **Transport over Tailscale** — rest/stream over tailnet IPs; authenticated ⇒
   not Byzantine.

**Checkpoint:** N engines converge a shared `mesh` link with no coordinator;
members join and leave; the algebra refuses an unsafe merge at compile time.

*Open frontier (honest):* atomic topology-rewrite across hosts, evolving-merge
AlChemy, distributed cyclic GC — genuinely unsolved; M2 fires reactions
cross-host **optimistically + converge via CRDT**, single-host **atomically**.

### M2 — Each domain over the mesh (mostly free) — composition

With M1 in place, the domains compose onto the mesh through the uniform codec:

- **Biology over the mesh** — the cross-composite reactor (already distributed
  over rest, #43) fires over `mesh:`; cells grow / divide / couple across peers;
  **diffusion across a peer boundary** is halo exchange (#26).
- **Quantum over the mesh** — entanglement / **teleportation across machines**;
  `quantum-locc.ys`'s classical wire becomes a mesh link; LOCC is the protocol.
- **AlChemy over the mesh** — reactions authoring reactions across peers (the
  rest case is done; mesh is the next address).

**Checkpoint:** a reaction fires across the mesh; a cell divides on a remote
peer; a teleportation completes across two machines.

### M3 — The synthesizers (the parallel front, offline-first) — #63

Genuinely new code, but **independent of the mesh** until A8, so it can proceed
in parallel. Build order (`synthesis-bigraphs.md`):

- **A1** — `Signal` type (Array repr, additive-mix `apply`, PCM/Arrow codecs) +
  an offline oscillator rendering a WAV deterministically under `cargo test`.
- **A2** — module library (Oscillator/LowPass/VCA/ADSR) + a hand-patched voice +
  a Rack mix bus + the units pack (Hz/dB/semitone/MIDI/V), offline.
- **A3** — **realtime device boundary (FIRST AUDIBLE MILESTONE).** gorgon →
  library crate; `prism-audio` Speaker/AudioIn over a lock-free ring; a pull
  driver locking wall-clock to logical time. *You hear the A2 voice play live.*
- **A4** — control-rate modulation (LFO/Sequencer/SampleHold) + CV as `Signal`.
- **A5** — **reactions rewiring a live patch** (a BRS over a patch bigraph:
  InsertFilter / Detune / Prune) — the same BRS, on audio.
- **A6** — **homoiconic module factory** — a reactum that authors a *new module
  type* (AlChemy on synth bigraphs — the mechanism is done, #61).
- **A7** — **merge / split voices** (`fold`/`unfurl` + `tensor`/`divide` on
  audio; couple = "voices share a link").
- **A8** — **`net:` protocol + distributed synths** (gorgon transport as a
  protocol; two machines; a jam over the mesh) — this is **synth-over-mesh**, and
  reuses M1.

**Checkpoint:** a `.ys` patch makes sound (A3); then a distributed jam (A8).

### M4 — The grand unification (THE ultimate demo) — A9 + everything

Compose M2 + M3 on **one distributed tick**. A streaming, mesh-distributed
simulation where, simultaneously:

- **biological colonies** grow / divide / diffuse across an
  **environment-of-environments** spanning machines (the octree, #27; halo
  exchange, #26; dynamic load balancing as a colony outgrows a host, #28);
- **quantum subsystems** entangle and teleport across the mesh;
- **synthesizer voices** stream, merge / split as they couple / decouple, and are
  **rewired live by a BRS that authors new modules** (A9);

…all ticking the **same engine**, coupled by **outer links + topology
reactions**, converging via **CRDT mesh links**, streamed out continuously.

This is `synthesis-bigraphs.md` §X's "homoiconic distributed mesh jam" with
**biology and quantum added as concurrent composites on the same distributed
tree** — same `fold`/`unfurl` algebra, same outer-link topology reactions, same
BRS, just more specializations plugged in.

### M5 — the living demo (the same vision, becoming) — #65 + #66

**M4 is a fixed checkpoint, not the edge of the world.** The vision is in a state of
becoming, and M5 is the same organism growing two faculties — *without enlarging or
deferring M4*. M5 is M4, now:

- **rendered** — the whole mesh drawn in 3D/4D via the **spatial** domain
  (`../parsimony`, #65); you *see* colonies, entanglement, patches, and the bigraph
  topology itself. `render` is a functor from each domain prop to the geometry prop.
- **adapting** — an **adaptive-network** layer (`../manifold`, #66) couples and
  modulates the domains and **learns its own topology** (Hebbian/STDP firing
  topology reactions, #43): the M/R closure made dynamical. Manifold is the coupling
  layer that turns five parallel domains into one *learning* organism.

All of it expressed through the **categorical core** ([`categorical-core.md`](categorical-core.md)):
domains = theories, couplings = functors, convergence = the shared fixpoint the
substrate iterates (confluence / CRDT / dynamical attractor). This is not
"bigger-and-more-remote" in a way that breaks doability — each piece is recognition
plus a small consumer, threaded by the spine. See `categorical-core.md` §9 and the
graded manifold demos there (Demo 1 runs on today's substrate).

---

## 3. The critical path & how the tasks feed it

```
                    ┌──────────────── M1: NATIVE MESH (#62) ───────────────┐
M0 (done) ──────────┤  CRDT-invariant → peer:/mesh: → gossip → SWIM → N-peer │──┐
  substrate ready   └───────────────────────────────────────────────────────┘  │
                         │                                                       │
                         ├── supported by the distributed/HPC ladder ───────────┤
                         │     #25 batched ray → #26 halo → #27 octree →         │
                         │     #28 load-balance → #29 cluster backend            │
                         │     (#48 TCK conformance, #49 sidecar-as-Process)     │
                         ▼                                                       ▼
              M2: domains over the mesh (≈ free) ──────────────► M4: THE ULTIMATE DEMO (A9)
              biology #43·#26 / quantum #36 / alchemy #61            all domains, one tick,
                         ▲                                           streamed over the mesh
M3: SYNTH (#63) ─────────┘
  A1·A2 offline → A3 AUDIBLE → A4 → A5 BRS → A6 homoiconic → A7 merge/split → A8 net:
```

**The two real builds are M1 (mesh) and M3 (synth).** M2 and M4 are
composition — the substrate already proves each piece locally and over rest; the
mesh just makes it an address. The HPC ladder (#25–#29) hardens M1/M2 from
"2 peers on a LAN" to "planet-scale colonies" and is where the
`cluster = a protocol with a pluggable backend` decision lands (adopt
Charm++-style migratable placement, or Ray, behind the `Protocol` interface — do
not rebuild a cluster runtime).

**Streaming** (the word in the demo's name) is largely done — the Trace kernel,
delta-log/Arrow codec, `chrysalis run --serve-stream`, schema-header `refines`
checks all shipped (#18); the remaining `--map`/`--adapt` + 4D structural plot
are tooling polish, not blockers.

### What's NOT on the critical path (but enriches the demo)

- **Fundamentals (E)** make M4 *clean* rather than possible: the unified entity
  registry (#30), schema-as-state (#15), and the generative-core program (#59)
  let the demo's reactions/types/modules be authored homoiconically and evolve.
- **Biology depth (B)** — SBML import + the repressilator (#8), the dFBA family
  (#13), KISAO/SED-ML export (#33) — make the *biological* half scientifically
  real rather than a toy colony.
- **Quantum depth (C)** — the effects/handlers generalization (#35 slices 2–10),
  auto-merge on cross-composite gates (#36 Q4), auto-divide on factorizability
  (#36 Q5) — make the *quantum* half structurally automatic.

---

## 4. Suggested order

1. **Now:** finish **M1 (#62)** — the mesh shared-links across composites. Start
   with the **CRDT closure invariant**, then the `peer:`/`mesh:` protocol + the
   δ-CRDT shared-link realization. This is the active front.
2. **In parallel:** open **M3 (#63)** to **A3** (the first audible milestone) — a
   self-contained, offline-first front that needs nothing from the mesh.
3. **Then M2** — exercise biology / quantum / AlChemy over the mesh (each a small
   composition demo); harden with the HPC ladder (#26 halo first — it's the
   neighbor-exchange gap the octree needs).
4. **Then M4** — A8 (distributed synth jam) → A9 (add biology + quantum as
   concurrent composites on the distributed tree). The ultimate demo.
5. **Throughout:** pull in fundamentals (#30/#15/#59) and domain depth (#8/#13,
   #35/#36) as each makes the corresponding half of M4 real instead of toy.

---

## 5. Pointers

- **Mesh / distribution:** [`distributed-execution.md`](distributed-execution.md)
  (the octree + the cluster-backend decision),
  [`distributed-bigraphs.md`](distributed-bigraphs.md),
  [`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md) (outer links =
  the implicit composite spanning machines). Memory [[mesh_as_protocol]],
  [[boundary_codec_algebra]].
- **Synthesizers:** [`synthesis-bigraphs.md`](synthesis-bigraphs.md). Memory
  [[synthesizer_project]].
- **Quantum:** [`quantum-bigraphs.md`](quantum-bigraphs.md),
  [`effects-and-handlers.md`](effects-and-handlers.md).
- **The spine:** [`categorical-core.md`](categorical-core.md) — domains = theories,
  couplings = functors, `fold`/`unfurl` = the compact cup/cap = M/R-closure =
  reflection, convergence = the fixpoint the substrate iterates.
- **Spatial & adaptive (M5):** `../parsimony` (spatial computing / the octree / the
  3D-4D eye), `../manifold` (adaptive resonance networks / the coupling layer).
- **Fundamentals:** [`schema-algebra.md`](schema-algebra.md),
  [`generative-core.md`](generative-core.md),
  [`state-schema-unification.md`](state-schema-unification.md).
- **Status & tasks:** [`NEXT-SESSION.md`](NEXT-SESSION.md).
