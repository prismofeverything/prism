# The Schema Algebra

prism's schema layer is an **algebra**, not a grab-bag of helpers. This
document is its *axioms*: the sorts, the closed set of operations, and the
laws they obey. Everything that manipulates schema or state — the engine,
the Composite, chrysalis — is expressed **only** in terms of these
operations. A transformation that isn't one of them is a bug, not a
shortcut.

This is the standard that `ChrysalisBrs`, `infer_and_merge`,
`overlay_apply_types`, the schemaless `Composite`, and the dead `reconcile`
all violated. They were "extra operations" outside the algebra. The point
of writing the algebra down is so the next one is caught.

## Why an algebra

A faithful port of one method at a time still drifts, because each call
site can quietly invent its own merge/diff/apply. An algebra closes that
hole three ways:

1. **Closure** — there is a fixed set of operations; new behaviour is a
   *new named operation with laws*, never an inline `Value`/`Schema` munge.
2. **Laws as executable axioms** — each law is a property test over random
   typed values. A shortcut that breaks a law fails `cargo test`.
3. **Composition** — higher layers (Composite, engine, chrysalis) are
   *defined* in terms of the operations, so they inherit the laws instead
   of re-deriving (and re-breaking) them.

This is not a metaphor: it is precisely an **algebraic theory** in the
Plotkin–Power sense (the foundation under *algebraic effects*) — a
signature of operations plus the equations they satisfy. See **The
formalism** below for the vocabulary and what it buys (chiefly: the laws
become a portability contract).

## Sorts and carrier

- **Sorts** = the `Schema` variants: `Any`, `Bool`, `Integer`, `Float`,
  `Delta`, `String`, `Enum`, `List`, `Map`, `Tree`, `Array`, `Tuple`,
  `Maybe`, `Overwrite`, `Const`, `Quote`, `Site`/`InnerName`/`OuterName`/
  `Interface`, `Link`/`StepLink`/`ProcessLink`/`CompositeLink`/`Bridge`,
  `Custom`. `Any` is the bottom (least-specific) sort.
- **Carrier** = a *typed value* `(schema, value)`. An *update* is a value
  in the same sort interpreted as a change (additive for `Delta`/numeric,
  replacing for `Overwrite`, structural `_add`/`_remove` for collections).
- *Future refinement:* **units** are not a sort here — a `Quantity` is
  compile-time-checked then erased to `Delta`/`Float` (dimension dropped), which
  is why a units `type` can't serialize. The plan is a *dimension refinement* on
  the numeric sorts (in the **type**, erased from the **value** → still zero-cost),
  so `check` does dimensional wiring and the boundary codec does conversion. See
  **#71** / `docs/units-in-the-schema.md`.

## Operations (the closed set)

Each is **total** over the sorts and dispatches on the schema. Upstream
origin in `bigraph_schema/methods/`.

| Op | Signature | Meaning |
|---|---|---|
| `default` | `s → v` | the canonical inhabitant (unit) of a sort |
| `check` | `(s, v) → bool` | membership: is `v` a value of sort `s` |
| `infer` | `v → s` | the least sort `v` inhabits (structural) |
| `realize` | `(s, v?) → v` | fill defaults to a complete value |
| `apply` | `(s, v, u) → v` | act an update on a value (the action) |
| `reconcile` | `(s, [u]) → u` | combine many updates to one (update monoid) |
| `merge` | `(s, v, v) → v` | combine two values |
| `diff` | `(s, v, v) → u` | the update taking the first value to the second |
| **`resolve`** | `(s, s) → s` | schema **join** — least sort refining both (global; union of trees) |
| **`promote`** | `(library s, sparse s) → s` | **local** resolve — restrict the join to the sparse update's paths (what `apply` walks) |
| `generalize` | `(s, s) → s` | schema **meet** — greatest sort both refine |
| `coerce` | `(s, v) → v` | fit a value to a sort |
| `divide` | `(s, v, ctx) → [v]` | split a value (extensive splits, intensive copies) |
| `tensor` | `(s, v, v) → v` | the dual of `divide` — recombine the split parts |
| `mesh_safety` | `s → ok / err` | is `s`'s reconcile a join-**semilattice** (CRDT)? — the mesh closure invariant |
| `serialize` / `deserialize` | `(s, v) ↔ json` | codec across boundaries |
| `project` / `view` | `(s, wires, state) → ports` | a Composite's bridge I/O, *defined via* `apply`/`merge` |

`resolve` vs `promote` is the key distinction we kept missing: `resolve`
walks the whole library schema (set **union** — every branch), `promote`
walks only the sparse update's keys and substitutes the library's typed
node there (set **restriction**). Per-tick `apply` uses `promote`; merging
two declarations uses `resolve`.

`mesh_safety` is the **closure invariant for the mesh** (`mesh.rs`): a
`mesh`/`peer` link is replicated per peer and must converge with **no
coordinator**, which holds iff its schema-`reconcile` (the δ-CRDT join) is a
**join-semilattice** — commutative + associative + **idempotent** (CALM:
monotone ⇒ coordination-free). It is the structural dual of `divide`'s
extensivity question, classifying each reconcile strategy: key-union
(`Map`/`RecursiveTree`) and immutable (`Const`) are safe; `Tree`/`Tuple`
recurse per field; additive (`Float`/`Integer`/`Delta`/`Array`) and
last-writer-wins (`Overwrite`/`Bool`/`String`/`Any`/…) and sequence (`List`)
are rejected (the footguns). The `mesh:` protocol calls it at instantiation, so
a non-convergent link is refused before any replica can diverge — *illegal
distributed states are unrepresentable*. Laws: `tests/crdt_laws.rs` (each
verdict cross-checked against actual `apply` idempotence/commutativity).

## Deltas and reactions — the basis (#60)

The `u` (update) in `apply` / `diff` / `reconcile` is a **delta**: a value built
from the algebra's delta vocabulary — typed scalar deltas (extensive *sum* /
intensive *overwrite*, chosen by the sort) plus the structural sentinels `_add` /
`_remove` / `_divide`. This vocabulary belongs to the **algebra**, not to
reactions: `diff(a, b)` *produces* `_add`/`_remove` with no reaction involved
(`diff.rs`); value-methods (`graph.add_node`) produce them; `apply` is the sole
consumer. They are **irreducible** — without `_add`/`_remove`, `diff` cannot say "a
key appeared/vanished," so the `apply ∘ diff = id` adjunction requires them.

A **reaction** is the *dynamical generator* layered on top: `redex => reactum`,
fired by `find_matches` + `fire_rule_at`. Firing matches a redex and *produces a
delta* (its localized effect) that `apply` enacts — exactly as `diff` produces a
delta. So reactions do not sit *under* the sentinels and are not *defined by*
them; they are one **producer** of deltas, beside `diff` and methods.

The bridge is a **duality**:

> A bare **delta is a degenerate reaction** — the effect of a reaction whose redex
> is trivially satisfied (apply it here, unconditionally). A **reaction is a
> guarded, match-parameterized delta** — a delta plus a precondition (the redex)
> and its bindings.

So for *dynamics*, reactions are primary and a delta is the precondition-free
case; for the *substrate*, the typed delta algebra is primary and reactions
produce its deltas. They compose; neither subsumes the other — "reactions all the
way down" can't eliminate the type-directed `apply` (sum-vs-overwrite comes from
the *sort*, not a rule).

**Made load-bearing — rules-as-state.** A reaction is a first-class value
(`Value::Foreign(FOREIGN_REACTION, ReactionRule)`), so a *rule is state*: the BRS
reads its active ruleset from the state subtree each tick (seed rules ∪
reaction-values under the `_rules` meta-slot). A reactum can therefore `_add` a
reaction-value and have it fire on a later tick — the duality turned into the
**reaction loop**: reactions producing reactions (#61 AlChemy). Proven by
`prism-bigraph/tests/rules_as_state.rs`. The one move — `_add` of a spec — is
uniform across adding a process, a composite, or a reaction (topology + AlChemy
are the same shape).

## Eval: state-data → runnable (the lower rung)

Three engine boundaries each read a **typed-data value out of state and bring it
to life**. This is the *lower* rung of the reflective tower
(`docs/homoiconic-unification.md` §3; the upper rung is the surface `eval`,
`Expr → spec data`). It is **not a new operation** — each boundary is an EXISTING
codec / instantiate, and they share one shape: *scan state for a typed value,
eval it to its runnable form, collect.*

| boundary | reads (state-data) | evals via | to |
|---|---|---|---|
| **nodes** | a spec `{_type, address, config, inputs, outputs}` | `discover_processes` → `Core::instantiate` (`engine.rs`) | a running `ProcessNode` |
| **rules** | a reaction `{_pat: "Rule", redex, reactum, …}` (or eager `Foreign(FOREIGN_REACTION)`) | `collect_reactions`/`push_reaction` → `ReactionRule::from_data_value` (`brs.rs` · `reaction.rs`) | an active `ReactionRule` |
| **patterns** | a pattern `{_pat: "Site" / "Map" / …}` | `Pattern::from_value` → `find_matches` (`reaction.rs`) | a matcher |

The recognition only became *true* once **reaction joined nodes in reading
transparent data**: `push_reaction` now evals `from_data_value` beside the eager
`Foreign` carrier. Before, a reaction had to be a pre-built runnable `Foreign` to
be active while a process spec was plain data — an asymmetry that forced the
FLAT/RICH constructor seam (`homoiconic-unification.md`). With all three boundaries
evaling data, a constructor can emit data uniformly.

The governing axiom is the **codec round-trip** (Law 8, specialized): `from_value ∘
to_value ≡ id` for patterns; `from_data_value ∘ to_data_value ≡ id` for rules —
*modulo closures*: a computed `guard` / `reactum_fn` / `rate_fn` has no wire form,
so `to_data_value → None` (the honest boundary, not a lossy encode), and such a
rule stays the in-process `Foreign`. Pinned in `prism-schema/tests/reaction_data.rs`.
So the rung is *in* the algebra, not beside it: the runnable is reconstructed by a
lawed codec — the inverse of the `serialize`/`to_value` that put it in state.

## Laws (the axioms)

Written as `≡` (must hold for all typed values of the sort). These become
property tests in `prism-schema`.

1. **Apply identity.** `apply(s, v, default-update(s)) ≡ v`. The empty
   update is the identity of the action.
2. **Reconcile coherence.** `apply(s, v, reconcile(s, [u₁..uₙ])) ≡
   foldl(apply, v, [u₁..uₙ])` for commutative sorts; for non-commutative
   sorts `reconcile` is the *defined* batching (sequential apply is not).
3. **Reconcile is a fold.** `reconcile(s, [u])  ≡ u`; `reconcile(s, []) ≡
   default-update`; associative over concatenation.
4. **Resolve is a semilattice.** idempotent `resolve(s,s) ≡ s`,
   commutative up to default, associative; `resolve(Any, s) ≡ s` (`Any` is
   ⊥). The result refines both inputs (`check`-compatible supertype).
5. **Promote ≤ resolve.** `promote(lib, sparse)` agrees with
   `resolve(lib, sparse)` on every path `sparse` touches, and leaves the
   rest of `lib` untouched.
6. **Check preservation.** `check(s, default(s))`; if `check(s, v)` and `u`
   is a valid update then `check(s, apply(s, v, u))`. `apply` never leaves
   the sort.
7. **Diff/apply inverse.** `apply(s, v₁, diff(s, v₁, v₂)) ≡ v₂`.
8. **Codec round-trip.** `deserialize(s, serialize(s, v)) ≡ v`.
9. **Divide conservation.** extensive fields sum back to the original;
   intensive fields are copied; identity reissued. (Keys on
   `TypeRegistry::type_divide`.)
10. **Composite = algebra.** A Composite's `update` is *defined* as
    bridge-in (`project`) → run inner → reconcile inner updates → `apply`
    on the inner schema → bridge-out (`view`). It introduces **no**
    bespoke merge/diff/apply. This is the law that the schemaless,
    ad-hoc `Composite` broke.

## Closure invariant

> The engine, the Composite, and chrysalis perform **no** schema or state
> transformation outside the operations above. There is no `Schema::Any`
> apply shortcut, no hand-rolled `_add`/`_remove`, no per-call
> merge/diff/overlay. If you need a behaviour the algebra can't express,
> you **add a named operation with a signature and laws (and property
> tests)** — you do not inline it.

## The formalism: an algebraic theory of effects

What we are building "ad-hoc with operation guarantees" is, exactly, an
**algebraic theory** — the foundation under *algebraic effects* (Plotkin &
Power): a signature of operations plus equations. The correspondence is
one-to-one:

| algebraic-effects term | here |
|---|---|
| operation signature | the closed op set (`resolve`/`apply`/`merge`/`reconcile`/…) |
| equations | the laws |
| a **term** of the theory | any engine / Composite / chrysalis program (built only from ops) |
| a **model** of the theory | a concrete implementation that satisfies the equations |
| a **handler** | an alternative interpretation of an operation |

Naming it that buys two things beyond vocabulary:

1. **The laws are a portability contract.** A *model* is any implementation
   satisfying the equations, so a port to another system — the original
   Python `bigraph-schema`, a GPU backend, a distributed runtime — is
   correct **iff it passes the laws**. The property tests
   (`algebra_laws.rs`) therefore double as a **cross-implementation
   conformance suite**: implement the ops elsewhere, run the same laws,
   green ⇒ faithful. This is the algebraic-effects principle that
   *operations are separated from their implementation* — the
   implementation is a handler/model; the laws are what every handler must
   respect.
2. **The runtime is handler-shaped.** The *schema* layer is a pure
   equational (data) theory; the *execution* layer — running a process,
   scheduling, where the body lives — is genuinely effectful, and there a
   **protocol (local / parallel / REST / Ray) is a handler** of the
   "run process" operation. Same operation, swap the handler, run anywhere.
   prism's protocol abstraction already is this, unnamed.

**We adopt the framing, not an effect-system runtime.** No
delimited-continuation handlers, no effect-typed evaluator (Koka / Eff /
OCaml-5 style) — Rust has no native support and the *data* algebra needs no
control effects. The win is the vocabulary (theory / model / handler) and
laws-as-conformance-suite. A deliberate *future* option that fits the
meta-circular north star: chrysalis-the-language could expose operations +
handlers as a **surface** feature — `.ys` programs declaring and handling
effects, like Koka / Unison "abilities" / Eff / Frank.

References: Pretnar, [*An Introduction to Algebraic Effects and
Handlers*](https://www.eff-lang.org/handlers-tutorial.pdf) (2015, the entry
point); Plotkin & Power, [*Computational Effects and Operations: An
Overview*](https://homepages.inf.ed.ac.uk/gdp/publications/Overview.pdf) and
*Notions of Computation Determine Monads* (2002); Plotkin & Pretnar,
[*Handlers of Algebraic
Effects*](https://link.springer.com/chapter/10.1007/978-3-642-00590-9_7)
(ESOP 2009); Hyland & Power, [*Lawvere Theories and
Monads*](https://www.irif.fr/~mellies/mpri/mpri-ens/articles/hyland-power-lawvere-theories-and-monads.pdf);
Bauer & Pretnar, [*Eff*](https://arxiv.org/pdf/1203.1539); Leijen,
[*Koka*](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/08/algeff-tr-2016-v2.pdf).

## Enforcement (how we stay inside the algebra)

There is no single editor "setting" that enforces semantics — the
guardrails are:

1. **This doc** — the axioms. Any change to the core conforms to it or
   amends it (deliberately, with laws).
2. **Property tests** (`prism-schema/tests/algebra_laws.rs`) — each law as
   a `proptest` over generated `(schema, value)` pairs. The executable
   axioms; they run every `cargo test`.
3. **A closure-guard test** — greps the engine/composite/chrysalis sources
   for forbidden shortcuts (`Schema::Any.apply_update`, bespoke
   `_add`/`_remove` outside `apply`, hand-rolled merges) and fails if any
   reappear.
4. **`CLAUDE.md` convention** — the rule, where every session sees it.
5. **Build the consumer.** A new operation lands with the call site that
   needs it, so we never add capability without proof it's used and lawful.

## Building it (axioms first)

The fresh-core rebuild proceeds **operation by operation, law first**:
write the signature + laws (property tests) → implement against the
upstream `methods/<op>.py` → make the property tests pass → express the
next layer (Composite, engine) in terms of it → delete the ad-hoc path it
replaces. Keep the existing suite green throughout as a behavioural net.

## Guaranteeing closure — what to stand up *first*

Closure can't be guaranteed by discipline alone (that's how we got here).
Make it **structural and executable**, and stand the scaffolding up before
porting any op so the port is *self-constraining*:

1. **Encapsulation — the structural guarantee (biggest lever).** All
   schema/state *transformation* lives behind one module's public API: the
   algebra. The raw mutators (`Value::set_path` / `as_map_mut`, the
   `_add`/`_remove` handling, `apply` internals) become module-private; the
   engine, Composite, and chrysalis depend **only** on the algebra ops. A
   shortcut then *doesn't compile* — there is no public door to squish
   through. Perfect type-level closure (making every `Value` mutation
   private) is more than we need; encapsulating the *merge/apply/diff*
   surface is the 90%.
2. **Executable laws — the soundness guarantee.** `prism-schema/tests/
   algebra_laws.rs`: each law a `proptest` over generated `(schema, value)`
   pairs, with shrinking. Every op lands *with* its laws; they run on every
   `cargo test`. These same laws are the **conformance suite for any other
   implementation** of the theory (a *model*/handler — see *The formalism*):
   a port to Python / GPU / a distributed backend is faithful iff it passes
   them.
3. **Closure-guard test — the residue.** Greps engine/composite/chrysalis
   for forbidden patterns (`Schema::Any.apply_update`, hand-rolled
   `_add`/`_remove`, bespoke merges/overlays) and fails if any reappear —
   catches what encapsulation can't.
4. **Behavioural net.** The existing workspace tests stay green throughout;
   the port changes observable behaviour only where intended (e.g. the
   diffusion additive-apply fix).

**Minimal "ready to launch":** this doc (axioms) ✓, plus the laws-harness
skeleton, the closure-guard test, and a sketch of the encapsulation
boundary (which mutators go private; what the algebra's public API is).
With those, the op-by-op port below cannot drift.

## Launch plan (op-by-op)

0. **Scaffolding.** Laws harness + `(schema, value)` generators +
   closure-guard test + the `algebra` module skeleton. Confirm the
   workspace is green.
1. **Schema lattice.** `generalize` (meet) → `resolve` (join) → `promote`
   (local resolve), with laws (idempotent / commutative / associative;
   `resolve(Any, s) = s`; `promote ≤ resolve`). Delete `infer_and_merge`,
   the `Schema::resolve` stub, and chrysalis `overlay_apply_types`.
   (`resolve.rs` is already started.)
2. **Value/update ops.** `apply` (faithful), `reconcile`, `merge`, `diff`
   — laws: apply-identity, reconcile-coherence, diff/apply inverse. **Wire
   `reconcile` into the engine accumulation** (delete the
   `Schema::Any.apply_update` spots) and use `promote` in `apply`
   (fixes #14 / diffusion).
3. **Composite on the algebra.** Rebuild `Composite::update` as
   `project → run inner → reconcile → apply → view`, carrying its real
   inner schema (no `Schema::Any`). This is law #10 and the heart of the
   rebuild.
4. **The rest.** `coerce`, `realize`/`serialize` alignment, faithful
   `default`/`check`/`infer`; `divide` already done. Port as consumers
   need them.
5. **Cut over + encapsulate.** Delete every stand-in; make the raw mutators
   module-private; closure-guard green.

**Definition of done (closure achieved):** every schema/state transform
goes through the algebra; laws green; closure-guard green; `Composite`
defined via the algebra (no `Schema::Any`); all stand-ins deleted; full
workspace green; diffusion (#14) un-ignored and passing.

> **✅ ACHIEVED 2026-05-21.** All of the above hold. `prism_schema::algebra`
> is the single public door (raw mutators are `pub(crate)`); 13 property laws
> in `prism-schema/tests/algebra_laws.rs`; the closure-guard
> (`prism-bigraph/tests/closure_guard.rs`) ratchet baseline is empty. See
> `docs/NEXT-SESSION.md` for the current state and follow-up milestones.

## North star: Composite as a program

The ultimate demonstration that these principles hold end to end — **not
required for closure**, but the proof: once the parser and the algebra
exist, the Composite *orchestrator itself* becomes expressible as a `.ys`
program — a **one-level meta-circular interpreter** grounded in the Rust
algebra (Reynolds 1972; Smith's 3-Lisp; Wand & Friedman 1986). The thing
that runs programs is itself a program in the language, using only the
algebra. State is already data (`Value`), composites are already
first-class (`CompositeLink`), and the engine already runs them by
reflection (`discover_processes`) — so prism is most of the way there; the
missing piece is exactly the Composite orchestrator this rebuild moves into
the algebra. We don't build a full reflective tower (overkill); one
self-describing level is the payoff. See `docs/chrysalis-design.md`
(homoiconicity goal) and the references therein.
