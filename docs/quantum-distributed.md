# Distributed quantum — the three directions

The quantum domain distributes along three axes, all governed by ONE physics
constraint: **entangled qubits share one joint amplitude in one composite (the
entanglement scope) — they cannot be split across machines without a *quantum
link*** (`quantum-bigraphs.md` §VI). So separable subsystems and classical
channels distribute freely; entanglement needs the quantum link.

This doc is the steward record for the distributed arc (companion to
`quantum-bigraphs.md` §VIII — the substrate gaps — and `grand-synthesis.md` M2).

## Status (2026-06-12)

- **#1 streaming tensor/factor — DONE (already existed).**
  `packages/quantum/ys/quantum-lifecycle-stream.ys` factorizes a joint register
  into two `serve-process` stream children (split) and tensors two children back
  into one (merge) — the divide↔tensor duality, distributed over real subprocesses.
  Complex amplitudes survive the stream codec (`Qubits` serialize/realize is
  identity-with-tag; `complex_qubits_survive_the_stream_codec`). Separable
  subsystems ONLY — respects the entanglement-scope constraint.

- **#2 classical channel over a `mesh:` link — DONE (grand-synthesis M2; the first
  distributed quantum demo).** `packages/quantum/ys/locc-alice.ys` +
  `locc-bob.ys` each declare `link channel :: map[String] mesh`. Alice measures her
  |+> and writes the outcome to HER key; ONE gossip round (the mesh M1 runtime —
  `MeshReplica`/`gossip`/`mesh_links`, reused not cloned) replicates the classical
  String to Bob, who reads it and prepares |bit>. Only the classical bit crosses;
  the quantum stays local. `crates/chrysalis/tests/quantum_locc_mesh.rs` drives the
  gossip (note: it uses `compile_with_core(std_core())` so the `Qubits` methods are
  present — a bare `compile()` core has the type but not the methods).

- **#3 typed QUANTUM link — TO BUILD (the deep one).** Below.

## #3 — the typed quantum link (entanglement across a composite boundary)

The link graph IS the entanglement graph (`quantum-bigraphs.md` §II): two composites
joined by a QUANTUM link share ONE joint amplitude. This is the substrate enabler of
true distributed teleportation/entanglement — the M2 quantum capstone.

**Quantum vs classical link — the type distinction (§VIII-1):**

| | classical (`mesh`) | quantum |
|---|---|---|
| carries | classical data (a String, a CRDT pool) | the JOINT amplitude (the entanglement itself) |
| sharing | replicated per-source, CRDT-merged | ONE shared joint state, single source of truth |
| compile gate | `mesh_safety` (carrier must be a join-semilattice) | NOT mesh-safe (quantum state isn't additive); enforce the entanglement-scope rule instead |
| example | the LOCC classical bit (#2) | the Bell pair Alice + Bob share |

A quantum link is a SHARED link (like the pool `~link`) but **non-CRDT**: the joint
state is single-source, never gossiped/merged (quantum state is not a
join-semilattice — `mesh_safety` would correctly REJECT a `Qubits mesh` link). So a
quantum link is a *distinct link type* that bypasses `mesh_safety` and instead
carries the entanglement-scope discipline.

**Physics to respect:**
- **No-cloning** — the joint state is the single resource, never copied.
- **No-signaling** — local operations transmit nothing; only the #2 classical
  channel carries information.
- **The joint state is ONE object** (the entanglement scope); each party's gates are
  local operations that act on the joint amplitude; measurement by one party
  collapses it for both.

**Slices — start at A next session:**

- **A — in-process quantum link + distributed teleportation, in-process.** Add a
  `quantum` link modifier (beside `mesh`): `link bell :: Qubits quantum = <Bell>`.
  Two composites (Alice, Bob) reference `~bell`, sharing the joint `Qubits`. Alice's
  gates + `observe` update the joint state; the collapse determines Bob's qubit; Bob
  corrects locally. **Reuse the existing shared-link mechanism (`~link`);** the new
  part is the `quantum` type that bypasses `mesh_safety`. This PROVES entanglement
  spanning a composite boundary — no network yet. *(Pair `core`: the link type + the
  engine's shared-joint-state handling.)*
- **B — the compile-time link-type gate.** `validate_connections` distinguishes
  `quantum` vs `mesh`/classical: a `quantum` link's carrier must be a quantum type
  (`Qubits`); a `mesh` link's must be mesh-safe. The §VIII-1 link types, first-class.
- **C — distributed transport (the finale).** The joint state HOSTED at one peer (a
  quantum-state owner); the other applies gates/measurement REMOTELY over the
  rest/stream transport; the measurement collapse propagates back. **Teleportation
  across two machines** — the M2 quantum capstone. Reuse the rest/stream transport
  but for a shared *mutable* quantum state (a "quantum-state server": operations are
  remote messages; the joint state is the single source of truth; no-cloning +
  no-signaling respected). *(Pair `mesh` transport + `core`.)*

**Coordination:** `core` (the link type + the engine entanglement-scope handling),
`mesh` (the distributed transport for the hosted joint state), `unify` (the spine —
the quantum link = the compact cup/cap = `fold`/`unfurl` = entanglement,
`categorical-core.md` §6). Start with Slice A (in-process), then B (the gate), then C
(the distributed capstone).
