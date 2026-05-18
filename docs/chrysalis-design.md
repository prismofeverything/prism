# Chrysalis Design

A surface programming language whose semantics compile to **prism**'s
Rust runtime. The substrate stays prism — `Topology`, `Process` trait,
`Step` trait, `BigraphicalReactiveSystem`, `Engine`, `ProcessRegistry`,
the `discover_processes` reflection mechanism. Chrysalis adds syntax,
expression-language update bodies, lexical scoping, parameterized
composites with closures, and the abstraction layer that turns prism
from a framework into a language.

**Home:** `crates/chrysalis/` (a member crate of the prism workspace).

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
rule = MassThresholdDivide[threshold: 2.0]
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

### 1. Grow/divide

Division becomes a runtime-constructed
`reaction MassThresholdDivide[threshold]` installed in a parent BRS,
not a hardcoded `step`. Tests: reactions-as-values, parameterized
rules, `where` guards on patterns.

```
process Grow[rate: Float = 0.2]
  ~{mass: Float, interval: Float = 0.1}
  ->{mass: Float}
(
  delta = mass * rate * interval |
  {mass: delta}
)

composite Cell[id: String, mass: Float = 1.0, growth_rate: Float = 0.02]
  ~{}
  ->{mass}
(
  mass: mass |
  Grow[rate: growth_rate] ~{mass: mass} ->{mass: mass}
)

reaction MassThresholdDivide[threshold: Float = 2.0] (
  ?cid : Cell[mass: ?m] where ?m > threshold
  =>
  { '{?cid}_0' : Cell[mass: ?m / 2],
    '{?cid}_1' : Cell[mass: ?m / 2] }
)

composite Environment[cells: Map[Cell], threshold: Float = 2.0]
  ~{}
  ->{cells}
(
  cells: cells |
  BRS[rules: [MassThresholdDivide[threshold: threshold]]]
    ~{state: cells} ->{state: cells}
)

main = Environment[cells: {'0': Cell[id: '0', mass: 1.2]}]
main.run(10.0)
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
