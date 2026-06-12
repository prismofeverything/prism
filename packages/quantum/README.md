# quantum — quantum bigraphs as a prism package

The **quantum** domain (#35 algebraic effects/handlers · #36 quantum bigraphs) broken
out of the monolith into a real package (`docs/packages-decomposition.md` §7). It is the
first **pure-`.ys`** domain package: where [`synth`](../synth) depends on a native audio
crate, quantum needs **no native crate** — the whole quantum substrate already rides the
**std** floor.

The tool is built to **explore quantum systems by composing canonical gates**: complex
amplitudes (the shared `Complex` sort), the full single-qubit gate set, and a **circuit**
abstraction (`state.run([h(0), cnot(0,1)])`) — Bell, GHZ, CHSH, and the tensor↔factorize
duality are all `.ys` compositions, not hand-rolled amplitudes. The Rust↔`.ys` boundary:
Rust holds the *primitives* (the gate kernel, `Complex`, `measure`, `factorize`), each
instantly callable from `.ys`; everything composed — circuits, algorithms, examples — is
`.ys` (see `feedback_rust_ys_boundary`).

```sh
chrysalis run packages/quantum/ys/quantum-circuit.ys         --time 0   # Bell/GHZ as gate circuits
chrysalis run packages/quantum/ys/quantum-chsh.ys            --time 0   # Bell inequality violated: S = 2√2
chrysalis run packages/quantum/ys/quantum-teleportation.ys   --time 6   # |ψ⟩ Alice→Bob
chrysalis run packages/quantum/ys/quantum-lifecycle.ys       --time 2   # entanglement split/merge
```

## How it works — a pure-`.ys` package (no codegen)

A package is a `Core` (`types · processes · methods · protocols`). quantum's Core is just
**std**: the quantum primitives are already there —

- **algebraic effects** — `meta::handle` (interference / Bell / GHZ via a handler);
- **measurement** — `meta::sample` (Born-rule collapse);
- **tensor / factorize** — `meta::tensor` / `meta::factorize` (the `divide`↔`tensor` duality);
- the **`Qubits`** type and its `.cnot` / `.tensor` / `.factorize` / `.separable` methods —
  the gate algebra dispatched on the live amplitude state.

So `project.ys` declares only the package's identity; **std is the implicit base every
package shares** (`resolver::resolve`'s `base: std_core()`), exactly as `synth` declares
only its native `audio` edge and lets std ride underneath:

```ys
# project.ys
def package = {
  name: 'quantum',
  version: '0.1.0',
}
```

Because there is no native crate, `chrysalis run packages/quantum/ys/<demo>.ys` runs the
demo **in-process** — no generated runner (that is the native path); the pure `.ys`
resolves against std, and relative `from .ast` / `from .quantum-system` imports resolve
file-locally inside `ys/`.

**`ys/ast.ys`** is a package-local copy of the AST-builder helper library (`var` / `call` /
`float` / …), imported `from .ast` by the interference / effects demos. It is shared
with the monolith's `hand-built-*` cell demos; both keep a copy until the planned
`lang-demos` / `examples` package lands (`docs/packages-decomposition.md` §4), at which
point the demos depend on it as a path dependency.

## The demos

The bigraph reading of each — and the physics — is in
[`docs/quantum-bigraphs.md`](../../docs/quantum-bigraphs.md) and
[`docs/effects-and-handlers.md`](../../docs/effects-and-handlers.md).

| file | what it shows |
|---|---|
| `quantum-gates.ys` | the canonical single-qubit gate set (X/Y/Z/H/S/T/phase/Rx/Ry/Rz) — incl. complex amplitudes |
| `quantum-circuit.ys` | the **circuit abstraction**: Bell/GHZ as gate-list compositions, run in one tick |
| `quantum-chsh.ys` | the **CHSH / Bell inequality** violated — S = 2√2 > 2 (entanglement ≠ classical) |
| `quantum-duality.ys` | `tensor` (join) ↔ `factorize` (split) round-trip, over ℂ |
| `quantum-deutsch-jozsa.ys` | **Deutsch–Jozsa**: constant vs balanced in ONE query |
| `quantum-grover.ys` | **Grover's search**: find the marked item with certainty (1 iteration) |
| `quantum-qft.ys` | the **Quantum Fourier Transform** — genuinely complex amplitudes |
| `quantum-superdense.ys` | **superdense coding**: 2 classical bits via 1 transmitted qubit |
| `quantum-measure.ys` | **measurement with collapse** (`observe`): measuring one qubit collapses its entangled partner |
| `quantum-phase-estimation.ys` | **quantum phase estimation** (core of Shor): reads an eigenphase via controlled-U + inverse-QFT + measurement |
| `quantum-bell-measure.ys` | a Bell state (from gates) measured 4× — always `00`/`11` (perfect correlation) |
| `quantum-interference.ys` | single-qubit interference through a handler (algebraic effects) |
| `quantum-engine.ys` / `quantum-engine-measure.ys` | gates as engine PROCESSES, wired in a place graph (+ a `Measure` step) |
| `quantum-two-bells.ys` | two **separable** Bell pairs side by side (independent systems) — Q1 |
| `quantum-locc.ys` | a classical wire across composites (Local Ops + Classical Comm) — Q2 |
| `quantum-tensor.ys` | `tensor`: the structural-composition inverse of `divide` — Q3 |
| `quantum-factorize.ys` | `factorize`: detect separability; the inverse of `tensor` — Q5 |
| `quantum-cross-cnot.ys` | a coupling gate across composites (tensor-then-apply) — Q4 |
| `quantum-system.ys` | the `QuantumSystem` sub-composite (a `Qubits` register on a bridge) — a library |
| `quantum-self-observe.ys` | a composite that reads its own factorizability |
| `quantum-self-decohere.ys` | a system that decodes its OWN entanglement via `.cnot`, making separability a *consequence* |
| `quantum-auto-split.ys` | physics-driven structural split: a separable system splits, an entangled one stays joint |
| `quantum-lifecycle.ys` | the place graph mutates through split → merge phases by factorizability — Q4/Q5 |
| `quantum-lifecycle-stream.ys` | the same lifecycle with each `QuantumSystem` a `stream:`-served child |
| `quantum-teleportation.ys` | the canonical protocol: |ψ⟩ Alice→Bob via a Bell pair + 2 classical bits — Q6 |
| `effects-handle-demo.ys` | a minimal `meta::handle` algebraic-effects demo |

## Status

Part of the `packages/` decomposition (#35/#36 ⋈ #67) — quantum is the first **pure-`.ys`**
domain package, following `synth` (the native template) toward the **M4** one-engine
`project.ys` (`bio + quantum + synth` colimited into one running engine,
`docs/grand-synthesis.md`). The per-demo regressions live in
`crates/chrysalis/tests/quantum_*.rs` (repointed here) and the run-sweep guard is
`crates/chrysalis/tests/quantum_package_runs.rs`. Remaining quantum *capability* work
(effects slices, Q4/Q5 compile-time forms) is tracked in `coord/quantum.next`.
