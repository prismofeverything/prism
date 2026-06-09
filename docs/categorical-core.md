# The categorical core — one substrate, many theories, the functors between them

> **Spine doc.** Every other design doc touches a facet of one structure: prism is
> a framework for **presented symmetric monoidal theories** (props / Lawvere
> theories). A *domain* is a presentation (generators + equations); a cross-domain
> coupling or a protocol boundary is a **functor**; the closed/compact structure
> gives reflection, entanglement, and self-production as **one** thing; and
> convergence — confluence, CRDT, dynamical attractor — is the shared **fixpoint**
> shape the substrate iterates. This doc *names* the through-line so the vision
> stays coherent as it grows.

The vision is in a state of becoming, and this is what is **conserved** as it
becomes. Biology, quantum, synthesizers, spatial computing, adaptive networks are
not five projects — they are five presentations of one engine. Nothing here is new
mechanism; it is **recognition**. The constructive moves it implies are small and
each *deletes* a special case (the wei-qi/Felleisen gate, `[[feedback_chrysalis_wei_qi]]`).
Read with [`generative-core.md`](generative-core.md) (the minimality philosophy),
[`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md) (the object↔morphism
maneuver), [`schema-algebra.md`](schema-algebra.md) (the algebra), and
[`grand-synthesis.md`](grand-synthesis.md) (the demo this serves).

## 1. The thesis: wiring is composition (already)

Two design docs already state the categorical structure without naming it as the
spine:

- [`chrysalis-design.md`](chrysalis-design.md) §"Categorical structure": bigraphs
  are morphisms in a symmetric monoidal **s-category** (Milner); `~{}->{}` is the
  morphism's type signature (domain → codomain); **`|` is tensor (⊗)**; pattern
  matching is **morphism factorization** `C ∘ (G ⊗ id)`.
- [`generative-core.md`](generative-core.md) §1: the schema algebra **is a Lawvere
  theory** — a signature (`apply`/`divide`/`merge`/`reconcile`/…) plus equations,
  whose models are the free algebra.

So the categorical core is load-bearing and unnamed. The name is **symmetric
monoidal theory (SMT)**: generators + equations presenting a monoidal category.
Two faces of one idea:

- **Schema side = Lawvere theory** — single-output algebraic operations on values.
- **Process side = a prop** (PROducts and Permutations) — multi-output morphisms
  with wire permutations; the permutations *are* `|` and wire-crossings. A prop is
  the multi-output generalization of a Lawvere theory.

The consequence you arrived at yourself: **a `.ys` module IS a presented theory**
— its definers (`process`/`step`/`composite`/`reaction`/`type`/`unit`/`context`)
are the *generators*; its reactions and algebra laws are the *equations*. Running
it = computing in that theory's free model (the BRS over the substrate). *The
language is also a theory.*

## 2. The domains are theories (one substrate, different presentations)

| Domain | Its categorical object | Prior art |
|---|---|---|
| Bigraphs | symmetric (partial) monoidal s-category | Milner, *Space and Motion* |
| Quantum | **†-compact-closed** category (FdHilb); ZX is its presented prop | Abramsky–Coecke; Coecke–Kissinger |
| Reaction networks (CRN) | category of open reaction nets; the rate eq. is a functor | Baez–Pollard |
| Petri nets | free symmetric monoidal category | Baez–Master |
| Synthesizers / signal flow | the **prop** of signal-flow graphs | Bonchi–Sobociński–Zanasi |
| Spatial / geometry | place graph as a category of regions; render = a functor | (this doc, §6/§9) |
| Adaptive networks (manifold) | ℂ-linear dynamics; the **CPM**/dissipative regime | Selinger (CPM); §6 |
| (M,R) systems | closure to efficient causation = a fixpoint in a **closed** category | Rosen; Louie |
| Schema algebra | Lawvere theory | `generative-core.md` |
| Distribution / mesh | a **monoidal functor** into "things over a transport" | `[[boundary_codec_algebra]]` |

The differences are the *presentation*, not the engine. This is the academic
backing for the one-engine thesis (`grand-synthesis.md` §1): **the demo is a
composition of specializations, not a new mechanism**, because the specializations
are literally choices of generators-and-equations over one substrate.

## 3. The keystone: `fold`/`unfurl` = cup/cap = entanglement = M/R-closure = reflection

A **†-compact-closed** category gives every object a dual with a *cup* `η : I → A*⊗A`
and a *cap* `ε : A⊗A* → I` (the snake/yanking law). Bending a wire with them turns
a morphism `A → B` into a *state* `I → A*⊗B` — **process–state duality** (Choi). That
bend is exactly the object↔morphism maneuver of
[`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md): "ports = where the
link graph was **cut**; the severed end becomes a name on the face." *A cut link
with a name on the boundary is a bent wire.* Therefore **one structure, five names**:

| name | where | identity |
|---|---|---|
| `fold` / `unfurl` | composite boundary | seal/open a sub-bigraph = bend/unbend the cut links |
| `tensor` / `divide` | schema/state | the same maneuver specialized to one schema (`[[reaction_delta_basis]]`) |
| cup / cap | quantum | Bell-prep / Bell-effect; **teleportation = the snake law** (runs, #36) |
| **closure to efficient causation** | (M,R) | the repair map `Φ: B → [A,B]` outputs a *morphism* ⇒ the category is **closed** ⇒ morphisms are objects the system can produce |
| reflection / self-production | language | a reaction authoring a reaction (AlChemy #61); the metacircular orchestrator |

The (M,R) line is the deep one. Rosen's organism is **closed to efficient
causation** — its own components produce the maps that produce its components. That
is the *internal hom* / closed-monoidal structure: morphisms live among the
objects. It is the **same closure** as homoiconic self-generation (`[[reaction_delta_basis]]`
rules-as-state), as the metacircular north-star (the orchestrator as a `.ys`
program), and as "useful→fundamental = closure under composition / self-catalysis"
(`[[project_chrysalis_evolution]]`). **M/R closure, AlChemy, metacircularity, and
reflection are one categorical property: the category being closed.**

*Status: built, unnamed.* `fold`/`unfurl` (S1/S2), `tensor`/`divide` (#39), Bell /
GHZ / teleportation (#36) all run. The work is to *name* the compact structure so
the distributed boundary, quantum teleportation, and reflection stop looking like
three machineries.

## 4. The one new definer: `functor`

A **functor** maps the generators of theory A to morphisms of theory B, preserving
`⊗` and `∘`. FIT-FIRST: this is not new behaviour — it unifies **four** things
prism already does as bespoke glue:

| existing glue | is a functor… | where |
|---|---|---|
| unit / context conversion | …on the dimension group (monoidal) | `prism_schema::units` |
| the **protocol codec** | …over a transport ("local works over a wire unchanged" = **functoriality**) | `[[boundary_codec_algebra]]` |
| `chrysalis::compile` | …syntactic theory → runtime category | `chrysalis/src/compile.rs` |
| a **cross-domain bridge** (bio→render, manifold→synth, order-param→filter cutoff) | …between domain props | *the new capability* |

The Felleisen gate: a first-class `functor` **deletes** the four hand-rolled glues
(or names them as instances), so it passes — it removes more than it adds. It is
the principled home for the cross-domain couplings that make the grand demo **one
organism rather than five silos**: "render the colony in 3D", "let the oscillator
net's coherence gate a synth voice", "read a quantum measurement into the network"
are each *a functor between two domain props*, not bespoke wiring.

*Status: potential.* The four instances exist; the unifying definer does not. First
consumer: re-state the codec and unit-conversion *as* functors (recognition, no new
code); then the cross-domain render bridge (§9, the spatial fold-in).

## 5. Convergence: prism is a fixpoint engine

You named "a space of convergence." There are **three** real notions of convergence
in play, and the discipline (`[[feedback_no_half_measures]]`) is to keep them
distinct while seeing the shared shape:

- **Confluence / normal forms** — rewriting settles to a canonical term
  (`generative-core.md` §2; Knuth–Bendix from the algebra's laws).
- **CRDT convergence** — replicas settle via an idempotent **semilattice** join
  (the mesh; `algebra::mesh_safety`, `crdt_laws.rs`).
- **Dynamical convergence** — a flow settles to an **attractor** (manifold: phase
  sync, resonance, a homeostat's set-point).

These are **not the same merge** — a Kuramoto sync is not a semilattice join — but
all three are *iterate-an-operator-to-a-fixpoint*, and prism already iterates (the
BSP tick + `reconcile` + the BRS). **prism is a fixpoint engine; each domain picks a
different operator to iterate.** The honest design constraint that falls out: a
*learning* link is **not** a CRDT-safe `mesh` link in general (Hebbian/phase updates
aren't idempotent) → it rides the **optimistic-fire-then-converge** path
(`grand-synthesis.md` M2's open frontier), not the semilattice-merge path.

The clean composition that makes a distributed manifold honest: a Kuramoto
mean-field carried as a **per-source `map[id → ℂ]` link** is CRDT-safe replication
(own-key overwrite = idempotent; key-union = commutative/associative — the survey's
"additive needs per-source accounting" fix, `[[mesh_as_protocol]]`), and
**synchronization is the dynamics reading that replicated field**. Two convergence
types **stacked**, each correct on its own terms. That stack *is* the space of
convergence, concretely.

*(The project's own vision is itself a trajectory converging on an attractor — in a
state of becoming. The categorical core is the invariant of that flow.)*

## 6. ℂ as shared substrate — quantum ⊕ manifold

The `manifold` seed (`../manifold`) is built on a complex-valued substrate: the
**real** axis carries amplitude (leaky adaptation, sigmoid bistability, Hebbian),
the **imaginary** axis carries phase (Kuramoto, STDP). That is not a coincidence
with quantum — it is the *same category, two regimes*:

- Quantum (closed/reversible) = **unitary** ℂ-linear maps (FdHilb, †-compact).
- Open/dissipative dynamics = **Selinger's CPM construction** on that category
  (completely-positive maps; irreversible).
- So **manifold is the dissipative specialization, quantum the unitary
  specialization, of one ℂ-linear dagger category.** A Kuramoto oscillator on the
  unit circle (`|state| = 1`, pure phase) is literally **U(1)** — unitary; the
  leaky-amplitude part is the dissipative part.

Consequence (parsimony win, not a manifold cost): **one `Signal[ℂ]` / `Complex`
base sort** shared across synthesizers (real audio), manifold (phase + amplitude),
and quantum (amplitudes). One sort, three domains. (A genuine new base sort — log it
as a deliberate conservative extension per `generative-core.md`, since it extends
the vocabulary `[[type_methods_delta_model]]`; quantum currently encodes ℂ as
`map[float]` pairs, which this unifies.)

## 6b. The fundamental-type catalog — shared representation, per-domain methods

The deepest design principle here ([[type_methods_delta_model]], #7): **a type is a
representation + its methods, and the methods ARE the type** — the data is often
trivial (an oscillator is "just" a phase), the *behaviour* is the type. So the right
move is **share representations across domains and vary the method-sets** — exactly
the categorical reading (one object, different morphisms). The catalog to land in the
Core (#68), organized that way:

| Type | Representation | Methods (the algebra) | Shared by |
|---|---|---|---|
| **`Complex`** | `array[[2]]` (re,im) — additive fold = ℂ addition | `re`/`im`/`abs`/`arg`/`conj`/`mul`/`scale`/`rotate`/`from_phase` | quantum (amplitudes), manifold (phase+amp), synth (analytic signal) |
| **`Vec[n]`** | `array[[n]]` | `at`/`add`/`scale`/`dot`/`norm`/`normalize` | everything spatial/field/signal (Demo 1's pool is `Vec2`) |
| **`Quaternion`** | `array[[4]]` | `mul`/`conj`/`normalize`/`rotate`/`slerp` | spatial (3D rotation), quantum (SU(2)) |
| **`Matrix`/`Tensor`** | `array[[shape]]` | `matmul`/`transpose`/`apply` | quantum gates, transforms, linear maps |
| **`Transform`** | `{pos: Vec3, rot: Quaternion}` | `compose`/`inverse`/`apply` | spatial (parsimony `Placement`) |
| **`Mesh`** | triangle soup + BVH (Foreign) | `contains`/`raycast`/`coarsen`/`render` | spatial |
| **`Aabb`/`Sphere`/`Capsule`** | analytic bounds | `contains`/`signed_distance`/`sample_surface` | spatial (parsimony `Compartment` = place graph) |
| **`Octree`/`VoxelField`** | sparse hierarchical grid (Foreign) | `insert`/`query`/`sample` | spatial **= the #27 distributed octree** |
| **`Oscillator`** | `{theta, omega}` | `rotate`/`phase`/`couple` — *the type IS the methods* | manifold (Demo 1's `Spin` lifted to a type) |
| **`PlasticSynapse`** | `weight` (a `link` value) | `hebbian`/`stdp` (Δ-producers, via `reconcile`) | manifold |
| **`Signal[ℂ]`** | `array` | `mix`/`filter`/`gain` | synth, manifold |
| *(exist)* `Qubits` | `map[float]` → should become `map[Complex]` | gates/`tensor`/`factorize`/`measure` | quantum |
| *(exist)* `CRN`,`TimeSeries`,`Figure`,`Trace`,`BRS`,`Reaction`,`Pattern`,`Path` | various | their domain algebras | biology/tooling |

Most representations are `array`-backed, so the **additive fold of the algebra is the
shared arithmetic** (no per-type summation code). The discipline is the wei-qi/axiomatic
floor: add only the methods a consumer needs, on the structural sort, and lift to a
named type when the method-set coheres. **Demo 1 seeds the catalog** with the first
entries — `Float.sin`/`cos`/`sqrt` and the axiomatic `List.at` (richer array algebra —
slicing `a[i:j]`, `dot`/`matmul`, numpy-style ops — is deferred to the `Vec`/array-type
work, #69, not bolted on now). `Oscillator` and `Complex` are the first two named lifts.

## 7. The reflection upgrade: `quote` / `eval`

The closed structure of §3, made surface. `eval` is `discover_processes` ("prism's
eval", `chrysalis-design.md`) promoted to a first-class value operation; `quote` is
`Expr::to_value` (exists). Then:

- `compile_reaction` (a special-cased definer→value bridge) dissolves into the
  general `eval ∘ quote` — one bolt-on deleted.
- the meta/object duplication collapses: the capitalized constructor (`Reaction[…]`,
  `ProcessDef[…]`) **is** `quote` of the lowercase definer (`reaction`, `process`);
  you stop maintaining two surfaces.
- the homoiconic round-trip becomes **total** (`from_value` covers every `Expr`,
  not just the common variants) — partial homoiconicity isn't homoiconicity.

This *is* the metacircular north-star, operational — and it is §3's closure with a
keyboard. *Status: potential; pieces exist (partial `to_value`/`from_value`,
special-cased `compile_reaction`).* 

## 8. Discipline — keep it operational (the anti-abstract-nonsense rule)

Category theory earns its place here only by **deleting special cases**, never by
adding a vocabulary layer:

1. Every categorical concept **cashes out as executable code with property-tested
   laws** — the schema-algebra pattern (laws as tests), `[[feedback_schema_algebra]]`.
   A functor ships with `F(g∘f)=F(g)∘F(f)` and `F(a⊗b)=F(a)⊗F(b)` as tests.
2. Introduce structure **only where it deletes a special case** (`generative-core.md`
   "find the generator, delete the duplicates"). Don't add a primitive; name the
   generator already load-bearing.
3. The two weight-pulling additions — `functor` (§4) and `quote`/`eval` (§7) — both
   pass the gate. The rest of this doc is recognition: naming `|`=⊗, the module=a
   theory, fold/unfurl=the compact structure.

## 9. How this enters the roadmap — the same vision, becoming

The categorical core is a **spine, not a milestone**. It is E-category fundamentals
threaded through the *existing* M0–M4 (`grand-synthesis.md`), mostly **recognition**
(cheap), and it makes the demo **more coherent** (fewer concepts), not bigger.

- **M4 (bio + quantum + synth, distributed) stays a fixed checkpoint** — unchanged,
  doable. You do not enlarge it or push it away.
- **M5 — the living demo** (the becoming; explicitly further-out): the M4 mesh, now
  **(a) rendered** in 3D/4D (spatial / `../parsimony` — you *see* the whole organism)
  and **(b) adapting** (manifold / `../manifold` — an oscillator/learning network
  couples and modulates the domains and learns its own topology), all expressed
  through the categorical core (domains = theories, couplings = functors). Not
  "bigger-and-more-remote" in a way that threatens doability — *the same organism
  becoming more itself.*

**The two new domains, on the same six aspects** as `grand-synthesis.md` §1:

- **Spatial (parsimony).** node = a placement/compartment (place graph); link =
  adjacency/constraint; couple = pack into a shared volume; split = partition the
  octree; distribute = subdomains across machines — **parsimony's `VoxelField`/QBVH
  *is* the #27 octree, already built**; generate = a BRS that packs/relaxes/rewrites
  geometry. Contributes the **eye**: render = a functor to the geometry prop (§4),
  retiring the graphviz viz appendage. *Fold-in: the type catalog now
  (`Snapshot↔Value`, `Space`/`Transform`/`Mesh`/`Field`, a `Render` method); the
  deep packing-as-BRS coupling after the M1 API settles (parsimony's own plan).*
- **Manifold (adaptive networks).** node = an oscillator/adaptive unit (a `process`);
  link = a (plastic) synapse (a first-class `link`, `[[link_surface]]`); couple =
  Kuramoto phase-coupling; split = decouple as a synapse decays; distribute =
  subnetworks across peers (§5); **generate = Hebbian/STDP firing topology reactions
  (#43) — the network learns its own topology = the M/R closure made dynamical (§3)**.
  Its plastic weight is a link-schema `reconcile`/`apply` of a Δ — *not* a new weight
  store (`[[reaction_delta_basis]]`); its two-phase update *is* the BSP tick. It is
  the natural **coupling layer** that ties the domains into one learning organism.

**Tasks** (in [`NEXT-SESSION.md`](NEXT-SESSION.md)): **#64** categorical core (E) ·
**#65** spatial + visualization (new category H) · **#66** manifold / adaptive
networks (new category I) · **#67** the dependability trio — trace-as-time-travel
debugger (the delta-log already records every tick; stepping/inspect/breakpoint is a
UI over it), the chrysalis LSP (over the REPL's type/env machinery), a package
registry + lockfile (a registry **of theories**) (F). #62 (mesh) gains a
transport-generalization clause: address+auth = **capability refs** (self-authenticating,
promise-pipelined), transport pluggable (QUIC/Noise/relay), **Tailscale demoted to
one backend, not a dependency**.

**Graded demos** (the manifold-over-bigraphs ask): **Demo 1 — DONE ✅**
(`crates/chrysalis/ys/kuramoto.ys` + `tests/kuramoto_sync.rs`): a Kuramoto tile of
oscillator processes synchronizing through one shared **additive `array[[2]]` link** —
the mean field Σⱼ[cosθⱼ,sinθⱼ], each oscillator emitting the *delta* of its
phase-vector so the engine's additive fold IS the reduction (the particle Δposition
model — no new primitive). Coupled **R=0.998** (phase-locked) vs uncoupled **R=0.380**
(drifted), on the same engine that grows a cell colony. Two lessons worth keeping: a
comprehension needs a *computed* key `'{id}'` (a bare `id:` is a literal field name, so
all oscillators collapse into one entry); and the integration step must be a fixed `dt`
config *decoupled* from the engine tick, because the per-oscillator composite ticks at
the default rate (→ **#70**, first-class multi-scale composite intervals). It also
seeded the shared math floor (`Float.sin/cos/sqrt`, `List.at`, `sum`). **Demo 2 —
local slice DONE ✅** (`kuramoto-mesh.ys` + `kuramoto_mesh.rs`): the mean field is now
a per-source `map[id → array[[2]]] mesh` link (each oscillator owns its key, accumulating
its phase-vector delta there; mean field = `field.sum()`). Key-union = join-semilattice
= exactly what `mesh_safety` blesses — *the same shape as the agent coordination board* —
so the coupling is CRDT-safe by construction. Result: **R = 0.998 / 0.380, identical to
Demo 1**, proving local coupling rides a `mesh`-declared link *unchanged* (the one-engine
thesis). **Distributed slice DONE ✅** (`kuramoto_mesh_distributed.rs`, on the mesh
agent's runtime): two tiles on two *separate engines* (disjoint keys `a*`/`b*`, n=8
global), the field map gossiped over a live rest bridge (`mesh_links` reflection +
`MeshReplica::gossip`), phase-lock across the machine boundary — **R_a = R_b = 0.964,
converged, no coordinator**. The `.ys` is byte-for-byte the local one; only the transport
between the field replicas differs (the mesh is an *address*, not a mode). The convergence
*stacks*: CRDT replication of the field map + the dynamical attractor reading it (§5) —
the round-trip "double-count" risk dissolves because each key is single-writer and the
local tick refreshes it (optimistic-fire-then-converge). A two-agent build: the manifold
dynamics on the mesh agent's CRDT runtime, coordinated through the per-source board.
**Demo 3** — plastic topology learning as a topology-rewriting BRS: Hebbian
co-activation fires a reaction that strengthens/prunes a link; the network rewrites
itself (#43; local-first; the closure payoff).

## 10. References

**Internal (each doc = a facet of this spine):**
[`chrysalis-design.md`](chrysalis-design.md) (the s-category; wiring = composition) ·
[`generative-core.md`](generative-core.md) (Lawvere theory; closure; the Felleisen
gate) · [`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md) (object↔morphism;
`fold`/`unfurl`; ports = cut links) · [`schema-algebra.md`](schema-algebra.md) (the
algebra as equations) · [`grand-synthesis.md`](grand-synthesis.md) (the demo) ·
`quantum-bigraphs.md` · `synthesis-bigraphs.md` · `distributed-execution.md`
(the octree) · `distributed-mesh-survey.md` (CRDT/CALM). Sibling projects:
`../parsimony` (spatial) · `../manifold` (adaptive networks).

**External:**
Milner, *The Space and Motion of Communicating Agents* (bigraphs) ·
Abramsky & Coecke, *A Categorical Semantics of Quantum Protocols*; Coecke &
Kissinger, *Picturing Quantum Processes* (CQM / ZX) ·
Selinger, *Dagger Compact Closed Categories and Completely Positive Maps* (the CPM /
dissipative regime) ·
Baez & Pollard, *A Compositional Framework for Reaction Networks*; Baez & Master,
*Open Petri Nets* ·
Bonchi, Sobociński & Zanasi, *Full Abstraction for Signal Flow Graphs* (signal flow
as a prop) ·
Rosen, *Life Itself*; Louie, *More Than Life Itself* ((M,R) closure) ·
Lawvere, *Functorial Semantics of Algebraic Theories*; Mac Lane, *Categories for the
Working Mathematician* ·
Fritz, *A synthetic approach to Markov categories* (the categorical-probability
corner of `[[user_interests]]`).

**Memories:** `[[boundary_codec_algebra]]` · `[[mesh_as_protocol]]` ·
`[[generative_core_doc]]` · `[[composite_is_a_type]]` · `[[reaction_delta_basis]]` ·
`[[link_surface]]` · `[[type_methods_delta_model]]` · `[[feedback_chrysalis_wei_qi]]` ·
`[[feedback_schema_algebra]]` · `[[feedback_no_half_measures]]` · `[[user_interests]]`.
