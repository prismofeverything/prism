# Synthesis bigraphs

A design document for a **generative modular synthesizer** on the prism /
chrysalis runtime. The thesis, in one line:

> **A synthesizer patch IS a bigraph** — modules are nodes with signal
> ports, patch cables are links, a voice is a composite — so *generative*
> synthesis is just a **BRS rewriting that bigraph**: rules that add,
> remove, and re-patch modules to yield new, related synthesizers.

This is the **audio twin of [`quantum-bigraphs.md`](quantum-bigraphs.md)**.
There, gates are link-rewrites, entanglement forces a joint composite, and
`tensor`/`divide`/`measure` merge and split composites. Here, patch cables
are links, a shared modulation bus forces a joint composite, and the *same*
`fold`/`unfurl`/`divide` machinery merges and splits voices. Both run on
the one engine, the one composite + link-graph machinery, the one schema
algebra. The synthesizer is chosen because it makes the bigraph rewriting
**audible** — you can *hear* a reaction fire.

We are **not** recreating VCV Rack. The goal is not a vast static module
catalog at sample-perfect latency; it is a **flexible generative system**
where the patch itself is a first-class, rewritable value: you can grow it,
fork it, distribute it, and let a process author new modules
homoiconically. We trade some raw DSP throughput for the structural freedom
that the rest of prism already gives us for free.

The realtime audio + network layer is **[gorgon](../../gorgon)** (the
user's P2P audio/CV messenger over Tailscale), folded in as a library —
see §IV. gorgon already owns the hard parts: the cpal device boundary, raw
bit-exact PCM over UDP, OSC control voltage, jitter buffering. We reuse
them rather than rebuild them.

---

## I. The mapping (synthesizer ↔ bigraph)

Read a modular synth and a bigraph side by side and they are the same
object:

| Synth concept | Bigraph / prism concept | prism mechanism |
|---|---|---|
| **module** (oscillator, filter, VCA, …) | an ion / node with typed ports | a `process` (a-rate) or `step` (k-rate) with `~{} ->{}` signal ports |
| **module with sub-modules** (a voice) | a node containing a sub-bigraph | a `composite` |
| **patch cable** | a link (a wire between ports) | a port binding `~{in: ~w}` / a relative-path wire (`%`, `^`) / a bridge |
| **a patch** | the whole bigraph | the state tree (place graph: module nesting) + the wiring (link graph: cables) |
| **a signal on a cable** | a value flowing on a link, per tick | a `Signal` value (a block of samples) — §III |
| **a mix bus** (many cables → one input) | many links into one port | additive `apply` at the BSP barrier sums them — §III |
| **patching / re-patching** | rewriting the link graph | a **BRS** (`BigraphicalReactiveSystem`) — §VI |
| **adding/removing a module live** | adding/removing a node | a reactum's `_add`/`_remove` → `discover_processes` — §VI/VII |
| **a process that builds modules** | a reaction whose reactum authors a node | homoiconic `ProcessDef`/spec values — §VII |
| **merging/splitting voices** | `fold`/`unfurl`, `tensor`/`divide` | the composite-level algebra — §VIII |
| **two synths jamming across machines** | composites joined by *outer* links | a distributed protocol over gorgon — §IX |

Everything in the right column already exists or is on the roadmap for
*other* reasons (cells, quantum, distribution). The synthesizer is a new
**consumer** of that substrate, not new substrate — exactly the discipline
in `CLAUDE.md` ("design *on top* of prism, not around it"; "build the
consumer so we know it works").

The two place graphs of
[`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md) appear
immediately: the state tree *inside* a voice (oscillators wired to a filter
wired to a VCA) and the voices *within* a rack. One self-similar bigraph;
one BRS can rewrite both levels.

---

## II. The one hard problem: logical time vs hard realtime

This is the only genuinely new engineering problem, so it comes first.

**prism's engine is purely logical-time.** The tick is a BSP superstep —
invoke (every due process runs against one immutable snapshot) → advance
logical `time` by the smallest interval → apply (reconcile all updates at a
barrier) → trigger steps + discover processes
([`execution-model.md`](execution-model.md)). There is **no wall-clock, no
`sleep`, no `Instant`** anywhere in the loop (confirmed:
`engine.rs:699–852`). `time` is a float accumulator; `interval()` is read
fresh each tick (so rates are data-driven); the run loop goes as fast as it
can.

**Audio is hard-realtime block streaming.** A device callback must be fed a
buffer of N samples every few milliseconds, from a high-priority thread,
with no allocation or locking. At 48 kHz a 64-sample block is ~1.33 ms.

The two reconcile through **three** moves:

### 1. The tick is one audio block (block-rate scheduling)

Make **one prism tick = one audio block of N samples.** A `Signal` value on
a wire is not one sample — it is a *block* (a `Vec<f32>` of length N). Each
tick, an oscillator fills N samples; a filter processes N samples; the BSP
barrier hands the assembled block to the device. This is the classic
control-rate / audio-rate split (Csound's k-rate/a-rate, SuperCollider's
block size, Pd/Max's signal blocks):

- **a-rate** modules (oscillators, filters) fill a whole `Signal` block per
  tick, sample-by-sample, in their `update` body (via a native DSP kernel —
  §III).
- **k-rate / control** modules (LFOs, envelopes, sequencers) emit *one*
  value per tick (per block), or run on a **longer interval** entirely —
  and prism's DES `fronts` already give heterogeneous per-process rates for
  free (a sequencer ticking every 16 blocks is just `interval()` = 16
  blocks of logical time).

Block size trades latency for cost: 64 samples → ~750 ticks/s (tight
timing, more overhead); 512 → ~94 ticks/s (loose, cheaper). It is a config
knob, not a law.

### 2. The device boundary is a lock-free ring (gorgon's seam), pulled

The cpal callback runs on the realtime thread and **only** drains/fills a
lock-free `ringbuf` — gorgon already implements exactly this
(`audio.rs`: `build_output_stream<C: Consumer<f32>>` does `cons.pop_slice`,
fills silence on underrun; `build_input_stream<P: Producer<f32>>` does
`prod.push_slice`). The prism engine runs in a **normal-priority driver
thread** and tops up the output ring with blocks, running *ahead* of the
device by the ring depth (gorgon's ring is ~170 ms; jitter buffer ~40 ms).

The model is **pull / back-pressure**: the device consumes from the ring at
its rate; the driver ticks the engine to produce another block whenever the
ring has room; when the ring is full the driver yields (the device paces
the engine). A new run mode — `run_realtime(device)` — distinct from the
free-running `run(duration)`. The audio thread and the engine thread never
share state except through the ring: **the ring is the seam between
wall-clock and logical time.**

```
[cpal RT thread]  pop_slice ← ┌─────────┐ ← push_block  [prism driver thread]
  device 48kHz                │ ringbuf │                  engine.tick():
  (hard deadline,            │ (lock-  │                   invoke‖ → apply →
   no alloc/lock)             │  free)  │                   one Signal block
  underrun → silence         └─────────┘                  back-pressured by ring
```

If the engine falls behind, the callback fills silence — **graceful
degradation, not a correctness violation.** BSP snapshot consistency holds
*within* each block regardless.

### 3. Offline render needs none of this

Because the engine is logical-time, the **first** target (per the chosen
plan) is *offline*: free-run `run(duration)`, capture the output `Signal`
trace via the existing `--trace`/`--out` delta-log machinery
([`delta-traces.md`](delta-traces.md)), and a tiny WAV writer turns that
trace into a `.wav`. **No cpal, no wall-clock, no hardware, fully
deterministic, runs under `cargo test`.** This de-risks the entire
signal-graph + module-library + reactions story before we ever touch the
realtime boundary. The realtime device (move 2) is then a *second*,
additive step — the same patch, a different sink.

### Honest cost

prism's per-tick work (snapshot, `reconcile`, `Value` cloning) is heavier
than a hand-rolled DSP graph's flat buffer arithmetic. So: **modest patch
sizes and larger blocks first**; the zero-copy `Signal` representation
(`Foreign(Arc<Vec<f32>>)`, §III) to avoid per-block clones on pass-through
wires; and the `tick_lifecycle` vectorization (`execution-model.md` Form B,
#20) for big homogeneous voice banks later. We will not match VCV Rack's
module count at 64-sample latency — and we do not need to. The payoff is
that the patch is a *rewritable bigraph*, which VCV Rack's patch is not.

---

## III. The `Signal` type (inside the schema algebra)

Per `CLAUDE.md`, the schema layer is a **closed algebra** — a `Signal` is a
*named type with methods*, never a `Schema::Any` dodge or hand-rolled
merge. `prism-schema` already has the variants we need
(`value.rs`/`schema.rs`).

**Representation.** A `Signal` is a fixed-size block of samples:

- structurally, `Schema::Array { shape: [block], element: Float }` (and
  `[channels, block]` for multichannel — the ES-9 is 16-in/16-out);
- registered as a first-class `Custom { name: "Signal", parameters: {
  rate, block } }` in the `TypeRegistry`, carrying `sample_rate` and
  `block_size` in its parameters the way a `Quantity` carries its unit.

**`apply` = the mixer.** `Schema::Array`'s `apply` is *additive,
element-wise*. So when several modules' output ports wire into one bus
path, the BSP `apply_reconciled` barrier **sums their blocks** — a mix bus
falls out of the existing reconcile pass, no `Mixer` module required.
Replacement (a module overwriting a bus) uses `overwrite[Signal]`; a 1:1
cable (osc → filter) is just a single wire. *Many cables into one input =
a summing mixer; that is the algebra, not a special case.*

**Boundaries.** Across a process/network boundary a `Signal` serializes via
the type's own codec: **Arrow-IPC** record-batches for the streaming/
delta-log wire (a block *is* a dense tensor segment — `delta-traces.md`),
and **raw f32 PCM** (`Value::Bytes`) for the gorgon packet (`packet.rs`:
magic + seq + samples). On a hot local pass-through wire, `Foreign(Arc<
Vec<f32>>)` shares the block with zero copy. The codec law
`deserialize(s, serialize(s, v)) ≡ v` (`prism_schema::algebra`) is what
lets a rendered patch round-trip across the wire unchanged.

**Audio quantities are units.** Reuse the units machinery (`grow.ys`,
`units.ys`, `nuclear-shuttle.ys`): `Hz` (`1/s`), `s`, `dB`, `semitone`, and
`V` for control voltage are dimensioned types, checked then erased to
floats. The cross-dimension conversions modular synthesis lives on —
**Hz↔period, dB↔linear gain, MIDI-note↔Hz, semitone↔ratio, V↔Hz
(1V/oct)** — are exactly the *unit contexts* `nuclear-shuttle.ys` already
motivates (concentration↔counts across compartments). A `freq :: Hz` port
fed a `note :: MIDI` value coerces through the `MIDI→Hz` context; wiring is
dimension-checked, so patching a gate into a pitch input is a *located
compile error*, not a silent bug.

**CV and audio are the same `Signal`.** The ES-9 carries DC-coupled control
voltage on the same jacks as audio, which is why gorgon ships bit-exact PCM
(no lossy codec). A `Signal` block is a `Signal` block whether it is a
220 Hz saw or a slow pitch CV — only its *rate* and *units* differ. This is
why the modular (CV) and audio worlds unify with no extra type.

---

## IV. Folding gorgon in (decision: gorgon → library)

gorgon becomes a **library crate** (`src/lib.rs` exposing `audio`,
`packet`, `jitter`, `osc_msg`, `network`, `config`); its binary stays as a
thin shell. A new **`prism-audio`** crate depends on it. One source of
truth; no drift.

```
gorgon/
  src/lib.rs   ← NEW: pub mod audio, packet, jitter, osc_msg, network, config
  src/main.rs  ← thin shell over the lib (gorgon stream/listen/cv as today)

prism/crates/prism-audio/   ← NEW   (gorgon = { path = "../../gorgon" })
  device.rs    — Speaker (sink) + AudioIn (source) processes; the run_realtime driver
  signal.rs    — the Signal type + TypeMethods (Array repr, PCM/Arrow codecs)
  net.rs       — the `net:` protocol (gorgon packet + jitter + OSC) for §IX
  kernels.rs   — native DSP kernels (osc/filter/env) registered on the MethodRegistry
```

What is **reused as-is** vs **new**:

| gorgon piece | role | reuse |
|---|---|---|
| `audio.rs` `build_input/output_stream` | the lock-free ring ↔ cpal seam | **as-is** (already generic over `ringbuf`) |
| `audio.rs` device selection | ES-9 `plughw`, 48k, channel counts, duplex open | **as-is** |
| `packet.rs` | raw-f32-PCM wire format | **as-is** (the `Signal` PCM codec) |
| `jitter.rs` | per-source jitter buffer, priming, PLC | **as-is** (distributed links) |
| `osc_msg.rs` | `/cv/* /gate/* /note/*` over `rosc` | **as-is** (control-rate links) |
| `network.rs` | UDP broadcast/receive | **as-is** |
| `stream.rs` orchestration | gorgon's peer-routing task trio | **not** reused — prism-audio writes its own driver (the ring is fed by the engine, not by a capture→send loop) |

The cleanest seam is already there: `prism-audio` creates the
`HeapRb<f32>` pairs, hands the producer/consumer to gorgon's stream
builders, and the engine driver fills/drains the other ends. The
hard-realtime constraints (no alloc, no lock, silence on underrun) stay
*inside* gorgon's callbacks, which are correct today.

A `Speaker` is a **sink process** (`->{}` empty; its `update` pushes the
input block into the output ring); `AudioIn` is a **source process**
(`~{}` empty; its `update` pops the latest block from the input ring).
Both are ordinary processes — the device is just another module at the edge
of the patch, exactly as `report.ys`'s stream sections are ordinary
composites.

---

## V. A module is a process; a patch is a composite

Modules are defined in `.ys` exactly like every other prism entity — the
DSP *kernel* is a native method (heavy lifting in Rust, per "value methods"
in `chrysalis-design.md` §"Value methods"), and the `.ys` is the thin
interface + wiring layer (per `feedback_chrysalis_thin_layer` — never
reimplement DSP in chrysalis when a registered kernel will do).

```
# oscillator.ys — one audio block per tick. Phase is carried across blocks
# on the self-wire (%.phase), exactly as grow.ys accumulates `mass`. The
# sample-filling DSP is the native `osc` kernel (MethodRegistry); the .ys
# declares the port interface and the phase recurrence.
from kernels import osc        # native: fills a Signal block, returns (block, next_phase)

unit Hz : [frequency] = 1/s

process Oscillator[wave :: Wave = Saw]
  ~{freq :: Hz = 220.0, phase :: Float, block :: Int = 64, rate :: Hz = 48000.0}
  ->{out :: Signal, phase :: Float}
(
  r = osc(wave, freq, phase, block, rate) |   # r.block :: Signal, r.phase :: Float
  {out: r.block, phase: r.phase}
)
```

The phase recurrence is the same self-wire (`%`) read-modify-write pattern
as `grow.ys`/`divide.ys`: a stateful module reads its state slot, writes
the next value. A filter carries its delay line the same way.

A **voice** is a composite that wires modules together — the place graph is
the module nesting, the link graph is the cables:

```
# voice.ys — saw → low-pass → VCA, amplitude shaped by an ADSR gated externally.
from oscillator import Oscillator
from filter      import LowPass
from amp         import VCA
from envelope    import ADSR

composite Voice[freq :: Hz = 220.0, cutoff :: Hz = 1200.0]
  ~{gate :: Float}
  ->{out :: Signal @ out}
(
  phase:  0.0 |
  level:  0.0 |
  raw:    Signal.silence |
  shaped: Signal.silence |
  out:    Signal.silence |
  osc:  Oscillator[wave: Saw]    ~{freq: freq, phase: %.phase} ->{out: %.raw, phase: %.phase} |
  flt:  LowPass[cutoff: cutoff]  ~{in: raw}                    ->{out: %.shaped}               |
  env:  ADSR                     ~{gate: gate}                 ->{level: %.level}              |
  amp:  VCA                      ~{in: shaped, gain: level}    ->{out: %.out}
)
```

A **rack** is a composite of voices; a **mix bus** is just several voices'
`out` wired to one `mix` path — the additive `apply` sums them (§III):

```
composite Rack ~{} ->{mix :: Signal @ mix} (
  mix:  Signal.silence |
  v0:   Voice[freq: 110.0] ~{gate: 1.0} ->{out: %.mix} |   # both `out`s target
  v1:   Voice[freq: 165.0] ~{gate: 1.0} ->{out: %.mix} |   # `mix` → they SUM
  spkr: Speaker ~{in: mix} ->{}                             # mix → the device ring
)
```

The **standard module library** to ship (each a small `.ys` over a native
kernel): `Oscillator` (sine/saw/square/tri/noise), `LowPass`/`HighPass`/
`SVF`, `VCA`, `ADSR`/`AR` envelopes, `LFO`, `Sequencer`/`ClockDiv`,
`Delay`/`Reverb`, `Mixer`/`Pan`, `SampleHold`, `Quantizer` (CV→scale),
`Speaker`/`AudioIn`. Plus the **units context pack** (Hz/dB/semitone/MIDI/
V). This library is the audio analog of the cell library
(`grow.ys`/`divide.ys`/`cell.ys`/`environment.ys`) and the quantum gate
set (`Hadamard`/`CNOT`/`Measure`).

---

## VI. Generative rewiring — reactions over the patch bigraph

The patch is a bigraph; a `BigraphicalReactiveSystem` rewrites it. The BRS
already exists with three modes (deterministic / stochastic / Gillespie),
already fires parametric rules (`find_matches`/`fire_rule_at`/`apply_fire`),
and a reactum can already author running processes via `discover_processes`
(`engine.rs:1407`). Nothing new in the runtime — a **new application** of
it. The bigraph atoms are first-class surface syntax already
(`chrysalis-design.md` §"Bigraph primitives as first-class atoms"): link
vars `~w`, sites `?x`, `redex => reactum`.

**Insert a module on a wire** (cut the cable `~w`, splice a filter in):

```
reaction InsertFilter[cutoff :: Hz = 1000.0] (
  Oscillator ~{out: ~w} | VCA ~{in: ~w}
  =>
  Oscillator ~{out: ~a} | LowPass[cutoff: cutoff] ~{in: ~a} ->{out: ~w} | VCA ~{in: ~w}
)
```

**Spawn a detuned voice** (a reactum `_add`s a node → discovery
instantiates it next tick — same mechanism as cell division's daughters in
`environment.ys`):

```
reaction Detune[cents :: Float = 7.0] (
  Voice[freq: ?f] ~{gate: ?g}
  =>
  Voice[freq: ?f] ~{gate: ?g} | Voice[freq: ?f.shift(cents)] ~{gate: ?g}
)
```

**Prune a silent module** (a redex matching a module whose output RMS is
below threshold → `_remove`), driven stochastically or by a `where` guard.

A BRS of such rules over a seed patch is a **generative synthesizer**: run
it deterministically for reproducible variations, or stochastically/
Gillespie for evolving textures, and each firing yields a *new, related*
patch. Because the rules are *values* (`reaction X[…]` desugars to a
`Reaction[…]` value, registered; #30 slice 1A landed), a process can build
and install **new rules at runtime** — the generator can rewrite its own
grammar. This is capstone **"reactions rewiring a live patch."**

---

## VII. Homoiconic modules — the synth writes synth modules

Two depths, both on existing mechanisms:

**Spec-level (today).** A process emits a `Value::Tree` with `{_type:
"process", address: "local:Oscillator", config, inputs, outputs}` and the
engine instantiates it next tick (`discover_processes`). So a **factory
process** authors module specs as data:

```
step Bloom ~{rack :: map[Module], seed :: Float} ->{rack :: map[Module]} (
  { rack: { _add: { 'osc{seed}': Oscillator[freq: 110.0 * seed] } } for ... if ... }
)
```

This is exactly how `environment.ys` adds daughter cells and
`quantum-lifecycle-stream.ys` adds quantum subsystems — a freshly-authored
`Term` lowers to a full node spec and discovery brings it to life,
local **or** streamed.

**Body-level (tier 2).** `chrysalis-design.md` tier 2 (and #30, the unified
entity registry) makes the *process body* a first-class value
(`ProcessDef` / `Expr → Value → Expr`, already round-tripping —
`entity_as_data.rs`). Then a reactum can synthesize a **brand-new module
type** — not just instantiate a known one — and install it, schema-checked
at construction so an illegal module is unrepresentable. The synth writes
new synth modules. This is capstone **"homoiconic module factory"**, and it
is the audio instance of the project's headline reflection feature ("state
IS code").

This sits right next to §VI: a reaction's reactum *is* the place where a
module gets authored. Live-rewiring and homoiconic-authoring are the same
capability seen from two angles, which is why they build together.

---

## VIII. Merge/split voices — `fold`/`unfurl` between composites

This is the direct audio image of `quantum-bigraphs.md` §VII (the
`divide ↔ tensor` duality) and
`bigraphs-all-the-way-down.md` §IV (`fold`/`unfurl`).

- **Two independent voices** sharing no link are two composites (the audio
  analog of separable qubits — `quantum-two-bells.ys`). They can be
  distinct stream subprocesses with genuinely local state.
- **Couple them** (patch a shared modulation bus, or a cross-voice FM
  cable) and a link now spans them → they should become **one composite**
  holding the joint sub-patch. This is `fold`'s inverse — `unfurl` the two
  into one flat graph, or rather `merge`/`tensor` them.
- **Decouple** (remove the shared cable) and the patch may **factorize**
  again → `divide` back into independent voices.

The operations are the schema-algebra `fold`/`unfurl` (the planned named
ops, `bigraphs-all-the-way-down.md` S1) plus `divide_by_schema` /
`tensor`/`merge` (which already exist — `divide` ships; quantum `tensor`/
`factorize` ship as HostFns). The synth difference from quantum is only the
*coupling predicate*: quantum merges on non-factorizability of an amplitude
matrix; a synth merges when two voices **share a signal/CV link** (the
`bigraphs-all-the-way-down.md` §VII connected-component reading: the
implicit composite = a connected component of the patch's link graph).
"Merge when patched together, split when un-patched" is the §VIII
topology-reaction pair, made audible. This is capstone **"merge/split
synths."**

---

## IX. Distributed mesh jam — outer links over gorgon/Tailscale

Two (or more) synth composites on **different machines**, joined by
distributed links. gorgon already does the transport end-to-end — bit-exact
PCM over UDP for audio, OSC for CV/gate/note, per-peer jitter buffering,
Tailscale for connectivity. We expose it as a prism **protocol**, the same
way `stream:`/`rest:` are protocols selected by a node's `address:` field
(`execution-model.md`; the protocol seam is `invoke → Defer → flush →
collect`):

```
# A voice running on the "studio" peer, its audio sent over the net, its
# pitch CV received back — analogous to: protocol StreamingCell = stream<Cell, …>
protocol RemoteVoice = net<Voice, peer: 'studio', send: [out], recv: [pitch_cv]>
```

A `net:` link's `invoke` writes the outgoing block into gorgon's send path
and returns a `Defer` that reads the jitter-buffered incoming block in the
collect pass — the *exact* shape of the `stream:`/`rest:` concurrent
dispatch already shipped (`#21`, `rest_engine.rs`). Audio rides the PCM
packet; CV/gate ride OSC; the jitter buffer absorbs network timing
(graceful, like the device underrun).

Structurally this is `bigraphs-all-the-way-down.md` §VII–VIII realized over
audio: the **outer link graph** joins streaming composites into one
**implicit composite** (a connected component spanning machines); a
topology reaction that adds a coupling link merges two peers' jams; one that
removes it lets them split. The LOCC parallel from `quantum-bigraphs.md`
§IV is precise — a **classical CV link** lets two synths stay separate
composites while sharing control (a shared clock, a pitch bus) without
fusing their audio graphs; a **signal link** that genuinely couples them
(cross-FM) merges them. This is capstone **"distributed mesh jam."**

---

## X. The build order — toward the dream

The user's dream: *"a homoiconic distributed mesh jam that merges/splits
synths with reactions and live rewiring."* That is the **integration of all
four capstones**, and there is a capability-dependency order that builds to
it. Each slice is *toward* the full design, test-guarded, never a
load-bearing workaround (`feedback_no_half_measures`). Slices A1–A2 are
deterministic and CI-testable with no hardware (the offline-first choice);
A3 adds the device; A5+ are the capstones.

- **A1 — `Signal` + offline render.** The `Signal` type (Array repr,
  additive-mix `apply`, PCM/Arrow codecs, registered with `TypeMethods`);
  `oscillator.ys` → render N blocks → `sine.wav`, asserted in a test. No
  cpal, no wall-clock. *Proves: the substrate, deterministically, under
  `cargo test`.* **(foundation)**

- **A2 — module library + a hand-patched voice, offline.** `Oscillator`/
  `LowPass`/`VCA`/`ADSR` over native kernels; `voice.ys` + a `Rack` mix
  bus; render to WAV. The units/context pack (Hz/dB/semitone/MIDI/V).
  *Proves: composition, the mix-bus-as-reconcile, dimension-checked
  wiring.* **(foundation)**

- **A3 — the realtime device boundary.** gorgon → library; `prism-audio`
  with `Speaker`/`AudioIn` over the gorgon ring; the `run_realtime(device)`
  pull driver. *Now you HEAR the A2 patch.* The first wall-clock↔logical
  seam. **(realtime)**

- **A4 — control rate + modulation.** `LFO`/`Sequencer`/`SampleHold`/
  `Quantizer` at k-rate (heterogeneous `interval`); CV as `Signal`; the
  MIDI→Hz / V/oct contexts live. *Proves: expressivity; CV ≡ audio.*
  **(expressivity)**

- **A5 — reactions rewiring a live patch.** A BRS over a patch bigraph
  (`InsertFilter`, `Detune`, `Prune`), deterministic then stochastic/
  Gillespie. *Capstone: live rewiring — you hear a reaction fire.*

- **A6 — homoiconic module factory.** Spec-level first (a `Bloom` factory
  via `discover_processes`), then tier-2 `ProcessDef` (a reactum authors a
  new module *type*). *Capstone: the synth writes modules.* (Pairs with A5
  — the reactum is where modules are authored.)

- **A7 — merge/split voices.** `fold`/`unfurl` as named algebra ops; the
  coupling predicate = "voices share a link"; merge on patch, split on
  factorize. *Capstone: merge/split — the `tensor`/`divide` twin, audible.*
  (Shares the cross-composite machinery the dream needs.)

- **A8 — the `net:` protocol + distributed synths.** gorgon transport as a
  protocol; two machines; audio over PCM, CV over OSC, jitter-buffered;
  outer links join streaming voices. *Capstone: distributed mesh jam.*

- **A9 — the dream.** Compose A5–A8: streaming voices across machines
  (A8), merging/splitting as they couple/decouple (A7), rewired live by a
  BRS (A5) whose reactums author new modules homoiconically (A6). A
  *homoiconic distributed mesh jam.* This is the integration demo, the
  audio sibling of `quantum-teleportation.ys` as the quantum capstone.

This refines the order the four capstones were listed in by moving the
homoiconic factory (A6) up next to live-rewiring (A5) — they are one
capability — and grouping merge/split (A7) with distributed (A8) as the
cross-boundary tier the dream needs last. (The listed order was a fine
first guess; this is the dependency-true version.)

---

## XI. Honest caveats

- **prism is not a low-latency DSP host.** The per-tick snapshot +
  `reconcile` + `Value` clone costs more than flat buffer DSP. Mitigations:
  bigger blocks, `Foreign(Arc<Vec<f32>>)` pass-through, `tick_lifecycle`
  vectorization for voice banks. We target structural flexibility, not
  module count at 64-sample latency. State this plainly to anyone expecting
  VCV Rack throughput.
- **The audio thread must never block on the engine.** The only shared
  state is the lock-free ring; the engine runs on its own thread; underrun
  → silence. Never put a `Mutex` (an `Engine` behind a `Mutex` is how
  composites work today!) on the audio callback's path. The `run_realtime`
  driver, not the callback, touches the engine.
- **Float determinism.** Offline render must be bit-reproducible for tests
  (fixed block size, fixed kernels, `OrderedFloat` already in `Value`).
  Realtime is inherently non-deterministic (device timing) — test the
  *offline* path; smoke-test the realtime path for liveness only (the
  quantum-stream tests' `strace`/`/bin/false` differential is the model for
  proving children are real).
- **gorgon → library is a real (small) refactor.** Adding `lib.rs` and
  `pub`-ing the modules is cheap (8 files, the seams are already clean), but
  it is a change to the gorgon repo — keep gorgon's binary behavior
  identical (its tests green) in the same change.
- **Distributed latency is physical.** A net jam has the jitter buffer's
  latency (gorgon default 40 ms) — fine for ambient/generative interplay,
  not for tight rhythmic lock across a slow link. This is gorgon's existing
  tradeoff, inherited, not introduced.
- **`fold`/`unfurl` on-demand only.** As in
  `bigraphs-all-the-way-down.md` §X, never materialize the flat ladder;
  merge only the voices a reaction names; re-seal immediately. "Unfurl the
  world" (matching deep into every remote peer) is the failure mode to
  avoid — match at the coarsest level that suffices.
- **Mix-bus summing is `apply` semantics, not free-for-all.** Additive
  `apply` sums *deltas*; a module that must *replace* a bus uses
  `overwrite[Signal]`. Document which library modules are additive
  (oscillators into a bus) vs overwriting (a master limiter) so patches
  don't silently sum where they meant to replace.

---

## XII. Why this is the audio twin of quantum

The two showcases are deliberately parallel — same machinery, two domains,
so that "reactions inside / between / across composites" is demonstrated
*twice*, audibly and physically:

| | quantum (`quantum-bigraphs.md`) | synthesis (this doc) |
|---|---|---|
| node | qubit | module |
| link | entanglement | patch cable / CV bus |
| local op | single-qubit gate | a-rate/k-rate module step |
| coupling | CNOT (entangling) | shared mod bus / cross-FM |
| **merge** | `tensor` (entangle across composites) | couple two voices into one composite |
| **split** | `divide` (factorize after measure) | un-patch → factorize into voices |
| classical link | LOCC (no entanglement) | CV link (shared control, separate audio) |
| distributed | separable systems on stream children | voices on different machines over gorgon |
| reflection | a reactum spawns a quantum subsystem | a reactum authors a synth module |

If both run on the one engine, the thesis of
`bigraphs-all-the-way-down.md` is demonstrated, not just argued: **one BRS
rewrites topology — biological cells, qubits, and synthesizer voices alike
— inside composites, between composites, and across the distributed mesh.**

---

## XIII. Design decisions

### Resolved

- **gorgon → library crate; `prism-audio` depends on it.** One source of
  truth (decision, §IV). gorgon's binary stays a thin shell.
- **Block-rate ticking: one tick = one audio block.** a-rate fills a block,
  k-rate emits one value/tick or runs on a longer DES interval (§II).
- **The device boundary is gorgon's lock-free ring, pull-driven.** The
  engine runs ahead on a normal thread; the cpal callback only
  pops/pushes; underrun → silence (§II).
- **Offline render is the first milestone.** Logical-time, deterministic,
  CI-testable; the realtime device is additive on top (§II/X).
- **`Signal` is a first-class type in the algebra** — `Array` repr,
  additive-mix `apply`, PCM/Arrow codecs, units for Hz/dB/semitone/MIDI/V
  via contexts (§III). No `Schema::Any`, no hand-rolled merge.
- **DSP kernels are native (MethodRegistry); `.ys` is the patch/interface
  layer.** chrysalis stays thin (§V).
- **Build order A1–A9, culminating in the homoiconic distributed mesh
  jam** (§X).

### Open

- **Block size & channel default.** Start 64 frames / 48 kHz / mono for A1,
  expose as config; multichannel (ES-9 16×16) by A3. Pick a default block
  for the realtime driver vs the offline renderer (offline can use a larger
  block for throughput).
- **`net:` protocol granularity.** One link per channel vs one per voice
  (gorgon's `send`/`recv` routing matrix maps cleanly to per-voice
  multichannel — likely per-voice). Decide when A8 starts.
- **Where the coupling predicate for merge/split lives** (§VIII) — a
  `where` guard on a topology reaction, vs an autonomous self-check like
  `quantum-auto-split.ys`. Likely the latter, to keep it director-free
  (`bigraphs-all-the-way-down.md` S0/S4).
- **MIDI/OSC *input* surface.** gorgon's `osc_msg` is output-oriented
  (`cv`/`gate`/`note` builders); driving a prism patch from an external
  MIDI/OSC controller (a `Control` source process) is desirable but not yet
  scoped — fold in around A4.
- **Tier-2 `ProcessDef` for module bodies** depends on #30 (the unified
  entity registry) landing; A6's body-level half waits on it (spec-level
  half does not).

---

## XIV. References

- [`quantum-bigraphs.md`](quantum-bigraphs.md) — the twin showcase;
  `tensor`/`divide`/entangle/measure as bigraph rewrites; LOCC; the
  separable-systems-as-stream-children pattern this mirrors for voices.
- [`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md) —
  `fold`/`unfurl`; the link graph's connected components as implicit
  composites; topology-rewriting reactions over the distributed/mesh
  bigraph (§VIII–IX here are its audio instance).
- [`execution-model.md`](execution-model.md) — the BSP tick, DES `fronts`
  (heterogeneous rates = a-rate/k-rate), the `invoke → Defer → flush →
  collect` protocol seam the `net:`/device boundaries hang on.
- [`distributed-execution.md`](distributed-execution.md) — the planet-scale
  outer-link form; `net:` is a protocol with a pluggable backend (gorgon).
- [`chrysalis-design.md`](chrysalis-design.md) — the surface syntax
  (`~{} ->{}`, `|`, `%`/`^`, `@`, units), bigraph atoms as first-class,
  value methods (DSP kernels), tiers (tier-2 = homoiconic module bodies).
- [`schema-algebra.md`](schema-algebra.md) — the closed algebra `Signal`
  and `fold`/`unfurl` must live inside.
- [`delta-traces.md`](delta-traces.md) — the delta-log / Arrow wire the
  offline render and `net:` link serialize over.
- [`merge-protocol.md`](merge-protocol.md) — bridges as symmetric update
  channels; the first instance of `unfurl`-`fire`-`fold` (voice merge).
- [gorgon](../../gorgon) — the realtime audio + CV + network layer being
  folded in (`audio.rs` ring seam, `packet.rs`, `jitter.rs`, `osc_msg.rs`,
  `network.rs`).
- Roads & Strawn, *The Computer Music Tutorial* (1996); Puckette, *The
  Theory and Technique of Electronic Music* (2007) — block-rate DSP, the
  a-rate/k-rate split.
- Milner, *The Space and Motion of Communicating Agents* (2009) — bigraph
  place/link orthogonality.
