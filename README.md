# prism | (chrysalis)

**many theories - one engine**

prism is a Rust runtime for **process bigraph** — Milner's calculus of
communicating, nesting processes expanded with arbitrary type schemas
and temporal/numerical dynamics — built so that wildly different domains
(cell biology, quantum mechanics, audio synthesis, adaptive networks, visual
dynamics) all run on *one* substrate, communicate through a shared common
language, and stand unified under a coherent compositional theory. From it grew
**chrysalis** (`.ys`), a small homoiconic language you write
the simulations in which then become butterflies (and other entities) in running.
It began as a faithful port of the
[vivarium-collective](https://github.com/vivarium-collective) python
stack (`bigraph-schema` / `process-bigraph` / `spatio-flux`); it has grown into a
general language for *composable, multi-scale, distributed* simulation.

→ **New here? Read the [chrysalis primer](docs/chrysalis-primer.md)** — the
executable, always-current introduction to the language (every example in it is
run by a test). Then skim the [docs map](#the-map) below.

---

## What is even happening

A **process bigraph** fuses three computational primitives under one engine:

- **processes** — time-stepped computation: `state + interval → update`, advanced
  on a clock (an ODE integrator, a physics step, growth);
- **steps** — dataflow with no clock: fire when their inputs change (analysis,
  transforms, IO). Wired together, steps form a DAG the engine schedules for you;
- **reactions** — Milner-style parametric rewrites over the place/link graph, run
  by a `BigraphicalReactiveSystem` (deterministic, stochastic, or Gillespie SSA).

These act on a state-tree (`Value::Tree`) that has an exact bigraph reading: keys
are parallel siblings, nesting is the **place graph** (containment), links wire
ports across the **link graph**. Two things make it more than a place/link graph:

1. **Schema is always present and inseparable from state.** Every transformation
   goes through *one closed algebra* — `apply` / `reconcile` / `resolve` /
   `divide` / `merge` / `diff` / … — so the core has real algebraic closure, not
   ad-hoc state munging. (See [`docs/schema-algebra.md`](docs/schema-algebra.md).)

2. **State is code.** When a write carries a `_type`/`address` annotation, the
   engine **discovers** it and lifts that code-shaped state into a *running*
   computation (`discover_processes`). A cell dividing into two live cells, a
   process that emits new reaction rules, a colony spawning workers — higher-order,
   self-modifying simulation falls out of reflection, for free.

The convergence is: because each domain can be joined as different *presentations*
(generators + equations) over the same substrate, **biology, quantum, synthesis,
adaptive networks, and spatial dynamics run on the same engine** — and because the
boundary between any two parts is always the *same* schema-algebra codec, that one
simulation **distributes across a coordinator-free mesh**, with the `.ys` source
**byte-identical** whether it runs in one thread or across machines. The
[**categorical core**](docs/categorical-core.md) is the theory that says why all
of this is one thing: a domain is a presented symmetric-monoidal theory, a
cross-domain coupling is a functor, and `fold`/`unfurl` = entanglement =
metabolism-repair closure = reflection — one structure playing many roles.

---

## it runs (!)

Build the tool once, then run any `.ys` file. (Or prefix with `cargo run -p
chrysalis --bin chrysalis --` instead of installing.)

```sh
cargo install --path crates/chrysalis     # gives you the bare `chrysalis` command
```

```sh
# biology — cells grow on a shared glucose pool, divide when big enough,
# and conserve mass every tick (glucose + Σ cell-mass stays constant)
chrysalis run crates/chrysalis/ys/grow-divide-glucose.ys --time 8

# adaptive networks — coupled oscillators synchronize (order parameter R → 1)
chrysalis run crates/chrysalis/ys/kuramoto.ys --time 10

# self-referential self-reconfiguring M/R-closure synthesizer
chrysalis run packages/synth/examples/mr-colony.ys --time 10
```

Every `process`, `step`, and `composite` is runnable on its own — seed inputs
with `--<port> VALUE`, set duration with `--time`, and the final state prints as
JSON. **The same file runs unchanged in-process, as a separate OS process, on an
HTTP server, or across a mesh** — the invocation chooses, not the source.

→ The whole language, with runnable examples: **[`docs/chrysalis-primer.md`](docs/chrysalis-primer.md)**.

---

## one engine, five domains (+++ and growing)

Each of these is a *specialization of the one substrate*, not a separate
simulator. What runs **today**:

| domain | what runs on the engine now | where |
|---|---|---|
| **Biology** | cells grow on a shared resource & divide (mass-conserving); MAPK signalling; CRN integration; spatial dFBA / diffusion / particles | `crates/chrysalis/ys/`, [`spatio-flux`](crates/spatio-flux/), [`prism-mapk`](crates/prism-mapk/) |
| **Quantum** | Bell · GHZ · teleportation · superdense · Grover · Deutsch–Jozsa · QFT · CHSH; algebraic effects & handlers; LOCC over a mesh link | [`packages/quantum/`](packages/quantum/) (30 `.ys` demos) |
| **Synthesizers** | modular audio — oscillators, filters, envelopes, FM, drums, west-coast / Serge patches; offline render **and** realtime device-out | [`packages/synth/`](packages/synth/), [`prism-audio`](crates/prism-audio/) |
| **Adaptive networks** | Kuramoto oscillators synchronize; the same, distributed over a mesh; self-rewiring (plastic) topology | `crates/chrysalis/ys/kuramoto*.ys`, `../manifold` (sibling) |
| **Spatial** | geometry, packing, the octree (folding in for the 3D/4D "eye") | `../parsimony` (sibling, M5) |
| **Distribution** | a coordinator-free **CRDT mesh**: replicated links converge over a live bridge; N-peer; continuous gossip | [`prism-bigraph/src/protocols`](crates/prism-bigraph/src/protocols/), `coord/` |

The north star — the **[grand synthesis](docs/grand-synthesis.md)** — is to run
all of these *at once*: one continuously-running, mesh-distributed, streamed
simulation in which biological colonies, quantum subsystems, and synthesizers
tick the same engine together. The domains compose onto the mesh almost for free
*because* every boundary is the same codec. (Honest status: the domains run
individually today and the mesh is real; the single all-at-once demo is the work
in progress — tracked in [`docs/NEXT-SESSION.md`](docs/NEXT-SESSION.md).)

---

## chrysalis — the nexus of theories

`.ys` files compile to the prism runtime. chrysalis is a deliberately **thin**
layer: it never reimplements the engine, it *composes* it (`parse → eval →
compile`). The whole language is a small kernel — `process` / `step` /
`composite` definers with `~{in} ->{out}` port interfaces and `|` parallel
composition (wiring *is* categorical composition) — plus `def` values/functions,
`type`/`contract` declarations, first-class `reaction`s and `link`s, native &
file `import`s, and units. It is **homoiconic**: programs are data (`quote` /
`eval`), so a reaction is a value a process can construct and install at runtime.

Don't take this section's word for it — the **[primer](docs/chrysalis-primer.md)**
is executable and can't drift from the language.

---

## Crate organization

Dependency layering is **prism core → chrysalis → packages**:

- **`prism-schema`** — the type system + the closed schema algebra.
- **`prism-bigraph`** — the runtime: `Process`, `Step`,
  `BigraphicalReactiveSystem`, `Engine`, `Composite`, the protocol/mesh layer.
- **`prism-std`** — prism's native **standard library**: reusable processes /
  types / value-methods (`RunProcess`, the ODE integrators, `CRN`, `TimeSeries`)
  that chrysalis exposes to `.ys` as importable modules.
- **`prism-trace`** — delta-log traces (event-sourced state histories;
  `state(t) = fold(apply, initial, deltas[..t])`) + their Arrow-IPC wire codec.
- **`prism-audio`** — the audio / modular-synthesis layer (`Signal` blocks,
  modules, offline render + realtime device boundary).
- **`prism-viz`** — visualization (Graphviz DOT + self-contained SVG).
- **`prism-mapk`** — the MAPK signalling cascade (a reaction-rule example).
- **`prism-derive`** — proc-macros for ergonomic `Process`/`Step` schemas.
- **`chrysalis`** — the surface language (parse → compile → run) **and** the
  `chrysalis` build tool. Bundles `prism-std` as its std prelude.
- **`spatio-flux`** — a downstream domain package: spatial physics (rapier2d) +
  FBA (HiGHS solver), diffusion, particles.
- **`coda`** — a research project *built on prism* (whale-communication: a
  question registry + DSP pipeline + sequence models) — evidence the substrate
  generalizes well past its origin.
- **`packages/quantum`, `packages/synth`** — domains broken out as real `.ys`
  packages (the package ecosystem: a manifest + resolver + registry; a package is
  a `Core`, linking is `Core::merge`). See [`docs/packages-ecosystem.md`](docs/packages-ecosystem.md).

## The port (process-bigraph)

prism faithfully ports the python stack — keeping the *mechanisms* intact (the
schema algebra, `divide`, reaction matching/firing, the composite bridge,
`discover_processes`), not just the surface:

| Python (vivarium-collective) | prism |
|---|---|
| `bigraph-schema` — the type system + algebra | `prism-schema` |
| `process-bigraph` — Process / Step / Composite / Engine | `prism-bigraph` |
| `spatio-flux` — FBA, diffusion, particles | `spatio-flux` |

See [`docs/upstream-alignment.md`](docs/upstream-alignment.md) for what is ported,
extended, and intentionally divergent.

## Build & run

```sh
cargo build --workspace          # build everything
cargo test  --workspace          # run all tests
cargo test  -p prism-bigraph     # one crate

cargo run -p chrysalis --bin chrysalis -- run   <file.ys> [--time T] [--<port> V]
cargo run -p chrysalis --bin chrysalis -- check <file.ys>     # parse + contract/connection checks
cargo run -p chrysalis --bin chrysalis -- repl               # interactive eval
```

> Programs that import *non-std* native packages (spatio-flux, synth, coda) use
> the **codegen path**: chrysalis finds the package's `project.ys`, generates and
> caches a small runner crate linking that native code (`rust-script`-style), then
> runs it. Pure-`.ys` packages (quantum) need nothing extra.

## The map

<a name="the-map"></a>The docs are layered — start at the top and descend:

| read | for |
|---|---|
| **[`docs/chrysalis-primer.md`](docs/chrysalis-primer.md)** | the **language** — executable, canonical, the recommended first read |
| **[`docs/README.md`](docs/README.md)** | the **index** to all 40 design docs, grouped by theme |
| [`docs/prism-architecture.md`](docs/prism-architecture.md) | the substrate: crates, the Process/Step/BRS/Engine primitives, `discover_processes` |
| [`docs/schema-algebra.md`](docs/schema-algebra.md) | the schema layer as a closed algebra (operations, laws, the closure invariant) |
| [`docs/categorical-core.md`](docs/categorical-core.md) | the **spine** — why every domain is one thing (theories, functors, fixpoints) |
| [`docs/grand-synthesis.md`](docs/grand-synthesis.md) | the **roadmap** to the all-at-once demo (the M0→M5 milestones) |
| [`docs/NEXT-SESSION.md`](docs/NEXT-SESSION.md) | the live status / task tracker |

## the project run on and builds itself

prism is built by a team of agents (with continuous human rambling as input)
that **coordinate over the very mesh prism
provides** — each owns a `coord/<role>.ys` heartbeat, merged through a CRDT
`mesh` link into one shared board (`coord/board.ys`), with no central
coordinator. The roster, the roles, and the protocol are in
[`coord/ROSTER.md`](coord/ROSTER.md). The distributed simulation engine and the
process that builds it are the same engine.

That is the whole idea:
create the process of creation
have the process of creation create the process
become creation by creating it

---

*Apache-2.0 · [github.com/prismofeverything/prism](https://github.com/prismofeverything/prism)*
