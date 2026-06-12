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
- ⬜ **`Wavetable`** — a `pos` CV scanning a table (a digital complex-osc timbre axis).

### Filters / resonators
- ✅ **`Svf`** — multimode TPT state-variable (joranalogue Filter 8): `cutoff_cv` (V/oct),
  `res_cv`; outputs `lp` `hp` `bp` `notch`; self-oscillates = a resonator. ✅ **`LowPass`** (1-pole, kept).
- ⬜ **`Ladder`** — 4-pole transistor-ladder (the other classic VCF colour).
- ⬜ **`Comb`** — tuned comb / Karplus-Strong basis (`pitch_cv`, `feedback_cv`).

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
- ⬜ **`Chaos`** — a chaotic system (Schlappi Three Body flavour): coupled oscillators / a map.

### Logic / comparators / counters  (Schlappi binary-as-music)
- ✅ **`Compare`** — comparator + analog logic (joranalogue Compare 2 / Schlappi Boundary): `input`
  vs `threshold_cv` → `gate`/`inv`; `min`/`max` of `input` & `b`; `rect` (full-wave). Stateless.
- ⬜ **`Logic`** — AND/OR/XOR/flip-flop over gate Signals.
- ⬜ **`Counter`** — the **Nibbler**: clocked binary accumulator; per-bit gate outs + a stepped
  CV out (D/A); `clock`, `reset`, `up_down` — rhythms + melodies from counting.

### Shapers
- ⬜ **`Fold`** — wavefolder (joranalogue Fold 6 / Serge wave multipliers): `fold_cv`, `bias_cv`.
- ⬜ **`Rectify`** / **`Clip`** / **`Drive`** — waveshapers (West-Coast timbre).
- ⬜ **`RingMod`** — four-quadrant multiplier (`a × b`; a VCA is the two-quadrant case).

### Sequencing / clocks
- ⬜ **`Sequencer`** — step sequencer (joranalogue Step 8): `clock`, `reset`; a `steps` list +
  per-step gate; CV + gate outs.
- ⬜ **`Clock`** / **`ClockDiv`** — master clock + dividers/multipliers (`rate_cv`).
- ⬜ **`Quantizer`** — snap a CV to a scale (`scale`, `input` → quantized pitch CV).

### Mixers / routing / VCAs
- ✅ **`Vca`** — linear VCA (`gain` CV). ⬜ **exp/AB** mode; **`Lpg`** (Buchla/Serge low-pass gate).
- ⬜ **`Mix`** — n-input mixer with per-channel level CV (the additive `Signal` bus is the core);
  **`Matrix`** (joranalogue Morph 4) — a crossfading routing matrix.
- ⬜ **`Pan`** — stereo/quad placement (ties to the ES-9 multichannel fan-out, A8/spatial).

### Time
- ⬜ **`Delay`** — delay line (`time_cv`, `feedback_cv`, `mix`) — the basis of echo/chorus/
  flanger; **large buffer state** (a follow-up needs an efficient buffer-on-state strategy).
- ⬜ **`Reverb`** — FDN/Schroeder over `Delay`s. ⬜ **`Granular`** — grain cloud over a buffer.

## How it composes (the bigger picture)

Because CV ≡ audio and a patch is a bigraph, these modules cross-patch freely
(`examples/cross-mod.ys`: one LFO → VCO pitch *and* filter cutoff). And because the device is a
**sink on the same BSP engine** (`docs/domain-libraries.md` §5), a patch of these modules
**plays** (`chrysalis run … --play`), runs **parallel** voice composites, streams, and
distributes over a **`mesh:`** link — the same modules, on the one engine, toward M4.
