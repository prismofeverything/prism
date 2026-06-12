# synth examples

Rich, runnable patches built from the synth module library
([`docs/synth-module-library.md`](../../../docs/synth-module-library.md)). Each is a
continuously-ticking engine that outputs audio on its `out` bus.

**Render** (prints the output `Signal` as JSON):
```sh
chrysalis run packages/synth/examples/<name>.ys --time 4
```

**Hear it** — today via the Rust runner (all output to `out`):
```sh
cargo run -p prism-audio --features realtime,ys --example play_ys -- packages/synth/examples/<name>.ys out
```
…and, once the `--play` sink lands (synth ⋈ pkg ⋈ lang), straight from the CLI:
`chrysalis run packages/synth/examples/<name>.ys --play`.

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
