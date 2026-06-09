# Homoiconic unification — one `quote`, one `reify`, one `eval`

> **Synthesis doc** (owner: `unify`; implementers: `lang` + `simplify`). The
> homoiconicity analysis was **scattered** across [`categorical-core.md §7`](categorical-core.md)
> (the `quote`/`eval` reflection upgrade), [`chrysalis-design.md`](chrysalis-design.md)
> ("Naming convention" — the definer/constructor duality; "Open design decisions →
> Quoting/unquoting" — the wei-qi no-sigil splice), [`generative-core.md`](generative-core.md)
> (find the generator, delete the special cases), and tasks **#64 / #30 / #32**. That
> fragmentation is itself why homoiconicity *feels* like a bolt-on. This doc gathers it
> into ONE design + a staged, Felleisen-gated plan.
>
> **Goal — a single homoiconic face:** the capitalized constructor *is* `quote` of the
> lowercase definer; **one** path instantiates an entity; the surface `eval` and the
> engine's `discover_processes` are the two rungs of **one** reflective tower.
>
> **STATUS (2026-06-09, multi-agent push — `unify`/`lang`/`core`/`simplify`):** Stages
> **1, 2, 3, 4c DONE + PINNED.** `quote` total; one `build_rule` lowering core; the
> capitalized constructors are `quote` of their definers; the param-gap closed (`X[args]`
> calls a function); **all reaction producers now emit transparent DATA — FLAT/RICH dissolved
> for reactions** — and the one-door guards (invariants #1+#2) fail the build if it re-forks.
> **Remaining:** **4a** (the metacircular close — a `.ys` `eval` calling
> `Core::instantiate_spec`; substrate ready), the Axis-A `from_value`-completeness (reify
> reaction/function/type), and cleanups (retire `ReactionType::realize`; the `pattern`-definer
> value form). See §4 + the "Constructor face" section.

## 1. The diagnosis — the vision vs. the code (2026-06-09 survey)

Two source sweeps of `crates/chrysalis` + `crates/prism-bigraph` established what is
actually built:

| Piece | Vision (docs) | Code reality | Site |
|---|---|---|---|
| `quote` (`Expr::to_value`) | first-class | ✅ **TOTAL** — all 29 `Expr` variants | `ast.rs:1638` |
| `reify` (`from_value`) | total | 🟡 **28/29** — `Comprehension` deferred | `ast.rs:1973` (gap at `:2203`) |
| splice `a\|b\|c` | falls out of `\|`-assoc, no sigil | ✅ **DONE, exactly so** | `eval.rs:1778` / `:1792` |
| entity storage (`EntityDef`/`EntityView`, #30) | one name, many roles | ✅ **UNIFIED** | `ast.rs:730`/`:762`, `Program::entity` `:697` |
| **instantiate a definer → spec Value** | one path | ❌ **N hand-written builders** | `build_reaction_value` `eval.rs:1202`; `build_pure_spec` (process/step) `:729`/`:735`; `build_composite_outer` `:723`; `build_protocol_outer` `:743` |
| **capitalized constructors as values** (`Reaction[…]`, `Pattern[…]`, `ProcessDef[…]`) | = `quote` of the definer | ❌ **MOSTLY MISSING** — only `BRS[…]` works; the rest fall through to inert `{_type:…}` maps | `build_brs_value` `eval.rs:768`/`:1354`; fall-through `build_plain_map_value` `:769` |
| `compile_reaction` | dissolves into `eval ∘ quote` | ❌ **SPECIAL-CASED** — re-parses via `from_value`+`eval_pattern`, duplicating the definer's lowering | `compile_reaction_value` `eval.rs:1292` (dispatch `:531`) |
| `eval` = `discover_processes` | one operator | ❌ **TWO evals** — surface `eval` (Expr→Value) and engine `discover_processes` (Value→running) are unrelated | `prelude.rs:329` ⟂ `engine.rs:1486` (`discover_all_processes` `:1449`) |

**One sentence:** the *storage* of the definer/constructor duality is unified
(`EntityDef`) and `quote` is total — but the **instantiate side is N hand-written
`build_X_value` builders plus two duplicative special-cases (`compile_reaction`,
`build_brs_value`), and the constructor surface (`Reaction[…]` as the value form of
`reaction`) barely exists.** Homoiconicity is a bolt-on, not a fall-out of the
existing generators. *That* is the un-parsimony.

## 2. The proven template — splicing already does it right

The parsimony target **already exists in one place**: pattern splicing. There is no
`$` / `,@` unquote sigil — a param bound to a parallel that lands in a parallel
context flattens by the monoidal law `(a | (b | c)) ≡ (a | b | c)`
(`flatten_parallel`, `eval.rs:1778`). Unquote = name substitution
(`substitute_vars`, `eval.rs:1792`); splice = associativity normalization.

> **The whole unification, in one line:** do for **constructors** and **`eval`** what
> splicing already does for **unquote** — make homoiconicity fall out of an existing
> generator (a law, a registry, a round-trip), **never a new sigil / builder /
> import.**

## 3. The target shape — three operators, one of each

The homoiconic loop has exactly three operations; the runtime should have exactly one
of each:

| Operator | Type | Status | One door |
|---|---|---|---|
| **`quote`** | `Expr → Value` | ✅ total | `Expr::to_value` |
| **`reify`** | `Value → Expr` | finish it (§4 Stage 1) | `Expr::from_value` |
| **`eval`** | a 2-rung tower (below) | unify it (§4 Stages 2,4) | one instantiate + one discover |

`quote` / `reify` are an inverse iso (`Expr ↔ Value`). `eval` is **two rungs of one
reflective tower**:

- **surface-eval** — `Expr → spec Value` (reduce a constructor to its data form; the
  chrysalis `Evaluator`).
- **instantiate / discover** — `spec Value → running node` (bring a spec to life;
  prism's `discover_processes` / `Core::instantiate`).

The metacircular identity is **`run(p) = instantiate(surface_eval(quote(p)))`**. "One
eval" is the recognition that these are the tower's two rungs, and making *both*
first-class so a `.ys` program can build a spec and `eval` it into a running thing —
closing the loop. (This is the reflective-tower of
[`exploring-the-computational-unknown.md`], operationalized.)

### The desugaring that forges the face

```
reaction X[args] (redex => reactum)   ≡   def X = Reaction[args, redex, reactum]
process  X[args] ~{in} ->{out} (body) ≡   def X = Process[args, in, out, body]
step / composite / pattern …          ≡   def X = Step[…] / Composite[…] / Pattern[…]
```

The **lowercase definer is sugar for `def NAME = Kind[…]`**; the capitalized
constructor is that quoted value. Today only the definer half works, and only via N
hand-written builders. The unification makes the desugaring *real* and the builder
*singular*: both surfaces lower through one `instantiate`.

## 4. The plan — staged; every move DELETES a special case (Felleisen gate)

> Discipline: each stage ships with its consuming test (`feedback_no_half_measures`),
> stays inside the closed algebra (`feedback_schema_algebra`), and **calls prism, never
> clones it** (`feedback_chrysalis_thin_layer`).

### Stage 0 — synthesis · `unify` · ✅ this doc
Gate: replaces 4 scattered analyses + 3 tasks with 1 design. *(Deletes the
fragmentation.)*

### Stage 1 — close the round-trip + delete one bolt-on · `lang`
- **1a.** Finish `from_value`: handle `Comprehension` (`ast.rs:2203`) so `Expr ↔ Value`
  is **total**. Gate: *partial homoiconicity isn't homoiconicity* (§7). Consumer: the
  round-trip property test becomes total over all variants.
- **1b.** Dissolve `compile_reaction_value` (`eval.rs:1292`) into the general
  `eval ∘ quote` — drop the bespoke `from_value`+`eval_pattern` re-parse. Gate: deletes
  the `compile_reaction` bolt-on (§7's named promise). Consumer: existing reaction tests
  stay green through the general path.

### Stage 2 — the unified face: ONE instantiate path · `lang` (design with `unify`)

*Factoring validated against the code (2026-06-09, `unify` reading `eval.rs`).* The
collapse is real, but it has **three layers** that must stay distinct — conflating them is
how a "unification" smuggles in a false equivalence:

1. **The envelope** `{_type, address, config, inputs, outputs}` — **already unified** for
   process+step (both go `build_pure_spec → build_spec_value`, `eval.rs:1095`/`:1117`,
   differing only by the `kind` string). The N→1 work is *folding the rest into this one
   builder*: `build_brs_value` (`:1354`) is a degenerate `build_native_spec` (no declared
   interface — config = call-site args, ports = call-site wires; `:777`), so **BRS dedups
   to the native path**; the reaction- and composite-envelopes share `build_spec_value`'s
   shell.
2. **Typed construction** — **already one door**: `resolve_args_against_params` (`:1162`)
   calls `algebra::realize_with(types, schema, value)`, so "the value is born carrying its
   type" (a `:: Qubits` param promotes a literal). A reaction's value form should be a spec
   whose `Reaction`/`Rule`-typed slot is lowered by `realize_with` — the **same** typed
   path, not a bespoke `build_reaction_value`. *(This is the accurate form of "the
   kind-specific part is the schema, not a function.")*
3. **Body-lowering** — `kind`-specific and *legitimately so*: a process body is a quoted
   `Expr`; a reaction body is `redex => reactum` needing pattern-eval (`eval_pattern`); a
   composite body is nested state. These are **three sub-languages, not duplication** —
   keep them as named sub-ops. The duplication to KILL is that the *constructor* path
   re-does the *definer's* body-lowering: `compile_reaction_value` (`:1292`) re-parses via
   `from_value` then re-runs `eval_pattern`, when it should call the **same**
   pattern-lowering the definer uses (lang's "one rule-builder behind two doors").

- **2a.** Fold the envelope builders into one (`build_spec_value` generalized: process/step
  ✓ already · BRS → native · reaction-envelope · composite-envelope-shell). Keep the three
  body-lowerers as named sub-ops; route typed construction through `realize_with`.
- **2b.** Route capitalized `Reaction[…]`/`Pattern[…]`/`Process[…]`/`Step[…]`/`Composite[…]`
  through the **same** envelope + body-lowerer as their definers (dispatch `eval.rs:722`,
  fall-through `:769`). Definer becomes sugar: `kind X[…](…)` ≡ `def X = Kind[…]`. This is
  where the duplication dies — one body-lowerer serves both surfaces.
- Gate: collapses the envelope builders **N→1**, deletes `build_brs_value` +
  `compile_reaction_value` (the two special-cases), and fills the missing constructor
  surface — all by *sharing*, not adding. Consumers (each a real test): **#61 AlChemy** (a
  reactum `_add`s a `Reaction[…]` value); **synth A6** (build+install a module value — the
  homoiconic module factory); **#30 Slice 3** (`?c.divide()` typed dispatch).

**Sharpened by `simplify`'s first-hand audit (2026-06-09).**
- **(A) 7 doors, and 2b intercepts at the fallback.** The capitalized constructors
  `Reaction[…]`/`Pattern[…]` currently mis-route through `build_plain_map_value` (`:1388`,
  the free-control fallback) to inert `{_type:…}` maps — so **Stage 2b must intercept
  there**. Two doors stay deliberately separate: `build_native_spec` (`:777`, wholesale
  imports) and `build_plain_map_value` — decide with `lang` whether they also funnel
  through `instantiate` or remain the "no declared theory" escape hatches.
- **(B) the crux, cleanest form: construction = quote = DATA; runnable = eval.** The node
  builders return **data**; `build_reaction_value` alone returns a **runnable**
  `Foreign(Rule)` — it **conflates quote and eval**. The §7-consistent target: a
  constructor produces *data* (quote of the definer), and the runnable is produced by
  **eval** (`compile_reaction = eval ∘ quote`, Stage 1b). So Rule-building moves OUT of
  construction onto the eval rung, making reaction uniform with the nodes. *(This
  supersedes "realize the Rule at construction" in layer 2 above: realize-at-construction
  is for typed config **values** — promoting a `:: Qubits` literal; the **runnable** —
  Rule, or a running node via `discover_processes` — is always the 2nd rung.)*
- **(C) verified non-duplicate (no action).** `reaction.rs Pattern::{to_value,from_value}`
  (pattern-as-data, keeps holes via `_pat`) vs `assembly.rs {to_value,from_value}`
  (ground-state rendering, errors on holes) are distinct **by design** — not a
  consolidation target (optional clarity nit: rename assembly's pair
  `to_ground_value`/`from_ground_value`).

**Stage-2 ↔ Stage-4 split RESOLVED (`unify`, 2026-06-09, reading the consumer boundary
`brs.rs`).** The BRS reads rules-as-state via `collect_reactions` → `push_reaction`
(`brs.rs:472`/`:452`), which accepts **only** a pre-built runnable
`Value::Foreign(ReactionRule)` (it downcasts; a reaction-DATA map is silently ignored). So
**reaction cannot become pure data in Stage 2** — its consumer demands a runnable.
Therefore:
- **Stage 2 (chrysalis, now):** unify the **data**-producing builders (process/step/composite
  + BRS→native + the capitalized constructors) into one `build_spec_value`; **keep
  `build_reaction_value` producing the eager `Foreign(Rule)`** as the *one documented
  exception* — not a workaround but a real consumer constraint (the BRS boundary expects a
  runnable). lang's 1b already collapsed reaction's two doors into one `eval_rule_expr`, so
  reaction is **already single-door**; it just yields a runnable, not data.
- **Stage 4 (core + lang + unify, later):** make `collect_reactions`/`push_reaction` **eval**
  a reaction-DATA value in state into a `ReactionRule` (via 1b's `eval_rule_expr` =
  `compile_reaction = eval ∘ quote`) — exactly symmetric with how `discover_processes` evals
  a process-DATA spec into a running node. THEN `build_reaction_value` produces pure data
  like every other constructor and the exception dissolves. A **core-crate** change → it is
  why Stage 4 wants `core` booted.

This tightens §7: the "two evals" are `discover_processes` (DATA → running node) **and**
`collect_reactions` (DATA → runnable rule) — the **same** pattern (eval data-in-state into a
runnable). Stage 4 promotes BOTH; "eval = `discover_processes` promoted" generalizes to
rules too.

### Constructor face — the FLAT/RICH seam is PROVISIONAL (review w/ human, 2026-06-09)

lang's 2b landed `Reaction[…]` / `Pattern( … )` (flat kinds) as bracket constructors but
deliberately gave **no** bracket to process/step/composite (rich kinds), on a Felleisen
argument. Reviewed against the code, that split is **not a permanent design law — it is the
Stage-2 reaction exception surfacing in the constructor**, and must be framed as provisional:

- **Two axes were conflated.** *Axis A* — the **definition** as data (`EntityDef::to_value`/
  `from_value`/`compile_value`) — is uniform on the *quote* (`to_value`) side but **NOT yet on the *reify* side** (gap 3; `lang`, 2026-06-09). *Axis B* — the
  **instance/runnable** as a value — is where the split lives: process/composite → transparent
  DATA (`build_spec_value` map), reaction/pattern → opaque `Foreign(FOREIGN_RULE/REACTION/
  PATTERN)`. The "`Process[…]` would duplicate `to_value`" argument conflates A (definition)
  with B (instance); a constructor that is *sugar for quote* (same value as `to_value`) is the
  **duality** we want, not duplication.
- **It dissolves at Stage 4.** When the consumer boundaries — `discover_processes` (nodes),
  `collect_reactions` (rules), `find_matches` (patterns) — all **eval DATA → runnable**, every
  constructor can produce DATA (quote) uniformly and the Foreign-vs-data asymmetry is gone. So
  FLAT/RICH is a *pre-Stage-4 state*, not a type law. **Correct the framing** in
  `chrysalis-design.md` (Naming note) + `lang.next`.
- **The bracket is sugar for quote**, offered where inline construction is ergonomic
  (reactions/patterns are built inline in reactums/BRS lists; processes are usually named) — a
  presentation choice, not a kind-distinction. Invariant to hold: `Kind[…]` ≡ `quote(kind
  definer)`.

**Three gaps to close (so 2b is actually done, not just the flat family):**
1. **Parameterization.** `def X = Reaction[…]` is a *value*, so `X[args]` errors
   (`eval.rs:758`) — the `def`≡definer equivalence holds **only for parameterless** entities.
   A parameterized `reaction X[p](…)` must desugar to a **callable** (`def X(p) = Reaction[…]`,
   or `[]`-callable bindings) — `lang` designed it (lang.next); landing next. Until then the
   "drop-in" claim is parameterless-only.
2. **Uniformity proof.** ✅ **DONE (`lang`, 2026-06-09)** — `definer_equals_its_quote_then_eval`
   (`programs_as_data.rs`). It drove out a **real bug**: `EntityDef::to_value` recorded the
   `binding` slot's *name* but never its *value*, and `from_value` rebuilt only
   process/step/composite — so a program's trailing `main` (a `Def::Binding` holding the initial
   state) was **LOST through quote** (the reified program ran with no cells). Binding round-trip
   now fixed. The proof discipline earned its keep.
3. **Axis-A reify completeness** (⚠️ NEW, `lang`, 2026-06-09 — corrects the "uniform across every
   kind" premise above). `EntityDef::from_value` rebuilds **only** process/step/composite;
   reaction/function are *serialized but not rebuilt*, and unit/type/contract/protocol/context
   aren't serialized at all. So the *quote* (`to_value`) is broad but the *reify* (`from_value`)
   is partial — the "definition-as-data uniform across every kind" premise the Stage-4 dissolution
   leans on is **not yet true**. A **`from_value`-completeness pass** (the reify side of the entity
   round-trip) is the honest prerequisite. *(The reaction-as-DATA dissolution itself rides the
   `Expr::from_value` / `ReactionRule::from_data_value` paths, which ARE total — so 4c / the
   seam-dissolution isn't blocked; but the broad Axis-A claim was overstated.)*

### Stage 3 — the one-door guard · `simplify`
- A build-failing test: **"exactly one function instantiates an entity spec"** — the
  generalization of `closure_guard` + #45 (the existing one-door guards). Gate: a second
  builder cannot silently reappear.
- The **#46 duplication audit** scoped to the homoiconic surface (definer ↔ constructor
  ↔ `to_value`/`from_value` ↔ `compile_reaction`); spin off one consolidation per
  confirmed multiplicity.

### Stage 4 — one eval / the reflective tower · `core` + `lang` + `unify`

**Stage 4 = the chrysalis↔prism SEAM = a `core`+`lang` joint.** Recognition: all four
boundaries below are ONE 2nd-rung eval (DATA → runnable). **`core`** owns the prism side (the
boundary evals + the recognition doc in `schema-algebra.md`); **`lang`** owns the chrysalis
surface (the `.ys` `eval` op + the `build_reaction_value`→DATA flip + query builtins); they
**pair at the seam — chrysalis CALLS prism** (thin-layer, never clones; cf. core's 4c using
`from_data_value`, not `eval_rule_expr`). **`unify`** keeps the "one eval" recognition + the
thin-layer discipline honest. **Sequencing:** (1) the **4c handoff** — lang flips
`build_reaction_value`→DATA now (core's substrate is live + 2b is green) — *this is what
dissolves FLAT/RICH*, so first; (2) **4a** (the metacircular close) in parallel; (3) the
`from_value`-completeness prereq (gap 3); then 4b / 4d / the recognition doc.
- **4a.** Expose the engine-level rung as a first-class `.ys` operation: a `.ys` program
  can `eval` a spec it built into a running node. **Must call** prism's
  `discover_processes` / `Core::instantiate` (thin layer — `engine.rs:1486`/`:1449`),
  never clone. This makes the surface `eval` and `discover_processes` the same operator
  at the object level.
- **4b.** Unify the `meta` import's `eval`/`load` (`prelude.rs:329`/`:317`) with the
  engine rung where they coincide — delete the duplication if it is real.
- **4c. ✅ DONE (core, 2026-06-09) — the BRS rule boundary evals data.**
  `collect_reactions`/`push_reaction` (`brs.rs`) now lowers a transparent reaction-DATA value
  in state to a `ReactionRule` via **`ReactionRule::from_data_value`** — the prism-side
  structural codec, *not* chrysalis's `eval_rule_expr` (which is `Expr → Rule`, the wrong
  layer: prism-bigraph can't call chrysalis). A `{_pat:"Rule"}` map (the closure-free
  `to_data_value` form) is what crosses; a computed rule (guard/reactum/rate closure) keeps the
  in-process `Foreign(FOREIGN_RULE)`. Additive beside the eager `Foreign` carrier, symmetric
  with `discover_processes`. Consumer: `prism-bigraph/tests/reaction_data_as_state.rs` (3 green).
  Recognized as one rung with nodes+patterns in `schema-algebra.md` ("Eval: state-data →
  runnable"). **→ HANDOFF to `lang`:** the substrate is live, so (i) flip `build_reaction_value`
  to emit pure DATA (the `to_data_value` form) — reaction becomes uniform with the node
  constructors and the FLAT/RICH seam dissolves; (ii) the `ReactionType::realize` re-wrap
  (`{_pat:"Rule"}` → `Foreign`, `runtime/rule.rs` ~461) existed ONLY because the BRS read
  `Foreign`-alone — it can RETIRE for the BRS path (verify no other consumer needs it first).
- **4d. (substrate ready — `Pattern::from_value`.)** The pattern boundary's eval is
  `Pattern::from_value` (`reaction.rs`) — already total + lawed (`reaction_data.rs`) and reused
  *inside* `from_data_value`, so 4c already carries it for the rules-as-state path. A standalone
  `find_matches`-from-DATA entry is consumer-driven (only if lang's `count()`/`.matches()`
  builtins want it). With 4a/4c/4d, **every** constructor produces DATA and the FLAT/RICH seam
  is gone — *this* is the move that forges the uniform face (see "Constructor face" above).
  **Stage 4 is therefore the linchpin of the whole unification, not a tail.**
- Gate: cash out **only** where it deletes a special case (the `meta`-eval ⟂ engine-eval
  split; the metacircular orchestrator). *Honest note:* this is the most M5-ish rung
  (`categorical-core.md §3`/`§7`, the M/R-closure = reflection identity); keep it last
  and consumer-driven. Consumer: the metacircular north star — the orchestrator as a
  `.ys` program (the `coord/` board is already `.ys`-as-data).

## 5. Invariants (the one-door guards this establishes)

1. **One instantiate.** Exactly one function turns an entity (definer *or* constructor)
   into a spec Value (Stage 3 guard).
2. **Total quote/reify.** `from_value ∘ to_value = id` over *every* `Expr` (Stage 1a).
3. **No second eval.** The surface and engine evals are named rungs of one tower, not
   two parallel mechanisms (Stage 4).
4. **No new sigil.** Quote/unquote/splice stay name-substitution + `|`-associativity
   (§2) — additions to the homoiconic surface must justify themselves against this.

## 6. The discipline (why this is principled, not a rewrite)

- **Thin chrysalis.** `instantiate` and the engine rung **call** prism
  (`Core::instantiate`, `discover_processes`, `prism_schema::reaction`) — chrysalis's
  legitimate job is `parse → eval → compile`, never runtime algorithms
  (`feedback_chrysalis_thin_layer`).
- **Closed algebra.** If `instantiate` needs a genuinely new operation, it is a **named
  algebra op with a signature + laws + property tests**, not ad-hoc munging
  (`feedback_schema_algebra`).
- **No half-measures.** Every stage lands with its consumer (the tests named above), so
  the capability is *exercised*, not speculative (`feedback_no_half_measures`).
- **Felleisen gate.** Each stage is justified by the special case it *removes*; if a step
  only adds surface, it doesn't ship (`generative-core.md`).

## 7. What it unlocks

- **The face the user asked for** — `reaction`/`Reaction`, `process`/`Process` are one
  thing in two readable surfaces, lowering through one path.
- **#61 AlChemy / rules-as-state** — a reaction can author a reaction (the constructor
  is now a real value a reactum can `_add`).
- **synth A6** — the homoiconic module factory (build + install a module value live).
- **The metacircular orchestrator** — the M5 north star; `eval(quote(program))` closes.

## 8. References

- [`categorical-core.md`](categorical-core.md) §3 (fold/unfurl = M/R-closure =
  reflection), §7 (quote/eval) · [`chrysalis-design.md`](chrysalis-design.md)
  ("Homoiconicity goal", "Naming convention", "Value methods/Tiers", "Open design
  decisions → Quoting/unquoting") · [`generative-core.md`](generative-core.md) (the
  generating basis; the one-door guard) · [`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md)
  (object↔morphism; ports = cut links) · [`exploring-the-computational-unknown.md`](exploring-the-computational-unknown.md)
  (the reflective tower / Futamura).
- Tasks: **#64** (categorical core), **#30** (entity registry / duality), **#32**
  (programs-as-data), **#46** (duplication audit).
- Code anchors: `crates/chrysalis/src/{ast.rs,eval.rs,prelude.rs,parse.rs}`,
  `crates/prism-bigraph/src/engine.rs`.
