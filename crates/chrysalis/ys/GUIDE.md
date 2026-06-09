# Writing `.ys` files — a guide

chrysalis (`.ys`) is a surface language that compiles to the prism runtime, a Rust
port of process-bigraph. This guide builds up from a one-liner to a full
workflow. For the formal design see
[`docs/chrysalis-design.md`](../../../docs/chrysalis-design.md); for the runnable
examples see the [README](README.md).

Run anything here with the `chrysalis` tool:

```sh
cargo run -p chrysalis --bin chrysalis -- run   path/to/file.ys     # run it
cargo run -p chrysalis --bin chrysalis -- check path/to/file.ys     # typecheck only
```

> **Mental model.** State is a tree of named values (a *place graph*); each
> `process` / `step` / `composite` constitutes a link in the (*link graph*),
> according to Milner's bigraph process calculus theory.

> There is also a way in which a process is a **morphism** with an input face and an
> output face, wired into that tree. "Writing a `.ys` file" is: declare the
> processes and steps and their interfaces, then wire them together into composites. 

---

## 1. The shape of a program

A `.ys` file is a sequence of **definitions** followed by a trailing **bare
expression** — that last expression is the program's value (the implicit
`main`). The smallest program is just an expression:

```ys
{greeting: 'hello', n: 42}
```

Definitions come first; the trailing expression usually instantiates a composite:

```ys
def x = 21.0
double(x)            # the program's value (assuming `double` is in scope)
```

A file with **no trailing expression** is a *library / component* file: its last
`process` / `step` / `composite` is the entry, runnable on its own (see §9) or
importable (§11). Comments are `#` to end of line.

---

## 2. Values and `def`

Literals: floats `1.0`, ints `2`, strings `'text'` (single-quoted), booleans
`true`/`false`. Containers: lists `[a, b, c]` and records/maps `{key: value, …}`.

Named values use the **`def`** definer (named values are *required* to use
`def` — a bare `name = …` is an error):

```ys
def rate = 0.7
def initial :: map[float] = {A: 1.0, B: 0.0}      # `:: Type` is an optional ascription
```

String interpolation uses `{…}`:

```ys
def label = 'cell {id}'
```

---

## 3. Types

Declare a record type with **`type`**; reference built-in schema leaves
(`float`, `int`, `string`, `bool`, `map[T]`, `list[T]`, `any`, `array[…]`):

```ys
type TimeSeries = {
  times: list[float],
  columns: map[list[float]]
}

type Figure = {svg: string}
```

A value whose `_type` field matches a declared type carries that type, and its
methods (see §6) dispatch on it. A **capitalized `Name = <schema>`** is a *type
alias* (e.g. `Mass = Quantity[unit: pg, extensive]`, §11) — it names a schema and
travels across `.ys` imports.

---

## 4. Functions

Functions are first-class values, also introduced with `def`:

```ys
def double(x) = x * 2
def apply(f, x) = f(x)        # functions can be passed/returned/stored

double(21.0)                  # → 42
apply(double, 21.0)           # → 42
```

Parameters may be typed (`def f(x: float) = …`) or untyped.

---

## 5. Comprehensions — iterating collections

A comprehension transforms a list or a map. The list form builds a list; the map
form builds a map (computed keys). You can bind just the value, or the key/index
and the value:

```ys
[ f(x) for x in items ]                       # list → list
[ x for x in items if x > 0 ]                 # with a filter
[ k for k, v in scores if v.ready ]           # over a MAP: bind key + value
{ '{k}_done': v for k, v in scores }          # MAP comprehension → a map (interpolated keys)
```

Iterating a map binds `(key, value)`; iterating a list binds `(index, value)` (or
just the value with one binder). A map comprehension with a **constant key**
collapses to the last matching entry — one entry out — which is how an
environment's division step emits a single `_divide` per tick from many candidate
cells (see `environment.ys`). Comprehensions also build structural deltas, e.g.
`{ edges: { _remove: [e for e in self.edges if e.from == id] } }` (see `graph.ys`).

---

## 6. Processes and steps — the two primitives

prism has two computational primitives; chrysalis declares both with a config, a
port interface, and a body:

- A **`process`** is **temporal** — it advances on a clock. Each tick the engine
  calls it with the current input state and an `interval` (the tick width), and
  it returns the next state. Use it for anything with dynamics — an integrator, a
  physics step, growth.
- A **`step`** is **dataflow** — no clock. It fires once when its inputs are
  ready (or change), computing outputs from inputs. Use it for analysis,
  transforms, reductions, IO.

```ys
process Grow[rate: float]                       # temporal
  ~{mass: float} ->{mass: float}
  ( {mass: mass + (mass * rate * interval)} )

step Compare ~{a: TimeSeries, b: TimeSeries} ->{mse: map[float]} (   # dataflow
  {mse: a.species_mse(b)}
)
```

### Config vs inputs vs outputs

Three distinct knobs — getting these right *is* writing a component:

- **Config `[name: Type]`** — *construction-time parameters*. Set **once** when
  the component is instantiated and fixed for its life (a rate constant, a
  reaction network, a file path). Think constructor arguments. Defaults allowed:
  `[rate: float = 0.02]`.
- **Inputs `~{name: Type}`** — the *per-update dataflow* it reads, delivered
  fresh each tick (process) or trigger (step). This is the morphism's **domain**
  (input face). Inputs may also carry **defaults** (`~{glucose :: Float = 0.0}`):
  the value used when the port isn't driven by a parent or seeded on the command
  line — which is what lets a component run standalone (§9).
- **Outputs `->{name: Type}`** — what it writes. The morphism's **codomain**
  (output face).

Inside the body **all of these are in scope by name** — config params, input
ports, and (for a `process`) `interval`. The body evaluates to a **record whose
keys are the output ports**:

```ys
process Rk4[network: CRN]                 # config: the fixed reaction network
  ~{state: map[float]}                    # input:  current concentrations (per tick)
  ->{state: map[float]}                   # output: next concentrations
  ( rk4.integrate(network, state, interval) )   # body uses config + input + interval
```

So the rule of thumb: **does it change every step?** → an input. **Is it fixed
for the run?** → config. A port's `Type` is a schema (§3) and may carry a
contract (§12). The interface (`[config] ~{in} ->{out}`) is the *only* thing a
caller needs to know — it's the contract between this component and whatever
wires to it.

---

## 7. Methods

`value.method(args)` calls a value-method dispatched on the value's type — either
a native method or one declared on a user `type … with { … }`. The receiver's
`_type` selects the implementation:

```ys
a.species_mse(b)                       # native TimeSeries method → map[float]
figure.svg(path)                       # native Figure method (writes a file)
```

---

## 8. Composites — wiring components together

A **`composite`** wires child processes/steps into a larger one. Children are
separated by `|` (parallel composition); each is `label: Control[config]
~{port: wire} ->{port: wire}`. A **wire** is a named edge: whatever a child
*writes* to a name, any child whose input is bound to that name *reads*. A bare
`name: value` introduces a slot (a named piece of state):

```ys
composite Pipeline ->{result: float} (
  seed: 21.0 |                                  # a slot (state)
  d:    Doubler  ~{x: seed}    ->{y: doubled} | # reads seed,    writes doubled
  out:  Identity ~{x: doubled} ->{y: result}    # reads doubled, writes result
)
```

Wiring is **categorical composition**: connecting `d`'s output to `out`'s input
is the morphism composite `out ∘ d`; `|` is the tensor (parallel) product. The
algebra (associativity, identity) is a theorem of the underlying s-category, not
something `.ys` invents — see the design doc's "Categorical structure". Composite
config takes defaults like any component (`composite C[n: int = 3] ->{…} ( … )`),
and top-level `def`s are in scope inside the body.

### Wire roots: siblings, `^` (parent), `%` (own node), `@` (bridge)

A wire names a path, resolved relative to where the child lives:

- a **bare name** is a **sibling** slot in the same composite (`~{x: seed}`);
- **`^.x`** is the **container** one level up — a child reading a shared pool
  (`~{glucose: ^.glucose}`);
- **`%.x`** is the child's **own node** — the *exported face* a parent can match
  and divide without knowing the child's key (`->{mass: %.mass}` puts `mass` on
  `cells.cN.mass`);
- an interface port may bridge to an inner slot with **`@`**:
  `mass :: Mass @ mass` maps the port to the inner state path `mass` (the default
  is name-inference). The bridge is how a composite's interface connects to its
  private inner state.

These four are how spatial/biological models wire up: a cell reads the shared
`^.glucose` pool, exposes its `%.mass` face, and the surrounding environment
matches that face.

### A composite *is* a process

A composite has the same `[config] ~{inputs} ->{outputs}` interface as a
`process`, and — because both are morphisms in the same category — it **composes
identically**. This is a fact, not a convenience: anywhere a process fits, a
composite fits, and vice versa. So you build **reusable components** by giving a
composite an interface and nesting it like any process:

```ys
# A reusable component with a clear interface…
composite Well[diffusion: float]
  ~{fields: map[float]} ->{fields: map[float]}
  ( … internal wiring … )

# …nested twice inside a bigger composite — the caller can't tell `Well` is a
# subgraph rather than a single process; only the interface is exposed:
composite Plate ->{a: map[float], b: map[float]} (
  well_a: Well[diffusion: 0.1] ~{fields: grad_a} ->{fields: a} |
  well_b: Well[diffusion: 0.3] ~{fields: grad_b} ->{fields: b}
)
```

A composite's inner state is **private** — a parent sees only the exported face,
never the children inside. (Reuse across files is `from other import Well`, §11.)

### Workflows and DAGs

A composite whose children are **one-shot `step`s** is a workflow. The wires are
**data dependencies**, and the engine fires the steps **in dependency order**
(producers before consumers) — a DAG *inferred from the wiring*, not declared.
You write the dependencies; the schedule is the engine's job:

```ys
composite IntegratorComparison ->{mse: map[float], figure: Figure} (
  init: {A: 1.0, B: 0.0} |
  rk4:   RunProcess[proc: Rk4[network: network],          runtime: 5.0, timestep: 0.2]
           ~{state: init} ->{timeseries: rk4_traj} |
  euler: RunProcess[proc: ForwardEuler[network: network], runtime: 5.0, timestep: 0.2]
           ~{state: init} ->{timeseries: euler_traj} |
  compare: Compare ~{a: rk4_traj, b: euler_traj} ->{mse: mse} |
  plot:    Plot    ~{a: rk4_traj, b: euler_traj} ->{figure: figure} |
  output:  Output[path: out] ~{a: rk4_traj, b: euler_traj, mse: mse, figure: figure}
)
```

`compare`/`plot` read `rk4_traj`/`euler_traj`, so they run after `rk4`/`euler`;
`output` reads everything, so it runs last. (`RunProcess` is a `core` import that
drives a temporal `process` over `[0, runtime]` and collects a `TimeSeries` — the
bridge from the temporal world into a dataflow step.) This is the SED-ML-style
"init → simulate → analyze" experiment expressed as a typed graph.

---

## 9. Running a file by itself — the default harness

**Every** `process`, `step`, and `composite` is runnable on its own — no
surrounding program needed. A composite runs its body directly; a bare
`process`/`step` is realized in a synthesized **default harness** (a state slot
per port, the component self-wired to those slots), so it runs "on a bench."
Seed inputs with `--<port> VALUE`; set the duration with `--time`; the final
state prints as JSON:

```sh
# a process on a bench — Grow eats a finite glucose pool, conserving mass
chrysalis run grow.ys --mass 1.0 --glucose 5.0 --time 5
# → { "mass": 4.0, "glucose": 0.0, "acetate": 2.0 }

# a composite standalone — seed its inputs the same way
chrysalis run cell.ys --glucose 5.0 --time 6
```

Unseeded inputs fall back to their declared defaults (§6), so a no-flag run is
valid (just inert if there's no fuel). This is the difference between **two
modes** of the same file: *standalone* (a bench, the default) and *driven* — fed
each tick by a parent over a transport (`--serve-process`, §10). The file doesn't
choose; the invocation does.

---

## 10. Protocols — where a component runs

A component is reached through a **protocol**, and the simulation **can't tell**
whether it runs in-thread or across a network — the boundary is transparent.
Address forms:

- **`local:`** — in-process (the default; you never write it).
- **`stream:`** — a child `.ys` file run as its own OS process, driven over pipes
  (`chrysalis run child.ys --serve-process` under the hood).
- **`rest:`** — a process on an HTTP server (`chrysalis server`).
- **`parallel:`** — a thread-pool worker.

You don't usually hand-write addresses — you bind a protocol + its config to a
**type**, then use it like any control. This is *protocols-as-types*:

```ys
from .cell import Cell
protocol StreamingCell = stream<Cell, path: 'cell.ys'>   # a Cell that runs as its own process

composite Environment ->{cells: map[Cell] @ cells} (
  cells: {
    c0: StreamingCell[mass0: 1.0] ~{glucose: ^.glucose} ->{mass: %.mass, …} ,
    c1: StreamingCell[mass0: 1.0] ~{glucose: ^.glucose} ->{mass: %.mass, …}
  }
)
```

Swap `StreamingCell` back to `Cell` and nothing else changes — same model, the
cells just run in-process instead of as separate processes.

**Concurrency is free.** The engine dispatches every due remote component
*before* collecting any result, so `stream:` / `rest:` / `parallel:` nodes
**overlap** — wall-clock per tick ≈ the slowest one, not the sum. The same seam
scales from threads to many machines; see
[`docs/distributed-execution.md`](../../../docs/distributed-execution.md) for the
plan (domain decomposition, halo exchange, the octree).

---

## 11. Importing — `.ys` files and native capabilities

Two kinds of import — the **shape of the path** decides which (explicit origin:
no search path, no precedence, so a sibling file and a native module can never
collide):

| write | resolves to |
|---|---|
| `from .name import …`     | a **sibling `.ys` file** (`name.ys`, next to the importer) |
| `from .sub.name import …` | a relative file `sub/name.ys` |
| `from name import …`      | a **native / registry module** (the std library / a host) |

**`.ys` file modules** — a **leading dot** pulls a definer
(process/step/composite/type) from a **relative file**:

```ys
from .grow import Grow            # brings `Grow` (+ its `Mass` / unit vocabulary)
from .divide import Divide
from .cell import Cell            # a composite from cell.ys
```

This is how the modular cell library composes: `grow.ys` and `divide.ys` are
single-process files; `cell.ys` imports both and wires them; `environment.ys`
imports `Cell`. A type alias / unit declared in the imported file comes along.

**Native modules** — reusable Rust pulled by a **bare** name (the replacement
for `extern`). Three kinds:

```ys
from core import RunProcess          # a whole native process, used as-is
from integrators import rk4, euler   # native objects → call methods: rk4.integrate(…)
from chem import CRN                 # a native type → use in annotations: [network: CRN]
```

A common pattern: import a native *function/object* and give it ports + a
contract by wrapping it in a `process` you declare — the interface is yours, the
math is borrowed. The std modules (`core`/`integrators`/`chem`/`io`) ship with
`chrysalis`; other packages need the codegen path (see the README).

---

## 12. Contracts

A **`contract`** states a process's *meaning* — which mathematical object it
approximates — not just its port shapes. `fulfills` annotates a process with the
contract it satisfies; `::` constrains a port to demand one. Two processes are
substitutable only when they share a contract, so an illegitimate comparison
won't compile:

```ys
contract DeterministicMassAction (
  target: MassActionODE,
  claims: Deterministic,
  advance: Continuous
)

process Rk4[network: CRN]
  fulfills DeterministicMassAction[method: Rk4]      # leaves `method` open per fulfiller
  ~{state: map[float]} ->{state: map[float]}
  ( rk4.integrate(network, state, interval) )
```

See [`docs/process-contracts.md`](../../../docs/process-contracts.md) for the
full story (it's the layer SED-ML/KISAO is missing).

---

## 13. Effects: a self-outputting workflow

Some native methods are *effectful* — they write files. A `step` with an empty
`->{}` runs purely for its writes. `path / segment` joins paths over the `Path`
type:

```ys
from io import Path

step Output[path: Path] ~{a: TimeSeries, mse: map[float], figure: Figure} ->{} (
  a.csv(path / a.name) |
  mse.csv(path / 'mse') |
  figure.svg(path / 'overlay')
)
```

Wire an `Output` child into your composite and the workflow emits its own
artifacts — no external harness.

---

## 14. Units (brief)

Declare units and dimensioned quantities; they're dimension-checked at compile
and erased to raw `f64` before running:

```ys
unit pg : [mass] = 1e-12 kg
Mass = Quantity[unit: pg, extensive]      # `extensive` ⇒ splits on division (Delta)
```

`extensive` opts a quantity into **halving on divide** (biomass), vs the default
*intensive* (a concentration, copied). See `units.ys` / `nuclear-shuttle.ys` and
the design doc's "Units and quantities".

---

## 15. Reactions and patterns (brief)

For Milner-style rewriting, a **`reaction`** has a redex `=>` reactum; `?name`
are pattern variables, `~name` link variables, `!` an unbound port:

```ys
reaction MakeB ( (?f :: F | ?a :: A) => (F[…] | B[…]) )
```

See `mapk.ys` / `mr.ys`.

---

## 16. A complete worked example

[`integrator-comparison.ys`](integrator-comparison.ys) ties it together: import
the std integrators + `CRN` + `Path`; declare two integrator `process`es that
`fulfill` the same contract; declare `Compare`/`Plot`/`Output` steps; define the
shared `network`; wire them in a composite workflow; run. Read it top-to-bottom —
every workflow construct in this guide appears, and:

```sh
cargo run -p chrysalis --bin chrysalis -- run crates/chrysalis/ys/integrator-comparison.ys
ls outputs/integrator-comparison/    # Rk4.csv  ForwardEuler.csv  mse.csv  overlay.svg
```

For the **spatial / distributed** side, the cell library — `grow.ys`, `divide.ys`,
`cell.ys`, `environment.ys` — shows imports, the `@`/`%`/`^` wires, protocols-as-
types, and a colony of stream cells that grow, divide (Form 3: the cell proposes
`%.divide`, the environment enacts via a `_divide` step over the cells map), and
**conserve mass** across the run:

```sh
chrysalis run crates/chrysalis/ys/environment.ys --time 40
```

---

## 17. Toolchain

- `chrysalis run <file.ys> [--time T] [--<port> SOURCE …] [--out FILE] [--trace]`
  — parse → compile → run. `--<port>` seeds an interface port (§9); `--trace`
  captures the run as a time-series; Output steps emit artifacts.
- `chrysalis run <file.ys> --serve-process` — run the file as a *driven child*
  over pipes (the `stream:` protocol's other half, §10).
- `chrysalis check <file.ys>` — parse + contract / connection checks, no run.
- `chrysalis bigraph <file.ys>` — emit the process-bigraph document (JSON).
- `chrysalis repl` — interactive eval (defs/functions accumulate; `:type`, `:env`).
- `chrysalis server [--port P]` — serve a `Core` over the rest-process protocol.

(Today: `cargo run -p chrysalis --bin chrysalis -- <subcommand> …`, or
`cargo install --path crates/chrysalis` for a bare `chrysalis`.)

Editor highlighting: [`crates/chrysalis/emacs/`](../emacs/).
