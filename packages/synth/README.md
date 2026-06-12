# synth — the audio domain as a prism package

The first prism **domain broken out into a real package** (`docs/packages-decomposition.md`
§7): `synth` is the audio/modular-synthesis theory — oscillators, filters, envelopes, the
block-sized `Signal` sort — packaged so a `.ys` patch links and plays it from the CLI.

```sh
chrysalis run packages/synth/ys/tone.ys           # a 220 Hz sine
chrysalis run packages/synth/ys/stack.ys          # two oscillators summed (a fifth)
chrysalis run packages/synth/ys/writes-synths.ys  # a Composite[…] authoring a 2-osc TYPE
```

Each prints the rendered `Signal` block(s) as JSON. (Hear them live with
`cargo run -p prism-audio --features realtime --example play`.)

## How it works — a native package

The audio DSP, processes, and types live in the native Rust crate
[`crates/prism-audio`](../../crates/prism-audio) (the `Oscillator` / `LowPass` / `Vca` /
`Envelope` kernels + the `Signal` type, assembled into one `Core` by `audio_core()`). This
package depends on that crate **natively**:

```ys
# project.ys
def package = {
  name: 'synth',
  version: '0.1.0',
  dependencies: { audio: { native: '../../crates/prism-audio' } },
}
```

chrysalis is a fixed binary and cannot link a domain crate in-process, so `chrysalis run`
takes the **codegen path** (`#67` Phase 5): it finds this `project.ys`, generates + caches a
small runner crate that links `prism-audio`, calls its `prelude::core()`, and colimits that
native `Core` into the program through `resolver::resolve_with_natives` — *the same resolver
a `.ys` path dependency uses, the native `Core` merely supplied*. A patch then writes
`from audio import Oscillator` and `:: Signal`; the imports are surfaced from the colimited
Core and the `Signal` type rides its type registry.

**Why a native *dependency* (not own-native `native: '.'`):** the dependency shape's runner
calls only `prelude::core()`, and `resolve_native` surfaces the imports — so `prism-audio`
stays **fully chrysalis-free** (its library never depends on the language), needing only a
zero-arg `core()` added. The own-native shape would call `prelude::modules()`, pulling the
language into the domain crate.

## The demos

| file | what it shows |
|---|---|
| `ys/tone.ys` | one `Oscillator` rendered to a `Signal` — the package links + plays |
| `ys/stack.ys` | two oscillators (220 + 330 Hz) summed into one additive `Signal` mix bus — an instrument assembled from native modules |
| `ys/writes-synths.ys` | a `Composite[…]` expression **authors a new 2-oscillator module type inline** and `instantiate` plays it — *the synth writes synths*, as a package |

A synth voice is a **composite** of module processes summed on a shared `Signal` bus, so it
must tick at **block rate** — each demo's authored composite carries `interval = block/rate`
(256/48000); without it the inner engine never advances and the bus stays silent.

## Status

Part of the `packages/` decomposition (#63 ⋈ #67) — synth is the first real domain package,
the template the other domains (`quantum`, `bio`, `spatial`, …) follow toward the M4
one-engine `project.ys`. Sound-quality polish (gain staging, band-limiting) and the
distributed jam (`net:` over the mesh, A8) are tracked in `coord/synth.next`.
