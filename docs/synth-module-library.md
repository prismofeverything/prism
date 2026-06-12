# The synth module library — a patch-programmable Eurorack in prism

> **Owner:** `synth`. The audio domain's curated **generators** (`docs/domain-libraries.md`):
> a library of modular-synthesis modules, designed — after **joranalogue**, **Serge**, and
> **Schlappi Engineering** — to be *maximally patch-programmable*. Examples (`packages/synth/`)
> are terms over these generators.

## The founding law — ONE Signal, modulate everything

There is a single signal language: **`Signal`** (a block of `f32`). **A control voltage and
an audio sample are the same thing** (CV ≡ audio), so *anything can modulate anything*. From
this one law everything follows:

1. **Every parameter is a modulation input** — a `Signal` port, not frozen config. A module
   exposes a *plethora* of CV inputs (the joranalogue/Serge ethos: bring every parameter to a
   jack). The per-parameter shape is **`base (knob) + Σ cv·depth (attenuverted CV in)`**.
2. **Unpatched = silence = no modulation.** An absent input reads as `0`
   ([`modulation::at`]), so `base + cv·depth` falls back to the base — a jack normalled to 0.
   Rich inputs therefore cost nothing until patched.
3. **Multiple simultaneous outputs.** A module presents every useful tap at once (joranalogue
   Generate's waveshapes, Schlappi Three Body's 14 outs, the SVF's four modes) — not one
   selected output.
4. **DC-coupled, bipolar `[-1, 1]`-ish.** CV and audio interchange. Pitch CV is **exponential,
   1.0 = +1 octave** (V/oct); linear/through-zero FM adds in Hz; level/offset CV adds linearly;
   gates/triggers/sync are rising edges through a threshold.
5. **Patch-programmable + self-patching.** A module's function depends on how it is patched
   (Serge: the DUSG is an env *or* LFO *or* VCO *or* slew depending on the patch); feedback and
   self-patching are first-class (Schlappi). prism makes this literal — a patch is a bigraph,
   a cable is a link, and an output may feed any input including its own module's.

The influences, concretely: **joranalogue** — entirely analog, every parameter modulatable at
audio rate, multi-output. **Serge** — *patch-programmable*; universal multi-function modules
(DUSG, SSG) reinvented by the performer. **Schlappi** — cross-modulation, chaos, feedback, and
"binary is inherently musical" (Nibbler).

## The implementation pattern (a kernel)

A module is a `Process` whose DSP math is native Rust (chrysalis-free; `feedback_chrysalis_thin_layer`):

- **state** (phase, filter memory, …) lives on engine state slots (`Schema::overwrite`), read +
  written each tick — a `Process` owns its own memory;
- **modulation inputs** are read with [`modulation::cv_in`] (silence if unpatched) and applied
  per sample with [`modulation::at`] — `base + at(&cv, i)·depth`;
- **`from_config`** maps the historical knob names to the *base*, plus `*_depth` attenuverters;
- **one block per tick** (`interval = block / sample_rate`), so logical time = audio time.

`.ys` surface conventions: port keys are plain identifiers — **`input` not `in`** (`in` is a
reserved word in `.ys`; flagged to `lang` for a contextual-keyword fix). Outputs are named taps
(`sine`/`saw`/…, `lp`/`hp`/…); a legacy `out` aliases the most-common tap so single-output
patches are unchanged.

## The catalog — families (ModularGrid taxonomy)

✅ = built · 🚧 = in progress · ⬜ = roadmap. Each ⬜ is "a kernel + `from_config` + register +
tests + an export", following the pattern above. Modulation inputs listed are the *minimum*.

### Sound sources
- ✅ **`Oscillator`** — complex VCO (joranalogue Generate / Schlappi Three Body). Inputs:
  `fm_exp` (V/oct), `fm_lin` (through-zero Hz), `pm`, `sync` (hard sync), `pwm`, `am`. Outputs:
  `sine` `saw` `square` `triangle` `sub` (all at once).
- ✅ **`Noise`** — white, a seeded `xorshift` (deterministic, state on a slot); `am` CV. → a
  `SampleHold` with a clock = random voltages. ⬜ pink/`rate`-reduced variants.
- ✅ **`Wavetable`** — morphs through sine→triangle→saw→pulse by a `pos_cv` (`fm_exp`/`am`); the digital complex-osc timbre axis.

### Filters / resonators
- ✅ **`Svf`** — multimode TPT state-variable (joranalogue Filter 8): `cutoff_cv` (V/oct),
  `res_cv`; outputs `lp` `hp` `bp` `notch`; self-oscillates = a resonator. ✅ **`LowPass`** (1-pole, kept).
- ✅ **`Ladder`** — 4-pole **Moog transistor-ladder** (`cutoff_cv` V/oct + `res_cv`; `tanh` drive
  for the Moog saturation; `out` = 4-pole, `lp2` = a 2-pole tap) — the warm/resonant color beside the SVF.
- ✅ **`Comb`** — tuned feedback comb = a **Karplus-Strong string**: excite `input` with a burst, it rings at `pitch_cv` with `feedback_cv`/`damping`; interpolated read. (buffer on the struct)

### Function generators / envelopes / LFOs  (the Serge universal slope)
- ✅ **`Slope`** — the **DUSG / Maths / Contour** core: a single rise→fall slope with `rise_cv`,
  `fall_cv`, `time_cv`, `trigger`, `cycle` (→ LFO/VCO when cycling); outputs `out`, `inv`, `eoc`.
  THE patch-programmable keystone (env · LFO · VCO from one module — `trigger`+no-cycle = AD env,
  `cycle` = LFO, `time_cv` at V/oct = pitch-tracking VCO).
- ✅ **`Envelope`** (AD, kept) — a `Slope` specialisation.

### Modulation utilities / random / slew
- ✅ **`SampleHold`** — sample `input` on a `trigger` edge (`stepped`) AND continuously slew toward
  it (`smooth`) — the Serge **SSG** in one. Noise → `input` + a clock → `trigger` = random voltages;
  a stepped CV → `input`, no trigger = a slew limiter / glide. (Subsumes a standalone `Slew`.)
- ✅ **`Chaos`** — a **Lorenz strange attractor** (Schlappi Three Body flavour): `x`/`y`/`z` orbit forever without repeating; `rate` (× `rate_cv`) from slow CV wander to audio-rate drone.

### Logic / comparators / counters  (Schlappi binary-as-music)
- ✅ **`Compare`** — comparator + analog logic (joranalogue Compare 2 / Schlappi Boundary): `input`
  vs `threshold_cv` → `gate`/`inv`; `min`/`max` of `input` & `b`; `rect` (full-wave). Stateless.
- ✅ **`Counter`** — the **Nibbler**: clocked 4-bit binary accumulator; `clock`/`reset`; outputs the
  per-bit gates `b0`..`b3` (clock ÷2/÷4/÷8/÷16 — an instant rhythm section) + a stepped `cv` (D/A).
- ✅ **`Logic`** — AND/OR/XOR/NAND over two gate Signals + a T-flip-flop (`flip` = ÷2 on `a`); the analog-logic glue for polyrhythms / clock division.

### Shapers
- ✅ **`Fold`** — wavefolder (joranalogue Fold 6 / Serge wave multipliers / Buchla): `fold_cv` drive
  + `bias_cv` (asymmetry → even harmonics); analytic triangle fold. *Drive `fold_cv` with an env =
  West-Coast timbre.*
- ✅ **`RingMod`** — four-quadrant multiplier (`a · b`; a VCA is the two-quadrant case).
- ✅ **`Shaper`** — `rect` (full-wave) · `clip` (hard) · `drive` (tanh soft-clip) at once, `drive_cv` into them — the saturation family (`Fold` is the wrapping one).

### Sequencing / clocks
- ✅ **`Sequencer`** — step sequencer (joranalogue Step 8): `clock`/`reset` step a `.ys` `steps`
  list → a held `cv` (a melody/staircase) + a `trig` pulse per step. The sequence is DATA (a
  reaction could rewrite it live — the homoiconic angle).
- ✅ **`ClockDiv`** — divide a `clock` by an arbitrary `div` (triplets/5s/polymeter; `Counter`'s bits do power-of-2); `trig`+`gate`+`reset`.
- ✅ **`Quantizer`** — snap a pitch CV to a `.ys` `scale` (nearest degree, octave-aware) → in-key
  melodies; emits a `gate` pulse on each note change (→ an envelope `trigger`).

### Mixers / routing / VCAs
- ✅ **`Vca`** — linear VCA (`gain` CV). ⬜ exp/AB mode.
- ✅ **`Lpg`** — Buchla/Serge **low-pass gate**: a `ping` lights a modeled vactrol that opens the
  filter AND the VCA together and decays naturally (louder = brighter — the West-Coast pluck, no env).
- ✅ **`Mix`** — 4-channel mixer with a CV level per channel (a VCA on each input, then a sum).
- ✅ **`Matrix`** — a 4-way morphing crossfader (joranalogue Morph 4): `morph_cv` (0…3) blends `in1`..`in4`.
- ✅ **`Pan`** — equal-power stereo placement (`pan_cv` → `left`/`right`); autopan with an LFO/Chaos. (full stereo *playback* = a later AudioOut channel extension)

### Time
- ✅ **`Delay`** — delay line: `time_cv` (modulate for chorus/flanger), `feedback_cv`, `mix_cv`;
  echo with feedback, and the basis of reverb. The buffer lives ON THE STRUCT (a `Mutex`, like
  `AudioOut`'s ring) — no clone-per-tick; fractional read position (interpolated) for smooth time CV.
- ⬜ **`Reverb`** — FDN/Schroeder over `Delay`s. ⬜ **`Granular`** — grain cloud over a buffer.

### World boundary — sinks & sources (the device as a graph element)
- ✅ **`AudioOut`** — the output device as a **SINK IN THE GRAPH** (the world-boundary face,
  `docs/web-bigraphs.md` §9). Wire a `Signal` into its `input`; it writes each block to the device,
  and the device's real-time drain **back-pressures the engine** — so `chrysalis run patch.ys`
  *plays* because the sink is wired in, **not** because of a `--play` flag (retired). Passes through
  (`out = input`) so the same patch renders offline AND plays under `--features realtime`; no device
  ⇒ silent passthrough. (Impl: the `!Send` `cpal::Stream` lives on a spawned audio thread, `AudioOut`
  holds the Send ring producer — the ring is the boundary.)
- ✅ **`AudioIn`** — the input device as a SOURCE (mic/line/duplex), the dual of `AudioOut`: cpal callback fills a ring (its thread), `update` drains a block. Realtime; a silence stub otherwise. ⬜ a `web:` served face (a
  bigraph as a live page) is the *other* world boundary (`mesh` owns it).

## How it composes (the bigger picture)

Because CV ≡ audio and a patch is a bigraph, these modules cross-patch freely
(`examples/cross-mod.ys`: one LFO → VCO pitch *and* filter cutoff). And the device is just another
**boundary element in the graph** — an `AudioOut` sink whose back-pressure paces the engine
(`docs/web-bigraphs.md` §9; *"if the boundary is wired in, it's live"*). So one patch of these
modules **plays** (wire in `AudioOut`), **renders** (read its passthrough), runs **parallel** voice
composites, streams, and distributes over a **`mesh:`** link — the same modules + the same engine,
toward M4. (`--play` is retired: a wired-in sink IS the play.)
