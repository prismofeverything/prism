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

**Contract:** `chrysalis::compile(source) -> (Topology, ProcessRegistry entries)`.
Everything below the compiler is prism-native.

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
   composite Cytoplasm[volume: Volume] using concentration(volume: @.volume) (
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
`concentration` context). **Remaining:** thread the resolved
conversions through `eval` so a checked body runs on bare `f64`, with
`ByState` factors (e.g. volume) read from the place graph at the
conversion site.

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

### Path syntax

| Symbol | Meaning |
|---|---|
| `@` | self / here / this composite as a value |
| `^` | parent (one place-graph level up) |
| `name` | child / field of current location |
| `@.id` | `id` field of self |
| `^.cells` | `cells` field on parent |

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

## Compilation map

| Chrysalis | Prism target |
|---|---|
| `process P[cfg] ~{in} ->{out} (body)` | `ProcessRegistry` entry; factory creates a `Process` whose update runs the interpreted body |
| `step S[cfg] ~{in} ->{out} (body)` | `ProcessRegistry` entry; factory creates a `Step` |
| `composite C[cfg] ~{in} ->{out} (body)` | `ProcessRegistry` entry; factory produces a state subtree (with `_type: "C"` markers + sub-component wires) realize-eligible for `discover_processes` |
| `pattern P[args] (body)` | function returning `Pattern` value |
| `reaction R[cfg] (redex => reactum)` | function returning `ReactionRule` value |
| `expr { … }` block (tier 2) | `Value` with `_type: "Expr"` + inferred return-schema field; constructors live in `MethodRegistry`. `ProcessDef.from_expr(e, schema)` lifts to an installable process if `e.schema` matches |
| `~{port: target}` | `Interface.inputs` IndexMap (domain of the morphism) |
| `->{port: target}` | `Interface.outputs` IndexMap (codomain of the morphism) |
| `^` in path | `..` in prism wire-resolution |
| `@` in path | empty / current relative root |
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

## Open design decisions

- **Quoting / unquoting** for pattern and expr literals. Likely:
  `pattern X[…] (body)` and `expr (body)` bodies are implicitly
  quoted; `$expr` inside unquotes. Settle the unquote sigil.
- **Rate expressions, not constants.** `rate: |MEK| * |ERK| * k_on / V`
  — rate becomes an expression closing over matched bindings.
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
