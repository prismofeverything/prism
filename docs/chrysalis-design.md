# Chrysalis Design

A surface programming language whose semantics compile to **prism**'s
Rust runtime. The substrate stays prism — `Topology`, `Process` trait,
`Step` trait, `BigraphicalReactiveSystem`, `Engine`, `ProcessRegistry`,
the `discover_processes` reflection mechanism. Chrysalis adds syntax,
expression-language update bodies, lexical scoping, parameterized
composites with closures, and the abstraction layer that turns prism
from a framework into a language.

**Home:** `crates/chrysalis/` (a member crate of the prism workspace).

**File extension:** `.ys` (chrYSalis). Source files live in
`crates/chrysalis/ys/`.

**Contract:** `chrysalis::compile(source) -> CompileResult { topology, core, … }`.
Everything below the compiler is prism-native.

## Where chrysalis is now (the story so far, and the road ahead)

Chrysalis began as AST fixtures hand-compiled to prism (the tier-1 acceptance
trio: grow/divide, MAPK, M/R). It is now a parsed, schema-aware language with a
build tool and a real distribution story. The arc, in layers:

**1. The surface language — built.** `process` / `step` / `composite` definers
with `~{in} ->{out}` port interfaces and `|` parallel composition (wiring *is*
categorical composition — see "Categorical structure"). `def` introduces
first-class values **and functions** (`def network :: CRN = {…}`,
`def f(x) = …`; functions pass/return/store). `from <module> import <names>`
pulls in native host capabilities — a whole process (`from core import
RunProcess`), or functions / types a `.ys` `process` wraps with its own ports +
contract (this **replaced `extern`**, now fully removed from the grammar).
**Process contracts** (`contract` / `fulfills` / `::`) give a
process its *meaning* — which mathematical object it approximates — so two
processes are substitutable only when they share a contract, and an illegitimate
comparison does not compile (`docs/process-contracts.md`; substitutability =
`algebra::refines`, no new op). **Units** are checked once and erased to raw `f64`.

**2. The runtime substrate — built, and prism-native.** chrysalis is a THIN
layer: it compiles to a prism `Topology` + a `Core`, and never reimplements the
engine. **prism-std** is prism's native standard library (`RunProcess`, the
mass-action integrators, `CRN`, `TimeSeries`) that chrysalis bundles as the
importable modules `core` / `integrators` / `chem` / `io`. The unified
**`Core`** (`prism_bigraph::Core` = types + processes + methods + protocols,
prism's port of upstream `core`/`link_registry`) is threaded through the engine
and **every subengine**, so a `Custom`-typed, method-using, or remote process
works inside a composite exactly as at the top level (before this, a composite's
subengine silently lost three of the four registries). **Incremental steps**
(`prism_bigraph::StepCache`): a workflow's steps are *skipped* when their output
is cached and fresh, *forced* per-step (`chrysalis run f.ys --steps a,b`), with
staleness cascading down the step DAG — the basis for "the report is one
workflow, each section a step you can selectively expire."

**3. The build tool — a real toolchain.** `chrysalis run | check | bigraph |
bigraph export/import | server | repl` are all live over the bundled std library:
`export`/`import` make the process-bigraph document a runnable artifact
(`import(export(f)) ≡ run(f)`); `server` exposes a `Core` over REST; `repl` is an
interactive homoiconic prompt (eval, `:type`/`:env`, live syntax highlighting,
completion, multi-line). **`compile`/codegen is live**: a program importing a
*non-std* native package (spatio-flux's FBA/particles) runs via the codegen path —
`chrysalis run` generates a runner crate, `cargo build`s + caches it (cargo-driven
staleness, so prism edits rebuild it), and runs it; a `project.ys` manifest names
the package. `run`/`server`/`import` share that one package-resolution +
`Core`-assembly path. The consumer that proved it: the whole spatio-flux demo
suite, rewritten as `.ys` (layer 5). **The manifest is the structured `def package = { … }` (#67 Phase 5c — ONE form).**
A package's manifest is `.ys`-as-data (pkg's `manifest.rs`); its native parts are a
top-level `native: '<crate>'` (the package's OWN native crate — the co-located *mixed*
shape, `'.'` next to `project.ys`, or `'<path>'` decoupled for a project outside the
monorepo) plus `dependencies: { d: { native: '<crate>' } }` (a native DEPENDENCY). The
runner is a **transport** for native Cores: it links each native crate, calls its
`prelude::core()`, and routes through the canonical resolver `resolve_with_natives` (the
*same* colimit as an in-process `.ys`-only resolve — native Cores merely *supplied*). An
own-native-only package skips the resolver — its crate's `core()` *is* the run-Core
(surface `std ⊔ its modules()`). The legacy one-line `package <name> [at <path>]` directive
**was retired**: the structured `native:` subsumes both its co-located + decoupled forms
(Felleisen — one manifest, one codegen path). See `docs/packages-decomposition.md` §3 +
`crates/chrysalis/src/codegen.rs::run_structured`.

**4. The boundary, made real.** A process or composite is a black box reachable
through **protocols** (`local` in-process; `rest` / `parallel` remote) — the
simulation can't tell whether a process runs in-thread or over HTTP. The
**REST process server** (`prism_bigraph::protocols::RestProcessServer`) exposes a
`Core` over the rest-process wire protocol and manages process lifecycles
(`initialize` → `update` → `end` deletes — verified by a no-leaks test). A
document that references a process the core can't build is **rejected with a clear
message** (`Core::missing_process_refs` / `Engine::check_references` / a server
400), not silently run as a partial graph — the first instance of the diagnostics
discipline below.

**5. The full circle — the spatio-flux report regenerates from `.ys`.** All six
families now run as `.ys` report sections through codegen —
`{kinetics,diffusion,dfba,brownian,newtonian,comet}-section.ys`, each
`Simulate[proc: <sim>] → trace → traj.plot → figure.svg`. The dogfooding loop
(turn a sim into `.ys`, find the gap, fix it IN the core —
`feedback_integrate_into_core`) drove the core additions:
- **delta-traces** (`prism_trace`: a `Trace` = `initial` + `deltas`,
  `state(t)=fold(apply,…)`, Arrow IPC codec) + **`Simulate`**, the engine-faithful
  runner (same-name wiring; folds the inner's REAL output schema — `RunProcess` is
  the integrator special case). The `stream:` protocol proxies a `.ys` as a
  `Process` over that trace wire.
- **schema-driven views** (`prism_viz`): a trace plots BY ITS ELEMENT TYPE —
  scalars→line, array fields→animated heatmap (`View::Field`), particles→scatter
  (`View::Particles`), field+particles→comet overlay (`View::Spatial`) — all
  place-graph SVG values.
- **the particle delta-motion model**: spatial movers emit **Δposition/Δvelocity**,
  folded additively (`Array[[2]]`), so Brownian + drift + interactions +
  boundary-correction *superpose* — particles fit the same fold-via-`apply` algebra
  as everything else (retiring their `Map[Any]` dodge + two more `Schema::Any` leaks).
- **`.ys` file modules** (explicit-origin resolution): a **leading dot** is a
  relative file — `from .file import <Def>` is the sibling `file.ys`, `from
  .sub.file import …` is `sub/file.ys`; a package path `from pkg.sub.file import …`
  is package-rooted (recursive; the module's types + host imports ride along). A
  **bare** name (`from core import …`) is a native/registry module — never a file.
  The name's shape decides (no path search, no precedence; the retired #50
  std-module-first), so a sibling and a native can't collide. The six sections are
  DRYed into shared modules — `report/section.ys` + `composites/comets.ys`.

**6. The road ahead.** Assemble the six sections into ONE report *workflow*
(run-all → a combined report; the report as an expirable step-DAG over the
incremental-step cache). The `import <module>` namespace forms (`comets.Comet`).
**SVG-as-place-graph everywhere** (retire the graphviz `render_dot` bigraph viz +
the string `render_timeseries_svg`). Comprehensible **diagnostics** (generalize the
missing-reference error; NEXT-SESSION #16). The `chrysalis new` scaffolder.
**First-class `Custom` types**. **SBML / repressilator** import. **Tier 2** —
process bodies as first-class values, evolving M/R. Then the **performance sweep**
(after features).

## Two kinds of process

chrysalis hosts **two kinds of process**, and the distinction is
load-bearing:

1. **Rust-native capabilities** — defined in Rust. The host registers
   process factories in a `ProcessRegistry`, value-methods in a
   `MethodRegistry`, and declares what each native **module** exports via
   a `ModuleRegistry`. A `.ys` pulls them in with
   `from <module> import <names>` — the **replacement for `extern`**,
   resolved at `compile_with_modules`. Two import granularities:
   - **whole process** — `from core import RunProcess` uses the native
     `Step`/`Process` as-is: no interface redeclaration, wired straight
     from the call site.
   - **function / object / type** — `from integrators import rk4, euler`
     imports native callables (their methods dispatch via the
     `MethodRegistry`); `from chem import CRN` / `from io import Path`
     import native *types* (first-class via a synthetic `type` def). A ys
     `process` then declares the ports + `fulfills` contract and calls the
     imported function in its body:
     `process Rk4[network: CRN] fulfills DeterministicMassAction[method: Rk4] ~{state: map[float]} ->{state: map[float]} ( rk4.integrate(network, state, interval) )`.
     The interface and contract are the surface language's; only the math
     is borrowed. This is the principled split that `extern` conflated
     (interface *and* implementation forced to mirror one native process).
2. **ys-native processes** — defined *in chrysalis* as `process` /
   `step` / `composite` whose bodies are expressions over the values
   arriving at their inputs/config. They compose the native primitives
   and each other; their "implementation" is the interpreted body (value
   methods dispatched via the `MethodRegistry`, native imported functions,
   structural deltas, sub-process wiring). No Rust required. Bodies may
   also call **effectful** writer methods (`a.csv(path)`, `figure.svg(path)`),
   so a workflow can emit its own artifacts — an `Output` step with `->{}`
   runs purely for its writes.
3. (`extern` has been fully removed; natives come in via `from … import …`.)

The two meet at the typed port interface (`~{} ->{}`): a ys-native
process can't tell whether what it wires to is native or ys-native, and
vice versa (composite-as-process is the categorical reality). The set of
types these processes exchange — quantities, `TimeSeries`, `CRN`,
`Figure`, `Path`, meshes, patterns, `ReactionRule`/`BRS`, contracts — is
the **fundamental-type catalog** to enumerate and organize into packages
(on the task list). The worked example is
[`ys/integrator-comparison.ys`](../crates/chrysalis/ys/integrator-comparison.ys):
`from` imports of `RunProcess` (process), `rk4`/`euler` (objects), `CRN`/`Path`
(types), function-bodied integrator processes, split `Compare`/`Plot` steps, and
a self-outputting `Output` step.

## A file is a composite — compositional invocation (decision #24)

**A `.ys` file's value is its *last top-level term*** — the same convention as
a body block, whose value is its last `|`-less line. From this one rule both
"file as package" and "file as composite" fall out, independently and opt-in:

- **Vocabulary.** `process`/`composite`/`step`/`def`/`reaction`/`type` are the
  file's definitions — all importable (`from things import Grow`). A file with
  only these is a **pure package**.
- **Entry point.** The file's *runnable interface* is its **last** definition
  that carries one: a `composite`/`process` (a simulation) or a `def` (a pure
  function). `chrysalis run file.ys` binds the command line to that interface
  and renders its outputs. An explicit trailing expression (the implicit `main`
  binding) still wins; the last-term rule is the no-`main` fallback. No
  interfaced term ⇒ not runnable on its own (import-only). [`Program::entry`]
- **Headless sugar.** A trailing `[config] ~{in} ->{out} ( body )` with no
  `composite Name` prefix is just an *anonymous composite* as the last term —
  optional sugar; identical to the named form. So the keyword is never required
  but always available.

### Invocation is `Trace[In] → Trace[Out]`, seeded by config

The naïve reading of "run a file" collapses two axes to a point: input bound as
one constant value, output read as one final value. The faithful object keeps the
**time axis** the rest of the system is built on
([`delta-traces.md`](delta-traces.md)):

> **A `.ys` invocation is the morphism `Trace[In] → Trace[Out]`, seeded by
> config.** It consumes an input *trace* (a procession of frames, one per tick)
> and emits an output *trace*; `[config]` is the **t=0 seed** that picks which
> morphism.

|         | construction (t=0)            | stream (t>0)                        |
|---------|-------------------------------|-------------------------------------|
| **in**  | `config` — set once, the seed | `inputs` — a trace fed in, 1/tick   |
| **out** | (schema header)               | `outputs` — a trace emitted, 1/tick |

**Batch — "set a config and run" — is the degenerate case**: a constant (or
absent) input trace, keeping only the *last* output frame. That `t=0 → t=final`
collapse is what shipped first (`runner::invoke`); it is *correct*, it is just the
point-projection of the trace transform, and growing the time axis is additive —
nothing built is wrong.

### One model, three transports

A composite is a **morphism** reached only through its port interface, over a
**protocol** (decisions #18/#20: `local` / `rest` / `parallel`). The CLI is just
*another transport* of that one boundary, and a shell pipe is the same s-category
composition the engine does internally with `~{} ->{}`:

| role                        | internal (`local`) | `rest`               | **CLI**                       |
|-----------------------------|--------------------|----------------------|-------------------------------|
| config (pick the morphism)  | `[...]`            | construction body    | **argv flags**                |
| input (domain, per tick)    | `~{}` wires        | per-step input state | **stdin stream** (+ flag t=0) |
| output (codomain, per tick) | `->{}` wires       | per-step update      | **stdout stream**             |

So **invocation, piping, and REST are three transports of one thing** — composing
a composite's port interface with an outside context. The pipe `A.ys | B.ys` *is*
`B ∘ A`, with the OS pipe as transport exactly as `rest:` is the network one.

### Config vs input: seed vs stream

Config and input are genuinely distinct — `Grow : Rate → (Mass → Mass)`, config
the first arrow (partial-applied once at construction, picking the experiment),
input the second (applied every tick). They only *look* identical in batch
because a constant function and a point coincide when sampled once.

You do **not** need two kinds of input. The cut falls out of the channel:

> **Config can only be *seeded* (flags). Input can be *seeded* (flag = t=0) or
> *driven* (stdin = stream).** The defining property of config is
> time-invariance — the moment a value varies in time it *is* an input.

This keeps the flag namespace flat and ergonomic (you just set named things) while
staying principled: a flag is a *construction* binding, the stream a *runtime*
binding, and config cannot appear on the stream. A stream frame naming a config
param is a located diagnostic (#16): *"`rate` is config (construction-time); it
can't be driven per-frame — pass `--rate`."* The distinction teaches itself where
it bites.

### Every definer runs standalone — the default harness

A composite carries its own place-graph (its body), so `chrysalis run env.ys`
just inlines it. A bare `process`/`step` does not — its ports dangle (*where do
inputs come from, where do outputs go?*). So running a `process`/`step` entry
**realizes it in a synthesized default harness**: a state slot per port, the
process self-wired to those slots (a read-modify-write port like `mass` thus
*accumulates* its delta), input slots seeded from `--port` (defaulted to a
type-zero), every slot exposed as an output. `chrysalis run grow.ys --mass 1
--glucose 5 --time 30` then runs the process on a bench — a heart beating outside
the body — and a composite is the degenerate case where the harness *is* the
composite (`cli::harness_process_entry`). So there are no second-class files:
every definer is runnable, and the *mode* is the invocation (default bench /
`--serve-process` driven child / `--trace`), never the filename.

This also closes a defaults gap. **Input-port defaults** (`~{glucose :: Float @
glucose = 0.0}`) must be honored when a composite's BODY references an input
(`glucose: glucose`) — otherwise the input is unbound at construction (and an
input default, by making the composite "bare-runnable", forces exactly that
eval). The single eval-side rule is `Evaluator::composite_param_env` (config
params *and* input-port defaults), shared by `eval_top_level` (the compile-time
inline) and `build_composite_outer` (a composite used as a value/node); the
runner's invoke/serve paths bind the same defaults via `bind_arg`. So a driven
component is also seedable on a bench: `chrysalis run cell.ys --glucose 5`
metabolizes the seeded pool (conserving mass), while the env still drives it each
tick when embedded. (Config defaults were always honored; input defaults were the
gap. Guarded by `tests/bench_run.rs`.)

### The command surface

There are not two input *mechanisms* (flags vs stdin); there is **one input
record** `{port: value, …}`, and:

- **flags are a record-builder** — the shell spreading a record literal across
  argv (`--mass 1.2` sets one field), each value a **seed source**: a literal,
  `file:PATH` (process-substitution `file:<(gen)` covers big values), or `lit:…`
  to force a literal that looks like a scheme.
- **the input stream** is stdin (or `--in FILE`) — the whole interface's trace.
- **the output stream** is stdout (or `--out FILE`) — the identical codec.

Reserved: `--time` (run duration — *not* `--interval`, the engine's per-step dt).
**Precedence:** a flag sets a port's **t=0** value; a stream frame naming the same
port overrides it from that frame onward. (This supersedes the batch-only
connector list's per-port `-`/`stdin:` source and reserved `stream:` — the stream
is now one interface-wide channel, not a per-port byte source.)

### I/O is the codec (no new operation)

Port I/O is exactly the schema algebra's **codec**, applied *per frame* over the
stream (and to the whole interface record at the seam):

- **input** = `realize`/`deserialize(port_schema, encoded)` — external → `Value`.
- **output** = `serialize(port_schema, value)` — `Value` → external.
- Law: `deserialize(s, serialize(s, v)) ≡ v` (`prism_schema::algebra`) — which is
  why a run's output round-trips as another run's input.

**Rich types carry their own codec** (per-type `serialize`/`realize` in the
`TypeRegistry`): a `CRN` ↔ SBML, a `Figure` ↔ SVG, a `TimeSeries` ↔ CSV — no CLI
special-casing; the type owns its external form. The input path finishes with
process **realization** (`discover_processes`), so a deserialized process-bearing
value becomes runnable.

### Pipes are morphism composition

`gen.ys | sim.ys | plot.ys` composes iff each stage's output record **refines
into** the next's input record — `algebra::refines`, the *same* structural check
the engine uses for internal wiring, with the same width tolerance as the
name-aligned matcher (extra fields dropped; required-and-defaulted fields
covered). "Match on a subset" is exactly `B.required_inputs ⊆ A.outputs`.

Two rules keep this honest, not magic:

1. **Names are the wires.** In the s-category, `B ∘ A` *shares names* — port names
   *are* the contract. Mismatches take an **explicit adapter**: `--map out=in`
   (sugar desugaring to a one-wire adapter morphism) for renames, and a
   `--adapt adapter.ys` (a real adapter composite) when the reshaping is richer
   than a rename. Never silent positional guessing.
2. **A mandatory schema header.** Every stream **leads with its port-record
   schema** (mirroring `document_of`, which already ships `schema` alongside
   `state`). So `refines` is checked **at connect time, before any data flows** —
   the "do they match?" question answered mechanically, with a clear error (#16)
   on mismatch — and a polymorphic consumer (a generic `plot.ys`) adapts to
   whatever shape arrived.

### The wire: the delta-log

The stream is the **delta-log** of [`delta-traces.md`](delta-traces.md): the
schema header, then a procession of `apply`-able schema-deltas (`diff` produces,
`apply` replays), **not** fat full-state frames. Dense fixed-shape segments are
**Arrow-IPC** record-batches (a batch *is* a tensor-block); structural deltas
(`_add`/`_remove`) are the segment boundaries (the keyframes that reshape the
index set). We go **straight to delta-log/Arrow** rather than landing JSON-first —
the streaming and distributed (`rest`/Flight) cases need it regardless; a
human-readable JSONL projection exists for eyeballing. Batch is the one-frame
collapse of this same log.

### In practice

| use case           | shape                                                      | exercises                                                       |
|--------------------|------------------------------------------------------------|----------------------------------------------------------------|
| parameter sweep    | `for r in …; do run grow.ys --rate $r; done`               | config flags, batch out — the `t=0→t=final` collapse           |
| pipeline           | `sbml.ys --file m.xml \| integrate.ys \| plot.ys --out f`  | composition by shared name; rich-type codecs on the wire       |
| forcing a sim      | `sensor.arrow \| run cell.ys --rate 0.2`                   | stdin = time-varying input — *requires* streaming; proves config≠input |
| replay / re-render | `run sim.ys > t.arrow; plot.ys < t.arrow`                  | output is a durable delta-log — re-render without re-simulating |
| distributed step   | `local`→`rest`                                             | same codec over the network — pipe and REST wire are one composition |

*Status & slices.* Batch **and** streaming shipped. Batch: `Program::entry`; the
`compile` no-`main` fallback; `runner::invoke` (config/inputs from flags via
`realize`, final-frame record). **Streaming — the full `Trace[In] → Trace[Out]`:**
✅ (1) **output→trace** — `runner::invoke_trace` + `chrysalis run --trace
[--sample-dt]` sample the `->{outputs}` into a delta-log trace on the Arrow wire;
✅ (2) **input→trace** — `runner::invoke_driven` + `--in TRACE` inject each Arrow
frame into the input bridge paths per tick (first frame the t=0 seed, flags win;
`queue_changes` so change-triggered steps fire too); ✅ (3) **schema header +
`refines`** checked at connect — so the shell pipe `A.ys --trace | B.ys --in -`
*is* `B ∘ A` (a round-trip test + the real pipe → `{out: 5.0}`). The wire is
`prism-trace::codec` (Arrow-IPC streaming + a mandatory schema-metadata header;
JSON payload cells today → native typed columns next). REMAINING: (4) **`--map`** /
**`--adapt`** for name-mismatched pipes; (5) **process/def entries** — the boundary
is uniform (composite-as-process), so this is mechanical, and `def` is the
pure-function, zero-time (length-1 trace) collapse. Tests: `tests/file_entry.rs`,
`tests/invoke.rs`, `tests/invoke_trace.rs`.

## Homoiconicity goal

**Chrysalis is homoiconic when a process can construct a new reaction
(or process body) as a value, deposit it in state, and have the engine
pick it up on the next tick.** The three benchmark examples below
collectively verify this.

State is already data (a `Value` tree) and `discover_processes` is
prism's `eval`. Composites can already emit composites (cell → 2 cells
via `divide`). What's missing is first-class **patterns** and
**reactions** as values constructed in expression bodies.

## Implied bigraph assembly (state representation)

**Trees are bigraph compositions, just sugared. Keep prism's
`Value::Tree` / `Value::List` as storage; the primitives below are
constructors and pattern-matchers over it. Do NOT switch to "all
state is a sequence of algebraic primitives."**

Canonical decomposition:

| prism `Value` | bigraph reading |
|---|---|
| `Tree({k: v, …})` with `v._type = T` | `(k: T.[content of v]) \| (k₂: T₂.[…])` — keys label parallel siblings |
| `List([v₁, v₂, …])` | `v₁ \| v₂ \| …` — anonymous parallel (the `B \| B \| B` of M/R histories) |
| Scalars (`Float`, `Int`, `String`, `Bool`) | attributes *on* an ion, not place-graph atoms |
| `_type: K` field | the control K of the enclosing ion |
| nested subtree | the `.` operator's right side |

Why this choice:

- Names are how biology talks. `cell.cytoplasm.mek` matches the domain.
- Existing prism machinery (delta sentinels `_add`/`_remove`/`_replace`,
  `discover_processes`, projection application, `Schema::Tree`) stays
  unchanged.
- Anonymous parallel is available via `List` — algebra's anonymity isn't
  lost, it's one of two presentations.

## Categorical structure

Bigraphs form a symmetric monoidal **s-category** (Milner). The
objects are **interfaces** `⟨m, X⟩` — `m` sites/holes plus a set `X`
of outer names. The morphisms are bigraphs themselves: a bigraph
`G : I → J` has inner interface `I` (where it plugs in) and outer
interface `J` (what it exposes). Composition `H ∘ G` plugs `G` into
`H`'s sites and shares names. Tensor `G ⊗ H` is parallel composition
with disjoint interfaces.

This is not decoration. It tells chrysalis four things:

1. **Wiring is categorical composition.** A `process`, `step`, or
   `composite` is a morphism with `~{inputs}` as its domain interface
   and `->{outputs}` as its codomain. The `~{} ->{}` notation is the
   morphism's type signature, not a Python-shaped keyword arg block.
2. **Composite-as-process is a fact, not a convenience.** Composites
   and processes are both morphisms — they compose identically because
   the category says so. The unified port-binding interface is the
   categorical reality.
3. **Tensor = `|`.** Parallel composition's algebra (associativity,
   the empty bigraph as unit) is a theorem of the s-category, not a
   choice we have to defend.
4. **Pattern matching is morphism factorization.** A redex match
   factors a state bigraph as `C ∘ (G ⊗ id)` where `G` is the matched
   fragment and `C` is the surrounding context. prism's BRS matcher
   is computing this factorization; naming it as such fixes the
   algorithm's specification.

The notation `process Grow[...] ~{in} ->{out} (body)` should be read
as "define a morphism `Grow : ⟨in⟩ → ⟨out⟩` parameterized by
config." The body is the morphism's content.

## Bigraph primitives as first-class atoms

From Milner's BRS algebra. Currently buried inside
`prism_schema::Pattern` constructors; promote to surface syntax that
evaluates to a value:

| Atom | Surface | Notes |
|---|---|---|
| ion (no body) | `K` or `K[args]` | bare control / control with args |
| ion (with body) | `K[args](body)` | parens hold the nested content |
| port-link binding | `K ~{port: link}` | Milner's link-graph; same notation for processes and pattern ions |
| input ports (process/composite) | `K ~{port: target}` | domain interface of the morphism |
| output ports (process/composite) | `K ->{port: target}` | codomain interface |
| site (subtree var) | `?name` | binds in redex, expands in reactum |
| name var (atom) | `?name` | distinguished from site-var by context — atoms can't be subtrees |
| typed site | `?name : Sort` | site whose match must satisfy the sort |
| region / parallel | `a \| b` | symmetric monoidal tensor |
| nesting | `K[args](body)` | implicit; explicit `.` available if needed |
| link / outer name | `~name` (bare token) | shared bond between ports |
| closed link | `/name in expr` | link internal to expr (ν) |
| unbound port | `!` | port has no link; visually distinct from numeric literals |
| guard | `where expr` | predicate over bound names; closure on the rule |
| instantiation | shared `?name` across redex/reactum | carry-through of bound sites/names |

Once these are first-class operators, **normalization** matters:
`(a \| b) \| c ≡ a \| (b \| c)`, `a \| nil ≡ a`, and for matching often
commutativity. Confirm the BRS matcher's notion of "same bigraph"
agrees with the algebra, or add canonicalization before installing a
rule — otherwise two syntactic forms of the same rule fire
independently.

## Value methods (host operations as first-class)

**Update bodies can call methods on the values they receive.** Methods
are Rust functions registered against prism types via a
`MethodRegistry`. The interpreter dispatches `value.method(args)` by
looking up the method on the value's type — same model as Python+numpy
or Julia+packages: heavy lifting stays in the host, but the surface
language has unrestricted access to it.

This closes the circularity that the original "out of scope" framing
accidentally left open. Prism's existing Rust API for patterns,
reactions, BRSes, meshes, FBA solvers, physics — all become callable
inside chrysalis as methods on the relevant values. Chrysalis doesn't
*reimplement* them (they're load-bearing native code); it *exposes*
them.

### What this unlocks

```
# Build a reaction in an update body, modify it, install a new BRS.
rule = Divide[threshold: 2.0]
  .with_rate(0.5)
  .with_label('faster_divide')

# Compose pattern fragments from values.
inside = Compartment[kind: ?k] (MEK ~{out: !} | ERK ~{out: !})
custom = Reaction[redex: inside, reactum: ...]

# Update-body result: emit a new BRS into state.
{ add: {'brs_evolved': BRS[rules: [rule, custom], mode: Gillespie]} }

# Read-only ops on rich domain types.
coarse = mesh.coarsen(factor: 10)
near = coarse.vertices_near(point: cell.position, radius: 0.5)

# Pattern algebra in source — the bigraph atoms are methods too.
combined = pattern_a | pattern_b                 # parallel composition
nested   = container (combined)                  # nesting via body parens
```

### Tiers

- **Tier 1 (v1, required for the three benchmarks):**
  - Method dispatch in the evaluator.
  - `MethodRegistry` alongside `TypeRegistry`. Rust crates register
    methods on their types (`Mesh::register_methods(reg)`,
    `Pattern::register_methods(reg)`).
  - Bigraph primitives, `Reaction` constructors/modifiers, `BRS`
    constructor exposed as methods. Standard list/map/string ops.
- **Tier 2 (future):** `ProcessDef` / `StepDef` values constructible at
  runtime. The expression-body AST itself as a value, so update bodies
  can synthesize a new process body and install it.
- **Tier 3 (research):** Reflection — a process inspecting another
  running process's source. Self-modifying process bodies.

The three benchmark examples need only tier 1. Tier 2 unlocks genuinely
meta examples (evolutionary code synthesis, learned process bodies);
flag as future scope but don't gate v1 on it.

### Boundary

Methods are pure or near-pure value transformations. State mutation
still happens **only** via the value returned by an update body
(the final `|`-less expression) — methods don't write to the engine's
state directly. This preserves the projection/structural-diff model.
Engine-level operations (scheduling, triggering, applying projections)
are NOT exposed as methods — that's the runtime calling chrysalis, not
the other way around.

## Units and quantities

Scalars carry **dimension** and **unit** in their *schema*, never in the
value. A `mass` field is a `Value::Float`; what makes it a mass is its
type. Units add no new value representation — they sit where prism
already reads `extensive` / `delta` / custom-type dispatch.

The *model* is pint's (a runtime dimensional system — uom's
compile-time, type-level units cannot describe schemas defined in data,
which chrysalis schemas are). The *execution* is uom's (erased,
zero-cost). chrysalis gets both because it is a compiler, not a runtime
wrapper — see "check once, erase, run raw".

| Layer | Representation | Role |
|---|---|---|
| **Dimension** | base-dimension → **rational** exponent (`[mass]^1`, `[substance]^1·[length]^-3`) | **compatibility** — ports wire iff dimensions are *equal* |
| **Unit** | belongs to one dimension; `(scale, offset, affine?)` vs. the dimension's canonical unit | **conversion** — m/ft, pg/kg, molecule/mol (Avogadro) are units of the *same* dimension |
| **Quantity** | a field schema: `(unit, extensive?, affine?)` | the typed scalar; the stored value is a bare `Float` magnitude in `unit` |

Base dimensions are the SI seven (`[length] [mass] [time] [substance]
[temperature] [current] [luminous]`) plus dimensionless. Exponents are
rationals, so √-dimensions (noise density, fractal scalings) are
representable; biology uses integer powers, but the dimension group must
be correct in general.

### Check once, erase, run raw (no per-op churn)

Units usually fail in practice because a library (pint, `Quantity`
wrappers) boxes every number as `(magnitude, unit)` and re-validates
dimensions on *every* arithmetic op — box/unbox churn to re-prove what
is already proven. chrysalis does not pay this, because it is a
**language with a check phase**, not a library evaluated per-op:

1. **Check (compile time).** Dimensional inference runs once over each
   expression body. `+`/`−` require equal dimensions; `*`/`/` compose
   them; `^` scales exponents; a literal in a dimensioned slot takes
   that slot's unit. Mismatches are construction errors (illegal
   programs unrepresentable — resolved decision #8).
2. **Lower + erase (construction time).** The checked body lowers to
   raw-`f64` ops; **units are erased**. Where a value in unit A meets a
   slot in unit B, the compiler bakes a single constant multiply (plus
   an add, for affine), factor computed once; same-unit paths get
   nothing. Wiring resolves each conversion per *wire*, once, when the
   topology is built — not per tick.
3. **Run (runtime).** The engine executes bare `f64`: no `Quantity`
   objects, no dimension vectors, no per-op checks, no allocation —
   identical cost to hand-written unitless code, save the occasional
   baked-in constant on a converting wire.

This is F#-/uom-style **erasure** achieved over *runtime-defined*
schemas: pint's flexibility (units in data) with uom's zero cost (units
gone before execution). A library cannot do this — with no compile phase
it must check every op; a language can. The one caveat is
schema-as-state: if a process rewrites a field's unit at runtime, the
affected wires/bodies are re-checked at that structural event —
amortized to the mutation, never per arithmetic op.

### Compatibility and conversion (wiring is dimension-checked)

Wiring is composition in the s-category. The check-phase rule:
**dimensions must be equal** (`[mass]`→`[mass]` wires; `[mass]`→
`[time]^-1` errors); **units may differ**, and the conversion (scale, or
scale+offset for affine) is computed once and baked into the wire.
Crossing *dimensions* is possible only through a **context** (below).

### Affine quantities reuse prism's `delta`

The subtle case is **affine** quantities — temperature, position,
absolute time — where a *value* and a *difference* differ ("37 °C" vs
"+0.5 °C"; "1 K" is ambiguous). prism already encodes this split: a
state field is an *absolute* (affine) quantity; a process update
`{x: delta}` is a *difference* (a vector). prism's `delta` schema type
*is* the vector companion. For multiplicative units (offset 0) absolute
and delta coincide — which is why grow/divide never noticed. The affine
algebra (checked, then erased):

| op | result |
|---|---|
| absolute − absolute | delta |
| absolute + delta | absolute |
| absolute + absolute | **error** |
| delta ± delta | delta |
| `*` / `/` on an affine absolute | **error** (operate on the delta) |

### Extensivity is orthogonal to dimension

Whether a quantity **splits or copies on divide** is a separate axis
from dimension: mass `[mass]` is *extensive* (splits); concentration
`[substance]·[length]^-3` is *intensive* (copies). So `Quantity` carries
an `extensive` flag independent of unit; it affects only divide, not
wiring or arithmetic. This is the flag
`prism_schema::TypeRegistry::type_divide` already keys on (its
`DivideContext`: "extensive scalars halve unconditionally; intensive
scalars copy").

A composite's `divide()` (a value method dispatched via `type_divide`)
recurses its fields by role: the **identity** (`id`, resolved decision
#6) is reissued per daughter; **extensive** scalars split; **intensive**
scalars copy; nested composites/collections divide by their own type
(lists partition, graphs cleave). The `Divide` reaction calls
`?c.divide()` and splices the daughters in place of the matched cell —
so adding `volume: Quantity[unit: fL, extensive]` to `Cell` makes
division halve volume too, with no rule change.

### Contexts (cross-dimension conversion)

Some conversions cross dimensions and hold only in a physical situation,
so they cannot live in the global unit table. A **context** (after
pint's `@context`) is a named, parameterized set of cross-dimension
transformation rules. Two matter for biology:

- **Molar mass** — `[mass] ↔ [substance]`, via a species' molar mass
  (glucose ≈ 180.16 g/mol). The factor is the substance's own property.
- **Concentration** — `[substance] ↔ [substance]·[length]^-3`, via the
  enclosing compartment's **volume** — and the factor is *state* that
  changes as the cell grows.

molecule↔mol is **not** a context — it is a unit (a fixed Avogadro scale
*within* `[substance]`). Contexts are only for crossing dimensions.

```
context concentration (volume: Volume) (
  [substance] <-> [substance]/[length]^3 : value / volume
)
context molar (mw: MolarMass) (
  [mass] <-> [substance] : value / mw
)
```

`value` is the source quantity; `<->` is bidirectional. A parameter
(`volume`, `mw`) is bound at the conversion site by ordinary name
resolution — a constant, a place-graph path, or the value's type
metadata. Two ways this differs from pint, both bigraph-native:

1. **Parameters can be place-graph state.** `volume` resolves to the
   enclosing compartment's volume, so concentrations track a growing
   cell's volume with no extra machinery. This is *why* concentration is
   intensive: it is amount(extensive) / volume(extensive), so on divide
   both halve and the ratio is preserved — resolved decision #11's
   extensivity falls out of the context relation rather than being
   declared.
2. **Activation is structural, not a `with` block.** A region brings a
   context into scope for everything nested inside it; the place graph
   *is* the scope:

   ```
   composite Cytoplasm[volume: Volume] using concentration(volume: %.volume) (
     ...   # a [substance] amount in here reads as a concentration
   )
   ```

   A reaction in the cytoplasm whose rate law is written in concentration
   units gets each amount read as `amount / cytoplasm.volume`
   automatically — the right volume because that is where the substance
   physically sits. One-off boundary conversions stay explicit, e.g.
   `g.to[mol](molar)` (the `mw` resolves from the value's type).

**Still zero-cost.** The context lookup, the check that the bridge is
legitimate, and the choice of factor-source happen once in the check
phase and are **erased**. At runtime a context conversion is a single
arithmetic op (`x / volume`) against a constant or a state reference —
the genuine physical computation, never a re-validation. Crossing
dimensions costs exactly one baked-in op, like a same-dimension unit
conversion.

A worked multi-compartment example —
[`crates/chrysalis/ys/nuclear-shuttle.ys`](../crates/chrysalis/ys/nuclear-shuttle.ys)
— synthesises a TF in the cytoplasm, shuttles it into a 10×-smaller
nucleus, and fires a nuclear sensor on *concentration*: the sensor names
no volume, yet is correct in either compartment because the enclosing
region's context supplies it. Transport straddles both compartments, so
it converts each side explicitly — the ambient/explicit contrast in one
file.

### Surface syntax

```
# A unit belongs to a dimension, defined by relation to a canonical unit
# (SI base units kg, m, s, mol, K, … are built in). '*'/'/' give
# multiplicative units; '+' marks an affine offset.
unit pg       : [mass]        = 1e-12 kg
unit fmol     : [substance]   = 1e-15 mol
unit molecule : [substance]   = mol / 6.02214076e23     # Avogadro — same dimension
unit degC     : [temperature] = K + 273.15              # affine

# A dimensioned scalar type names a (unit, flags); dimension is inferred
# from the unit. `extensive` opts into splitting on divide (default
# intensive); `affine` marks a point quantity (default vector).
Mass = Quantity[unit: pg,   extensive]
Rate = Quantity[unit: 1/s]                 # intensive
Conc = Quantity[unit: fmol/fL]             # [substance]·[length]^-3
Temp = Quantity[unit: degC, affine]        # differences are deltas
```

`Quantity[…]` lowers to a `Float` schema annotated with the unit;
field-position literals read in the field's unit (`Cell[mass: 1.2]` is
1.2 pg). A unit-annotated literal (`1.2 pg`) is dimension-checked
against the slot.

### Runtime support required

- prism-schema: a `Dimension` (rational-exponent vector) and `Unit`
  `(dimension, scale, offset, affine)`, a unit registry, and optional
  `unit`/`extensive`/`affine` metadata on the `Float` schema. Dimension
  equality + conversion-factor computation are ordinary functions called
  by the checker — never on the hot path.
- A dimensional-inference pass in the chrysalis compiler (the "check"
  phase) that erases to raw-`f64` bodies with conversions baked as
  constants.
- `type_divide` reads the `extensive` flag for scalar fields; affine
  arithmetic rules in `eval.rs`. uom may back canonical-SI scale
  constants internally; it is not the surface or runtime value model.
- Context resolution: a cross-dimension conversion resolves its context
  and factor-source (constant, place-graph path, or type metadata) in
  the check phase and erases to one runtime op, like a unit conversion.

**Status (implemented).** `prism_schema::units` provides `Dimension`
(rational exponents), `Unit`, `Context`/`Bridge`, and `resolve_conversion`
→ `Conversion` — the erased, single-`f64`-op form (`Conversion::apply`).
`chrysalis::units` resolves a program's `unit`/`context` declarations
over the SI base units and dimensionally checks expression bodies,
resolving in-scope context coercions (exercised by the `nuclear-shuttle`
fixture tests: `Transport`'s `flux` works out to `[substance]`; `Sense`'s
`tf > k_on` is a mismatch alone but type-checks under the nucleus's
`concentration` context). `UnitEnv::lower_body` then **erases** a
checked body to a unit-free `Expr` with conversions baked in as ordinary
arithmetic (`tf > k_on` → `tf > k_on * volume`), which runs on bare
`f64` through `eval` with unit-correct results — the un-erased body gives
the wrong answer (`lowered_sense_runs_unit_correct_through_eval`).
`compile()` then wires this into the live engine: it lowers each process
body, surfaces each context factor as an input port, and the engine
wires it from the enclosing compartment's slot — so a context coercion
runs unit-correct inside a real composite (`tests/units_engine.rs`: a
below-threshold count gives a negative result *only* because the volume
coercion ran). `using`-path injection also handles a factor name that
differs from the compartment slot — `compile()` injects each `using` arg
into the child processes that declare it
(`using_injection_handles_factor_name_differing_from_slot`).
**Cross-boundary (RESOLVED 2026-05-20):** routing a factor from an
*ancestor* compartment to a process nested inside a child composite. The
earlier "blocked" diagnosis (a deeper composite-scheduling bug) was a
mis-diagnosis — composites constructed in another composite's body DO
execute as real sub-engines (see `project_composite_execution_gap`). The
real fix is a small compile-time transform (`thread_factor_inputs` in
`compile.rs`): a composite that transitively contains a process needing
factor `F`, but doesn't itself provide `F` via `using`, gains `F` as an
input port. The auto-built input bridge (`F → [F]`) plus same-name default
wiring then carry `F` down each link from the activating ancestor to the
consumer. Proven by `context_factor_crosses_composite_boundary`: the
nested `Report` receives the `Tissue`'s volume across the `Cell`
sub-engine boundary, so the below-threshold result is negative (the
coercion ran) rather than 0 (never arrived) or positive (units ignored).

## Syntactic kernel

The whole language is built from a small set of forms. Everything else
is sugar over them.

### Forms

| Form | Use |
|---|---|
| `K`, `K[args]`, `K[args](body)` | term construction (control + named/positional args + optional nested body) |
| `a \| b` | parallel composition (symmetric monoidal tensor) |
| `K ~{port: target}` | input port bindings |
| `K ->{port: target}` | output port bindings |
| `~name` (bare token) | link variable |
| `?name`, `?name : Sort` | pattern variable, optionally sort-constrained |
| `!` | unbound port / empty |
| `K[args] (body)` after a definer keyword | top-level definition (see Naming) |
| `name = expr` | value binding (top-level or inside a body) |
| `redex => reactum` | reaction rule (only valid inside `reaction K[…] (…)`) |
| `let x = e in body` | local binding (sugar) |
| `if c then a else b` | conditional (sugar) |
| `value.method(args)` | UFCS: sugar for `method[args](value)` |
| infix `+ - * / == < > && \|\|`, prefix `not` | sugar for control terms (`Add[a, b]`, `Not[x]`, etc.) |
| `'literal text {expr}'` | string with embedded expression interpolation |
| `\| ` at line end (body separator) | body is a `\|`-separated sequence; the line without `\|` is the value |
| `# …` | line comment |

### Naming convention

**Meta-syntax is lowercase; object-syntax is capitalized.**

- Lowercase **definer keywords** introduce entities: `process`, `step`,
  `composite`, `reaction`, `pattern`, `let`, `if`, `where`, `in`.
- Capitalized **controls** are the things being defined or
  constructed: `Cell`, `Grow`, `MEK`, `Phosphorylate`, `InCompartment`,
  `Reaction`, `ProcessDef`, `BRS`.

The same name carries the definer/constructor duality:

| Definer (lowercase, meta) | Constructor (capitalized, value) |
|---|---|
| `reaction Phosphorylate[…] (redex => reactum)` | `Reaction[redex, reactum, rate]` — build at runtime |
| `process F[…] ~{} ->{} (body)` | `ProcessDef[expr, schema]` (tier 2) |
| `pattern InCompartment[…] (body)` | `Pattern[…]` |

`reaction X[args] (body)` desugars to a value `Reaction[args, body]`
registered under the name `X`. Same content, two surface forms — the
lowercase/capitalized distinction is what makes it readable that those
*are* the same thing.

**Status (homoiconic-unification Stage 2b — `docs/homoiconic-unification.md`):**
the FLAT-kind capitalized constructors are now **implemented** as real values that
route through the same lowering core as their definers, holding the invariant
**`Kind[…]` ≡ `quote(kind definer)`**:
- **`Reaction[redex: …, reactum: …, rate: …]`** → the reaction value (via `build_rule`,
  the one reaction-lowering core). A `BRS[rules: […]]` accepts both a `reaction` definer
  (`FOREIGN_RULE`) and a `Reaction[…]` value (`FOREIGN_REACTION`), so **`def X =
  Reaction[…]` is a drop-in for `reaction X (…)`** (parameterless — see the gap below) —
  and a capitalized value-`def` now resolves as a value (the `entity.binding` dispatch).
- **`Pattern( fragment )`** → a matcher value (`FOREIGN_PATTERN` wrapping a `prism
  Pattern`, lowered via `eval_pattern_top` — a reaction redex's form), usable with
  `find_matches` (the substrate for `count(…)` / `.matches(…)`).

**The FLAT/RICH split is PROVISIONAL, not a design law** (reviewed w/ the human,
2026-06-09). Two axes: *Axis A* — the **definition** as data
(`EntityDef::to_value`/`from_value`/`compile_value`) — is **already uniform across every
kind**; *Axis B* — the **instance/runnable** value — is where the split lives today:
process/composite → transparent DATA (`build_spec_value` map), reaction/pattern → opaque
`Foreign`. That is the **Stage-2 reaction-opacity exception** surfacing in the
constructor, *not* a "rich kinds can't have a bracket" law: a `Process[…]` constructor
would be Axis B (an instance) and so would **not** duplicate `to_value` (Axis A). The
asymmetry **dissolves at Stage 4**, when every consumer boundary —
`discover_processes` (nodes), `collect_reactions` (rules), `find_matches` (patterns) —
evals DATA → runnable, so every constructor can produce DATA (quote) uniformly. The
bracket is offered where inline construction is ergonomic (reactions/patterns are built
inline in reactums / BRS lists; processes are usually named) — a presentation choice.
**Update (synth A6 Phase 2):** `Composite[state:, bridge:, schema:] ~{in} ->{out}` now
*exists* as a rich-kind value constructor — a reactum authors a NEW composite module type
inline as data, lowering to the composite instance envelope (`{_type:"composite",
address:"local:Composite", config:{…args}, …}`) that `discover_processes` /
`Composite::from_config` instantiate (`tests/composite_constructor.rs`, proven via the 4a
node rung). So the FLAT/RICH asymmetry is further dissolved on the constructor surface; the
remaining gap is purely ergonomic (`Process[…]` generic is deferred — a process's inner
nodes are reached as named-native brackets like `Oscillator[…]`).
2b gaps **closed**: (1) parameterization — `X[args]` now *calls* a function, so a
parameterized `reaction X[p](…)` has the def-form `def X(p) = Reaction[…]` instantiated
`X[p: v]` (the `[]`/`()` reconciliation; the param reaches a fire-time guard/rate via
the captured closure); (2) uniformity — `definer ≡ quote ↔ reify ↔ run` is demonstrated
(and it fixed a real bug: a program's `main` binding was dropped through `quote`).

**Stage 4c landed — the FLAT/RICH dissolution.** A STRUCTURAL reaction, from ANY
producer (the `reaction` definer, `Reaction[…]`, `=>`-in-value, `compile_reaction`), now
evaluates to **transparent `{_pat:"Rule"}` DATA** (`ReactionRule::to_data_value`), not an
opaque `Foreign`; the BRS rule boundary evals it back (`collect_reactions`/`push_reaction`
in `brs.rs` — core; `brs_rules` in chrysalis — both via `ReactionRule::from_data_value`).
So a reaction is now plain data, **uniform with a process spec**, and crosses every
boundary (link / bridge / rest / JSON wire) as JSON. A COMPUTED reaction (guard / computed
reactum / rate closure) keeps the in-process `Foreign(FOREIGN_RULE)` carrier (its closures
need the evaluator at fire time).

**Stage 4a landed — the node rung as a surface builtin.** The reflective tower's LOWER
rung is now first-class in `.ys`: **`instantiate(state, time?)`** (a core-reflection
builtin in `eval_call`, beside `core_processes` / `compile_reaction`) brings a
spec-bearing STATE to life and runs it for `time` (default 0 = instantiate + settle),
returning the evolved state. It is a **thin** call to prism — `Engine::from_state` +
`discover_all_processes` + `run` — the surface exposure of `discover_processes`; it never
clones engine logic, and instantiation bottoms out in the SAME
`ProtocolRegistry::instantiate` door the engine, `Core::instantiate`, and
`Core::instantiate_spec` use. Crucially it lives in the **Evaluator**, so it runs against
the program's OWN `Core` — a built spec's `local:Tick` address resolves to *this program's*
`process Tick` factory (a core-less `meta::` host fn could not). Paired with `meta::eval`
(the UPPER rung, `Expr → spec data`), it makes the metacircular identity
**`run(p) = instantiate(surface_eval(quote(p)))`** a callable loop — `instantiate(spec-as-data)
≡ run(spec-as-body)` (`tests/metacircular_node_rung.rs`).

**Axis-A `from_value` completeness landed (12/12 kinds — total).** `EntityDef`/`Program`
`to_value` ↔ `from_value` now round-trips **every** kind: process / step / composite /
reaction / function / pattern / type / contract / protocol / unit / context / binding — the
strong witness is the total identity *quote ∘ reify ∘ quote == quote* over a program touching
all twelve (`tests/entity_roundtrip_complete.rs`). Along the way `to_value` was made faithful
where it had been lossy: reaction + function params went from name-only to full params (schema
+ default), reaction `guard` / `rate` were being dropped, and `type` / `contract` / `protocol`
/ `pattern` / `unit` / `context` weren't fully serialized. `unit`/`context` reify via
**structural** `Dimension` / `UnitExpr` / `Ratio` / `ContextRule` codecs — all surface AST
(`ast.rs`), so no core dependency. And the **metacircular orchestrator** north star *runs*
(`coord/orchestrator.ys`: `load('board.ys').run(1.0)` over our own coordination board) — once
a path-blind-`parse_program` bug in `load`/`run_from_source` was fixed to `parse_file` (so a
loaded program's own relative sibling imports resolve). The homoiconic face is closed and
dogfooded end to end.

### Evaluation: every name a value, every value composes

The REPL (and eval generally) follows one rule — **read → AST → eval → Value →
print, where every name has a value and every value composes**:

- A **bare definer** is a value: a `def` function → a first-class function; a
  `composite`/`process`/`step` → its **no-arg instantiation** (the
  composite-as-data spec), so `all` ≡ `all[]`. A definer that needs args reports
  the missing arg (not "unbound"). Composites are *data*; functions are
  *behaviour* — hence the spec vs callable distinction.
- **Field access composes on any expression**, not just place-paths: `.field`
  on a term/method/call result is a value field read (`all[].config.bridge`,
  `all.config.state.field.what`), exactly as `.method()` already works on any
  base. An absent field is `none`, never an error. (`var.seg` and the bracketless
  `all.config.bridge` resolve their root through the same logic, so `[]` is
  optional.)

A composite-as-data spec is `{address, config: {state, bridge, schema}, inputs,
outputs}` — so the *interface* view is `…config.bridge` (the wiring contract) and
the *internals* are `…config.state` (reaching in; prefer the bridge, per the
black-box principle).

### Path syntax

| Symbol | Meaning |
|---|---|
| `%` | self / here / this composite as a value |
| `^` | parent (one place-graph level up) |
| `name` | child / field of current location |
| `%.id` | `id` field of self |
| `^.cells` | `cells` field on parent |
| `@` | **composite bridge operator** (in a port decl) — not a path |

`%` is the place-graph self-reference (the empty wire `[]`, which the
container-relative engine resolves to the process's own container).
`@` was the self sigil before decision #23 and is now exclusively the
composite-bridge operator — see *Composite bridge syntax* below.

### Composite bridge syntax (decision #23)

A composite's interface ports map to paths in its inner state — the
**bridge**. By default the bridge is *name-inferred*: a port `F` maps to
the same-named top-level field `[F]`. To target a *nested* or
*differently-named* inner path, declare it explicitly with `@`:

```
composite Group
  ~{values :: Map[Float] @ fields.values}   # input bridges to fields.values
  ->{total  :: Float      @ stats.total}     # output bridges to stats.total
( fields: { values: {} } | stats: { total: 0.0 } )
```

The full port-decl grammar is
`name :: Type [@ inner.path] [fulfills Contract] [= default]`. The bridge
clause is optional; absent, name-inference applies (backward-compatible).
The explicit path lands in `config.bridge.{inputs,outputs}` as a wire
(segment list) consumed by `Composite::from_config` — `eval::build_composite_outer`
builds it. This makes the bridge a *declared* part of the interface rather
than an assumption recovered from port names.

### Records vs maps

- **Records** (compile-time-known schemas: output ports, state fields,
  config params): `{ field: expr, field: expr }` — bare identifiers
  are literal field names.
- **Maps** (runtime-keyed collections): `{ 'key_string': expr, … }` —
  keys are string expressions. Use string interpolation `'{var}'` for
  computed keys.

### Operator precedence

From tight to loose:

1. `.` (field access / method call)
2. unary `-`, `not`
3. `*`, `/`
4. `+`, `-`, `++` (string concat)
5. `==`, `!=`, `<`, `>`, `<=`, `>=`
6. `&&`
7. `||`
8. `|` (parallel composition) — left-associative
9. `=>` (reaction) — non-associative, one per `reaction` body

### Structural delta sugar

`replace id with { 'key1': val1, 'key2': val2 }` desugars to
`{ _remove: [id], _add: { key1: val1, key2: val2 } }`.

### Cross-composite redexes are LINK-GRAPH matches (resolved #40)

A cross-composite redex matches **sealed composites by their published
ports on a shared link** — never by descending into inner structure.
This is MAPK's `~bond` mechanism (link-graph matching of molecules)
**lifted to composites**: the principled form uses `?`-prefixed site
binders for the composite heads and a shared link var `~e` for the
coupling:

```
reaction Diffuse (
  ?west :: Cell ~{edge: ~e} | ?east :: Cell ~{edge: ~e}
  => ?west.balance(~e) | ?east.balance(~e)
)
```

- **`?west` / `?east`** — `?`-prefixed SITE BINDERS (the existing site
  sigil) — the redex item's HEAD is the binder, not a control `K[args]`
  (restriction (1) removed). `:: Cell` is the (recommended) sort
  annotation — like every other redex item, a cross-composite item is
  typed; the binder constrains to `Cell`. The `?` does the
  disambiguation — no case rule needed.
- **`~{edge: ~e}`** — `~{}` keeps its ONE meaning (ports↔links); `~e`
  is the shared LINK VAR that COUPLES the two composites.

**Status (2026-06-08, #43/#40 matcher slice).** The matcher is LIVE: the
sorted form above parses, lowers (`eval_pattern_top` → `Pattern::Bind`
over `Pattern::LinkVar`, the same machinery as `~bond`), and COUPLES
two composites on a shared edge link (and does NOT couple on different
links) — `crates/chrysalis/tests/cross_composite_link_redex.rs`. `|`
works directly at the reaction top level (no required wrapping parens):
the reaction's redex / reactum positions parse with the SAME body
grammar (`parse_parallel_items`) as a term/composite body, so `|` means
the same thing everywhere a parallel composition appears, and a wrapped
`( a | b )` is just optional grouping (MAPK's form is unchanged).
DEFERRED: the reactum-as-method firing (`?west.balance(~e)`) through a
reactor end-to-end; the SORTLESS sugar `?west ~{edge: ~e}` (no
`:: Sort`) — would need an `Expr::Site` ports field for faithful
unparse, and the sorted form is more type-safe (so it's canonical);
restriction (2), a `?v` port target that BINDS the port value (today a
`?v` target lowers to an anonymous `Pattern::Site` whose binding isn't
recorded) — add when a consumer needs it.

This **expresses the coupling** — `~e` IS the connection. It
generalizes over which composites (any two coupled by `e`, never
hardcoded keys). It is encapsulation-clean by construction (we never
read inner structure). And it unifies the two scales: ONE BRS matches
links at the molecule level AND at the composite level — the "one BRS
rewrites both levels" thesis as syntax.

**Place-graph vs link-graph patterns — distinguish them.**

- **Place-graph patterns** (nesting `( )`, map literals
  `{ alice: { state: ?a } }`) match WITHIN AN OPEN REGION you own and
  can see into. They describe *location* — "the child named alice has
  state". Use them when you're INSIDE a composite, matching its body.
- **Link-graph patterns** (`?site ~{port: ~link}`) match ACROSS
  SEALED BOUNDARIES via published ports. They describe *coupling* —
  "two things sharing link e". Use them BETWEEN composites.

The earlier sketches `alice~{state: ?a} | bob~{state: ?b}` (raw
control-name as peer name) and `{ alice: { state: ?a }, bob: ... }`
(map literal descent across the boundary) conflate these two graphs:
they use place-graph machinery for what is really a link-graph
operation. Use the link-graph form for cross-composite, the
place-graph forms only WITHIN a region you own.

The orchestration that runs a cross-composite redex
(`prism_schema::fire_across_composites`, S2 / merge-protocol slice 6)
operates on the FLAT bigraph after unfurl: it sees the link graph
spanning the unfurled composites. The site binders + shared link var
syntax is what lets the redex name peers on that link without
piercing the still-sealed composites.

### Body convention

Process/step/composite/pattern bodies are a parallel composition of
statements (each ending in `|`) followed by a final expression
(no `|`) which is the body's value. Bindings introduced before the
value expression are in scope inside it. The same convention applies
inside update bodies and inside reaction redex/reactum positions.

```
(
  delta = mass * rate * interval |
  ratio = delta / 2.0 |
  {mass: delta, ratio: ratio}
)
```

The trailing line without `|` carries semantic weight: it's the
return value, and the lack of separator marks it visually.

## Type names resolve through one registry (a composite *is-a* type)

A surface type name (`Cell`, `Mass`) appears in two guises — nominally (the name
`Cell`) and structurally (the schema it stands for). Chrysalis used to bridge them
by **eagerly resolving names to structural schemas in several different lowering
functions**, some program-aware (→ `CompositeLink`), some not (→ opaque
`Custom{"Cell"}`). They disagreed, and the looser result kept winning — a step's
`cells :: map[Cell]` lowered program-unaware to `Map{Custom{Cell}}`, the engine
`promote`d it over the slot's declared `Map{CompositeLink}`, and
`divide_by_schema` then saw an unregistered `Custom` and *shared* a cell's
extensive `mass` instead of halving it. Re-deriving and tweaking the schema at
runtime to chase the answer.

The model now is single-sourced:

- **A named type is one entry in the `TypeRegistry`, and its representation is its
  real schema.** A `composite C` registers with representation =
  its `CompositeLink` (`register_user_types`); a type alias `Mass =
  Quantity[…, extensive]` registers with representation = `Delta`. So a
  `Custom{Cell}` value — however a slot got lowered — **delegates the whole
  algebra** (`divide`/`apply`/`serialize`) to its structural representation:
  `divide_by_schema(Custom{Cell})` → `type_divide("Cell")` →
  `divide_by_schema(CompositeLink)`. "Nominal `Cell` *is-a* structural
  `CompositeLink`" — the delegation a typeclass/representation gives you, without
  Rust inheritance. (This *defines* the composite-as-`Custom` case rather than
  forbidding it.)
- **One name-resolving lowering** (`lower_schema_in_program`), used at every site —
  composite/process/step ports, `ExprProcess` and `ExprStep` ports (via the
  evaluator's program). The program-unaware `lower_schema` survives only as the
  leaf base it delegates to and as the `composite_link` cycle terminator. There is
  no second way to turn a type name into a schema, so nothing can disagree.
- **Type aliases are first-class `Def::Type`** (recorded for same-file inlining so
  the units pass still sees the `Quantity`, *and* emitted as a def) — so a
  cross-file `mass :: Mass` (alias from `grow.ys`, used in `cell.ys`) carries
  through `.ys` imports and registers, instead of resolving to nothing.
- **The declared schema is authoritative; the runtime doesn't degrade it.** The
  composite-as-type registration makes the algebra robust to whichever schema a
  slot carries, so a stray `Custom{Cell}` still divides correctly.

Gotcha for maintainers: `composite_link`'s cycle guard (`building` stack) must
wrap the **ports**, not just the inner schema — once ports lower program-aware
they reference composites (`Cell`'s `environment :: map[Cell]`), and computing
them after the guard popped recurses forever. A back-edge emits a shallow link
(empty inner + program-unaware ports = the terminator).

The modular cell library demonstrates the whole thing: `grow.ys` (metabolism),
`divide.ys` (the propose step), `cell.ys` (imports both), `environment.ys`
(stream-addressed, parallel cells + a `.ys` `Divider` step that scans the cells
**map** — `.ys` now has map comprehensions, `{k: v for k, x in m if p}`). `chrysalis
run environment.ys` runs parallel `stream:cell.ys` children that grow, divide, and
**conserve mass (42.0)**, terminating — the surface twin of
`prism-bigraph/tests/cells_division.rs`.

## Compilation map

| Chrysalis | Prism target |
|---|---|
| `process P[cfg] ~{in} ->{out} (body)` | `ProcessRegistry` entry; factory creates a `Process` whose update runs the interpreted body |
| `step S[cfg] ~{in} ->{out} (body)` | `ProcessRegistry` entry; factory creates a `Step` |
| `composite C[cfg] ~{in} ->{out} (body)` | `ProcessRegistry` entry; factory produces a state subtree (with `_type: "C"` markers + sub-component wires) realize-eligible for `discover_processes` |
| `pattern P[args] (body)` *(planned — gated on splice semantics)* | a named redex fragment, substituted + spliced into a redex |
| `reaction R[cfg] (redex => reactum)` | function returning `ReactionRule` value |
| `expr { … }` block (tier 2) | `Value` with `_type: "Expr"` + inferred return-schema field; constructors live in `MethodRegistry`. `ProcessDef.from_expr(e, schema)` lifts to an installable process if `e.schema` matches |
| `~{port: target}` | `Interface.inputs` IndexMap (domain of the morphism) |
| `->{port: target}` | `Interface.outputs` IndexMap (codomain of the morphism) |
| `^` in path | `..` in prism wire-resolution (one level up — the place-graph container) |
| `%` in path | `["%", …]` — the process's OWN node (self face, e.g. a Map cell's `%.mass`). NOT the container; a child reading its composite's slot is a bare sibling `["slot"]`. (A `using ctx(f: @.slot)` factor injected onto a child rebases `@.slot` to that bare sibling — see `sibling_wire`.) |
| `port :: T @ inner.path` | explicit `config.bridge` wire (else name-inferred `[port]`) |
| `Cell[mass: 0.5]` inside a delta | `Value::Map` with `_type: "Cell"`, `config: {…}` — `discover_processes` instantiates |
| `replace x with {…}` | `Value::Map` with `_remove: [x]`, `_add: {…}` |
| `'{var}'` | string with template interpolation at runtime |
| `!` (as port binding) | `Pattern::absent()` in patterns; no-link in concrete bigraphs |
| `K \| L` | parallel composition; lowered to `IndexMap` siblings or `List` siblings depending on naming |

## Benchmark examples (acceptance test)

The benchmarks are tiered. Tier-1 benchmarks (1–3, static) verify
homoiconicity of patterns and reactions. The tier-2 benchmark (3b,
evolving M/R) verifies process bodies as first-class values with
schema-driven typed construction.

All must be expressible in chrysalis **with reactions, patterns, and
(for tier 2) process-body Exprs constructed in surface syntax as
values**.

### 1. Grow/divide (unbounded)

Division becomes a runtime-constructed `reaction Divide[threshold]`
installed in a parent BRS, not a hardcoded `step` — and it calls the
cell's own `.divide()` instead of hard-coding the split, so the rule is
agnostic to which fields are extensive. Tests: reactions-as-values,
parameterized rules, `where` guards, `.divide()` dispatch, units.
Surface:
[`crates/chrysalis/ys/grow-divide-unbounded.ys`](../crates/chrysalis/ys/grow-divide-unbounded.ys).

**Status (2026-05-20) — staged; three runnable rungs.**

1. **Internal division** —
   [`tests/grow_divide.rs`](../crates/chrysalis/tests/grow_divide.rs): a
   `Divide` *step inside* each cell writes daughters up to the parent
   `cells` map through the bridge. Faithful port of upstream
   `growth_division.py`. (Cell `['0'] → ['0_0','0_1']`.)
2. **Homoiconic external division** —
   [`tests/grow_divide_homoiconic.rs`](../crates/chrysalis/tests/grow_divide_homoiconic.rs)
   (fixture `grow_divide_homoiconic`): the research point — a first-class
   `Divide` reaction installed in a parent `BRS` matches `?c :
   Cell[mass:?m]` and replaces the matched cell. Enabled by (a) the
   **container layout** — each cell is `{_type: Cell, mass: <exported>,
   body: <subengine>}`, so the exported mass is a matchable per-cell
   sibling (no collision) — and (b) the **name-aligned matcher**
   (`prism_schema::reaction`): a redex key that names a state field binds
   that field BY NAME, independent of order, tolerating extra fields. This
   rung (Step 2a) uses a LITERAL split (`?m / 2`) in the reactum.
3. **`?c.divide()` — NOW LIVE (2026-05-20).**
   `tests/grow_divide_homoiconic.rs` runs the real reaction and divides for
   real (`0` → `{0_0, 0_1}`, mother removed). Surface:

   ```
   reaction Divide[threshold :: Mass = 2.0] (
     ?cid : ?cell::Cell  where ?cell.mass > threshold  =>  ?cell.divide(?cid)
   )
   ```

   - **`::` = typing, `:` = key:value** (disambiguates the overloaded colon).
   - **As-pattern:** a nested typed site `?cell::Cell` binds the matched cell
     VALUE to `?cell` (`prism_schema::reaction::Pattern::Bind`); the
     top-level `?cid : …` still binds the key.
   - **`.divide()` is type-relative + schema-driven:** a value method
     (`MethodRegistry`) dispatched on the cell's `_type`, deriving the cell's
     instance schema from the program (mass = extensive → `Delta`) and
     running `divide_by_schema` — extensive halves, the rest shared, **no
     literal `mass / 2`**. The id is passed in (`divide(?cid)`), so the cell
     stores no id (the map key IS the id). The BRS does the structural
     rewrite (`_remove ?cid` + `_add daughters`) — a reaction changes
     structure; it does not emit a divide sentinel.

Memories: `project_grow_divide_internal_first`, `reference_schema_driven_divide`,
`reference_matcher_name_aligned`, `reference_bridge_state_in_updates_out`.

```
unit pg : [mass] = 1e-12 kg
Mass = Quantity[unit: pg, extensive]
Rate = Quantity[unit: 1/s]
Time = Quantity[unit: s]

process Grow[rate: Rate = 0.2]
  ~{mass: Mass, interval: Time = 0.1}
  ->{mass: Mass}
(
  delta = mass * rate * interval |   # [mass] = [mass]·[1/time]·[time]
  {mass: delta}
)

composite Cell[id: String, mass: Mass = 1.0, growth_rate: Rate = 0.02]
  ~{} ->{mass}
(
  mass: mass |
  Grow[rate: growth_rate] ~{mass: mass} ->{mass: mass}
)

reaction Divide[threshold: Mass = 2.0] (
  ?c : Cell[mass: ?m] where ?m > threshold
  =>
  ?c.divide()           # mass (extensive) halves; rate (intensive) copies; id reissued
)

composite Environment[cells: Map[Cell], threshold: Mass = 2.0]
  ~{} ->{cells}
(
  cells: cells |
  BRS[rules: [Divide[threshold: threshold]]] ~{state: cells} ->{state: cells}
)

main = Environment[cells: {'0': Cell[id: '0', mass: 1.2]}]
main.run(10.0)
```

### 1b. Grow/divide on a shared resource (glucose)

Same cells and the same `Divide` rule, but growth is bounded by a finite
glucose pool shared across the environment — one link (a hyperedge) over
all cells. Each cell reads the pool, grows at a Monod-saturating rate
`mu = mu_max·S/(k_half+S)`, and draws glucose in proportion to biomass
made (`consumed = grew/yield`), emitted as a negative `delta` the pool's
additive apply depletes. As the population goes exponential the pool
empties and `mu → 0`, so total biomass *saturates* at ≈ `yield·S₀`
rather than diverging — that asymptote is the test. Adds: input/output
exchange ports bridged to a shared parent field; and `HalfSat`, a
quantity of the *same dimension* as the pool but intensive — extensivity
⊥ dimension. Surface:
[`crates/chrysalis/ys/grow-divide-glucose.ys`](../crates/chrysalis/ys/grow-divide-glucose.ys).

```
Glucose = Quantity[unit: fmol, extensive]   # the shared pool — depletes
HalfSat = Quantity[unit: fmol]              # Monod K — intensive, same dimension
Yield   = Quantity[unit: pg/fmol]           # biomass per glucose

process Grow[mu_max: Rate = 0.2, k_half: HalfSat = 50.0, yield: Yield = 0.5]
  ~{mass: Mass, glucose: Glucose, interval: Time = 0.1}
  ->{mass: Mass, glucose: Glucose}
(
  mu       = mu_max * glucose / (k_half + glucose) |
  grew     = mu * mass * interval |
  consumed = grew / yield |
  {mass: grew, glucose: -consumed}     # +biomass here; −glucose to the shared pool
)

composite Cell[id: String, mass: Mass = 1.0,
               mu_max: Rate = 0.2, k_half: HalfSat = 50.0, yield: Yield = 0.5]
  ~{glucose: Glucose} ->{mass: Mass, glucose: Glucose}
(
  mass: mass |
  Grow[mu_max: mu_max, k_half: k_half, yield: yield]
    ~{mass: mass, glucose: glucose} ->{mass: mass, glucose: glucose}
)

# Divide as in #1. The Environment holds the shared `glucose: Glucose`
# pool, which bridges to every cell's glucose port.
composite Environment[cells: Map[Cell], glucose: Glucose = 1000.0, threshold: Mass = 2.0]
  ~{} ->{cells, glucose}
(
  glucose: glucose |
  cells: cells |
  BRS[rules: [Divide[threshold: threshold]]] ~{state: cells} ->{state: cells}
)
```

### 2. MAPK signaling

Port of `crates/prism-mapk/src/rules.rs` to chrysalis. Seven reactions
built from a shared `InCompartment[kind, contents]` pattern fragment.
Tests: pattern composition / parameterized patterns, link variables
(`~bond`), unbound ports (`!`), deep nesting (Cytoplasm > Nucleus > pERK).

```
pattern InCompartment[kind, contents] (
  Compartment[kind: kind] (contents | ?rest)
)

reaction Phosphorylate[rate: Float = 2.0] (
  InCompartment[?k, MEK ~{out: !} | ERK[name: ?n] ~{out: !}]
  =>
  InCompartment[?k, MEK ~{out: ~bond} | pERK[name: ?n] ~{out: ~bond}]
)

reaction Dissociate[rate: Float = 0.5] (
  InCompartment[?k, MEK ~{out: ~bond} | pERK[name: ?n] ~{out: ~bond}]
  =>
  InCompartment[?k, MEK ~{out: !} | pERK[name: ?n] ~{out: !}]
)

# (... five more analogous rules for dephosphorylation and translocation)
```

### 3. Rosen M/R closure (tier 1, static)

F:A→B, B+F→φ, φ+B→F. Each entity carries a `blueprint` payload;
lineage of mechanism flows through data. The B+F→φ rule matches on
*shared* blueprint between two co-located ions.

```
process F[blueprint: FRecipe, rate: Float]
  ~{a_pool: Map[A], self: @, interval: Float}
  ->{products: Map[B]}
(
  a = a_pool.consume_one() |
  {products: {'{fresh_id()}': B[blueprint: blueprint, source: a]}}
)

composite B[blueprint: FRecipe, source: A]
  ~{}
  ->{blueprint, source}
(
  blueprint: blueprint |
  source: source
)

step Phi[blueprint: FRecipe]
  ~{b: B}
  ->{add: Map, remove: Set}
(
  { add: F[blueprint: b.blueprint, rate: b.blueprint.rate],
    remove: b }
)

reaction MakePhi[rate: 0.3] (
  F[blueprint: ?bp] | B[blueprint: ?bp]
  =>
  F[blueprint: ?bp] | Phi[blueprint: ?bp]
)

reaction MakeF[rate: 0.4] (
  Phi[blueprint: ?bp] | B[blueprint: ?bp]
  =>
  F[blueprint: ?bp, rate: ?bp.rate]
)
```

### 3b. Evolving M/R (tier 2 — process bodies as first-class)

The static M/R example uses an `FRecipe` payload (just config
parameters). The evolving version replaces it with an **`Expr`-valued
blueprint**: F's body is itself a value, propagated forward, mutated,
and reinstantiated by φ. The result is a population of M/R closures
that experiment with variants — lineages whose mutations preserve
viability persist; lineages whose mutations break the F → B → φ → F
loop die out.

This is **Fontana's AlChemy** with explicit M/R roles. The contribution
beyond AlChemy is **schema-driven typed construction**: prism-schema
guarantees mutations preserve the F-body signature, so the population
is *always* well-typed. Selection acts on closure viability, not on
syntactic correctness — there are no garbage variants to filter.

Tests: process-defs as values (tier 2), `Expr` as a first-class value
with `MethodRegistry`-registered constructors, mutation operators as
schema-preserving methods (`point_mutate`, `crossover`,
`subtree_swap`), runtime `ProcessDef.from_expr` install with schema
matching.

```
# Implicitly-quoted Expr; every operation inside is schema-checked
# at construction time. Inferred schema:
#   ~{a_pool, self, interval} -> {products}
default_F_body = expr (
  a = a_pool.first() |
  {products: {'{fresh_id()}': B[blueprint: self.blueprint, source: a]}}
)

process F[
  blueprint: Expr where blueprint.schema = F.body_schema,
  rate: Float
]
  ~{a_pool: Map[A], self: @, interval: Float}
  ->{products: Map[B]}
(
  new_blueprint = blueprint.point_mutate(rate: 0.05) |
  a = a_pool.first() |
  {products: {'{fresh_id()}': B[blueprint: new_blueprint, source: a]}}
)

composite B[
  blueprint: Expr where blueprint.schema = F.body_schema,
  source: A
]
  ~{}
  ->{blueprint, source}
(
  blueprint: blueprint |
  source: source
)

# Phi installs a fresh F process whose body IS b.blueprint.
# ProcessDef.from_expr is schema-checked; phi cannot install a
# malformed F.
step Phi[]
  ~{b: B}
  ->{add: Map, remove: Set}
(
  let new_F = ProcessDef.from_expr(b.blueprint, schema: F.signature) in
  { add: {'{fresh_id()}': new_F[blueprint: b.blueprint, rate: 1.0]},
    remove: b }
)

reaction MakePhi[rate: 0.3] (
  F[blueprint: ?bp] | B[blueprint: ?bp]
  =>
  F[blueprint: ?bp] | Phi[blueprint: ?bp]
)

reaction MakeF[rate: 0.4] (
  Phi[blueprint: ?bp] | B[blueprint: ?bp]
  =>
  F[blueprint: ?bp, rate: ?bp.rate]
)
```

If tier-1 examples round-trip from chrysalis source → prism runtime →
expected histories, the language has homoiconic range for state and
reactions. If 3b also runs — a sustained, drifting population of M/R
closures with measurable lineage variation and no schema violations —
the language has homoiconic range for process bodies themselves, which
is the full target.

## Resolved design decisions

1. **Initial state via `realize`**, not pre-instantiated. The top-level
   `main = Cell(1.0)` compiles to a state value the engine discovers
   and instantiates on the first tick.
2. **Expression language has its own typed AST**; boundary with prism
   is `Schema` for state slots only.
3. **Standalone steps with composite-typed parameters**. A step
   declared as `step Divide[…] ~{trigger: Float, self: Cell} ->{…}`
   means divide is reusable across composites that satisfy the Cell
   schema. Needs row polymorphism / duck-typed schemas in the type
   checker.
4. **String interpolation `'{var}'` is the "compute me" operator** for
   dynamic map keys. No `@` for key disambiguation.
5. **Records vs maps disambiguate structurally** (compile-time field set
   vs runtime keyset).
6. **Composites pass their own id as explicit config** rather than
   deriving it from the place-graph path.
7. **Trees are bigraph compositions, just sugared** (this doc's
   "Implied bigraph assembly" section). Don't switch storage.
8. **prism-schema is chrysalis's construction-time type system.**
   `Expr` constructors are typed: each takes typed sub-Exprs and
   either returns a new well-typed Expr or errors. Method dispatch
   consults the `MethodRegistry` entry's argument/return schemas at
   construction time. **Ill-typed Exprs are unrepresentable, not
   "constructible-but-caught."** Mutation operators
   (`point_mutate`, `crossover`, `subtree_swap`) are schema-preserving
   by definition — they cannot emit ill-typed offspring. This is the
   tagless-final / typed-AST approach, encoded via the existing
   `Schema` engine; no separate type theory needed.
9. **Syntactic kernel committed** (see "Syntactic kernel" section):
   `K[args](body)` for terms, `~{} ->{}` for port-graph interface
   (the morphism's domain and codomain), `|` for parallel composition,
   `?name` for pattern variables, `~name` for link variables, `!` for
   unbound port, `=>` for reactions, lowercase definers
   (`process`/`step`/`composite`/`reaction`/`pattern`) with capitalized
   controls (`Cell`/`Grow`/`MEK`/`Phosphorylate`). Body is a
   `|`-separated parallel composition with the last (un-`|`-ed) line
   as the value. Single-quote strings with `'{expr}'` interpolation.
10. **Bigraphs are an s-category** (see "Categorical structure"
    section). Wiring is morphism composition; pattern matching is
    morphism factorization; composite-as-process is the categorical
    reality. This justifies the `~{} ->{}` interface and the `|`
    algebra without separate motivation.
11. **Units live in the schema; the checker erases them.** Quantities
    are dimension + unit + extensivity metadata on `Float` schemas, not
    boxed values. Compatibility is dimension equality (rational
    exponents); differing units auto-convert; affine quantities reuse
    prism's `delta` as their vector companion. Dimensional checking is a
    one-time compile/construction pass and units are **erased** before
    execution — runtime is bare `f64`, no per-op validation. See "Units
    and quantities".
12. **Cross-dimension conversion is contextual.** A `context` gives
    named, parameterized rules between dimensions — molar mass
    (`[mass]↔[substance]`) and concentration
    (`[substance]↔[substance]/[length]^3`). Unlike pint, parameters may
    be place-graph state (a compartment's `volume`) and activation is
    structural (a region scopes a context over its contents), so
    concentrations track changing volumes natively. Resolved + erased in
    the check phase; runtime is one op. molecule↔mol stays a unit, not a
    context. See "Contexts (cross-dimension conversion)".
13. **chrysalis must be fully schema-aware** (it was stamping
    `Schema::Any` almost everywhere — a tier-1 shortcut that forced the
    engine's address-scanning fallback, blocked clean method dispatch,
    and prevented compile-time connection validation). `crate::schema`
    derives the real schema from the AST: `process` → `ProcessLink`,
    `step` → `StepLink`, `composite` → `CompositeLink` (with the
    inner-state `Tree`), data slots typed, custom/rich types → `Custom`
    (so the `TypeRegistry` dispatches their methods), and
    `Array[shape, element]` carrying its element's units element-wise (a
    field of concentrations *is* a concentration dimensionally). Staged:
    (1) derive — done, tested; (2) thread into `topology.state_schema` +
    composite inner schemas (keeping grow_divide green) — next; (3)
    typed scheduling (should fix the nested-composite bug), method
    dispatch, and compile-time wire-schema validation. This realizes the
    schema-driven-dispatch goal end to end.
14. **Native imports replace `extern` (now fully removed).** `from <module>
    import <names>` (`compile_with_modules` + a `ModuleRegistry`) pulls in host
    capabilities: a whole process, or functions/types a `.ys` `process` wraps
    with its own ports + `fulfills`. A native process is a wholesale import
    wired straight from its call site — no declared interface in the `.ys`
    (`eval::build_native_spec`); its real port schema is the instance's
    `inputs()`/`outputs()` (threading that into the state schema so a native
    node is discovered schema-first, not by the address scan, is the #28
    follow-up).
15. **`def` is the binder for values AND functions.** `def name :: T = expr`;
    `def f(args) = body`. Functions are first-class values (pass / return /
    store; `Expr::Call`). Bare `name = …` is an error — naming requires `def`.
16. **Process contracts are the meaning layer.** A contract is to an interface
    what a schema is to a value; `fulfills` rides on the producer's output-port
    schema; `:: C` makes a port *demand* a contract; substitutability is
    `algebra::refines` (`resolve==`, no new op). See `docs/process-contracts.md`.
17. **prism-std → chrysalis → spatio-flux; chrysalis bundles prism-std and IS the
    build tool.** prism-std is prism's native std library; chrysalis exposes it as
    `core`/`integrators`/`chem`/`io` and owns the `chrysalis` CLI
    (`run`/`check`/`bigraph` live; `compile` codegen pending). chrysalis NEVER
    depends on spatio-flux; spatio-flux is a downstream demo package.
18. **One unified `Core`, not four registries.** `prism_bigraph::Core` =
    `{types, processes, methods, protocols}` (upstream `registry` +
    `link_registry`), threaded through the engine and every subengine via
    `Composite::from_config(&Core)`. Fixes the class of bug where a subengine lost
    types/methods/protocols. `Engine::from_state(…, impl Into<Core>)` keeps old
    registry-only callers compiling.
19. **Steps are incrementally cacheable.** `prism_bigraph::StepCache`: a step is
    skipped when its on-disk output is fresh; `--steps a,b` forces specific steps;
    the step DAG cascades staleness. The substrate for the report-as-expirable-DAG.
    (Increment 1 = presence + force + cascade; granular content-fingerprint
    invalidation is the planned increment 2.)
20. **The composite boundary is real and protocol-mediated.** Composites are black
    boxes reached only through their ports, dispatched via `local`/`rest`/`parallel`
    protocols; the `RestProcessServer` runs one over HTTP with full lifecycle
    cleanup. Cross-boundary config (incl. a future cache directive) travels as
    DATA in config, never as a Rust handle threaded inside.
21. **A missing reference is an error, not a silent drop.** A document referencing
    a process/type the core can't build is rejected up front
    (`Core::missing_process_refs` / `Engine::check_references` / rest server 400).
    This is the seed of the diagnostics goal: chrysalis errors must be
    comprehensible *within chrysalis* (surface terms), located, and educational —
    a first-class subsystem to build out (NEXT-SESSION #16).
22. **`::` = type, `:` = value, `fulfills` = contract** (decided 2026-05-23). One
    operator per concept, applied everywhere:
    - **`::`** ascribes a TYPE — `def x :: T`, function params `f(x :: T)` and
      return `(…) :: Ret`, port types `~{state :: map[float]}`, config params
      `composite C[out :: Path = …]`, pattern-var sorts `?c :: Cell`.
    - **`:`** binds a VALUE — map/record entries `{a: 1.0}`, keyed call args
      `RunProcess[proc: Rk4]`, port wirings `~{a: r.trajectory}`.
    - **`fulfills`** is the contract relation, the same word on both sides: a
      process *declares* `process Rk4 fulfills Det[…]`; a port *demands* it
      `~{a :: TimeSeries fulfills Det}`. (Replaces the old `a: T :: Contract`,
      where `:` meant "type" and `::` meant "contract" — both retired.)
    Rationale: the prior rule was *positionally* consistent but used `:`/`::` both
    for "has type" depending on bracket depth. The Haskell-style `::`=type is the
    recognizable convention; freeing the contract onto its own keyword keeps type
    and contract visually distinct. **Enforcement is a migration** (parser flips
    type-position `:`→`::`; the port contract `:: C`→`fulfills C`; unparser emits
    the new forms; regenerate every `.ys` + the docs/GUIDE; update parse tests) —
    NEXT-SESSION task. Do it while the syntax is young.

## Open design decisions

- **Quoting / unquoting** for pattern and expr literals. `pattern X[…] (body)`
  and `expr (body)` bodies are implicitly quoted; params substitute by bare name.
  **Proposed (wei-qi, no new sigil):** there is NO `$` / `,@` unquote sigil — a
  param bound to a parallel that appears in a parallel context *splices*
  (flattens) by the monoidal associativity law `(a | (b | c)) ≡ (a | b | c)` the
  algebra already commits to. So unquote = name substitution, unquote-splicing =
  associativity normalization in `eval_pattern`. Gates the `pattern` row above.
- **Rate expressions, not constants** — ✅ RESOLVED (implemented 2026-06-06). A
  reaction carries an optional `rate ( expr )` clause after its body; the
  expression closes over config params AND matched bindings, evaluated per match
  at fire time (parser: contextual `rate (`; eval threads `ReactionDef.rate`;
  runtime `to_prism_rule` → `RateFn`; unparser round-trips it).
  `tests/reaction_rate.rs`; live in `mapk.ys` / `mr.ys`. Match-derived terms like
  `count(MEK)` await `pattern` + those query builtins.
- **Pattern variable kinds.** Two binding regimes: name-vars `?n`
  (atomic) vs site-vars `?rest` (subtree). Distinguished by context
  (subtree position vs atomic position), but worth confirming in the
  parser spec.
- **Schema inference for composite outputs**: full inferred schema of
  `Cell` so `Map[Cell]` in `Environment` resolves.
- **Type system surface for `self: Cell`**: row-typed / structural?
- **Mutation strategy library (tier 2).** Minimum viable set is
  probably `point_mutate` (replace a leaf with a schema-compatible
  alternative), `subtree_swap` (replace a subtree with another of
  matching schema), and `crossover` (exchange subtrees between two
  Exprs at schema-compatible cut points). All schema-preserving by
  construction. What's the right surface for *constraints* on mutation
  (preserve certain literals, freeze certain subtrees)?
- **Parser tool choice**: chumsky vs pest vs hand-written.

## Implementation plan

> **Status (2026-05-22):** the original build order below (steps 1–8 — the
> tier-1 fixtures, `MethodRegistry`, `eval`, `compile`, and the parser) is
> **done**, and the language has grown well past it (import model, `def` +
> functions, contracts, prism-std, the `chrysalis` tool, the unified `Core`,
> incremental steps, the REST boundary). See **"Where chrysalis is now"** above
> for current state and **`docs/NEXT-SESSION.md`** for the live roadmap. Tier 2
> (steps 9–11) and the milestones in NEXT-SESSION are what remain. The original
> plan is kept below for provenance.

1. Create `crates/chrysalis/` skeleton: `Cargo.toml` (added as workspace
   member in prism root), `src/lib.rs`, empty modules.
2. Write `ast.rs`: `ProcessDef`, `StepDef`, `CompositeDef`,
   **`PatternExpr`, `ReactionDef`, `MethodCall`**, `Expr`, `Wiring`,
   `Path` types. Patterns, reactions, and method calls are first-class
   AST nodes from day one.
3. Build **all three** examples (grow/divide, MAPK, M/R) manually as
   AST literals in test fixtures. Forces the AST to support pattern
   composition (MAPK), parameterized recipes (M/R), and runtime-built
   reactions (grow/divide).
4. **`MethodRegistry` in prism** (lives in `prism-schema` or a new
   small crate). Keys: `(TypeId, method_name)`. Values: typed Rust
   closures `Fn(&Value, &[Value]) -> Result<Value>`. Each prism crate
   registers its types' methods (`Pattern::register_methods`,
   `ReactionRule::register_methods`, `BRS::register_methods`,
   `Mesh::register_methods`, etc.). Bigraph primitives (`|`, ion
   construction, port-link binding, `?`-site, `~`-link, `!` for
   unbound) are exposed here.
5. Write `eval.rs`: interpreter for expression bodies. Evaluates
   pattern expressions to `Pattern` values, reaction expressions to
   `ReactionRule` values, scalars, and method calls (dispatched via
   `MethodRegistry`).
6. Write `compile.rs`: AST → `ProcessRegistry` factory entries +
   initial state `Value`. Patterns/reactions/BRSes emitted by update
   bodies flow into engine state via the standard delta-sentinel path;
   no chrysalis-specific runtime.
7. End-to-end tests for each example:
   - grow/divide: cell at mass 1.2 grows past 2.0, divides — via a
     parent-installed reaction, not a hardcoded step.
   - MAPK: matches the existing `prism-mapk` histogram of states.
   - M/R: starting from {F, B, φ, A-pool}, run long enough to see new
     F's and new φ's produced, verify each new F's blueprint matches
     its lineage parent.
8. **Then** write the parser. All three tier-1 example files become
   parser fixtures.
9. **Tier 2: `Expr` as a first-class value.** Add `Expr` to prism's
   `TypeRegistry` with `_type: "Expr"` and a schema field. Register
   typed constructors in `MethodRegistry` (literal, var, method-call,
   if, let, …) — each enforces schema compatibility at construction.
   Add `ProcessDef.from_expr(e, schema)` that schema-matches `e`
   against a target process signature.
10. **Tier 2: mutation operators.** Register `point_mutate`,
    `subtree_swap`, `crossover` as methods on `Expr`. All
    schema-preserving by construction.
11. **Tier 2 benchmark (3b — evolving M/R).** Run a population for
    long enough to observe lineage variation and verify no schema
    violations across mutations / installations.

### Why this order

Steps 1–7 prove the semantics work against the real prism runtime
*before* a parser exists. Three examples (not one) up front because
homoiconicity is a property visible only when reactions are constructed
in code — the grow/divide example alone passed in the original plan
without forcing the question. MAPK forces pattern composition; M/R
forces lineage-carrying construction. All three together are the
tier-1 acceptance test.

`MethodRegistry` (step 4) is the structural change that distinguishes
"chrysalis composes pre-built processes" from "chrysalis has full
operational access to prism." Doing it before `eval.rs` (rather than
patching it on later) means method calls are a first-class expression
form from the start, not an afterthought.

Tier 2 (steps 9–11) is sequenced after the parser because it doesn't
require new infrastructure — `Expr` is just another prism type with a
method table. Schema-driven construction reuses the `MethodRegistry`
machinery already built. The evolving M/R benchmark (3b) is the
final acceptance test: closures that drift, never break type, and
sustain themselves.

## What chrysalis does not implement (defers to prism)

Implementations of the following stay in prism's Rust crates. Chrysalis
**uses them via the value-method interface** (see "Value methods"
section above); it does not reimplement.

- Numerical primitives (PDE solvers, FBA via HiGHS, Gillespie SSA,
  Newtonian particles via rapier2d, MCMC). Wired in as native
  processes and as methods on their result types; chrysalis composes
  them.
- Schema engine, type registry, custom-type dispatch.
- The Engine, scheduling, projection application, structural diff.
  *(Not exposed as methods either — the engine calls chrysalis, not
  the other way around.)*
- BRS pattern matcher (`prism_schema::reaction::find_matches`,
  `fire_rule_at`). Patterns and reactions are first-class values
  chrysalis constructs and emits; matching is performed by the matcher
  natively when a BRS process runs.

These are load-bearing infrastructure; chrysalis is the abstraction
layer that makes them composable from a source language. **Access is
unrestricted** — any Rust method registered in the type's method
table is callable from update bodies. This section is about not
duplicating implementation, not about restricting access.
