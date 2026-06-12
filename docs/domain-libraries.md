# Domain libraries — each domain a foundational library + rich examples

> **Steward:** `unify`. Refines [`packages-decomposition.md`](packages-decomposition.md) with
> the human's direction (2026-06-11): a domain package is **not a demo dump** — it is a
> **foundational LIBRARY** (generally-useful types / steps / processes / composites people
> build on) **+ rich EXAMPLES** that showcase it. The project matures from demos to a usable
> library ecosystem.

## 1. The shift — library-first, then examples

The first-pass breakout just moves the `chrysalis/ys/` monolith into `packages/<domain>/`.
The direction goes further: **curate each domain into a real library** — the reusable building
blocks a user imports and composes — and *then* write rich examples over it. Each package has
two faces:

- a **LIBRARY** (`lib.ys` + modules) — the domain's curated **generators**: the types, steps,
  processes, and composites that are *generally useful*, documented for reuse. A dependent
  package or an external project imports these.
- **EXAMPLES** (`examples/`) — rich, runnable showcases that compose the library into
  compelling programs (and, for `synth`, *play sounds*). Not imported by dependents.

## 2. The spine — a library IS the theory's generators

A domain is a **presented theory** (`categorical-core.md`); its **generators** are exactly the
reusable primitives. So **a foundational library is the theory's presentation, curated for
reuse** — and an example is a *term* (a composition) over those generators. "Generally useful"
is the generative-core ethos (`generative-core.md`): a **minimal, composable basis**, not a
pile of one-offs. Designing the library = choosing the domain's generators well.

## 3. Package structure

```
packages/<domain>/
  project.ys        # name, version, deps (+ native:, + sink: — §5)
  lib.ys            # THE LIBRARY surface — re-exports the generators a dependent imports
  ys/               # library modules (the types / steps / processes / composites)
  examples/         # rich runnable showcases (`chrysalis run examples/<x>.ys [--play]`)
  README.md
```

## 4. The library per domain (generators to curate — suggestive, the domain agent decides)

- **synth** — the `Signal` type; `Oscillator` / `LowPass` / `Vca` / `Envelope`; composites
  `Voice` / `Stack` / `Instrument`; the `play` sink (§5). Examples: `tone` / `live` /
  `writes-synths` / a sequenced piece.
- **quantum** — a `Qubit` type; gates (`H` / `CNOT` / …); `measure` / `entangle` / `teleport`
  composites. Examples: bell / ghz / teleportation.
- **bio** — `Cell` / `grow` / `divide` / `environment`; the CRN / reaction generators.
  Examples: grow-divide / mapk / a colony.
- **manifold** — an `Oscillator` / `Kuramoto` tile; the plastic-weight link; the `Complex`
  sort. Examples: kuramoto sync / mesh / plastic.
- **spatial** — geometry generators (`Space` / `Transform` / `Mesh` / `Field`); a `Render` sink.

## 5. The output boundary — a `sink:` (the play mechanism)

A library that produces output to the **world** (an audio device; later a display, a file, a
network stream) declares a **`sink:`** in `project.ys`. Then `chrysalis run <x>.ys --play`
(through the codegen runner, which already links the native crate) drives the engine to the
domain's sink fn instead of rendering offline JSON. **A sink is the domain's OUTPUT functor**
— the dual of an input source (`categorical-core.md` §4) — and it composes by construction
(the device is a sink on the same BSP engine that mesh / streaming / parallel already drive).

*Status (lang ⋈ pkg ⋈ synth, 2026-06-11): **BUILT + verified.*** `chrysalis run <x>.ys --play`
drives a sink-declaring package to its device through the codegen runner. What landed:
- the **`sink:` manifest field** (`sink: 'audio'`, `manifest.rs`) — names the part (a native-dep
  edge) whose device receives the engine;
- the **codegen PLAY runner** (`codegen.rs` `run_play` / `render_play_runner`) — a SEPARATE
  `<name>-play` runner cache that resolves the package Core, BUILDS the engine via
  `chrysalis::runner::build_engine`, and drives it to the sink crate's **`prelude::run_realtime(
  engine, seconds)`** convention; the sink crate compiled `--features realtime`;
- the **`--play` flag** routes through the existing `run` dispatch (`run_play`) — no new verb;
- the convention `prelude::run_realtime(engine, seconds)` (prism-audio) takes a *built* engine, so
  the PLAY path needs only `realtime`, **not `ys`** — prism-audio stays chrysalis-free even for play.

Verified e2e: `chrysalis run packages/synth/ys/live.ys --play --time 1` builds the play runner +
drives `live.ys` to the device (exit 0). Generalizes past audio: the same `sink:` mechanism is how
`spatial` renders to a display, etc. *(Follow-up: the OWN-native sink — a `native: '.'` package
whose own crate is the sink — synth is dep-shape so it isn't exercised yet.)*

## 6. Owners
- **domain agents** — curate the library (the generators) + write the rich examples.
- **pkg ⋈ lang** — the `sink:` mechanism (`--play` dispatch through the codegen runner).
- **unify** (steward) — this vision + the spine (library = generators; sink = output functor).
