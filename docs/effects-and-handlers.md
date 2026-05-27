# Algebraic Effects and Handlers in chrysalis / prism

A design sketch for reframing chrysalis's current orthogonal mechanisms
(protocols, scheduling, observation, state) as **algebraic effects** with
**handlers**, plus what new capabilities that frame unlocks — especially
in combination with the homoiconic AST infrastructure (`load`, `eval`,
`compile_value`, `Program::to_value`/`from_value`).

The thesis: today's chrysalis has SEVERAL implicit effect systems baked
in — each clean on its own, but unrecognized as instances of the same
algebraic-effect-and-handlers pattern. Making them explicit collapses the
implementation surface, lets handlers compose freely, and (because
programs are data) lets handlers themselves be hand-built and swapped.

---

## I. Yes — protocols ARE an effect system

Look at what `local:` / `rest:` / `parallel:` / `stream:` actually share:

> **The abstract operation `invoke(process_id, input_state) → output_delta`,
> with one *handler* per protocol.**

| Protocol | Handler implementation |
|---|---|
| `local:`    | call `ProcessFactory(config).update(state, dt)` in-process |
| `rest:`     | HTTP POST input, await deltas |
| `parallel:` | enqueue on ParallelPool, slot-Defer await |
| `stream:`   | write Arrow frame to child pipe, read response |

The composite / engine code doesn't know which protocol is in play — it
calls a uniform `invoke()` and the protocol layer dispatches. The
**signature is one operation; the handlers vary; the using code is
identical**. That is the textbook Plotkin-Pretnar algebraic-effects shape.

What we've called "the protocol seam" is the effect-handler seam. The
`docs/distributed-execution.md` "pluggable backend" view is the
multi-handler view. The doc just doesn't *name* it as algebraic effects.

So: **YES, naming them as effects is the right move.** It clarifies what
the abstraction IS, opens the door to composable handlers, and shows the
shape other mechanisms could fit.

---

## II. What else in chrysalis IS already an effect system?

Once you look, they're everywhere:

| Implicit effect | Operation(s) | Today's handler | Other possible handlers |
|---|---|---|---|
| **Process invocation** | `invoke(id, state) → delta` | local/rest/parallel/stream | speculative, cached, mocked-for-test |
| **Method dispatch** | `dispatch(receiver, method, args) → value` | `MethodRegistry::dispatch` (key on `_type`) | logged, intercepted, virtualized |
| **State update** | `apply(schema, state, delta) → state'` | `algebra::apply_with` | journaled, undo-able, conflict-resolving (CRDT) |
| **Time / scheduling** | `tick(t) → t'` | BSP fixed-dt + per-process intervals | event-driven, Gillespie τ, real-clock, reversed |
| **Discovery** | `discover(state) → process_set` | walk state for `address` / `_type: process` | cached, fuzzy, lazy, just-in-time |
| **Schema check** | `check(schema, value) → bool` | strict per-sort | inferring, refining, fuzzing, runtime-extending |
| **Trace / observe** | `emit_frame(t, state)` | optional Arrow trace | rich logging, sampled, lossless replay |

Each row is a place where **the operation is already abstracted from the
implementation**, but we haven't given the abstraction a *name*. Effect
handlers are how you name them.

---

## III. The full landscape: effects we DON'T have yet

What new effects open if we adopt the model? The interesting ones aren't
just refactorings of today's machinery — they're capabilities that need
the handler indirection to exist at all:

### Choice (non-determinism, probability, quantum)
`op choose<A>(options: List<A>, weights?: List<Float>) → A`
- **Deterministic handler**: always `options[0]`. Today's behavior.
- **Random handler**: sample by weight. Stochastic simulations.
- **All-paths handler**: enumerate every choice, return list of all
  possible outcomes. Logic-programming-style backtracking.
- **Probability handler**: don't pick — carry the *distribution*. Sample
  at the end. Probabilistic programming.
- **Quantum handler**: choose is a superposition; only "measurement"
  collapses. Quantum simulation.

A single body using `choose` runs as five different paradigms by handler
swap.

### Differentiation
`op param(name, value: Float) → Float`
- **Plain handler**: returns `value`. Normal run.
- **Forward-mode**: returns `Dual(value, ∂)`. Forward autodiff.
- **Reverse-mode**: returns `value` but records the tape. Backprop.
- **Symbolic**: returns `Var(name)`. Builds a symbolic expression.

Wrap `algebra::apply` with the differentiation handler → gradients of
final-state observables w.r.t. rate constants. Learn rate constants from
trajectories.

### Reversibility
`op step(state) → state'` paired with `op unstep(state') → state`
- **Forward-only handler**: discard inverse.
- **Journaled handler**: record (state, action) tuples; can rewind.
- **Pure-reversible handler**: require every rule to have a paired inverse.

Reversible bigraph rewriting is one example. Time-travel debugging in
simulations is another.

### Resource / cost accounting
`op consume(cost: Float) → ()`
- **Free handler**: ignore.
- **Counter handler**: accumulate; expose total.
- **Budget handler**: abort if total > budget.
- **Cost-routing handler**: pick cheaper alternatives.

Spent CPU per cell, energy per reaction, simulation-time per tick — all
become uniform: `consume(x)` from inside the body, handler decides.

### Sandboxing / capability
`op invoke_remote(addr)`, `op write(path, val)`, `op read_secret(name)`
- **Open handler**: allow everything.
- **Restricted handler**: deny based on capability set.
- **Audit handler**: log every privileged operation.

A composite is given a handler bundle as part of its launch context — it
literally cannot perform operations its handlers don't catch.

### Hot reload / live coding
`op resolve_factory(name) → Factory`
- **Static handler**: look up in compile-time registry.
- **Live handler**: re-read source / hand-built AST on every call.

The running engine reflects fresh code without restart.

### Inverse / backward semantics
`op solve_for(goal: State, rules: BRS) → InitialState`
- **Forward handler**: simulate the rules forward (today).
- **Backward handler**: search backward from the goal.
- **Mixed handler**: bidirectional search.

The user asks "what initial cell config divides exactly twice in 60s?"
The handler picks the search strategy.

### Persistence
`op snapshot() → SnapshotId`, `op restore(id)`
- **In-memory handler**: keep in process.
- **Disk handler**: serialize via the existing Document codec.
- **Event-sourced handler**: store deltas, replay.
- **CRDT handler**: merge concurrent snapshots.

### Concurrency primitives
`op atomic(body)`, `op signal(channel, value)`, `op await(channel)`
- Pick the synchronization model by handler. Same body runs as
  Goroutine-flavored, actor-flavored, STM-flavored.

---

## IV. The shape of the system

Picture the runtime as a **handler stack** wrapped around the engine
tick:

```
   ┌─────────────────────────────────────────────┐
   │ outermost: trace + budget                    │
   │  ┌────────────────────────────────────────┐ │
   │  │ logging + persistence                  │ │
   │  │  ┌──────────────────────────────────┐  │ │
   │  │  │ invoke-protocol stack            │  │ │
   │  │  │  ┌────────────────────────────┐  │  │ │
   │  │  │  │ apply / dispatch handlers  │  │  │ │
   │  │  │  │  ┌──────────────────────┐  │  │  │ │
   │  │  │  │  │ engine.tick()        │  │  │  │ │
   │  │  │  │  └──────────────────────┘  │  │  │ │
   │  │  │  └────────────────────────────┘  │  │ │
   │  │  └──────────────────────────────────┘  │ │
   │  └────────────────────────────────────────┘ │
   └─────────────────────────────────────────────┘
```

Every effect op `bubbles up`. The closest matching handler catches it.
Handlers can *transform* (e.g., logging wraps the inner op and emits a
log line), *replace* (e.g., test-mock returns a canned response), or
*forward* (pass to the next handler up).

The data layout is straightforward:

```
struct EffectHandler {
    operation: Name,
    catch: Fn(Op, K) -> Value     // K is the continuation
}

struct HandlerStack { handlers: Vec<EffectHandler> }
```

When the engine wants to do `invoke(id, state)`, it walks the stack
looking for a `catch` whose `operation == "invoke"`. The first match
fires; if none match, the *default* handler fires (the current built-in).

---

## V. The homoiconic angle — handlers as data

Because programs are data, **handlers are programs**. A handler is just a
function `Op → Value`. With `compile_value` + `eval`, a handler can be:

1. **Hand-built in `.ys`** — author writes the dispatch logic with the
   `ast` library.
2. **Loaded from disk** — `load("handlers/mock.ys")` returns a handler.
3. **Generated programmatically** — a meta-handler builds handlers based
   on configuration values.
4. **Inspected** — print the handler's body to see exactly how an op
   interpretation is implemented.
5. **Differentiated** — apply a differentiation handler TO ITSELF.
   Meta-circular.

The natural surface form:

```ys
from meta import handle, op
from ast    import handler, dispatch, ...

# A logging handler that wraps `invoke` ops.
def log_invoke = handler('invoke', |args, k|
  let _ = print('invoke called: ' + args.id)
  in k(args)   # delegate to the next handler down
)

# A handler that mocks `invoke` for testing.
def mock_invoke = handler('invoke', |args, _k|
  if args.id == 'cell_0' then {mass: 5.0} else {mass: 0.0}
)

# Run the same composite under different handler stacks.
def real_result = handle(Cell[mass0: 1.0].run(10),  [log_invoke])
def test_result = handle(Cell[mass0: 1.0].run(10),  [mock_invoke])
```

**The handler stack is itself a Value** — a list of `handler(...)`
records. Compose stacks: `concat(stack1, stack2)`. Save stacks to disk;
share between runs; transform programmatically.

---

## VI. Concrete applications (each unlocks new behavior)

1. **Drop-in stochastic ⇄ deterministic.** Wrap the same `cell.ys` in a
   `random(seed=42)` handler for one run, a `deterministic` handler for
   another. The composite's source is identical; only the wrapper
   changes. Today this needs separate Process implementations
   (`MinimalGillespie` vs an integrator).

2. **Reversible debugger.** A `journaled` handler records every `apply`
   call. The user can scroll back through simulation history. Set
   breakpoints at "first divide event." The body of the simulation
   doesn't know.

3. **Differentiable simulation.** A `forward_ad` handler turns every
   `Float` into a `Dual` carrying its gradient. Gradients of
   `cell.acetate(t=100)` w.r.t. `Grow.k` flow out. Calibrate parameters
   from observed trajectories. **The simulation logic is unchanged.**

4. **Capability-restricted sub-composites.** A cell that's allowed to
   read env state but not write it. The `write` op handler refuses for
   that cell. Sandboxed bigraphs.

5. **Cross-cluster transport swap.** A composite running with
   `invoke=parallel-pool` locally moves to `invoke=ray-cluster` for a
   bigger run with one config change. Same `cell.ys`.

6. **Compile-time / runtime split via staging.** A `staged` handler runs
   some ops at compile-time (specializing the program) and others at
   runtime. Futamura projections in `.ys`.

7. **A/B testing.** A `compare` handler runs two implementations of an op
   in parallel and reports divergence. Find which integrator handler
   gives different results in which regimes.

8. **Replay from trace.** A `replay(trace)` handler returns canned
   responses from a recorded trace. Re-run an old simulation
   deterministically without re-executing.

9. **Hot reload.** A `live(file)` handler re-reads `cell.ys` on every
   `invoke` — the simulation picks up edits without restarting.

10. **Symbolic execution.** A `symbolic` handler returns symbolic
    expressions instead of numeric values. Get a closed-form
    `mass(t) = …` from a numerical simulation.

11. **Time-warping for analysis.** A `dilated_time` handler runs certain
    cells at 10× simulation time for high-resolution analysis, others at
    normal rate.

12. **Multi-physics composition.** Different sub-composites get
    different `apply` handlers (one cells uses chemical-rate algebra,
    another uses Brownian-motion algebra). Compose via handler-per-region.

13. **Auditing for compliance.** Every `apply` is logged with provenance
    — required for reproducible-science workflows.

14. **Speculative parallelism.** A `speculate` handler runs both branches
    of an `if` in parallel, picks the one whose condition won.

15. **Probabilistic programming inside chrysalis.** `choose(distribution)`
    + a `weighted-sample` handler gives a PPL. Combine with the existing
    Gillespie SSA: variational inference on rate-constant posteriors.

The list keeps going — every row in §III combined with every layer of the
existing chrysalis stack is at least one new use case.

---

## VII. Implementation spec (incremental)

A path that doesn't break anything and stops being useful at every step:

### Slice 1 — Name the protocol as an effect.
Surface a `ProtocolHandler` trait that wraps today's `ProtocolRegistry`.
Programs still use the same syntax; the runtime now dispatches `invoke`
through `HandlerStack::find("invoke")`. **Refactor only — no behavior
change.**

### Slice 2 — Add the second effect: `dispatch` (method calls).
Pull `MethodRegistry::dispatch` into the same handler model. Two
operations, same stack. Demonstrates the abstraction is reusable.

### Slice 3 — `handle <expr> with <handlers>` surface form.
A new `.ys` keyword for a handler block. Lowers to a `Handle` AST node
that the evaluator interprets by pushing the handlers onto the stack for
`<expr>`. Most basic: handlers replace the default behavior for the
duration of `<expr>`'s evaluation. Then pop.

### Slice 4 — Handlers as homoiconic values.
A handler is `{_type: 'Handler', op: 'invoke', body: <Expr>}`. Build
them with `ast.handler(...)`. Hand-construct or load them from `.ys`.
`compile_value` and the rest of the homoiconic plumbing work unchanged.

### Slice 5 — One new effect: `log` / `observe`.
The cleanest one to add. `op log(message)` with handlers: stdout, file,
silent, captured-for-test. Lets every `.ys` add observability without
plumbing through arguments.

### Slice 6 — A second new effect: `choose`.
Implements non-determinism / probability. Two handlers ship:
deterministic-first and weighted-random. Now any `.ys` body can use
`choose([…])` and the wrapping context decides the interpretation.

### Slice 7 — Tier-2 effects (differentiation, reversibility).
The big ones. Each is its own slice. By this point the substrate is
proven; new effects just register more handlers.

---

## VIII. The unification — and what it costs

The payoff is large: ~7 implicit effect systems collapse into one
abstraction, with a uniform surface for adding more. Programs become
TRULY composable — combine any two and the handler-stack composition
rules say how their effects interleave.

The risks are real:

1. **Performance.** Effect-op dispatch is more indirection than direct
   calls. Today's tight `algebra::apply` is a function call; under
   effects it's a stack walk. Mitigations: monomorphize handlers at
   compile time (the staged-handler pattern); flatten the default handler
   into the engine's hot path.

2. **Error messages.** "No handler for `invoke`" beats today's
   "process Cell not registered" only if the diagnostics are good. Wire
   into #14.

3. **Type opacity.** Handlers obscure the static call graph — harder to
   reason about what a program does without running it. Mitigation:
   effect-typing (linear types track which effects an expression uses).

4. **Combinatorial explosion.** N effects × M handlers = NM combinations.
   Need a discipline for which compositions are *meaningful*; without
   it, the user gets paralyzed.

The chrysalis design already absorbs the first three (algebra is closed,
diagnostics is a planned task, schema is checked); the fourth is a
documentation discipline.

---

## IX. Where prism / chrysalis sit (re-update from
`docs/exploring-the-computational-unknown.md`)

| Axis | Today | With effect handlers |
|---|---|---|
| Effects model | implicit (7 systems) | algebraic + handlers (one system) |
| Manipulator | author + runtime | + the program itself (homoiconic handlers) |
| Compositionality | per-protocol | full Plotkin-Pretnar composition |
| Determinism | deterministic + Gillespie | + arbitrary via `choose` handler |
| Reversibility | algebra `diff` (data-level) | + handler-based time-travel debugging |
| Differentiation | none | + handler-based autodiff |
| Probability | rate-based (Gillespie) | + general PPL via `choose`+`observe` |
| Distribution | protocols | one effect among many |

The point isn't that protocols WERE wrong — they're right, just narrow.
**Naming them as one instance of a bigger pattern opens the rest of the
pattern.**

---

## X. The natural ordering with the homoiconic work

We just landed (#34): programs are data, runnable from data. **Handlers
need exactly the same substrate.** A handler is a function value (already
homoiconic via `Expr::from_value`); the handler stack is a list of
handler values (homoiconic); `compile_value` works on programs that
declare their handlers (homoiconic).

So the algebraic-effects work is a natural *next chapter* — it consumes
the homoiconic substrate we've just built, and it gives the substrate a
much larger surface to demonstrate on (every paradigm in §III becomes a
`.ys`-level demo).

The slicing in §VII puts the first concrete payoff (`handle … with …`)
on the table without any of the deeper changes. Each later slice
unlocks one paradigm.

---

## XI. Case study: Quantum bigraphs

The strongest test of an effect-handlers design is whether it can express
genuinely different paradigms, not just refactorings. **Quantum bigraphs
are that test.** Working the design backward from a quantum target
surfaces requirements the rest of the doc didn't quite cover — and the
surfaced requirements turn out to be exactly the right generalizations.

### What "quantum" needs

Quantum semantics differs from classical along four axes that other
paradigms (PPL, autodiff, reversibility) each touch only one of:

1. **Amplitudes, not probabilities** — state carries complex numbers
   whose squared moduli are probabilities. Interference (cancellation
   between amplitudes) is observable behavior.
2. **No-clone** — duplicating an unknown quantum state is forbidden
   (linearity).
3. **Reversible evolution** — every non-measurement step is a unitary
   (invertible operator).
4. **Measurement collapses** — observation forces the state to one
   eigenvalue, sampled by the amplitude distribution; measurement is
   the ONLY non-unitary primitive.

### Mapping each onto effect handlers

| Quantum requirement | Effect operation | Handler-side mechanism |
|---|---|---|
| Amplitudes | `op apply(gate, state) → state'` | Handler interprets `apply` over `Vec<Complex>`, not `Map<Path, Float>` |
| No-clone | `op copy(state) → (state, state)` | Quantum handler **refuses** `copy`; classical handler permits |
| Reversible | linear/unitary constraint on `apply` | Handler **type-checks** that the gate is unitary (preserves inner product) |
| Measurement | `op measure(qubit) → Bit` paired with state collapse | Handler returns one outcome PLUS rewrites state to the collapsed projection; classical handler is a no-op (state is already definite) |

So a quantum-bigraph run is **the same composite source, under a
different handler bundle**:

```ys
def quantum_handlers = [
  apply_handler('unitary'),         # rejects non-unitary gates
  copy_handler('no_clone'),          # refuses Cell duplication
  measure_handler('amplitude_sample'),
  state_rep('amplitude_vector')      # swaps state representation
]

handle BellPair[...].run(10) with quantum_handlers
```

### What this surfaces about the EFFECTS design (the triangulation gain)

Working backward from quantum reveals THREE things the §IV-VII spec
underspecifies. Adding them strengthens the design for ALL paradigms:

1. **Handlers can swap STATE REPRESENTATION, not just operation
   implementations.** The §VII slices assumed a fixed state type
   (`Value::Map`). Quantum needs `SparseVec<Bitstring, Complex>`. The
   generalization: an effect signature includes a *state-representation
   parameter*; handlers declare which representation they implement.

   Concrete addition: `effect Apply<S: StateRep> { apply(s: S, op: Op<S>) → S }`.
   Classical handlers bind `S = Map<Path, Float>`; quantum binds `S =
   AmplitudeVec`. This same generalization lets autodiff handlers bind
   `S = Map<Path, Dual<Float>>` (dual-number tape) — and we'd have
   wanted that anyway.

2. **Handlers can carry CATEGORICAL CONSTRAINTS that the runtime
   enforces.** Quantum requires unitarity; classical doesn't. Reversible
   computing requires bijective rules; classical doesn't. PPL requires
   measurable distributions; classical doesn't.

   Concrete addition: a handler declares its *invariants*
   (`category: 'symmetric_monoidal'`, `category: 'dagger_compact'`); the
   runtime can refuse to compose handlers from incompatible categories
   (you can't put a non-linear-in-amplitude classical-apply UNDER a
   quantum-measure). This is type-classes-for-effects.

3. **Composition rules for handlers play with composition rules for the
   underlying category.** Bigraphs form an s-category (symmetric
   monoidal); quantum is a dagger compact closed category. Where the
   two agree on tensor and morphism composition, the same bigraph term
   means the same thing. Where they disagree (e.g., bigraphs allow
   weakening/contraction but quantum doesn't), the quantum handler has
   to enforce the restriction.

   **The bigraph LINK GRAPH already expresses entanglement.** A shared
   link between two cells is structurally a *correlation* — in the
   classical handler it's a wire carrying values; in the quantum
   handler it's the tensor-product entanglement. **Same syntax, two
   semantics by handler.**

### What CHRYSALIS / PRISM gains from absorbing this case

Symmetrically: working forward from chrysalis surfaces things that help
quantum:

- **Bigraph normalization for circuit simplification.** Bigraphs have a
  canonical form (Milner's normalization). Applied to a quantum circuit,
  it gives gate-level simplifications "for free" (commuting gates,
  cancellation of adjacent inverses).
- **Composite encapsulation for module boundaries.** A quantum circuit
  built as `Composite(Composite(Composite(...)))` gets MODULE-level
  reasoning (a sub-circuit's effects are sealed behind its interface).
  This is exactly what Quipper / Q# struggle with: how to compose
  parameterized quantum routines.
- **Protocols for distributed quantum.** Today's `local:` / `rest:` /
  `stream:` could become quantum-distributed: cells on machine A
  entangled with cells on machine B via classical channels carrying
  measurement results + amplitudes-as-data. This is in the LITERATURE
  (distributed quantum computing) but lacks a clean engineering
  substrate.

### Concrete sketch — a 2-qubit Bell state in chrysalis

The shape (illustrative; needs the §VII slices + the §XI generalizations
landed first):

```ys
from meta import compile_value, handle
from ast    import mk_process, mk_composite, term, var, …
from quantum import h_gate, cnot_gate, measure, quantum_handlers

# Hadamard on qubit 0:
def hadamard = mk_process('Hadamard', [], {q: qubit_port}, {q: qubit_port},
  apply_unitary(h_gate(), var('q')))

# CNOT entangling qubit 0 (control) and qubit 1 (target):
def cnot = mk_process('CNOT', [], {c: qubit_port, t: qubit_port},
                     {c: qubit_port, t: qubit_port},
  apply_unitary(cnot_gate(), var('c'), var('t')))

# Bell state: H on q0, then CNOT(q0, q1). Wires `c` of CNOT to q0
# (the H output) — q0 and q1 are now entangled via the shared `c` link.
def bell = mk_composite('Bell', [], {},
  {q0: qubit_port, q1: qubit_port},
  parallel([
    keyed('q0', qubit_zero()),
    keyed('q1', qubit_zero()),
    keyed('h',   term('Hadamard', ports({q: var('q0')}, {q: var('q0')}))),
    keyed('cx',  term('CNOT',     ports({c: var('q0'), t: var('q1')},
                                        {c: var('q0'), t: var('q1')})))
  ]))

# Run under the quantum handler bundle. The same Bell composite under
# CLASSICAL handlers is a runtime error (gates aren't classical-apply).
handle compile_value(program([hadamard, cnot, bell])).run(1) with quantum_handlers
```

The composite's BODY is bigraph-syntax. The handler bundle decides
whether `apply` is classical addition or unitary multiplication, whether
`copy` is a passthrough or a refusal, whether `measure` exists at all.

**Bell-state correlations come out of the SHARED-LINK structure already
in the bigraph.** No new syntax — just a handler that interprets shared
links as tensor-product entanglement.

### Whether to design together or sequentially

**Together.** Three concrete reasons:

1. **The handler model gets fixed faster.** State-representation swap,
   categorical constraints, and composition rules are surfaces the
   classical-only effects design wouldn't have stress-tested. Quantum
   forces the questions.
2. **The implementation can share infrastructure.** The state-rep swap
   is what autodiff also needs; the categorical constraints are what
   reversibility also needs; the measurement-as-non-unitary is what PPL
   `observe` also is. Doing quantum well derisks 4 other paradigms.
3. **The demo is unique.** No production system runs both classical
   biological simulations AND quantum circuits through the same engine.
   That's the kind of unification chrysalis is designed for, and it
   demonstrates the unification at maximum amplitude.

### Slicing (extends §VII)

Adds after §VII slices 1-6 land:

- **Slice 7' — state-representation parameter on effects.** Generalize
  the effect signature to `effect Apply<S: StateRep>`. Lets handlers bind
  `S` to whatever they need.
- **Slice 8' — categorical constraints + handler typing.** Each handler
  declares its category; runtime refuses ill-typed composition.
- **Slice 9 — quantum handler bundle.** Implements amplitude-vec state,
  unitary apply, no-clone, amplitude-sample measure. Ship `from quantum
  import …`.
- **Slice 10 — Bell state demo.** A `.ys` constructing a 2-qubit Bell
  state via map literals + the quantum handler bundle. The substrate
  is by now familiar (it's just `compile_value` + `handle … with …`).

Each slice unlocks a real capability. By the end: a `.ys` file builds
a quantum circuit from data, runs it through the chrysalis engine, and
gets correct quantum statistics. **And the same engine still runs
classical mass-balanced cells.**

---

## XII. References (incl. quantum)

- Plotkin & Power, *Algebraic Operations and Generic Effects* (2003).
- Plotkin & Pretnar, *Handlers of Algebraic Effects* (2009).
- Bauer & Pretnar, *Programming with Algebraic Effects and Handlers*
  (Eff language).
- Leijen, *Type Directed Compilation of Row-Typed Algebraic Effects*
  (Koka, 2017).
- Lindley, McBride, McLaughlin, *Do Be Do Be Do* (Frank, 2017).
- Brachthäuser et al., *Effekt: Capability-Passing Style for Type- and
  Effect-Safe, Extensible Functional Programming* (2020).
- Pretnar, *An Introduction to Algebraic Effects and Handlers* (tutorial).

Quantum:
- Abramsky & Coecke, *A categorical semantics of quantum protocols*
  (2004); *Categorical Quantum Mechanics*.
- Coecke & Kissinger, *Picturing Quantum Processes* (2017) — the
  diagrammatic-bigraph-adjacent presentation.
- Selinger, *Towards a Quantum Programming Language* (2004).
- Quipper, Q#, Cirq, Silq — implementations.

prism-side:
- `docs/distributed-execution.md` — the current "protocol as pluggable
  backend" framing (an effect-handler discipline by another name).
- `docs/schema-algebra.md` — the closed algebra (an effect signature
  for state-update).
- `docs/exploring-the-computational-unknown.md` — broader survey.
- `docs/chrysalis-design.md` — the homoiconic substrate.
- `crates/chrysalis/ys/hand-built-cell.ys` — programs are data, proven.
