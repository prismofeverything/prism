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

Comments are `#` to end of line.

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
methods (see §6) dispatch on it.

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

## 5. Processes and steps — the two primitives

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
  (input face).
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
contract (§9). The interface (`[config] ~{in} ->{out}`) is the *only* thing a
caller needs to know — it's the contract between this component and whatever
wires to it.

---

## 6. Methods

`value.method(args)` calls a value-method dispatched on the value's type — either
a native method or one declared on a user `type … with { … }`. The receiver's
`_type` selects the implementation:

```ys
a.species_mse(b)                       # native TimeSeries method → map[float]
figure.svg(path)                       # native Figure method (writes a file)
```

---

## 7. Composites — wiring components together

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

Reuse across files with `import Name from "other.ys"` (see
[`spatio-flux/ys/culture.ys`](../../spatio-flux/ys/culture.ys), which imports and
nests `Dish` from `dish.ys`).

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

## 8. Importing native capabilities

Reusable native code (Rust) is pulled in with **`from <module> import <names>`**
— the replacement for `extern`. Three kinds of import:

```ys
from core import RunProcess          # a whole native process, used as-is
from integrators import rk4, euler   # native objects → call methods: rk4.integrate(…)
from chem import CRN                 # a native type → use in annotations: [network: CRN]
```

A common pattern: import a native *function/object* and give it ports + a
contract by wrapping it in a `process` you declare — the interface is yours, the
math is borrowed:

```ys
process Rk4[network: CRN]
  ~{state: map[float]} ->{state: map[float]}
  ( rk4.integrate(network, state, interval) )
```

The std modules (`core`/`integrators`/`chem`/`io`) ship with `chrysalis`. Other
packages need the codegen path (see the README).

---

## 9. Contracts

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

## 10. Effects: a self-outputting workflow

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

## 11. Units (brief)

Declare units and dimensioned quantities; they're dimension-checked at compile
and erased to raw `f64` before running:

```ys
unit pg : [mass] = 1e-12
Mass = Quantity[unit: pg, extensive]      # `extensive` splits on division
```

See `units.ys` / `nuclear-shuttle.ys` and the design doc's "Units and quantities".

---

## 12. Reactions and patterns (brief)

For Milner-style rewriting, a **`reaction`** has a redex `=>` reactum; `?name`
are pattern variables, `~name` link variables, `!` an unbound port:

```ys
reaction MakeB ( (?f :: F | ?a :: A) => (F[…] | B[…]) )
```

See `mapk.ys` / `mr.ys`.

---

## 13. A complete worked example

[`integrator-comparison.ys`](integrator-comparison.ys) ties it together: import
the std integrators + `CRN` + `Path`; declare two integrator `process`es that
`fulfill` the same contract; declare `Compare`/`Plot`/`Output` steps; define the
shared `network`; wire them in a composite workflow; run. Read it top-to-bottom —
every construct in this guide appears, and:

```sh
cargo run -p chrysalis --bin chrysalis -- run crates/chrysalis/ys/integrator-comparison.ys
ls outputs/integrator-comparison/    # Rk4.csv  ForwardEuler.csv  mse.csv  overlay.svg
```

---

## 14. Toolchain

- `chrysalis run <file.ys> [--time T]` — parse → compile → run (Output steps emit
  artifacts).
- `chrysalis check <file.ys>` — parse + contract / connection checks, no run.
- `chrysalis bigraph <file.ys>` — emit the process-bigraph document (JSON).

(Today: `cargo run -p chrysalis --bin chrysalis -- <subcommand> …`, or
`cargo install --path crates/chrysalis` for a bare `chrysalis`.)

Editor highlighting: [`crates/chrysalis/emacs/`](../emacs/).
