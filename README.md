# prism

A Rust workspace implementing Milner-style **process bigraphs** for composable,
multi-scale simulation — a port of the vivarium-collective Python stack
(`bigraph-schema` / `process-bigraph` / `spatio-flux`) — with **chrysalis**, a
surface language that compiles to the runtime, layered on top. Cellular biology
is the driving application domain.

## What it is

A *process bigraph* unifies three computational primitives under one runtime, so
multi-scale phenomena compose without bespoke glue between simulators:

- **Processes** — time-stepped computations (an ODE integrator, a physics step):
  `state + interval → update`, run on an `interval()`.
- **Steps** — dependency-triggered dataflow (no clock): fire when their inputs
  change.
- **Bigraphical reaction rules** — Milner-style parametric rewrites over the
  place/link graph, run by a `BigraphicalReactiveSystem` (deterministic,
  stochastic, or Gillespie SSA).

State is a tree-of-maps (`Value::Tree`) with an exact bigraph reading: keys are
parallel siblings, nesting is the place graph, links wire ports. **Schema is
always present and inseparable from state** — every transformation goes through
one closed algebra (`apply` / `reconcile` / `resolve` / `divide` / …), so the
core has real algebraic closure instead of ad-hoc state munging.

The **Engine** schedules processes, triggers steps, fires reaction rules, and —
the load-bearing trick — **discovers new processes from state at runtime**: when
a write contains a `_type` / `address` annotation, the engine lifts that
code-shaped state into a running computation. State *is* code; this reflection
(`discover_processes`) is what makes higher-order, self-modifying simulation —
a cell dividing into two running cells, a process emitting new reaction rules —
fall out for free.

## The port

prism faithfully ports the Python stack — keeping the *mechanisms* intact (the
schema algebra, `divide`, reaction matching/firing, the composite bridge,
`discover_processes`), not just the surface:

| Python (vivarium-collective) | prism |
|---|---|
| `bigraph-schema` — the type system + algebra | `prism-schema` |
| `process-bigraph` — Process / Step / Composite / Engine | `prism-bigraph` |
| `spatio-flux` — FBA, diffusion, particles | `spatio-flux` |

See [`docs/prism-architecture.md`](docs/prism-architecture.md) and
[`docs/schema-algebra.md`](docs/schema-algebra.md).

## chrysalis — the surface language

**chrysalis** (`.ys` files) compiles to the prism runtime. It is a *thin* layer:
it never reimplements the runtime, it composes it.

- **`process` / `step` / `composite`** definers with `~{in} ->{out}` port
  interfaces and `|` parallel composition (wiring is categorical composition).
- **Native imports** — `from <module> import <names>` pulls in host
  capabilities: a whole process (`from core import RunProcess`, used as-is) or
  functions / types (`from integrators import rk4`, `from chem import CRN`) that
  a `.ys` `process` wraps with its own ports + contract. (Replaces `extern`.)
- **`def`** — first-class values and functions: `def network :: CRN = {…}`,
  `def double(x) = x * 2` (functions are values — pass / return / store them).
- **Process contracts** — `contract` + `fulfills` give a process a *meaning*
  (which mathematical object it approximates), so two processes are
  substitutable only when they share a contract; an illegitimate comparison does
  not compile. See [`docs/process-contracts.md`](docs/process-contracts.md).
- **Units** — dimensioned quantities, checked once at compile and erased to raw
  `f64` (no per-op overhead).

Design: [`docs/chrysalis-design.md`](docs/chrysalis-design.md). Examples:
[`crates/chrysalis/ys/`](crates/chrysalis/ys/). Editor support:
[`crates/chrysalis/emacs/`](crates/chrysalis/emacs/).

## Crate layout

Dependency layering is **prism core → chrysalis → spatio-flux**:

- **`prism-schema`** — the type system + the closed schema algebra.
- **`prism-bigraph`** — the runtime: `Process`, `Step`,
  `BigraphicalReactiveSystem`, `Engine`, `Composite`.
- **`prism-std`** — prism's native **standard library**: reusable
  processes / types / value-methods (`RunProcess`, the mass-action ODE
  integrators, `CRN`, `TimeSeries`) that chrysalis exposes as importable modules.
- **`prism-viz`** — visualization (Graphviz DOT + self-contained SVG / plotters).
- **`prism-mapk`** — the MAPK signaling cascade (reaction-rule example).
- **`prism-derive`** — proc macros for ergonomic schema declarations.
- **`chrysalis`** — the surface language (parse → compile → run) and the
  `chrysalis` build tool. Bundles `prism-std` as its std prelude.
- **`spatio-flux`** — a downstream demo package: spatial physics (rapier2d) +
  FBA (HiGHS solver), diffusion, particles.

## Build & run

```sh
cargo build --workspace          # build everything
cargo test  --workspace          # run all tests
cargo test  -p prism-bigraph     # one crate
```

### Running `.ys` programs with the `chrysalis` tool

The `chrysalis` build tool runs `.ys` source over the bundled std library:

```sh
# parse → compile → run; the program's Output steps write any artifacts
cargo run -p chrysalis --bin chrysalis -- run     crates/chrysalis/ys/integrator-comparison.ys

# typecheck only (parse + contract / connection checks)
cargo run -p chrysalis --bin chrysalis -- check   crates/chrysalis/ys/integrator-comparison.ys

# emit the process-bigraph document as JSON (compile, no run)
cargo run -p chrysalis --bin chrysalis -- bigraph crates/chrysalis/ys/integrator-comparison.ys
```

Install it once for the bare command:

```sh
cargo install --path crates/chrysalis
chrysalis run crates/chrysalis/ys/integrator-comparison.ys
```

The flagship example — the **process-contract integrator comparison** — runs two
ODE integrators (RK4 and forward Euler) that `fulfill` the same
`DeterministicMassAction` contract, then MSEs and plots their trajectories. The
workflow writes its own artifacts:

```sh
chrysalis run crates/chrysalis/ys/integrator-comparison.ys
ls outputs/integrator-comparison/    # ForwardEuler.csv  Rk4.csv  mse.csv  overlay.svg
```

> Programs that import *non-std* native packages (e.g. spatio-flux's) need the
> codegen path (`chrysalis compile`, in progress): chrysalis generates and
> compiles a small runner crate linking those packages — `rust-script`-style —
> then caches and runs it.

## Docs

- [`docs/prism-architecture.md`](docs/prism-architecture.md) — crates, the
  Process / Step / BRS / Engine primitives, `discover_processes`.
- [`docs/schema-algebra.md`](docs/schema-algebra.md) — the schema layer as a
  closed algebra (the operations, their laws, the closure invariant).
- [`docs/chrysalis-design.md`](docs/chrysalis-design.md) — the chrysalis
  language design.
- [`docs/process-contracts.md`](docs/process-contracts.md) — process contracts:
  the SED-ML / KISAO reproducibility critique and the typed alternative.
