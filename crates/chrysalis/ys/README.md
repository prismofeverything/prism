# Chrysalis `.ys` sources

Source files in the chrysalis surface syntax (extension `.ys`, chrYSalis).
These are **real inputs**: the parser, compiler, and runtime are implemented, so
each file parses to an AST, compiles to prism runtime artifacts, and runs.

> **Writing your own?** See [**GUIDE.md**](GUIDE.md) — a hands-on walkthrough from
> a one-liner to a full workflow (values, `def`, processes/steps, composites,
> imports, contracts, effects).

## Running them

Via the `chrysalis` build tool (from the repo root):

```sh
# run a program (its Output steps write any artifacts)
cargo run -p chrysalis --bin chrysalis -- run     crates/chrysalis/ys/integrator-comparison.ys
# typecheck only (parse + contract / connection checks)
cargo run -p chrysalis --bin chrysalis -- check   crates/chrysalis/ys/integrator-comparison.ys
# emit the process-bigraph document (JSON)
cargo run -p chrysalis --bin chrysalis -- bigraph crates/chrysalis/ys/integrator-comparison.ys
```

`chrysalis` runs programs over its bundled **std library** (`prism-std`:
`core` / `integrators` / `chem` / `io`). Programs importing *non-std* native
packages need the codegen path (`chrysalis compile`, in progress). Every file
here parses and round-trips (guarded by `chrysalis/tests/ys_files_roundtrip.rs`).

## Files

| `.ys` file | what it demonstrates |
|---|---|
| [`integrator-comparison.ys`](integrator-comparison.ys) | **flagship** — process contracts: two integrators `fulfill` `DeterministicMassAction`, so MSE-comparing them is warranted by construction. Self-outputs CSVs + an overlay SVG. Std-only → runs with `chrysalis run`. |
| [`grow-divide-unbounded.ys`](grow-divide-unbounded.ys) | tier-1 grow/divide: a cell grows and divides; units + structural division. |
| [`grow-divide-glucose.ys`](grow-divide-glucose.ys) | resource-limited growth: Monod kinetics on a shared, depleting glucose pool. |
| [`grow_divide.ys`](grow_divide.ys) | grow/divide, compact runnable form. |
| [`nuclear-shuttle.ys`](nuclear-shuttle.ys) | units **contexts**: concentration↔counts across nested compartments (cytoplasm/nucleus). |
| [`mapk.ys`](mapk.ys) | the MAPK signaling cascade as reaction rules (generated from the fixture, round-trip-verified). |
| [`mr.ys`](mr.ys) | Rosen (M,R)-system closure as reaction rules. |
| [`graph.ys`](graph.ys) · [`units.ys`](units.ys) · [`bump.ys`](bump.ys) | small examples: a user `type` with methods, unit declarations, a minimal process. |

## Syntax kernel

A hands-on tutorial is in [GUIDE.md](GUIDE.md); the full design in
[`docs/chrysalis-design.md`](../../../docs/chrysalis-design.md). The kernel is
small:

- **Definers** (lowercase): `def` (values + functions), `type`, `contract`,
  `process`, `step`, `composite`, `reaction`, `pattern`, `unit`, `context`.
  **Controls** (Capitalised) are constructed values (`Cell`, `Rk4`, `MEK`).
- `Name[args]` — term construction · `a | b` — parallel composition (tensor) ·
  `~{port: target}` / `->{port: target}` — input / output port bindings.
- `from <module> import <names>` — pull in native host capabilities (the
  `extern` replacement): a whole process, or functions / types a `process`
  wraps with its own ports + `fulfills` contract.
- `def name = expr` · `def name :: Type = expr` · `def name(args) = body` —
  named values and (first-class) functions.
- `contract C (axis: …)` + `fulfills C[…]` — a process's *meaning*; `::`
  constrains a port to a contract.
- `value.method(args)` — a value-method dispatched on the value's type;
  `f(args)` — a function call.
- `?name`, `?name :: Sort` — pattern variables · `~name` — link variable ·
  `!` — unbound port · `=>` — reaction redex/reactum separator.
- `unit u : [dim] = expr`, `Quantity[unit: u, extensive]` — dimensioned scalars,
  checked at compile and erased before execution.

