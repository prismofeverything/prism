# Chrysalis primer — the executable, canonical reference

This is the **fluency reference** for chrysalis (`.ys`), the surface language that
compiles to the prism process-bigraph runtime. It is the recommended first read
for any new session — and the **executable nexus** of the project: every domain
(biology, quantum, synthesis, adaptive networks, the distribution mesh) is a
specialization of the *one* substrate this language drives, so learning the
language is learning the system.

**It cannot go stale.** Every ` ```ys ` block below is extracted and run by
`crates/chrysalis/tests/primer_doctest.rs`. If the language changes under a block,
that test goes red — so what you read here is what the language *actually does*.
Fence tags:

| fence | meaning |
|---|---|
| ` ```ys ` | must **parse** (`parse_program` succeeds) |
| ` ```ys run ` | must parse **and run** clean through the engine |
| ` ```ys ignore ` | **planned, not yet live** — shown so the roadmap is honest, skipped by the net |

Every fenced `ys` block is a *complete program* (some defs and/or a trailing
term). Sub-expression fragments appear as inline `code`.

> **Status:** the verified core, **plus** types & methods (`type … with`, §7),
> units (§14), contracts (§13), imports/modules (§15), protocols-as-types (§16),
> the first-class **`link`** (§11) and replicated **`mesh`** link (§12),
> reaction-driven **division** (`?c.divide()`) and first-class **`functor`**s
> (§10), and homoiconic **`eval`** (§17) — the surface as it stands today. Two
> forms remain `ys ignore` (the honest roadmap, §20): match-derived rate and the
> cross-composite redex. See [`README.md`](README.md) for the doc map,
> `docs/NEXT-SESSION.md` for status.

---

## Mental model — it's a bigraph

State is a **bigraph**: two orthogonal structures.

- **Place graph** — nesting/containment. A tree of named values (`Value::Tree`)
  or anonymous lists (`Value::List`). `cell.cytoplasm.mek` is a place-graph path.
- **Link graph** — wiring. Ports (`~{} ->{}`) connected by shared names/links.

A `process` / `step` / `composite` is a **morphism** with an input face `~{…}`
(domain) and an output face `->{…}` (codomain). Wiring *is* categorical
composition; `|` is the monoidal tensor (parallel). Keep this in view — almost
every "how do I say X?" question is answered by "which graph is X about?".

The whole language is a small kernel; everything else is sugar over it. Prefer
composing the kernel to reaching for a new construct.

---

## 1. The shape of a program

A file is a sequence of **definitions** followed by an optional trailing **term**
(the implicit `main` — the program's value). Comments are `#` to end of line.

```ys
# defs first, then a trailing term = the program's value
def greeting = 'hello'
{ note: greeting, n: 42 }
```

A file with **no trailing term** is a library/component: its last interfaced
definition (a `composite` / `process` / `def`) is the entry, runnable on its own
(§18) or importable (§15). A defs-only file with no interface is import-only.

---

## 2. Values and `def`

Literals: floats `1.0`, ints `2`, strings `'text'` (single-quoted), booleans
`true` / `false`. Containers: lists `[a, b]`, records/maps `{ key: value }`.

Named values use **`def`** — a bare `name = …` is an error (naming requires
`def`, so a value can't silently shadow a control or type).

```ys
def rate = 0.7
def initial :: map[float] = { A: 1.0, B: 0.0 }   # `:: T` is an optional ascription
def label = 'cell at rate {rate}'                # '{…}' interpolates
```

Two escapes, the inverse of how they round-trip: `''` is a literal apostrophe
(`'it''s'` → `it's`); `{{` / `}}` are literal braces (`'paths {{a,b}}'` →
`paths {a,b}`), so prose with a brace doesn't start interpolation.

---

## 3. Types

Declare a record type with **`type`**; reference built-in schema leaves (`float`,
`int`, `string`, `bool`, `map[T]`, `list[T]`, `array[…]`, `any`):

```ys
type TimeSeries = { times: list[float], columns: map[list[float]] }
type Cell = { mass: float, alive: bool }
```

A value whose **`_type`** field matches a declared type *carries* that type: its
methods (§7) dispatch on it, and the schema travels across `.ys` imports. The
type system is a brand-and-subtype lattice (a value of a subtype is usable where
a supertype is demanded; `algebra::refines` decides substitutability).

```ys
def c :: Cell = { _type: 'Cell', mass: 1.0, alive: true }
```

A **capitalized `Name = <schema>`** is a *type alias* — it names a schema and
travels across imports (e.g. `Mass = Quantity[unit: pg, extensive]`, §14).

---

## 4. Functions

Functions are first-class values, also introduced with `def`:

```ys
def double(x) = x * 2.0
def apply(f, x) = f(x)        # pass / return / store functions
```

---

## 5. Comprehensions

The list form builds a list; the map form builds a map (computed keys).
Iterating a map binds `(key, value)`; a list binds `(index, value)`.

```ys
def scale(items) = [ x * 2.0 for x in items ]
def positives(items) = [ x for x in items if x > 0.0 ]
def doubled(m) = { k: v * 2.0 for k, v in m }
```

Comprehensions cover map/filter, and they also build **structural deltas** — e.g.
`{ edges: { _remove: [e for e in self.edges if e.from == id] } }` removes every
matching edge (§7). There is no surface `fold`/`reduce` yet; aggregation across
siblings rides on the engine's additive `apply`.

---

## 6. Processes and steps — the two leaf morphisms

`process` and `step` are leaf (non-composite) morphisms. Ports are typed with
`::`. A body is a `|`-separated parallel of statements; the final line *without*
`|` is the body's value (the update the engine applies).

- A **`process`** is **temporal** — it advances on a clock. Each tick the engine
  hands it the current inputs + an `interval` (the tick width); it returns the
  next update. Use it for dynamics (an integrator, a physics step, growth).
- A **`step`** is **dataflow** — no clock. It fires when its inputs are ready or
  change. Use it for analysis, transforms, reductions, IO.

```ys
process Grow[rate :: float = 0.2] ~{mass :: float} ->{mass :: float} (
  delta = mass * rate |        # a binding
  { mass: delta }              # the value = the update (a delta on `mass`)
)

step Threshold[limit :: float = 2.0] ~{mass :: float} ->{over :: bool} (
  { over: mass > limit }
)
```

Three distinct knobs — getting them right *is* writing a component:

- **`[config]`** — construction-time parameters, set **once** (a rate constant, a
  network, a path). Defaults allowed: `[rate :: float = 0.2]`.
- **`~{inputs}`** — the per-update dataflow, delivered fresh each tick. The
  morphism's **domain**. Inputs may carry defaults (`~{glucose :: float = 0.0}`),
  which is what lets a component run standalone (§18).
- **`->{outputs}`** — what it writes. The morphism's **codomain**.

The update returned by the body is a **delta** — how a port's value changes — not
a full overwrite, which is why `{ mass: delta }` accumulates for additive types.

---

## 7. Methods — values that carry behavior

`value.method(args)` calls a value-method dispatched on the receiver's `_type` —
native, or declared on a user type with **`type … with { … }`**. Inside a method,
`self` is the receiver; write methods return **deltas** over the complete
`_add`/`_remove` basis:

```ys
type Graph = { nodes: list[string], edges: list[{ from: string, to: string }] }
with {
  add_node(id: string) = { nodes: { _add: [id] } }
  add_edge(from: string, to: string) = { edges: { _add: [{ from: from, to: to }] } }
  remove_node(id: string) = {
    nodes: { _remove: [id] },
    edges: { _remove: [e for e in self.edges if e.from == id or e.to == id] }
  }
  neighbors(id: string) = [ edge.to for edge in self.edges if edge.from == id ]
}
```

`g.add_node('a')` yields the delta the engine applies; `g.neighbors('a')` is a
read. Methods are how a `Custom` type behaves like a built-in sort.

---

## 8. Composites — wiring components into a sub-bigraph

A `composite` body declares state slots (`name: value`) and wires sub-components
to them with `|` (parallel). Each child is `label: Control[config] ~{port: wire}
->{port: wire}`; a **wire** is a named edge — what one child *writes* to a name,
any child reading that name *sees*. Wire roots:

| root | meaning |
|---|---|
| `name` | a sibling slot / child of the current composite |
| `^` | the parent (one place-graph level up) — e.g. a shared pool `^.glucose` |
| `%` | this node itself (the own face a parent can match, e.g. `%.mass`) |
| `@` (in a port decl) | the **bridge** — map this interface port to an inner path |

A composite is reached only through its ports — **a composite *is* a process**
(the category says so); a caller can't tell a subgraph from a leaf, so a composite
nests anywhere a process fits.

```ys run
composite World[seed :: float = 2.0] ->{count :: float @ count} (
  count: seed                  # output port `count` bridges to inner slot `count`
)
```

`chrysalis run` on `World` with `--seed 3` (or the default 2.0) yields
`{ "count": 2.0 }`. Now wire a process to a slot:

```ys run
process Tick ~{count :: float} ->{count :: float} (
  { count: 1.0 }
)

composite Main ->{count :: float @ count} (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}   # keyed sub-process, wired to the slot
)
```

After `--time 5`, `Main`'s engine has scheduled `tick` five times against the
`count` slot.

**Workflows and DAGs.** A composite of one-shot `step`s is a workflow: the wires
are data dependencies and the engine fires them **in dependency order** (a DAG
*inferred* from the wiring — you write the dependencies, the schedule is the
engine's job).

```ys
composite Experiment ->{ mse :: map[float] } (
  init: { A: 1.0, B: 0.0 } |
  run:     RunProcess[proc: Rk4[network: net], runtime: 5.0, timestep: 0.2]
             ~{state: init} ->{timeseries: traj} |
  measure: Analyze ~{traj: traj} ->{mse: mse}     # reads `traj`, so runs AFTER `run`
)
```

(`RunProcess` drives a temporal `process` over `[0, runtime]` and collects a
`TimeSeries` — the bridge from the temporal world into a dataflow step.)

---

## 9. The three separators — `::` type, `:` value, `fulfills` contract

One operator per concept, everywhere:

- **`::`** ascribes a **type** — `def x :: T`, ports `~{s :: map[float]}`, config
  `C[out :: Path = …]`, pattern-var sorts `?c :: Cell`.
- **`:`** binds a **value** — map/record entries `{a: 1.0}`, call args
  `Grow[rate: 0.2]`, port wirings `~{mass: mass}`.
- **`fulfills`** relates a **contract** (a process's *meaning*, §13) — declared
  `process Rk4 fulfills DeterministicMassAction[…]`, demanded `~{a :: T fulfills C}`.

```ys
def x :: float = 1.0
process P[k :: float = 0.1] ~{a :: float} ->{a :: float} ( { a: a } )
```

---

## 10. Reactions — match-and-rewrite (the one dynamical move)

A `reaction` is a redex `=>` reactum, optionally guarded by `where`. Inside:
`?x` is a site/binder, `~e` a shared **link** (a bond), `!` an unbound port,
`K[args](body)` an ion with nesting. A `BRS` runs a set of reactions over state.

```ys
# link-graph matching: A and B become bonded on a shared link `~e`.
# A parallel redex/reactum (`a | b`) is parenthesized.
reaction Bind[k :: float = 1.0] (
  (A ~{site: !} | B ~{site: !})
  =>
  (A ~{site: ~e} | B ~{site: ~e})
)

composite System[soup :: map[any]] ->{soup :: map[any]} (
  soup: soup |
  brs: BRS[rules: [Bind], mode: 'gillespie', seed: 1] ~{state: soup} ->{state: soup}
)
```

Reactions are **first-class values** — `rules: [Bind]` references one by name; a
process body can construct and install new ones (the homoiconic loop, §17). The
reactum may compute with a bound site: `?c :: Cell` binds the matched cell as a
typed value, and `?c.divide()` dispatches division on the cell's **type** — the
engine splits it by its schema (extensive fields halve, the rest is shared and
inherited), so one `Divide` rule works for any cell and **mass is conserved**:

```ys
reaction Divide[threshold :: float = 2.0] (
  ?c :: Cell[mass: ?m] where ?m > threshold
  =>
  ?c.divide()
)
```

A reaction's **rate** is an expression — its propensity — closing over config
params and matched bindings, so stochastic kinetics are real, not nominal:

```ys
reaction Convert[k :: float = 0.5] (
  (?a :: A)
  =>
  B
) rate ( k )
```

A `pattern` names a reusable redex FRAGMENT — write a shared shape once instead
of inlining it. A use `Name[args]` substitutes the args and splices the body in
(a parallel arg flattens by `|` associativity — no sigil):

```ys
pattern InCompartment[kind, contents] (
  Compartment[kind: kind] (contents | bystanders: ?rest)
)
```

**Two graphs, two pattern kinds** (this distinction matters):

- **Place-graph patterns** (nesting `( )`, map literals `{ k: … }`) match *within
  an open region you can see into* — they describe *location*.
- **Link-graph patterns** (`?x ~{port: ~e}`) match *across sealed composites* via
  published ports on a shared link — they describe *coupling*. Use these between
  composites; never reach into another composite's private inner fields.

**Functors** point the same reaction spine at *translation* instead of dynamics. A
`functor` maps each source generator to a construction in the target and fires
**once, exhaustively** (an endofunctor-safe BRS); `apply_functor(F, state)` lifts
it over the whole bigraph. It is the structure-preserving map *between* domains
(render — a `Cell` becoming an SVG `Circle` — is the first):

```ys run
functor Relabel :: Things -> Things (
  Foo => { _type: 'Bar' }            # rewrite every Foo node to a Bar node
)

apply_functor(Relabel, { x: { _type: 'Foo', n: 1.0 } })
```

---

## 11. Links — the value-bearing hyperedge

§10's `~e` is a *bond* a reaction matches. A **`link`** lifts that to a
first-class, **value-bearing hyperedge**: declare it once, and any node attaches a
port to it **by name** (`~{port: ~name}`). Every attached port reads the same
value, and updates from all of them accumulate on the one slot. The engine
resolves `~name` up the place graph to the nearest `link`, so it is
**depth-independent** — a daughter cell attaches wherever it lands.

```ys run
process Eat[k :: float = 0.1] ~{glucose :: float} ->{glucose :: float} (
  { glucose: -(k * glucose) }              # draw down the shared pool
)

composite Dish ->{glucose :: float @ glucose} (
  link glucose :: float = 100.0 |          # ONE shared pool — a hyperedge
  a: Eat ~{glucose: ~glucose} ->{glucose: ~glucose} |
  b: Eat ~{glucose: ~glucose} ->{glucose: ~glucose}   # both attach by NAME
)
```

Both eaters draw on the *same* 100 — after one tick it's 80, not 90. This is the
link graph made first-class, and the *same* primitive is a resource pool, a
quantum-entanglement edge, or a diffusion halo.

Combine it with §10's `?c.divide()` and you get the whole story —
`crates/chrysalis/ys/grow-divide-glucose.ys`: cells grow on a shared glucose
pool, divide when big enough (daughters inherit the attachment), and
`glucose + Σ mass` stays constant every tick.

---

## 12. Mesh links — the replicated, CRDT-safe link

A **`mesh`** link is a `link` (§11) that the engine may **replicate across peers**
and converge with *no coordinator*. Declare it `mesh`; locally it runs exactly
like §11, but its schema's `merge` must be a **join-semilattice** (commutative,
associative, idempotent — the CALM theorem) so peers converge regardless of order
or duplication. `map` (key-union) and `const` (immutable) qualify; additive
floats and last-writer-wins do not — and the **compiler refuses** an unsafe mesh
link (an additive/LWW/untyped replicated link is a *compile error*, not a runtime
surprise).

```ys run
process Post[who :: string = 'a'] ~{board :: map[float]} ->{board :: map[float]} (
  { board: { '{who}': 1.0 } }              # each peer contributes under its own key
)

composite Mesh ->{board :: map[float] @ board} (
  link board :: map[float] mesh = {} |     # replicated hyperedge (key-union = CALM-safe)
  a: Post[who: 'a'] ~{board: ~board} ->{board: ~board} |
  b: Post[who: 'b'] ~{board: ~board} ->{board: ~board}
)
```

The `.ys` is **byte-identical** whether this runs in one engine or across machines
over a live bridge — the mesh is an *address*, not a rewrite. This is what lets
one simulation distribute coordinator-free; it is also exactly how the agents
building prism gossip their `coord/<role>.ys` heartbeats into one board.

---

## 13. Contracts — a process's *meaning*

A **`contract`** states which mathematical object a process approximates — not
just its port shapes. `fulfills` annotates a process; `::` can demand one. Two
processes are substitutable only when they share a contract, so an illegitimate
comparison won't compile (the layer SED-ML/KISAO is missing).

```ys
contract DeterministicMassAction (
  target: MassActionODE,
  claims: Deterministic,
  advance: Continuous
)

process Rk4[network :: CRN]
  fulfills DeterministicMassAction[method: Rk4]   # leaves `method` open per fulfiller
  ~{state :: map[float]} ->{state :: map[float]}
  ( rk4.integrate(network, state, interval) )
```

`crates/chrysalis/ys/agreement.ys` runs RK4, Euler, Gillespie and FBA as
fulfillers of the *same* contract and asks the three distinct notions of "agree."

---

## 14. Units — dimensioned quantities

Declare units and dimensioned quantities; they are **dimension-checked at compile,
then erased to raw `f64`** (no per-op overhead — check once, run raw). `extensive`
opts a quantity into **halving on divide** (biomass); the default is *intensive*
(a concentration, copied).

```ys run
unit pg : [mass] = 1e-12 kg

process Grow[rate :: Quantity[unit: 1/s] = 0.5]
  ~{mass :: Quantity[unit: pg, extensive], interval :: Quantity[unit: s] = 1.0}
  ->{mass :: Quantity[unit: pg, extensive]}
  ( { mass: mass * rate * interval } )            # [mass]·[1/time]·[time] = [mass] ✓

composite Cell[mass :: Quantity[unit: pg, extensive] = 1.0]
  ->{mass :: Quantity[unit: pg, extensive]}
  ( mass: mass | grow: Grow ~{mass: mass} ->{mass: mass} )

Cell[mass: 1.0]
```

A dimension mismatch in the body (`mass * rate` without the time factor) is a
compile error. See `crates/chrysalis/ys/units.ys` and `nuclear-shuttle.ys`.

---

## 15. Imports & modules

The **shape of the path** decides the kind — explicit origin, no search path, so a
sibling file and a native module can never collide:

| write | resolves to |
|---|---|
| `from .name import …` | a sibling `.ys` **file** (`name.ys`, next to the importer) |
| `from .sub.name import …` | a relative file `sub/name.ys` |
| `from name import …` | a **native / registry** module (the std library / a package) |

```ys
from .grow import Grow              # a sibling .ys file
from .lib.cell import Cell          # a relative file lib/cell.ys
from core import RunProcess         # a native module (std: core / integrators / chem / io / meta)
from integrators import rk4, euler  # native objects → call methods: rk4.integrate(…)
```

A type alias / unit declared in an imported file comes along. A common pattern:
import a native function and *give it ports + a contract* by wrapping it in a
`process` you declare — the interface is yours, the math is borrowed. (A package
is a `Core`; importing one is `Core::merge` — see `docs/packages-ecosystem.md`.)

---

## 16. Protocols — where a component runs

A component is reached through a **protocol**, and the simulation **can't tell**
whether it runs in-thread or across a network — the boundary is transparent.
Bind a protocol + its config to a **type**, then use it like any control
(*protocols-as-types*):

```ys
from .cell import Cell
protocol StreamingCell = stream<Cell, path: 'cell.ys'>   # a Cell that runs as its own OS process

composite Environment ->{cells :: map[Cell] @ cells} (
  cells: {
    c0: StreamingCell[mass0: 1.0] ~{glucose: ^.glucose} ->{mass: %.mass},
    c1: StreamingCell[mass0: 1.0] ~{glucose: ^.glucose} ->{mass: %.mass}
  }
)
```

Address forms: **`local:`** (in-process, the default), **`stream:`** (a child
`.ys` as its own OS process over pipes), **`rest:`** (an HTTP server),
**`parallel:`** (a thread-pool worker). Swap `StreamingCell` back to `Cell` and
nothing else changes. **Concurrency is free**: the engine dispatches every due
remote node *before* collecting any result, so they overlap — wall-clock per tick
≈ the slowest one, not the sum.

---

## 17. Homoiconic — programs as data

An expression *is* a `{_type: …}` value (the same shape the parser emits). `from
meta import eval` interprets one — it can't tell a parsed expression from one you
assembled by hand. This is the substrate for runtime-built process bodies and
reactions: a process can `quote` a shape, transform it as data, and `eval` it.

```ys run
from meta import eval

def expr = { '_type': 'BinOp', 'op': 'Add',
             'lhs': { '_type': 'Float', 'value': 2.0 },
             'rhs': { '_type': 'Float', 'value': 3.0 } }

eval(expr)                                          # → 5.0  (the same as parsing `2.0 + 3.0`)
```

`eval(expr, { x: 5.0 })` resolves a free `Var('x')` in an environment. Because a
reaction is a value and an expression is a value, "a process that emits new
reaction rules" is just data construction — the higher-order, self-modifying loop
that `discover_processes` closes at the engine level.

---

## 18. Running a file, and the toolchain

**Every** `process`, `step`, and `composite` is runnable on its own — no
surrounding program. A composite runs its body; a bare `process`/`step` is
realized in a synthesized **default harness** (a state slot per port, self-wired),
so it runs "on a bench." Seed inputs with `--<port> VALUE`, set duration with
`--time`; the final state prints as JSON.

```sh
chrysalis run grow.ys --mass 1.0 --glucose 5.0 --time 5     # a process on a bench
chrysalis run crates/chrysalis/ys/grow-divide-glucose.ys --time 8

chrysalis check <file.ys>      # parse + contract / connection checks, no run
chrysalis bigraph <file.ys>    # emit the process-bigraph document as JSON
chrysalis repl                 # interactive eval (:type, :env)
chrysalis run <file.ys> --serve-process   # run as a driven child (the stream: other half)
```

The same file runs unchanged standalone (a bench) or *driven* by a parent over a
transport — the **invocation chooses, not the source** (§16). (Today, prefix with
`cargo run -p chrysalis --bin chrysalis --`, or `cargo install --path
crates/chrysalis` for the bare `chrysalis` command.)

---

## 19. Gotchas (read these once)

- **Subprocesses are auto-keyed by lowercased control name.** `Tick ~{…}` inside a
  composite becomes `tick: Tick ~{…}`. For two of the same control, **key them
  explicitly** (`a: Tick | b: Tick`) or one silently shadows the other. When in
  doubt, key.
- **Records vs maps disambiguate by key syntax.** Bare-ident keys `{a: 1}` →
  record; quoted/interpolated keys `{'a': 1}` / `{'{id}': v}` → map. A quoted key
  flips the representation. (On the "subtract" list — expect it to unify.)
- **Composite state is private.** Expose values through an output port/bridge to a
  parent slot; never read `inner.field` across the boundary. Encapsulation is
  definitional, and it's *why* cross-composite matching is link-graph (§10).
- **`--time` is run duration; the engine's per-step `dt` is `interval`.** Don't
  confuse them.

---

## 20. Planned — shown honestly (`ys ignore`)

These aren't live surface yet. Shown so you don't mistake them for live syntax.
(The `functor` definer that used to sit here just *graduated* — it's live now, §10.)

**Match-derived rate** — a propensity that *counts matched ions*, for full MAPK
kinetics. (Rate expressions are live, §10; `count(…)` over the match is missing.)

```ys ignore
reaction Phosphorylate[k :: float = 2.0] (
  InCompartment[?c, MEK ~{out: !} | ERK[name: ?n] ~{out: !}]
  => InCompartment[?c, MEK ~{out: ~b} | pERK[name: ?n] ~{out: ~b}]
) rate ( k * count(MEK) * count(ERK) )
```

**Cross-composite redex** — a reaction matching across SEALED composites by their
published ports on a shared link. The mechanism (`fire_across_composites`) exists;
wiring the surface matcher to it is the remaining slice.

```ys ignore
reaction Diffuse (
  ?west ~{edge: ~e} | ?east ~{edge: ~e}
  => ?west.balance(~e) | ?east.balance(~e)
)
```

---

## 21. The two laws for working *in* this codebase

1. **chrysalis is a THIN layer — never reimplement prism.** Matching, firing,
   BRS, scheduling, structural diff, the schema algebra all live in prism
   (`prism_schema::reaction` — `Pattern`/`find_matches`/`fire_rule_at`/
   `instantiate`; `prism_bigraph::BigraphicalReactiveSystem`, `Engine`,
   `Composite::from_config`, `discover_processes`; `prism_schema::algebra`).
   **Call them; don't clone them.** If prism lacks something, fix/extend prism in
   place (with a prism-side test), then call it. chrysalis's lane is
   `parse → ast → eval → compile`.

2. **The schema layer is a closed algebra.** All schema/state manipulation goes
   through the named operations (`default`/`check`/`apply`/`reconcile`/`resolve`/
   `promote`/`merge`/`diff`/`generalize`/`coerce`/`divide`/`serialize`). Never
   hand-roll a merge/diff/apply or stamp `Schema::Any` to dodge a type. See
   `docs/schema-algebra.md`.

**Design philosophy:** grow this language like *go / wei qi*, not *Twilight
Imperium* — capability through emergence/composition, not a rule per feature.
Before adding a primitive, check it isn't already derivable; prefer dissolving a
special case to adding one.

---

## Where to look next

- **[`README.md`](README.md)** — the index to all 40 design docs (start here for depth).
- `docs/chrysalis-design.md` — the full language design (kernel, categorical
  structure, units, contracts, tiers).
- `docs/NEXT-SESSION.md` — the live roadmap and task list.
- `crates/chrysalis/ys/` — the runnable corpus (`grow-divide-glucose.ys`,
  `mapk.ys`, `environment.ys`, `kuramoto.ys`, `units.ys`, `graph.ys`,
  `eval-demo.ys`); `packages/quantum/ys/` and `packages/synth/examples/` for the
  quantum & audio domains.
