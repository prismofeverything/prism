# synth examples

Rich, runnable patches built from the synth module library
([`docs/synth-module-library.md`](../../../docs/synth-module-library.md)). Each is a
continuously-ticking engine that outputs audio on its `out` bus — and each has an
**`AudioOut` sink wired in**, so it **plays straight from the CLI**:

```sh
chrysalis run packages/synth/examples/<name>.ys --time 4
```

No `--play` flag — *the sink is in the graph* (`docs/web-bigraphs.md` §9): `AudioOut` taps
the `out` bus, drives the device, and its back-pressure paces the engine, so `--time T`
plays **T real seconds**. (On a machine with no audio device, `AudioOut` is a silent
passthrough and the same command just renders the `out` Signal as JSON — render and play
are one patch, one command.)

| example | what it shows | modules |
|---|---|---|
| **`cross-mod`** | one LFO modulates VCO pitch **and** filter cutoff — the universal Signal | Oscillator, Svf |
| **`generative`** | self-playing: a clock samples a wandering slope → stepped pitch + a per-beat pluck | Slope ×3, SampleHold, Oscillator, Svf |
| **`sequence`** | an 8-step melody; the envelope drives a **wavefolder** per note (West Coast) | Sequencer, Slope, Oscillator, Fold, Svf |
| **`acid`** | a 303 bassline — a high-resonance filter **swept by the envelope** (the squelch) | Sequencer, Slope, Oscillator, Svf |
| **`drums`** | the **Nibbler's bits as rhythm** — kick on `b0`, noise hat on `b1`, summed on one bus | Counter, Slope, Oscillator, Noise, RingMod |
| **`krell`** | a **self-generating** Buchla/Serge patch — feeds back through S&H to never repeat | Slope, SampleHold, Noise, Oscillator, Svf |
| **`fm-bell`** | **FM synthesis** — the modulator's level (the FM index) is shaped by an envelope | Oscillator ×2, Slope |

Two recurring tricks (both fall out of CV ≡ audio): a voice shapes its **own** amplitude
through the oscillator's `am` input (no VCA needed), and one envelope does **double duty**
(level + timbre, e.g. driving a filter cutoff or a wavefolder at once).

Simpler primitives live in [`../ys/`](../ys) — `tone` (one oscillator), `stack` (two
summed), `live` (a continuous engine), `writes-synths` (a `Composite[…]` authoring a module
type inline).
