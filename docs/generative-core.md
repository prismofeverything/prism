# The generative core — minimality, unification, and the one-door discipline

> *Goal: maximize emergent capability from a minimal basis, so there is "one
> way to do each thing," while staying free to add the features real work
> requires. A balance between **ideal forms** (a minimal, confluent basis) and
> **practical tools** (the primitives ergonomics/performance demand). This doc
> names the theory behind that balance and the discipline that keeps it.*

This is the philosophy companion to [`feedback_chrysalis_wei_qi`] (the wei-qi /
Felleisen north star), [`project_chrysalis_evolution`] (useful→fundamental =
closure under composition), and [`schema-algebra.md`](schema-algebra.md) (the
schema layer as a closed algebra). Read those for the *practice*; read this for
*why it converges* and *where automation can live*.

## The three ideas usually conflated

"Reduce the codebase to its essential core; every feature exists for a reason;
only one way to do each thing; automate an invariant that always merges
duplications" — that single wish is actually **three** different ideas, and
separating them is the whole point, because two are beautifully solved and one
is provably impossible. Knowing which is which tells you exactly where a guard
can live and where you must rely on taste.

| The wish | The real name | Status |
|---|---|---|
| "a complete set of features that builds everything else" | a **generating basis** (closed under composition) | well-developed; *constructive* |
| "only one way to do each thing" | **confluence + normal forms** | well-developed; *partially automatable* |
| "automatically merge any duplication that appears" | **program-equivalence** | **undecidable** (Rice) — approximate via *one-door guards* |

## 1. A complete set from which everything is built = a generating basis

The oldest dream in the field, and very well-developed:

- **Combinatory completeness.** `S` and `K` generate *every* computable
  function — the purest "tiny axiomatic set that builds everything." λ-calculus
  does it with three forms (variable / abstraction / application).
- **Post's lattice** (Emil Post, 1941) is the object closest to what we're
  reaching for: the *complete map* of every set of Boolean operations and what
  each generates by composition — i.e. the lattice of *all* minimal complete
  bases. (`{NAND}` alone is complete.) It is literally "the space of generative
  cores."
- **Universal algebra / Lawvere theories.** A theory = generating
  **operations + equations**; everything expressible = the *free algebra*
  (terms modulo the equations). [`schema-algebra.md`](schema-algebra.md) **is**
  this: a signature (`apply`/`divide`/`merge`/`reconcile`/…) plus laws.
- **Bigraphs** (Milner, *The Space and Motion of Communicating Agents*) were
  *designed* as a minimal universal model — place graph + link graph +
  reaction — with CCS, π-calculus, Petri nets, and ambient calculus all
  embedding. prism's substrate already *is* a basis claim from the literature;
  see [`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md).

**The completeness test for a basis is closure under composition** — the exact
phrase from [`project_chrysalis_evolution`] ("useful→fundamental = closure under
composition / self-catalysis"). In universal-algebra terms a *clone* is a set
of operations closed under composition; the question "is my core complete?" is
"is my clone the whole space?" The metacircular north-star (the orchestrator
expressible as a `.ys` program) is the operational proof of closure.

## 2. One way to do each thing = confluence and normal forms

- A rewriting system that is **confluent (Church–Rosser) + terminating** has
  **unique normal forms**: every expression reduces to exactly one canonical
  representative. *That* is the formal shadow of "one obvious way to do it."
  The repeated "folding and kneading" of the code until it settles **is**
  normalization toward a fixed point — the intuition is literally correct and
  has a name.
- **Knuth–Bendix completion** is the exciting part: an *algorithm* that takes a
  set of **equations** (an algebra's laws — "these two paths are equal") and
  tries to orient them into a confluent terminating rewrite system — i.e. it
  *derives a canonicalizer / decision procedure from the axioms.* That is the
  nearest real thing to "automate a process that always merges duplications,"
  and it is exactly why the schema layer is stated as an **algebra with laws**:
  the laws are the equations a normalizer would consume. (Book: Baader &
  Nipkow, *Term Rewriting and All That*.)

So the duplication-merging dream is real, but it operates on **terms in an
algebra**, not on arbitrary code. The leverage is to push capability *into*
algebras (schema, units, the brand lattice) where "equal" is decidable.

## 3. The honest wall, and the way around it

- **The wall.** Detecting that *two arbitrary code paths compute the same
  function* is **undecidable** (Rice's theorem; program equivalence is
  undecidable). There is no total automatic dup-merger. This is *why* the
  invariant feels "not well-defined" — in full generality it provably isn't,
  and chasing it would become the straitjacket we want to avoid (forever
  fighting the false positives of an uncomputable approximation).
- **The way around it — guard the choke points ("one door").** Don't detect all
  semantic duplicates; make each capability have **one named door**, and add a
  *build-failing test* that fails if a second door appears. prism already has
  two:
  - `closure_guard` — "nothing manipulates schema/state outside the algebra."
  - #45 — "one `address → process` instantiation path."

  These are decidable, cheap, and they **don't constrain new features** — they
  only forbid a *second* way to do an *existing* thing. That is the
  non-straitjacket form of the invariant. The generalization is a small family
  of **architectural-invariant tests**: a sealed trait with one impl, a single
  exhaustive `match` the compiler protects, or a grep/AST test ("exactly one
  function builds a node spec").
- **Core + desugaring (the structural complement).** A tiny audited **core**
  calculus, and every surface form must **desugar** into it (Scheme/Racket).
  New features enter as **derived forms**, not new primitives — which is exactly
  **Felleisen's macro-expressibility** test (*On the Expressive Power of
  Programming Languages*, 1991): a feature that is macro-expressible from the
  core is a **conservative / definitional extension** — pure convenience,
  eliminable, safe to add. Only a genuine non-macro-expressible gap earns a new
  primitive.

## The synthesis

What we are building is:

> **a generating basis (closed under composition), whose surface desugars to a
> core, whose core is an equational algebra with confluent normal forms, and
> whose "one door" properties are pinned by build-failing invariants.**

The **ideal-vs-practical** tension is real and named: the ideal is the minimal
confluent basis; the practical reality is that we will *knowingly* keep a few
non-minimal primitives for pragmatics (performance, ergonomics). The discipline
is not to forbid them but to **document each as a deliberate conservative
extension**, so the exceptions stay visible instead of silently accreting into
the alternate paths churn introduces. (Brooks, *No Silver Bullet*: eliminate
**accidental** complexity, protect **essential** complexity.)

## How this maps to prism today

- **The basis:** bigraphs (place + link + reaction) + the schema algebra +
  reflection (`discover_processes`). Capability — composites, BRS, quantum,
  distributed, audio — is *emergent* from that, not a feature per case.
- **A worked unification (2026-06-06, the brand/subtype change):** `<:`
  (subsumption) became **one generator** that now produces matching, method
  dispatch, *and* node-kind recovery — three former special-cases collapsed —
  and `divide_by_schema(CompositeLink)` became the single division mechanism
  for both `?c.divide()` (reaction) and the Form-3 Divider. *Find the
  generator, delete the special cases.* See [`composite-is-a-type`] and the
  brand lattice in `prism-schema/src/registry.rs` (`is_a`).
- **Existing one-door guards:** `closure_guard` (schema algebra), #45 (process
  instantiation).
- **The recurring smell:** when a surface decision seems to need a
  case/side-channel heuristic, you are probably modeling the wrong graph or
  have thrown away a brand — *return to the core*, don't add a rule. See
  [`feedback_no_case_heuristics`].

## The practice ("folding and kneading")

1. **Subtract before adding.** Dissolve a special case back into the core
   before reaching for a primitive (the Felleisen gate).
2. **Find the generator.** When two paths exist, look for the one relation /
   operation that generates both, and delete the duplicates (the divide / brand
   unifications are the template).
3. **Pin it with a guard.** Where a capability has converged to one door, add a
   build-failing invariant so a second door can't reappear silently.
4. **Log the exceptions.** A retained non-minimal primitive gets a one-line
   "deliberate conservative extension because X" note, not silent tenure.
5. **Survey periodically.** Churn reintroduces alternate paths; sweep for them
   (task #7) and spin off one consolidation per confirmed duplication.

## Reading (shortest path to longest)

- Felleisen, *On the Expressive Power of Programming Languages* (1991) — the
  "is it a primitive?" test, formalized.
- Baader & Nipkow, *Term Rewriting and All That* — confluence, normal forms,
  Knuth–Bendix (the auto-merge engine).
- Post's lattice / Boolean clones (any survey) — the map of all complete bases.
- Milner, *The Space and Motion of Communicating Agents* — bigraphs as the
  universal substrate.
- Cardelli & Wegner, *On Understanding Types, Data Abstraction, and
  Polymorphism* — orthogonality of features (and Cardelli, *Type Systems*, for
  structural-vs-nominal and subsumption).
- Brooks, *No Silver Bullet* — essential vs accidental complexity.

[`feedback_chrysalis_wei_qi`]: ../../.claude/projects/-home-pattern-code-prism/memory/feedback_chrysalis_wei_qi.md
[`project_chrysalis_evolution`]: ../../.claude/projects/-home-pattern-code-prism/memory/project_chrysalis_evolution.md
[`composite-is-a-type`]: ../../.claude/projects/-home-pattern-code-prism/memory/composite_is_a_type.md
[`feedback_no_case_heuristics`]: ../../.claude/projects/-home-pattern-code-prism/memory/feedback_no_case_heuristics.md
