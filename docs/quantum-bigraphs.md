# Quantum Bigraphs

A design document for unifying quantum computation with bigraph
rewriting in chrysalis. The core insight (extending
`docs/effects-and-handlers.md` §XI): **the bigraph link graph IS the
entanglement graph**. Qubits sharing a link are entangled and must
share a joint state; qubits not sharing a link are separable and can
genuinely be distinct processes. The bigraph's place/link structure
gives quantum mechanics a syntactic substrate it didn't have before.

This doc captures the design ideas surfaced while building the
quantum-engine series (`quantum-bell-state.ys`, `quantum-engine.ys`,
`quantum-ghz.ys`) and exploring the question "can each qubit be its
own streaming process?"

---

## I. The fundamental constraint: non-factorizability

A Bell state `(|00⟩ + |11⟩)/√2` cannot be written as
`(a|0⟩ + b|1⟩)_A ⊗ (c|0⟩ + d|1⟩)_B` for any (a, b, c, d). This is
provable: the rank of the 2×2 amplitude matrix is 2 for the Bell state
(entangled) and 1 for any factorizable state.

**The physics doesn't have local hidden variables** (Bell's theorem,
Aspect's experiments). There is genuinely no "qubit A's state"
separate from "qubit B's state" when they're entangled — only the
joint amplitude.

Practical consequence: any system that wants to compute quantum
mechanics must hold the JOINT amplitude representation of all
mutually-entangled qubits in one place.

This isn't a chrysalis limitation — it's a constraint of quantum
mechanics. Every quantum simulator (Cirq, Q#, Qiskit, the lot)
maintains the joint amplitudes for entangled qubit groups.

## II. The bigraph insight: links carry entanglement

The §XI thesis: in a chrysalis bigraph,

> **two qubits are entangled iff they share a link in the link graph.**

This is structural — you can read entanglement off the bigraph
diagram. Three immediate consequences:

1. **Separable qubits can live in different processes / composites.**
   If `q3` and `q4` share no link with `q0..q2`, then `(q0,q1,q2)` and
   `(q3,q4)` are TWO independent quantum systems. The total state
   factors. They can be different `stream:`-served subprocesses with
   genuinely local quantum states.

2. **Entangled qubits must share a joint state.** If `q0` and `q3`
   share a link, `q0..q3` are one quantum system and their amplitudes
   live in one composite.

3. **Gates that create entanglement are link-creation rewrites.**
   `CNOT(q_a, q_b)` becomes "add a link between `q_a` and `q_b` and
   merge their state composites if they weren't already in the same
   one." Measurement is the reverse — link decay + state collapse.

In other words: **the bigraph's place graph hosts the qubits as
nodes; the link graph carries the entanglement; the algebra rewrites
both together.**

## III. Three operations that compose

The natural quantum operations are *all* bigraph rewrites:

### Tensor (separable composition)
Two unentangled systems combine into one composite holding both.
Bigraph: a new composite encloses both, with no new links.

**This is the reverse of `divide`.** Just as `divide` splits a
composite into independent daughters, **tensor** merges independent
composites into a joint composite. They form a duality:

| Operation | Bigraph effect | Quantum effect |
|---|---|---|
| **divide** | one composite → two daughters | factorize a separable joint state |
| **tensor** | two composites → one composite | combine two separable systems |
| **entangle** | add a link in an existing composite | apply a coupling gate |
| **measure** | collapse a link + amplitude | observe; may allow re-factorization |

Tensor and divide are **inverses on separable states**. You can
divide a separable composite into independent sub-composites, then
re-tensor them, and get the same joint state back.

### Entangle (apply a coupling gate within a composite)
A two-qubit gate like CNOT, applied to qubits already in the same
composite, MAY create entanglement (it does for some input states
and not others — e.g., CNOT on `|00⟩` is unentangled output `|00⟩`
classically; CNOT on `(|00⟩ + |10⟩)/√2` produces the Bell state).

After entanglement, the joint state of the affected qubits is no
longer factorizable. The bigraph adds a logical "entanglement link"
between the qubits — they're now in the same entanglement-component.

### Measure (collapse)
Born-rule sampling. After measurement of qubit `q`, the joint state
is projected onto the measured eigenstate. The qubit's entanglement
with its partners is REDUCED — if it was maximally entangled, after
measurement the entanglement is gone (the partners are forced into
the correlated post-measurement state).

After enough measurements, an entangled composite may become factorable
again. At that point, `divide` becomes available — the joint composite
can split into independent sub-composites.

## IV. Entangled systems can interact in TWO ways

The user's question — "are entangled systems always becoming one
joint system?" — has a richer answer:

### Way 1: Quantum coupling (entangle further)
Apply a multi-qubit gate that touches qubits across the two systems.
The systems merge into one composite. Bigraph: a new link spans the
formerly-separate composites. Joint state now lives in one composite.

### Way 2: Local Operations + Classical Communication (LOCC)
Each system stays independent. Communication is via CLASSICAL
channels — measurement outcomes, instructions. No new quantum
coupling. The systems can do remarkable things via LOCC:

- **Quantum teleportation** — Alice has a state she wants to send to
  Bob. They share a pre-existing Bell pair (entanglement RESOURCE).
  Alice measures her qubits, sends 2 classical bits to Bob. Bob
  applies a corresponding Pauli to his half of the Bell pair.
  Result: Bob's qubit now carries Alice's state. The entanglement
  was *consumed* to transmit the state; no quantum channel was used.
- **Entanglement distillation** — many noisy Bell pairs → fewer
  high-fidelity Bell pairs via LOCC.
- **Quantum key distribution** (BB84) — random key generation with
  guaranteed secrecy from the laws of physics.

So **entangled systems can interact while staying as separate
quantum systems**, mediated by classical channels. The bigraph would
model this as: each system is its own composite; a CLASSICAL channel
(an ordinary bridge wire) carries measurement outcomes between them.
**LOCC is exactly the case where quantum systems stay independent
composites and communicate via classical-typed wires.**

This is structurally beautiful: in a chrysalis quantum bigraph,
- **Quantum links** (red wires, say) — entanglement, force joint state.
- **Classical links** (blue wires) — classical communication, no
  entanglement; carry numbers and bits.

The link TYPE encodes whether two composites share a quantum or
classical interaction.

## V. The proposed bigraph-native architecture

Putting it all together, the chrysalis quantum-bigraph runtime:

### Data: composites and links

- Each `Qubit` is a place-graph node.
- Each **entanglement component** is a composite enclosing the
  entangled qubits + holding their joint amplitude state.
- **Quantum links** in the link graph mark which qubits within a
  composite share entanglement (today's BiGraphs already let you
  attach typed information to links).
- **Classical links** between composites carry measurement-outcome
  bits or other classical data.

A 3-qubit GHZ state: ONE composite enclosing q0, q1, q2 with full
joint state `(|000⟩ + |111⟩)/√2`. Three nodes, all in one component.

Two independent GHZ states: TWO composites, each enclosing its own
three qubits. They can coexist as separate stream subprocesses with
genuinely local quantum states.

### Operations as bigraph rewrites

- **Hadamard(q)**: a single-qubit gate — local rewrite within the
  composite containing q. No new links.
- **CNOT(q_c, q_t)** when q_c and q_t are in the SAME composite:
  rewrite the joint amplitude; may create or destroy entanglement
  link.
- **CNOT(q_c, q_t)** when q_c and q_t are in DIFFERENT composites:
  the bigraph rewrite first MERGES the two composites (tensor),
  THEN applies the gate. The composites become one.
- **Measure(q)**: Born-rule sample; collapse the joint amplitude;
  reduce/remove q's entanglement links. After measurement, if the
  remaining qubits' joint state is now factorizable, the bigraph can
  `divide` into independent sub-composites.
- **Classical message(q_a → q_b)**: send a classical value over a
  classical-typed link. Bigraph: a bridge wire of classical type
  between composites.

### The composite is the entanglement scope

This is the central invariant: **each composite holds the joint
amplitude state of all qubits inside it.** Operations that don't
cross composites are local; operations that cross composites either
merge them (quantum coupling) or communicate classically.

When a composite's qubits become factorizable (e.g., after enough
measurements), the runtime can **divide** the composite into smaller
ones — each holding a now-independent sub-state. This is the dual of
the `tensor` / `merge` operation. **Division reflects the physics**:
you can only divide what's separable.

## VI. The classical / quantum interplay

LOCC's existence means a chrysalis quantum bigraph naturally
supports hybrid classical-quantum protocols:

```
        ┌────────────────────────┐         ┌────────────────────────┐
        │   Alice's composite    │         │    Bob's composite     │
        │   (3 qubits, GHZ)      │ ─clas─→ │    (2 qubits, |0⟩|0⟩)  │
        └────────────────────────┘  bit    └────────────────────────┘
                  │                                   │
              measure q0                   apply Pauli based on bit
```

A classical wire crosses composite boundaries. No entanglement
created. Each side runs as its own subprocess with its own joint
state. This is **LOCC, modeled as a bigraph**.

If instead we did `CNOT(alice.q0, bob.q0)` — a quantum coupling
crossing composites — the bigraph rewrite would MERGE Alice and Bob
into one composite. Their joint state now lives together. This is
quantum communication that creates entanglement.

The bigraph syntax distinguishes the two cases visibly.

## VII. The divide ↔ unify duality

The user's framing: "they could divide when they become independent
and unify (reverse-divide) when they become entangled."

**Yes — exactly.** This is the operational meaning of the bigraph
algebra on quantum systems:

| State of qubits | Bigraph topology | Operations available |
|---|---|---|
| Separable (independent) | Multiple composites | `divide` (already), `tensor` (merge to one) |
| Entangled (joint) | One composite | Gates within; only LOCC across |

The transitions:
- **Tensor / unify**: two separable composites → one composite (allowed
  any time; physically a structural rebrand).
- **Entangle**: gate inside one composite creates correlations; the
  composite is now an entanglement component that can't be divided.
- **Measure**: reduces entanglement; the composite may become
  factorizable and can `divide`.

**The bigraph divide operation we already have IS the right primitive
for un-tensoring a separable quantum composite.** Its restriction
("only divide what's factorizable") is exactly the physical
constraint.

This is a beautiful unification: the same `divide` that splits a
biological cell into daughters splits a separable quantum composite
into independent qubit registers. The bigraph algebra was always
about decomposition; quantum mechanics is just another instance of
"what's decomposable when, and what must stay joint."

## VIII. What's tractable to build today

The chrysalis substrate (composites, links, algebra, `apply` / `divide`
/ matching) is the SHAPE of this design. What's missing for full
quantum-bigraph semantics:

1. **Link types** (today links are mostly untyped). Adding "quantum"
   vs "classical" link tags is straightforward.
2. **Composite-as-entanglement-scope discipline**. A rule that every
   composite's qubits share their joint state. Today's composite
   already encloses inner state — we just need to USE it for
   amplitudes.
3. **Merge / tensor operation**. The inverse of `divide`. Needs:
   take two composites with compatible schemas, combine their state
   into a tensor product, produce one composite.
4. **Cross-composite gate rewrite**. When a multi-qubit gate spans
   composites, automatic `tensor` first, then apply.
5. **Decomposition check after measurement**. Detect when a
   composite's qubits become factorizable; auto-`divide`.

Each of these is incremental. None require a runtime rewrite.

## IX. Suggested slices

After the engine-level quantum demos (today): build toward this
architecture in small steps.

### Slice Q1 — Independent quantum subprocesses (separable only). ✅
A `qubit.ys` (or `qubit-register.ys`) composite holding a small
register. Each is a stream subprocess. Multiple instances coexist
holding SEPARATE quantum systems. Show that two independent Bell
states can run in two stream cells simultaneously, deterministically.
**Demonstrates**: separable quantum systems CAN be distributed.
**Status**: shipped as `quantum-two-bells.ys` — two Bell pairs
side-by-side, no link between them, each measures independently.

### Slice Q2 — Classical wire (LOCC).
A classical bridge between two qubit composites carrying measurement
outcomes. One composite measures, sends a bit; the other applies a
conditional gate. **Demonstrates**: LOCC as a chrysalis pattern.

### Slice Q3 — Tensor / merge operation. ✅
The `meta::tensor` HostFn (or method on `QuantumComposite`): take two
composites with compatible quantum schemas, produce one merged
composite with their tensor-product state. **Demonstrates**: the
inverse of `divide` for quantum.
**Status**: shipped as the `meta::tensor` HostFn (cross-product over
bitstring keys, amplitude multiplication). Regression tests in
`tests/quantum_tensor.rs` cover separable-embedding, norm
preservation, and cross-product cardinality. Demo:
`quantum-tensor.ys`.

### Slice Q4 — Auto-merge on cross-composite gate.
When the source AST has `CNOT(alice.q0, bob.q0)`, the compiler
inserts a `tensor(alice, bob)` first. The composites merge; CNOT
applies. **Demonstrates**: quantum coupling = automatic merge.

### Slice Q5 — Auto-divide on factorizability. 🧩
After measurement, check if the composite's qubits factor. If yes,
divide into sub-composites. **Demonstrates**: decomposition reflects
physics.
**Status — substrate**: `meta::factorize(joint_state, k)` shipped as a
HostFn. Algorithm: form the m×n amplitude matrix indexed by (left-bits,
right-bits); check rank 1 by deriving candidate factors from any
nonzero row and verifying every other entry matches `a[i]·b[j]`.
Returns `{separable: bool, a, b}`. Bell and GHZ states correctly
detected as non-separable; `|+⟩⊗|+⟩` and `tensor(a,b)` round-trip back
through `factorize`. Tests in `tests/quantum_factorize.rs`; demo
`quantum-factorize.ys`.
**Status — runtime use**: `quantum-self-observe.ys` shows
`FactorizeCheck` calling `factorize` from inside a process body —
two side-by-side composites (Bell-state vs `|+⟩⊗|+⟩`) self-observe and
report their separability. The composite *knows* whether it's
entangled.
**Landed (2026-05-30)**: `quantum-auto-split.ys` +
`tests/quantum_auto_split.rs` (3 green) — a `factorize`-gated
`AutoSplit` process splits a streaming system *because its state
factorizes* (separable `|++⟩` → two stream children), while an entangled
Bell stays one joint composite. The observation now drives structural
change; the place graph's shape comes to match the entanglement
structure on its own (the §VII duality, realized over the stream
protocol). Each system is its own `--serve-process` child.
**Still ahead**: express it through the schema-driven `_divide` sentinel
+ `tensor_by_schema`/`divide_by_schema` (the full divide↔tensor algebra
duality, #39) rather than an explicit `_remove`/`_add`; and `fold` /
`unfurl` as named algebra ops (`docs/bigraphs-all-the-way-down.md`).

### Slice Q4/Q5 — Structural lifecycle ✅ (data-map flavor)
The user's vision realized: a single demo where the place-graph
mutates as a function of factorizability. The Lab composite holds
`systems :: map[any]`; a single `Lifecycle` process picks the
appropriate structural intent each tick:

- If `ab` (a joint system) is present → SPLIT: `_remove: [ab]` +
  `_add: {a, b}` with the factor states from `meta::factorize`.
- Else if `a` and `b` (separable systems) both exist → MERGE:
  `_remove: [a, b]` + `_add: {ab2}` with the `meta::tensor`
  product state.
- Else → no-op (settled).

Visible at three `--time` snapshots: `--time 0` shows the initial
joint system, `--time 1` shows the split state, `--time 2+` shows
the merged state. Three distinct phases in the place graph,
deterministically. Demo: `packages/quantum/ys/quantum-lifecycle.ys`;
regression: `tests/quantum_lifecycle.rs` (4 tests).

**Caveats / remaining work toward full Q4**:
- ✅ Each system is now a REAL sub-composite (a `QuantumSystem` with its
  own `Publish` + bridge), local OR streamed. The **streaming variant**
  (`quantum-lifecycle-stream.ys`, `map[StreamingQuantumSystem]`) landed
  (2026-05-30): every system runs as its own `chrysalis run
  quantum-system.ys --serve-process` child over Arrow pipes, and the
  split/merge crosses the wire (`tests/quantum_lifecycle_stream.rs`,
  4 green; a `/bin/false` differential + an `strace` `serve-process`
  count confirm the children are real subprocesses, not a local
  fallback). This **answers the open question** — `_add` of a
  freshly-authored `StreamingQuantumSystem[state0: …]` Term lowers to a
  full stream-node spec (`address: stream:…` + config + bridge +
  schema), and engine discovery instantiates the child, exactly as
  `environment.ys`'s `StreamingCell` daughters do. **Remaining
  honesty**: the children aren't yet load-bearing for the *decision* —
  the parent `Lifecycle` reads each system's seeded `state` and drives
  factorize/tensor; the children spawn + republish but don't yet compute
  anything the lifecycle reacts to. Making the children evolve their own
  state (so factorizability is a *consequence* of their computation,
  then moving the split/merge decision INTO the peers) is the bridge to
  the autonomy / mesh direction.
- Two parallel `step`s (Splitter + Merger) race in the BSP cycle and
  produce non-deterministic results; collapsing into one `process`
  with sequential `if/else` fixes this. The Splitter/Merger race is
  the kind of issue that motivates explicit ordering primitives.
- True compile-time Q4 (detect `CNOT(alice.q0, bob.q0)` in source and
  inject `tensor`) is still future work — it requires sibling-composite
  addressing syntax that doesn't exist yet.

### Slice Q6 — Quantum teleportation ✅
The canonical demo: Alice teleports a state to Bob using a
pre-existing entangled pair + 2 classical bits. Exercises everything:
LOCC, measurement-driven branches, classical-conditional gates.
**Status**: shipped as `quantum-teleportation.ys` + regression
`tests/quantum_teleportation.rs`. Five processes — `CnotA1A2`,
`HadamardA1`, `MeasureA1A2`, `ExtractBobState`, `BobCorrect` — chain
through six state slots. Initial state |ψ⟩=0.6|0⟩+0.8|1⟩ on A1,
pre-shared Bell pair on (A2, B). After 6 BSP ticks (the process chain
needs all five stages to propagate), Bob's qubit reconstructs |ψ⟩
exactly within float tolerance, regardless of which `(a1, a2)`
measurement outcome happened — the conditional Pauli correction
(`Z^a1 · X^a2` as nested `if/then/else`) handles all four cases.
Only 2 classical bits cross from Alice's side; the entanglement does
the rest. No-cloning preserved (Alice's measurement destroys her
copy). The canonical quantum protocol, end-to-end through the prism
engine.

By the end: chrysalis runs both classical biology (`hand-built-cell.ys`)
AND quantum protocols (teleportation, GHZ, distributed multi-system
experiments) through the SAME engine, with the SAME composite +
link-graph machinery deciding what's local, what's entangled, what's
classical-channeled.

## X. References

- Bell, *On the Einstein-Podolsky-Rosen Paradox* (1964).
- Aspect et al., experimental Bell tests (1982).
- Greenberger, Horne, Zeilinger, *Going Beyond Bell's Theorem* (1989).
- Bennett et al., *Teleporting an Unknown Quantum State via Dual
  Classical and EPR Channels* (1993).
- Nielsen & Chuang, *Quantum Computation and Quantum Information* —
  canonical textbook for LOCC, teleportation, distillation.
- Coecke & Kissinger, *Picturing Quantum Processes* (2017) — the
  diagrammatic / categorical view; bigraph-adjacent.
- Milner, *The Space and Motion of Communicating Agents* (2009) —
  bigraph foundations.
- `docs/effects-and-handlers.md` §XI — the algebraic-effects framing.
- `docs/exploring-the-computational-unknown.md` — broader landscape.
- `packages/quantum/ys/quantum-*.ys` — the running demos (the `quantum`
  package; #67 decomposition).
