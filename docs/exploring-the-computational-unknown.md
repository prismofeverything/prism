# Exploring the Computational Unknown

A survey of methods for programs that operate on programs, and a sketch of
the orthogonal axes that compose into the space of computational methods.
The aim: map where humanity has been so we can see where it hasn't, and
locate prism / chrysalis on that map.

---

## I. The self-referential lineage

### Lisp's quote/eval/apply (1960)

McCarthy's `eval` is the seed. Programs are S-expressions; `(eval form
env)` interprets the form. `quote` defers a form; `apply` calls a function
on a list of arguments. The homoiconic property: **the program's syntax
IS the data structure programs manipulate**. Every Lisp dialect since has
inherited this — programs are values; the interpreter is a function over
values.

Chrysalis is in this lineage: `Value::Map` is the tree-of-maps that IS
the syntax (an Expr serializes to one; `Expr::from_value` parses it
back).

### Meta-circular interpreters (SICP Ch. 4, 1985)

Write the interpreter for a language **in that same language**. The Scheme
evaluator in Scheme — every form's semantics is a clause in `eval`. Reveals
the language's essence: anything not in `eval` is sugar.

This is what `chrysalis::eval::Evaluator` is — but in Rust. The full
meta-circular move (`Evaluator` written in chrysalis itself) is open.

### Macros: syntactic abstraction (Lisp → Scheme → Racket → Rust)

A function `Syntax → Syntax`, run at compile time. Lisp's `defmacro`
(unhygienic), Scheme's `syntax-rules` and `syntax-case` (hygienic), Rust's
`macro_rules!` and procedural macros. Common Lisp's reader macros extend
the parser itself.

Chrysalis has no macros today. The substrate is in place — programs are
data, hand-construction works — but no compile-time hook from `.ys`
authors to manipulate the AST before evaluation. **Open axis.**

### Reflective towers (Brian Cantwell Smith, 1982-84)

Smith's **3-Lisp** generalizes meta-circular interpretation into an
infinite *tower*: every level is interpreted by the next level up, and
every level can **reify** (turn a process into data inspected from above)
or **reflect** (descend into the process being interpreted from below).

```
                 …
   level 3 ─┬─────────── interprets level 2
   level 2 ─┴─ reify ─→ data; ←─ reflect ─ data
   level 1 ─┴─ reify ─→ data; ←─ reflect ─ data
   level 0 (the executing user program)
```

Practically deep but rare in production. Implementations: 3-Lisp (Smith),
Brown (Wand), Black (Asai/Kameyama), Pink/Purple (Amin/Rompf, modernized).

**Chrysalis is one level. The infinite tower is open.** What we have:
- ✅ Level 0 — running programs.
- ✅ Level 1 — `load()` / `compile_value()` lift programs into data.
- ⏳ Level 2+ — meta-circular `Evaluator` written in chrysalis;
  reify/reflect across levels.

### Futamura projections (Yoshihiko Futamura, 1971)

If you have an interpreter `int(prog, input) → result` and a partial
evaluator `pe`:

| Projection | Specializes | Yields |
|---|---|---|
| 1st | `pe(int, prog)` | a *compiler* (a specialized interpreter = compiled program) |
| 2nd | `pe(pe, int)` | a *compiler-generator* (specializes interpreters) |
| 3rd | `pe(pe, pe)` | a *cogen-generator* (compiles compiler-generators) |

Implementations: Unmix (1991), Tempo (1996), MetaOCaml (staging-based),
LMS (Scala, Rompf et al.).

**Chrysalis is the interpreter half. A partial evaluator for it is
unimplemented.** The seed of a JIT-by-staging.

### Staging / multi-stage programming

Explicit annotations separate compile-time from runtime. MetaML and
MetaOCaml use `.<…>.` brackets and `~e` escapes; Lightweight Modular
Staging (LMS) uses types (`Rep[T]`). The compiler runs through the
program, eliminates the brackets, and emits a residual program.

**A staged chrysalis** would let `.ys` authors mark which Exprs are built
at "compile time" (during `compile_value`) vs evaluated each tick.

### Reflection in the OOP world

- **Smalltalk** — the canonical Meta-Object Protocol (MOP). Every class
  is an object; every send goes through `doesNotUnderstand:` if
  unhandled. The image IS the program; mutate any class at runtime.
- **CLOS** — Common Lisp Object System with full MOP (Kiczales et al.).
  Method dispatch itself is replaceable.
- **Java reflection / `java.lang.reflect`** — read-only mostly;
  bytecode-rewriting (ASM, ByteBuddy) for write access.
- **Ruby/Python `eval`** — string-based, late-bound. Powerful but lossy.
- **Erlang's hot code reloading** — swap modules in a running system.

Chrysalis's `MethodRegistry` is small-MOP — dispatch by `_type` is open
to user methods. No replaceable dispatcher yet (sealed at compile time).

### Quines and self-replication

A quine: a program that outputs its own source. Trivial in Lisp
(`((lambda (x) (list x (list 'quote x))) '(lambda (x) (list x (list 'quote x))))`).
Hofstadter's *Gödel, Escher, Bach* makes this the centerpiece of
self-reference.

A **chrysalis quine**: a `.ys` whose composite outputs its own source
text. Trivially possible today: `composite Self ->{src :: String} ( src:
load('this-file.ys')._source.read() )` — if we had a string-from-file
reader. The deeper version (output the AST that IS this file) needs
`Program::to_value` aimed at *the running program*.

---

## II. Less-explored families

### Term rewriting systems

Maude (Clavel et al.), ELAN. Programs are sets of rewrite rules over
terms; computation = applying rules until normal form. Bigraph
reactive systems (Milner) generalize this with named contexts and
hyperedges.

**Chrysalis IS bigraph rewriting** — that's the substrate (`prism`). The
unconventional moves are HIGHER on top: bigraph rewrites whose rules are
themselves bigraphs you can rewrite. Reflective bigraph systems are open
territory.

### Algebraic effects and handlers

Plotkin & Pretnar (2009), Bauer & Pretnar. Programs are terms over an
**effect signature** (operations like `read`, `write`, `choose`);
**handlers** are interpreters that give those operations meaning. State,
exceptions, non-determinism, async — all unify as effects + handlers.

Implementations: Koka (Daan Leijen), Multicore OCaml's eff, Eff (Bauer &
Pretnar), Frank (McBride). Conor McBride's "freer monad" Haskell
encoding.

**Method dispatch in chrysalis is effect-handler-shaped.** A
`MethodRegistry::dispatch(recv, method, args)` matches the
algebraic-effect-handler pattern: an operation (`recv.method(args)`) gets
interpreted by the handler attached to the receiver's type. The full
theory — composable handlers, scoped effects — is open.

### Logic programming / Prolog

Programs are *clauses*; computation is *unification* + *resolution*. The
direction is bidirectional — query a relation, get all solutions. Newer
extensions: λProlog (Miller, higher-order), Mercury (typed), miniKanren
(embedded relational).

**Chrysalis's reaction-rule matching IS unification-flavored** — the
matcher finds bindings that make a pattern hold in state. A full Prolog
inside chrysalis (backtracking, constraint stores) is open.

### Constraint programming

Programs are constraint networks; the runtime is a solver. Eclipse,
MiniZinc, Choco. SAT/SMT solvers (Z3, CVC5) are the modern engines.

Bigraphs + constraints: solving "for which initial state does this BRS
reach a desired final state?" is open. Inverse simulation.

### Probabilistic programming

Programs are *distributions*; "evaluating" them runs inference. Stan,
Pyro, Anglican, WebPPL (Goodman & Stuhlmüller). The Church language
(Goodman 2008) is a Scheme dialect where every term is a random sample.

**Stochastic bigraphs** exist (Krivine, Milazzo). The chrysalis
`MinimalGillespie` is one path — Gillespie's SSA is exact-Bayesian
inference over reaction networks. Lifting to general PPL: open.

### Differentiable programming

Programs are smooth functions; the runtime computes gradients. JAX,
PyTorch, Zygote (Julia), Enzyme (LLVM-level AD). Beyond ML: differentiable
physics (Brax, Taichi), differentiable rendering.

**A differentiable bigraph** — gradients of simulation outputs w.r.t. rate
constants — would let you *learn* rate constants from observed
trajectories. Open.

### Reversible computing

Every step is invertible. Janus (Lutz), Theseus (Bowman et al.). Quantum
gates are reversible by physical necessity. Bennett's reversible Turing
machines.

**Bigraph rewrites can be reversible** if redex ↔ reactum pairs match
under permutation. The schema-driven `apply` / `diff` algebra already has
the inverse — `apply(a, diff(a, b)) ≡ b` (law #7). Lift to reactions:
open.

### Quantum programming

Programs are unitaries; measurement collapses superposition. Q#
(Microsoft), Cirq (Google), Quipper, Silq (ETH), Qiskit. Categorical
foundation: dagger compact categories (Abramsky & Coecke).

Bigraph rewriting in superposition is unexplored. Combine with reversible:
**quantum bigraphs** — categorical, reversible, distributed. Unmined.

### Unconventional / natural computing

- **Cellular automata** — Rule 110 Turing-complete (Cook, 2004).
- **Membrane computing** — Păun's P-systems. Bigraphs generalize.
- **DNA computing** — Adleman (1994); molecular self-assembly.
- **Chemical computing** — Banâtre & Le Métayer's Γ-calculus.
- **Spiking neural networks** — Maass's third generation.
- **Reservoir computing** — physical media (water, ferrofluid) as computers.

**Bigraphs ARE the formal language for many of these.** P-systems lift to
bigraphs (Milner's original motivation). DNA tile assembly fits the
schema-driven divide. The substrate is here; the use cases are unmined.

### Concurrent / process calculi

π-calculus (Milner, Parrow, Walker), join calculus (Fournet & Gonthier),
ambient calculus (Cardelli & Gordon), bigraphs (Milner — unifies all of
the above).

**This is prism.** Composite-of-composites with explicit interfaces and
the local/rest/parallel/stream protocols IS π-calculus operationalized.

### Spreadsheets / dataflow

LabVIEW, Max/MSP, Pure Data, Excel, Glamorous Toolkit. Programs are
*wires* between *nodes*. Reactive — outputs update when inputs change.

**Chrysalis bigraphs are dataflow at the structural level.** Wire
declarations (`~{in}`, `->{out}`) bind processes; updates flow through.

### Game semantics

Programs are *strategies* in two-player games (Player = the program,
Opponent = the environment). Abramsky, Jagadeesan, Hyland, Ong.

Bigraphs in adversarial environments: open.

### Embodied / physical / soft computing

Computation = relaxation of physical systems. Hopfield networks,
analog computing, Lenia, neural cellular automata (Mordvintsev). Programs
as initial conditions; results as fixed points.

---

## III. Axes of variation

Every method is a point in a high-dimensional space. Listing axes (each
genuinely orthogonal — combining differently-valued axes gives genuinely
different methods):

1. **Representation**: text · AST · bytecode · binary · closure · proof ·
   bigraph · constraint network · distribution · circuit · physical state.
2. **Manipulation phase**: parse · macro · type-check · compile · stage ·
   runtime · between-ticks · between-sessions.
3. **Manipulator**: human (DSL) · system (auto-derivation) · runtime
   (JIT) · another program (synthesis, GP) · the environment (learning).
4. **Closure / self-reference**: open fragment · whole-program · reflective
   tower · fixed-point.
5. **Semantic direction**: forward (input → output) · backward (constraint
   solving) · bidirectional (logic programming) · differentiable
   (gradient).
6. **Determinism**: deterministic · non-deterministic (angelic /
   demonic) · stochastic · quantum-probabilistic.
7. **Concurrency**: sequential · CSP-style · π-calculus · actors ·
   bigraphs · join · async/await · data-parallel.
8. **Type discipline**: untyped · simply typed · System F · dependent ·
   refinement · linear · session-typed · effect-typed.
9. **Effects**: pure · effectful (operational) · effectful + handlers
   (algebraic) · monadic · capability-based.
10. **Reversibility**: irreversible · diff-reversible · gate-reversible ·
    physically reversible (thermodynamic).
11. **Distribution**: single-thread · multi-thread · multi-process ·
    multi-machine · cluster · planet-scale · solar-scale.
12. **Persistence**: stateless · single-image · journaled · event-sourced
    · CRDT-merging · blockchain-consensus.
13. **Time model**: discrete-step · continuous · superstep (BSP) · event-
    driven · physical-clock · logical-clock (Lamport, vector).
14. **Granularity**: instruction · function · object · process · agent ·
    bigraph node · whole simulation.

Compose any two axes ≠ at the **already-explored** points and you can
locate an open sector.

---

## IV. Where prism / chrysalis sits, today

| Axis | Position |
|---|---|
| Representation | AST + bigraph + tree-of-maps (`Value::Map`) |
| Manipulation phase | parse, compile, runtime (`compile_value` / `eval`); no macro phase |
| Manipulator | human + (open: another program) |
| Closure | one-level reflective; tower open |
| Semantic direction | forward; backward via patterns (limited) |
| Determinism | deterministic + stochastic (Gillespie); quantum open |
| Concurrency | bigraph composites + protocol-typed channels |
| Type discipline | refinement + nominal contracts; dependent open |
| Effects | small-MOP (method dispatch); algebraic-handler theory open |
| Reversibility | diff-reversible (law #7); rule-reversal open |
| Distribution | local / rest / parallel / stream; cluster planned (#25-29) |
| Persistence | engine-tick + delta trace + Document round-trip |
| Time model | discrete + BSP; continuous via integrator processes |
| Granularity | bigraph node / process / composite |

---

## V. Unexplored intersections (a partial list)

Each combines axes that, to my knowledge, no production system fully
inhabits:

- **Reflective bigraph tower** — meta-circular `eval` written in chrysalis
  itself, with reify/reflect across levels. (Smith's 3-Lisp, but bigraph-
  shaped.)
- **Macro phase for bigraph rewriting** — compile-time rewriting of
  reaction rules. A meta-BRS over BRSs.
- **Bigraph quines** — a `.ys` whose composite produces its own AST.
- **Probabilistic bigraphs (general)** — every reaction rule has a rate;
  the simulation IS the inference engine. Chrysalis has SSA; lift to
  variational inference.
- **Differentiable bigraphs** — gradients w.r.t. rate constants flow back
  through the BSP tick.
- **Reversible bigraph rewriting** — rules that pair-up so the rewrite
  group has inverses; chemical-reaction-network microscopic reversibility
  baked into the algebra.
- **Quantum-bigraph** — categorical (dagger compact) bigraphs whose state
  is a superposition; measurement is a place-graph operation.
- **Distributed reflection** — a remote bigraph node's contents are
  reachable as data via the same `_entities` field `load()` uses. Pair
  with #25 (ray protocol) for cross-machine introspection.
- **Bigraphs over algebraic-effect handlers** — bodies are effect terms;
  handlers swap interpretation (`local` execution vs `rest` proxy vs
  symbolic).
- **Constraint-solving bigraphs** — "for which initial state does this
  BRS reach a desired final state?" — backward semantics over reactions.
- **Differentiable simulation parameters** — autodiff through the engine
  tick, gradients of `total_biomass(t=T)` w.r.t. `mu_max`.
- **Self-organizing bigraphs** — neural cellular automata where the
  cells are bigraph composites; growth + division are learned, not
  programmed.
- **Spreadsheet-bigraph** — every place-graph cell shows its current
  value AND the formula that produced it. Interactive editing.
- **Inverse simulation as proof search** — given a goal state, find a
  reaction sequence (chemical retrosynthesis, planning).
- **Bigraphs as session types** — wire protocols typed by their message
  sequence; type-safe distributed handshakes.

Each intersection is an axis-pair (or triple) where the implementation
work would be specific but the *substrate* of bigraph + homoiconic AST
already exists.

---

## VI. References and pointers

Foundational:
- McCarthy, *Recursive Functions of Symbolic Expressions* (1960).
- Smith, *Reflection and Semantics in LISP* (1982); *3-LISP* (1984).
- Futamura, *Partial Evaluation of Computation Process* (1971).
- Milner, Parrow, Walker, *A calculus of mobile processes I & II* (1992).
- Milner, *The Space and Motion of Communicating Agents* (2009) —
  bigraphs canonical reference.
- Plotkin & Pretnar, *Handlers of Algebraic Effects* (2009).
- Abramsky & Coecke, *A categorical semantics of quantum protocols* (2004).

Practical:
- Kiczales, des Rivières, Bobrow, *The Art of the Metaobject Protocol*
  (1991).
- Friedman & Felleisen, *The Little Schemer* + *The Little Prover*.
- *SICP* Chapter 4 — Metalinguistic Abstraction.
- Rompf & Odersky, LMS papers (2010-).
- Goodman & Stuhlmüller, *Church / WebPPL* (2008-).
- Hofstadter, *Gödel, Escher, Bach* (1979) — non-technical, the right
  cultural primer.

prism-side:
- `docs/chrysalis-design.md` — the surface language.
- `docs/distributed-execution.md` — protocol-as-pluggable-backend.
- `docs/schema-algebra.md` — the closed algebra.
- `docs/cells-and-division.md` — composite-as-process.
- `crates/chrysalis/ys/eval-demo.ys` — the `(eval '(+ 2 3))` in `.ys`.
- `crates/chrysalis/ys/hand-built-demo.ys` — a program built from data.
- `crates/chrysalis/tests/entity_as_data.rs` — the AST round-trip suite.

---

## VII. The composition principle

The "space" of computational methods is **closed under composition** in
the axis-cartesian-product sense: you can pick a value on each axis
independently, and the resulting point is *a coherent method* whether
anyone has explored it or not. The job is to *navigate by composition* —
move along one axis at a time, see where you land.

Chrysalis's design happens to occupy a corner that few production systems
do (bigraph rewriting + homoiconic AST + algebraic schema + protocol-as-
type + distributed-by-default). The adjacent unexplored neighborhoods are
correspondingly fresh.

What we have now — programs ARE data — is the *enabling* axis. Most of
the entries in §V become tractable once the program is reflectively
available as a value the system can transform. We are one move from
several of them.
