# Chrysalis primer — the executable, canonical reference

This is the **fluency reference** for chrysalis (`.ys`), the surface language that
compiles to the prism process-bigraph runtime. It is the recommended first read
for any new session.

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

> **Status:** the verified core **plus the link surface**. `pattern`, rate
> expressions, first-class **`link`** (§9), and reaction-driven **division**
> (`?c.divide()`, §8) are all live — and `ys run`-tested below. Only two forms
> stay `ys ignore` (match-derived rate, cross-composite redex), shown honestly in
> §11. See `docs/NEXT-SESSION.md` + memory `project_chrysalis_evolution`.

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
(§7) or importable. A defs-only file with no interface is import-only.

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

---

## 3. Functions

Functions are first-class values, also introduced with `def`:

```ys
def double(x) = x * 2.0
def apply(f, x) = f(x)        # pass / return / store functions
```

---

## 4. Comprehensions

The list form builds a list; the map form builds a map (computed keys).
Iterating a map binds `(key, value)`; a list binds `(index, value)`.

```ys
def scale(items) = [ x * 2.0 for x in items ]
def positives(items) = [ x for x in items if x > 0.0 ]
def doubled(m) = { k: v * 2.0 for k, v in m }
```

(Comprehensions cover map/filter. There is no surface `fold`/`reduce` yet —
aggregation across siblings rides on the engine's additive `apply`.)

---

## 5. Processes and steps — the two leaf morphisms

`process` and `step` are leaf (non-composite) morphisms. Ports are typed with
`::`. A body is a `|`-separated parallel of statements; the final line *without*
`|` is the body's value (the update the engine applies).

```ys
process Grow[rate :: float = 0.2] ~{mass :: float} ->{mass :: float} (
  delta = mass * rate |        # a binding
  { mass: delta }              # the value = the update (a delta on `mass`)
)

step Threshold[limit :: float = 2.0] ~{mass :: float} ->{over :: bool} (
  { over: mass > limit }
)
```

`[config]` is the construction-time seed (set once); `~{inputs}` arrive per tick.
The update returned by the body is a **delta** — how a port's value changes — not
a full overwrite (that's why `{ mass: delta }` accumulates for additive types).

---

## 6. Composites — wiring components into a sub-bigraph

A `composite` body declares state slots (`name: value`) and wires sub-components
to them. Wire roots:

| root | meaning |
|---|---|
| `name` | a sibling slot / child of the current composite |
| `^` | the parent (one place-graph level up) |
| `%` | this node itself (the own face, e.g. a map cell's `%.mass`) |
| `@` (in a port decl) | the **bridge** — map this port to an inner path |

A composite is reached only through its ports — **a composite *is* a process**
(the category says so); a caller can't tell a subgraph from a leaf.

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

---

## 7. The three separators — `::` type, `:` value, `fulfills` contract

One operator per concept, everywhere:

- **`::`** ascribes a **type** — `def x :: T`, `f(x :: T) :: Ret`, ports
  `~{s :: map[float]}`, config `C[out :: Path = …]`, pattern-var sorts `?c :: Cell`.
- **`:`** binds a **value** — map/record entries `{a: 1.0}`, call args
  `Grow[rate: 0.2]`, port wirings `~{mass: mass}`.
- **`fulfills`** relates a **contract** (a process's *meaning*) — declared
  `process Rk4 fulfills DeterministicMassAction[…]`, demanded `~{a :: T fulfills C}`.
  Substitutability is `algebra::refines`; an illegitimate comparison won't compile.

```ys
def x :: float = 1.0
process P[k :: float = 0.1] ~{a :: float} ->{a :: float} ( { a: a } )
```

---

## 8. Reactions — match-and-rewrite (the one dynamical move)

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
process body can construct and install new ones. The reactum may compute with a
bound site: `?c :: Cell` binds the matched cell as a typed value, and
`?c.divide()` dispatches division on the cell's **type** — the engine splits it
by its schema (extensive fields halve, the rest is shared and inherited), so one
`Divide` rule works for any cell and **mass is conserved**:

```ys
reaction Divide[threshold :: float = 2.0] (
  ?c :: Cell[mass: ?m] where ?m > threshold
  =>
  ?c.divide()
)
```

This runs end-to-end — §9 puts it on a shared resource pool (the full grow/divide).

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
of inlining it in every reaction. A use `Name[args]` substitutes the args and
splices the body in (a parallel arg flattens by `|` associativity — no sigil):

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

---

## 9. Links — the value-bearing hyperedge

§8's `~e` is a *bond* a reaction matches. A **`link`** lifts that to a
first-class, **value-bearing hyperedge**: declare it once, and any node attaches
a port to it **by name** (`~{port: ~name}`). Every attached port reads the same
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

Combine it with §8's `?c.divide()` and you get the whole story —
`crates/chrysalis/ys/grow-divide-glucose.ys`: cells grow on a shared glucose
pool, divide when big enough (daughters inherit the pool attachment), and
`glucose + Σ mass` stays constant every tick.

---

## 10. Gotchas (read these once)

- **Subprocesses are auto-keyed by lowercased control name.** `Tick ~{…}` inside a
  composite body becomes `tick: Tick ~{…}`. If you need two of the same control,
  **key them explicitly** (`a: Tick[…] | b: Tick[…]`) or one silently shadows the
  other. Bare unkeyed subprocesses in non-obvious positions can silently not run —
  when in doubt, key.
- **Records vs maps disambiguate by key syntax.** Bare-ident keys `{a: 1}` →
  record; quoted/interpolated keys `{'a': 1}` / `{'{id}': v}` → map. A quoted key
  flips the representation. (This is on the "subtract" list — expect it to unify.)
- **Composite state is private.** Expose values through an output port/bridge to a
  parent slot; never read `inner.field` across the boundary. Encapsulation is
  definitional, and it's *why* cross-composite matching is link-graph (§8).
- **`--time` is run duration; the engine's per-step `dt` is `interval`.** Don't
  confuse them.

---

## 11. Planned — shown honestly (`ys ignore`)

Two forms aren't live yet (the active directions; memory
`project_chrysalis_evolution`). Shown so you don't mistake them for live syntax.

**Match-derived rate** — a propensity term that *counts matched ions*, for the
full MAPK kinetics. (Rate expressions themselves are live, §8; what's missing is
`count(…)` over the match.)

```ys ignore
reaction Phosphorylate[k :: float = 2.0] (
  InCompartment[?c, MEK ~{out: !} | ERK[name: ?n] ~{out: !}]
  => InCompartment[?c, MEK ~{out: ~b} | pERK[name: ?n] ~{out: ~b}]
) rate ( k * count(MEK) * count(ERK) )
```

**Cross-composite redex** — a reaction matching across SEALED composites by their
published ports on a shared link (the link-graph form of §9, *not* place-graph
descent — see §8 and memory `feedback_no_case_heuristics`). The mechanism
(`fire_across_composites`) exists; wiring the surface matcher to it is the
remaining slice.

```ys ignore
reaction Diffuse (
  ?west ~{edge: ~e} | ?east ~{edge: ~e}
  => ?west.balance(~e) | ?east.balance(~e)
)
```

---

## 12. The two laws for working *in* this codebase

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

**Design philosophy** (memory `feedback_chrysalis_wei_qi`): grow this language
like *go / wei qi*, not *Twilight Imperium* — capability through
emergence/composition, not a rule per feature. Before adding a primitive, check
it isn't already derivable; prefer dissolving a special case to adding one.

---

## Where to look next

- `docs/chrysalis-design.md` — the full design (kernel, categorical structure,
  units, contracts, tiers).
- `docs/NEXT-SESSION.md` — the live roadmap and task list.
- `crates/chrysalis/ys/` — the runnable example corpus (`grow-divide-unbounded.ys`,
  `mapk.ys`, `environment.ys`, the `quantum-*` suite).
- `crates/chrysalis/ys/GUIDE.md` — the longer-form tutorial (being folded into
  this primer).
