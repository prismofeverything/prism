# The merge protocol — composite unification across the bridge

The dual of `divide`. The structural primitive #36 (quantum bigraphs)
needs for entanglement, that #15/#30 needs for schema-as-state, and that
the distributed slices (#25-#29) need for "two subdomains decided to fuse."

## Principle

The composite boundary is *non-negotiable* — for distribution reasons,
the outer engine never touches a sub-composite's internals. All I/O goes
through the bridge. A composite may be local; it may be a stream child
holding state on another machine; the OUTSIDE shouldn't care. (Memory:
`feedback_composite_boundary`.)

So `merge` must be implementable using ONLY bridge I/O:

1. **Drain.** Each source composite already exposes its state on the
   bridge (the schema's `@`-bound output ports). One tick of normal
   I/O is sufficient — the bridge tap already gives the outer engine a
   snapshot of each source's external face.
2. **Unify.** A merge function combines the snapshots — for quantum,
   `meta::tensor`; for the general case, a schema-driven combine that
   sums extensive quantities, takes union of structure, reconciles
   delta logs, etc. (The dual operations of `divide_by_schema` —
   `tensor_by_schema` is the planned name.)
3. **Allocate.** A `_add` intent on the parent's `systems` map (or
   wherever the composites lived) installs the new merged composite,
   typed by the map's element type. If the type is `stream<T, path: …>`,
   a new stream child spawns automatically (same mechanism as a
   divider's daughters today).
4. **Tear down.** A `_remove` intent on the same map drops the source
   composites; for stream children this terminates the child processes
   cleanly.

`_remove` + `_add` in one reconciled batch IS the merge — the lifecycle
demo (`quantum-lifecycle.ys`) proves the pattern for raw data maps.
The remaining work makes it work for real (and streaming) composites.

## Reactions are bigraphs that cross bridges

A second insight that simplifies cross-composite reactions: reactions
themselves are first-class bigraph-shaped values (a redex bigraph + a
reactum bigraph + matcher metadata). They can be SENT over a bridge,
into a sub-composite, just like state.

So a "cross-composite reaction" doesn't have to pierce composite
boundaries during matching. It can compose as:

```
1. Outer reactor identifies: this redex spans composites A and B.
2. Outer reactor emits a merge intent for A and B (the protocol above).
3. Engine processes the merge — A and B become one composite C.
4. The reaction's redex now lives entirely inside C; the reaction is
   sent (across the bridge) into C and applied there.
5. The result is the reactum's effect on the merged state.
```

Step 4 — sending a reaction across a bridge — already has all the
infrastructure: reactions ARE values, Documents and reactions can be
serialized (`EntityDef::to_value`), the stream protocol already moves
arbitrary Values. We just need to extend the bridge to carry "fire this
reaction" as one of the command-types it accepts (alongside state
deltas and time advances).

This is what makes it *transparently distributed*: the merge moves
state across the network if A and B are on different machines; the
reaction-send moves the rule across the network. The matcher and
reactor never need to know they're crossing nodes.

## What's already there

Today's prism has:

| Piece | Location |
|---|---|
| Reactions as first-class bigraphs | `prism_schema::reaction` (Pattern, ReactionRule, find_matches, fire_rule_at) |
| BRS / Gillespie / deterministic firing | `prism_bigraph::BigraphicalReactiveSystem` |
| Schema-driven divide (the dual we're inverting) | `prism_schema::divide_by_schema` |
| `_add` / `_remove` reconciler primitives | `prism_schema::reconcile` |
| Stream child lifecycle (auto-spawn on add) | `prism_bigraph::engine` via `StreamingCell` pattern in `environment.ys` |
| Bridge state I/O | composite ports + `Composite::from_config` |
| Composite-as-value serialization | `chrysalis::ast::EntityDef::to_value` + `compile_value` (#32) |

What's NOT there yet:

| Gap | Implication |
|---|---|
| `tensor_by_schema` (the dual of `divide_by_schema`) | No generic "given two same-typed values, unify them" operation |
| `_add` with sub-composite values (not just data) | Today `_add: {a: <data>}` adds a leaf; `_add: {a: T[…]}` doesn't instantiate T |
| Reaction-send command on the bridge protocol | No way today to ship a rule into a sub-composite |
| Cross-composite reactor: detect-spans + auto-merge + react | The orchestrator that ties it all together |

## Slices

Roughly in dependency order:

1. **`tensor_by_schema`** — the schema-driven dual of `divide_by_schema`.
   For each kind: how do you unify two instances? `Float` sums (or
   averages, with extensivity), `Delta` concats logs, `Map` merges via
   reconcile, `Link/Composite` requires schema reconciliation.
   *Schema-algebra work, mirrors divide.*

2. **`_add` with composite values** — `_add: {key: T[args]}` instantiates
   T with the given args, including for `map[T]` slots whose T is a
   composite or `stream<…>`. Today's `_add` adds raw values; this slice
   makes it spawn real composites. *Engine work.*

3. **The local merge protocol** — a step or process pattern that takes
   two slot keys on a `map[T]` and emits `{_remove: [a, b], _add: {ab: T[merged_state]}}`.
   For quantum: merged_state = tensor; for general: `tensor_by_schema`.
   This is `quantum-lifecycle.ys`'s `Lifecycle` process, refactored
   into a reusable primitive. *Chrysalis-level.*

4. **Reaction-send over the bridge** — extend the stream protocol's
   command vocabulary: today it carries state deltas; tomorrow it can
   carry "fire this reaction at this position." The bridge stays opaque
   (the receiving composite decides whether the reaction matches); the
   sender doesn't need to know the inner schema. *Protocol-runtime work.*

5. **Cross-composite reactor** — the orchestrator. Walks reactions
   each tick; for each, finds matches that span sibling composites;
   emits the merge intent; sends the reaction into the merged composite
   on the next tick. *Reactor extension.*

6. **Distributed merge** — slices 1-5 working when the source
   composites are stream children on remote machines (or other backend
   protocols). Tests that "merge produces identical result whether A
   and B are local or remote." Pairs with #25-#29. *Distributed test.*

## First-probe findings (2026-05-27)

Wrote `crates/chrysalis/ys/quantum-cross-cnot.ys` as the simplest
failing test for cross-composite reactions: two sibling
`QuantumSystem` composites in a `systems :: map[QuantumSystem]` slot.
Before the reactor work can even start, three lower-level blockers
surfaced:

1. **Composite output leaks internals.** With two `QuantumSystem`
   instances in a map, the parent's `systems` output contains nested
   `state.state.…` plus schema descriptors (`_type: "Any"`) plus the
   instances' inputs/outputs port specs. The bridge tap is exposing
   the entire schema tree, not just the slot value. Until the output
   is just the slot value, the matcher / merger can't easily reason
   about what's there.
   *This is the "_process leak" pattern observed in
   `project_composite_execution_gap`, surfacing for `any`-typed
   composite slots.*

2. **`any`-typed slots accumulate (don't overwrite).** A trivial
   `Hold ~{state :: any} ->{state :: overwrite[any]}` process that
   re-emits `{state: state}` each tick produces `{0: 2.8284, 1: 2.8284}`
   at `--time 2` (4× the initial `0.7071` — the value is being SUMMED
   per tick, not overwritten). `overwrite[any]` should be the override
   sentinel, but `any`'s default semantics under `apply` are clearly
   not "replace" — looks like it's falling through to the additive
   default. Need to audit how `Schema::Any` interacts with `overwrite`
   in the algebra (specifically `apply.rs` / `reconcile.rs`).

3. **No sibling-addressing syntax.** Today `~{state: %.state}` (self) and
   `~{state: ^.state}` (parent) work. There's no `siblings.alice.state`
   or `^.alice.state` to reach across into a sibling. A reaction that
   wants to bind `alice` and `bob` together needs SOME way to refer to
   them — either explicit sibling paths in the redex, or a
   pattern-matcher convention ("redex matches a `systems` map; binding
   names become the matched siblings' keys").

These three are all "obstacle 1" from the table at the top of this
doc, surfacing concretely. Fix order is probably 1 → 2 → 3:

- **(1) Composite output leak.** Trace where in `engine.rs` the
  bridge tap snapshots the inner state, see why the schema/port-spec
  is included in the output Value. Probably wants a "project to slot
  value only" pass on bridge output.
- **(2) `Any` + `overwrite`.** Audit `apply_with_schema(Schema::Any,
  base, overwrite-update)`. The `overwrite` modifier should force
  *replace* regardless of inner schema. May need to short-circuit
  before dispatching by schema kind.
- **(3) Sibling addressing.** Probably a tiny parser slice — allow
  `<ident>.<path>` in port wiring contexts where `<ident>` resolves
  against the enclosing composite's child entries.

Once (1) is clean, the cross-cnot probe will at least produce
inspectable state to write reaction predicates over.

## Quantum as the test case

Quantum is the cleanest first test because the merge operation is
crystal-clear (tensor product). For #36's Q4 to be done:

- Slice 1 (`tensor_by_schema`): for `state :: any`-typed slots, the
  unify operation calls `meta::tensor` on the amplitude maps. (Special-
  case for now; the general case is the closed schema algebra.)
- Slice 2 (`_add` with composites): an `_add: {ab: QuantumSystem[state: tensor(a,b)]}`
  spawns a new QuantumSystem with the right initial state.
- Slice 3 (merge primitive): a generic `Merge` step the user
  parameterizes over (which slot, which merger function).
- Slice 4-6: stream variants.

Quantum-lifecycle.ys today is a working slice-3 prototype for raw
data maps. Promoting it to real composites is the slice-1-2 work.

## Provenance / inspiration

The shape (composites stay sealed; merge via bridge protocol; reactions
travel) generalizes the divide↔unify duality past quantum into the
broader bigraph-rewriting world. Milner's bigraph paper has divide
implicitly (a node splits); the unify direction is less classical but
fits the algebra. The distributed-transparency goal is from the
distributed-execution plan (`docs/distributed-execution.md`).

Two cells fusing (membrane biology), two RDFs reconciling (knowledge
graphs), two LLMs sharing context (multi-agent systems) — all are
instances of the same merge protocol. Quantum just makes the math
sharp.
