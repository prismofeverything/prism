# quantum — quantum bigraphs as a prism package

The **quantum** domain (#35 algebraic effects/handlers · #36 quantum bigraphs) broken
out of the monolith into a real package (`docs/packages-decomposition.md` §7). It is the
first **pure-`.ys`** domain package: where [`synth`](../synth) depends on a native audio
crate, quantum needs **no native crate** — the whole quantum substrate already rides the
**std** floor.

```sh
chrysalis run packages/quantum/ys/quantum-bell-state.ys      --time 0   # (|00⟩+|11⟩)/√2
chrysalis run packages/quantum/ys/quantum-teleportation.ys   --time 4   # |ψ⟩ Alice→Bob
chrysalis run packages/quantum/ys/quantum-ghz.ys             --time 4   # (|000⟩+|111⟩)/√2
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
`float` / …), imported `from .ast` by the bell / interference / effects demos. It is shared
with the monolith's `hand-built-*` cell demos; both keep a copy until the planned
`lang-demos` / `examples` package lands (`docs/packages-decomposition.md` §4), at which
point the demos depend on it as a path dependency.

## The demos

The bigraph reading of each — and the physics — is in
[`docs/quantum-bigraphs.md`](../../docs/quantum-bigraphs.md) and
[`docs/effects-and-handlers.md`](../../docs/effects-and-handlers.md).

| file | what it shows |
|---|---|
| `quantum-bell-state.ys` | the canonical entangled state `(|00⟩+|11⟩)/√2`, built via `handle` |
| `quantum-bell-measure.ys` | Bell state + a Born-rule measurement collapsing it |
| `quantum-interference.ys` | single-qubit interference through a handler |
| `quantum-ghz.ys` | the 3-qubit GHZ state `(|000⟩+|111⟩)/√2` |
| `quantum-engine.ys` / `quantum-engine-measure.ys` | the Bell physics as engine processes (+ a `Measure` step) |
| `quantum-two-bells.ys` | two **separable** Bell pairs side-by-side (no link → independent) — Q1 |
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
