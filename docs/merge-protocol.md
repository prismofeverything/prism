# The merge protocol — composite unification across the bridge

The dual of `divide`. The structural primitive #36 (quantum bigraphs)
needs for entanglement, that #15/#30 needs for schema-as-state, and that
the distributed slices (#25-#29) need for "two subdomains decided to fuse."

## The foundation: bridges are SYMMETRIC update channels

**The principle**: a composite's bridge is symmetric. Both directions —
input and output — carry **updates** (whatever the schema's `apply`
accepts) along **schema-typed wires**. Not states. Not deltas
specifically (delta is a degenerate update). Just updates.

An update at a port is whatever `apply(T, current, update) → new_current`
accepts at that port's declared schema `T`. For `Float`, it's a numeric
delta (additive). For `overwrite[T]`, it's a replacement. For a `Map`
or `Tree`, it's a partial map with optional `_add` / `_remove`
sentinels. For a `Custom` type with a registered `apply` method,
it's whatever that method's signature says — including a **Bigraph**
type whose `apply` is a reaction (redex/reactum) being fired against
the receiver's internal state.

**Projection always carries schema.** When a wire projects an update
from one slot to another, the source's port schema travels with the
update; the receiver applies via that schema. This is the same rule
already used for output-port schema promotion (engine.rs:856-870 —
"the writer's port schema per slot so the promote-to-additive
survives"); we just need it on the input side too.

### The single rule (input ≡ output)

```
update at port :: T  ──[apply with schema T]──>  internal state
internal state  ──[diff/emit with schema T]──>  update at port :: T
```

Today's bridge is asymmetric: inputs `set` (replace internal state with
the parent's value), outputs forward deltas/updates intact. The
`set` is just a degenerate apply (it's `apply` with implicit
`overwrite[T]`). Making it explicit + extending to other schemas
unlocks the rest.

### What it unlocks

Once bridges accept arbitrary updates at typed ports, several
capabilities become natural:

1. **Send a reaction across a bridge.** An input port typed
   `redex :: Bigraph` (or whatever the surface name is) accepts a
   reaction value (a `{redex, reactum}` pair) as an update. The
   composite's `apply` for that type finds matches in its internal
   state and fires the reactum. No need for the OUTSIDE matcher to
   pierce the composite boundary — the reaction crosses as a typed
   value, the composite applies it inside.

2. **Send a merge command across a bridge.** A composite typed
   `tensor_with :: T` accepts another T-instance as an update; its
   `apply` for the union schema produces the merged internal state
   (via `tensor_by_schema`). The outer orchestrator sends two updates —
   one to alice saying "tensor with bob's state", one to the parent
   map saying `_remove: [bob]` — and the merge is done.

3. **Distribution-transparent.** The update + its schema travel as a
   Value. They serialize/deserialize the same whether the receiving
   composite is local or remote. The same protocol works over an
   Arrow pipe (stream:), over HTTP (rest:), over Ray (ray:). No
   special distributed code paths.

4. **One mechanism, many uses.** The same input bridge that carries
   "here's the new glucose level" can carry "here's a reaction to fire"
   can carry "merge with this state." The schema at the port tells the
   composite how to apply.

### Today's asymmetry — what to change

`composite.rs:200-274` `update()`:
- **Input** (line 204-208): `engine.state_mut().set_path(internal_path, val.clone())` — SETS the value. Should: call `apply_with_schema(port_schema, current, update)` so additive types accumulate, `_add`/`_remove` work, custom apply (Bigraph) fires.
- **Output** (line 240-273): already correct — taps `bridge_out_deltas` accumulated from inner writes, forwards intact.

Once the input side uses `apply`, both directions are update channels
running through the schema algebra.

### Implications for current tests / `.ys` (the audit)

The asymmetric `set` is functionally equivalent to `apply` with an
`Overwrite[T]` schema. So existing input ports that today rely on
"set each tick" continue to work if their schema is treated as
implicit-overwrite. The migration path:

- **Default: explicit `overwrite[T]`** at input ports where the
  current behavior is "parent owns this, cell observes." Annotate
  it. No behavioral change.
- **Annotate `Float` / `Delta`** at input ports where additive
  semantics SHOULD apply (e.g. cumulative deposits from the parent).
- **Add `Bigraph` / custom-apply types** to introduce reaction-over-
  bridge.

No `.ys` test should break if (a) input ports are reviewed for their
intended apply semantics and (b) the default for un-annotated inputs
stays `overwrite[T]` (the today's-behavior compatibility floor).

### Audit — which `.ys` files actually need attention

Surveyed every `composite` in the repo. The blast radius is small —
most composites use **params** (set at instantiation, set once) not
**inputs** (re-fed each tick via the bridge). Only the latter
interact with the input-bridge apply semantics.

| File / Composite | Inputs (`~{…}`) | Needs annotation? |
|---|---|---|
| `cell.ys` `Cell` | `mass :: Mass @ mass = mass0, glucose :: Float @ glucose = 0.0` | **YES.** `mass`'s `Mass` schema is `Delta` (extensive, additive). Today's "set" hides this; symmetric apply would add the parent's value as a delta each tick → exponential explosion. Annotate as `mass :: overwrite[Mass] @ mass = mass0` (and `glucose :: overwrite[Float]`). Behavioral equivalence preserved. |
| `environment.ys` `Environment` | none (params only) | No. |
| `mapk.ys` `Mapk` | `cell : map[any]` | Re-type as `cell :: overwrite[map[any]]` (parent passes the whole cell shape each tick). |
| `nuclear-shuttle.ys` `Cytoplasm` / `Nucleus` / `Cell` | none (params only) | No. |
| `grow-divide-unbounded.ys` `Cell` / `Environment` | none (params only) | No. |
| `grow-divide-glucose.ys` | params only | No. |
| `gillespie.ys` `Gillespie` | none (params only) | No. |
| `integrator-comparison.ys` | params only | No. |
| `bump.ys` `Leaf` | none (`v` is a param) | No. |
| `homoiconic-demo.ys` + variants | none / no input bridges | No. |
| `quantum-*.ys` (all quantum demos) | none (params + output-only) | No. |
| `hand-built-*.ys` | none | No. |
| `report-section.ys`, `eval-demo.ys`, `effects-handle-demo.ys` | none | No. |
| `quantum-cross-cnot.ys` (probe) | `state :: map[float] @ state = state0` | **YES.** Same pattern as cell.ys mass — annotate `overwrite[map[float]]`. |

**Two files need annotation**: `cell.ys` (the canonical Cell — mass +
glucose) and `mapk.ys` (the `cell` input). Plus the cross-cnot probe
we're actively working on. Everything else is unaffected because the
prevailing pattern is `composite Foo[params] ~{} ->{outputs}` —
params set at instantiation, no per-tick input bridge.

No FUNDAMENTAL problems. The migration is a two-line annotation pair
in `cell.ys` plus a one-line annotation in `mapk.ys`. Existing
mass-conservation / division / glucose-pool tests preserve their
semantics exactly under explicit `overwrite[T]`.

### Implications for streaming composites

The stream protocol today carries: input snapshots (set-style)
parent→child, output delta-frames child→parent. Under symmetric
updates, the protocol becomes UNIFORM: both directions carry
updates + their port schema. The stream serializer doesn't care
which direction — it's just `Update + Schema` Values.

This actually SIMPLIFIES `crates/prism-bigraph/src/protocols/stream.rs`
+ the Arrow codec — fewer special cases. The codec already
serializes any Value; adding the port schema as part of the message
is a small extension.

For distribution (`rest:`, `ray:`), same simplification. The merge
protocol then works distributed-transparently — send an "I'm
tensoring you with bob's state" update to a remote alice, alice
applies it inside its own engine, emits the post-tensor state as
an output update; orchestrator sees the result.

### Implications for the merge aspirations

The merge protocol falls out:

1. **Each composite declares an input port** for the merge update —
   e.g. `merge_with :: Composite` whose `apply` schema is
   `tensor_by_schema` for that composite's state type.
2. **The orchestrator sends two updates per merge**:
   (a) to alice: `merge_with: bob.snapshot` — alice's internal
   `apply` produces the tensored state;
   (b) to the parent's `systems` map: `{_remove: [bob], _add: ...}`
   if the structure changes (or just keep alice as the survivor).
3. **Both updates pass through the same bridge mechanism.** No
   special "merge protocol" wire — it's just typed updates.

Reactions, merges, divides, deposits — all the same shape. That's
the point.

## Reactions are bigraphs (the second principle)

Reactions are first-class bigraph-shaped values — a redex bigraph + a
reactum bigraph + matcher metadata. Combined with symmetric-update
bridges, this means a reaction can CROSS A BRIDGE as a typed update.
The receiving composite's schema for that input port (e.g. `:: Bigraph`)
has an `apply` that interprets the update as "fire this reaction
against my internal state."

So a cross-composite reaction doesn't pierce composite boundaries
during matching. It composes as:

```
1. Outer reactor identifies: this redex spans composites A and B.
2. Outer reactor emits a MERGE update to one composite (A receives
   "tensor with B's snapshot" via its merge_with input).
3. Engine processes the merge — A's internal state now contains B's.
4. Outer reactor sends the REACTION update to A's redex input.
5. A's apply for `:: Bigraph` fires the reactum inside.
```

Reactions, merges, divides, deposits — all are typed updates over
symmetric bridges. Distribution-transparent: the same protocol runs
when A is local OR a stream child OR a remote node.

## What's already there

Today's prism has the pieces; what's missing is the symmetric apply
on input + a few schema-algebra operations.

| Piece | Location | Status |
|---|---|---|
| Reactions as first-class bigraphs | `prism_schema::reaction` (Pattern, ReactionRule, find_matches, fire_rule_at) | ✅ |
| BRS / Gillespie / deterministic firing | `prism_bigraph::BigraphicalReactiveSystem` | ✅ |
| Schema-driven divide (the dual we're inverting) | `prism_schema::divide_by_schema` | ✅ |
| `_add` / `_remove` reconciler primitives | `prism_schema::reconcile` | ✅ |
| Stream child lifecycle (auto-spawn on add) | `prism_bigraph::engine` + `StreamingCell` pattern | ✅ |
| Bridge OUTPUT — update via tap + forward | `composite.rs:240-273` (`bridge_out_deltas`) | ✅ already update-based |
| Bridge INPUT — currently SET (overwrite-implicit) | `composite.rs:204-208` | ⚠ needs symmetric apply |
| `tensor_by_schema` (dual of `divide_by_schema`) | — | ⏳ |
| Reaction-as-update schema (`:: Bigraph` apply) | — | ⏳ |

## Slices (in dependency order, refreshed under the symmetric-bridge
principle)

1. **Symmetric input apply** (foundational). Change `composite.rs`
   input bridge from `set_path(val)` to `apply_with_schema(port_schema,
   current, val)`. Default port semantics: if no annotation, treat as
   `overwrite[T]` (today's behavior). Annotate `cell.ys` and `mapk.ys`
   inputs as `overwrite[T]` per the audit table. Verify mass /
   division / glucose tests pass. *Engine + 2 `.ys` annotations.*

2. **`tensor_by_schema`** — schema-driven dual of `divide_by_schema`.
   For each kind: how do you unify two instances? `Float` sums (or
   averages, with extensivity), `Delta` concats logs, `Map[T]` merges
   per-key recursively, `map[float]` (quantum) cross-products +
   multiplies. *Schema-algebra work; mirrors divide.* (Task #39.)

3. **Snapshot-publish in `QuantumSystem`** — small `Publish` process
   so the sub-composite's `state` propagates each tick (or on
   request). Then the orchestrator can read `cells.alice.state`. With
   symmetric apply, this becomes trivial: the publish is an
   `overwrite[map[float]]` update each tick. (Task #37.)

4. **Promote `quantum-lifecycle.ys` to real `map[QuantumSystem]`** —
   replace raw data maps with typed sub-composites. Splitter/Merger
   read state via bridge (slice 3), call `tensor_by_schema` (slice 2),
   emit `_remove` + `_add`. (Task #38.)

5. **`Bigraph` input port type + apply** — schema kind that accepts a
   `{redex, reactum}` value as an update; its `apply` finds matches
   internally and fires the reactum. *Schema-algebra extension.*
   Together with symmetric apply, this is reaction-send-across-bridge.

6. **Cross-composite reactor** — orchestrator that walks reactions,
   identifies cross-composite redexes, emits the merge update to one
   composite + the reaction update to its `Bigraph` input. *Reactor
   extension.*

7. **Sibling-addressing in reaction syntax** — small parser slice so
   `alice~{state: a} | bob~{state: b}` can express the cross-composite
   pattern in source. (Task #40.)

8. **Distributed merge** — slices 1-7 working when source composites
   are stream children on remote machines. Test that "merge produces
   identical result whether A and B are local or remote." Pairs with
   #25-#29. *Distributed test.*

## Provenance — first-probe and the debugging trap (2026-05-27)

The first cross-cnot probe (`crates/chrysalis/ys/quantum-cross-cnot.ys`)
surfaced what LOOKED like three engine blockers: composite outputs
leaking schema descriptors; `any`-typed slots accumulating despite
`overwrite[any]`; no sibling-addressing syntax. After actually
reading the engine (`composite.rs:200-274`, `engine.rs:854-980`,
`schema.rs:1076-1110`), the first two turned out to be **a debugging
trap, not an engine bug**: the `grep -v "^   "` filter I was using
to strip compiler warnings *also matched indented JSON lines*,
making multi-line nested map outputs look like empty `{}`. Removing
the filter (or restructuring the grep) revealed the bridge propagates
correctly.

What's actually true was already in the codebase:
- Bridges emit DELTAS, not snapshots (`composite.rs:240-273`,
  `is_zero_delta` at 288-298).
- `Schema::Any` apply IS additive for floats (`schema.rs:1076-1083`)
  — use real types (`Float`, `map[float]`, `overwrite[T]`) for
  explicit semantics. (Memory `feedback_real_types_not_any`.)
- Sibling propagation works (verified with a trivial Float-typed
  test).

The third — sibling-addressing in REACTION REDEX SYNTAX — is real
but small (parser slice; tracked as task #40).

Lessons (saved as memories): `feedback_no_grep_filter`,
`feedback_read_dont_guess`, `feedback_real_types_not_any`.

### Landed (2026-05-28)

Three slices in: the foundational symmetric-apply at the input bridge
(#41), the snapshot-publish convention (#37 — `Publish` process in
`QuantumSystem`), and the lifecycle's promotion to real
`map[QuantumSystem]` sub-composites (#38).

Symmetric apply: `composite.rs` input bridge routes through
`algebra::apply_with(Overwrite[port_schema], current, val)`. Today's
"set" behavior preserved (Overwrite wraps everything → replace).
Future custom-apply port types (e.g. a Bigraph schema) can bypass the
wrap once added.

Unified merge: `apply_map_with(cur, upd, schema_for)` collapses the
four arms (Tree / Map / Any) that used to duplicate the `_remove` +
`_add` + per-key apply dance. `_add` values are realized through the
per-key element schema — the schema algebra's `realize` is now the
deserialization step every map-`_add` flows through.

Snapshot publish: the `Publish` process inside `QuantumSystem`
re-emits state every tick with `overwrite[map[float]]` semantics, so
the bridge taps it and the parent's slot tracks the inner state.

Composite `_add` over the bridge: a process body's `_add` value is a
**Term expression** (`QuantumSystem[state0: …] ~{…} ->{…}`) — the
sender constructs a fully-formed spec; the receiver's realize passes
through (`_type: composite` is the spec sentinel). This keeps the
round-trip law `r(s(v)) ≡ v` intact for arbitrary values, with the
spec form as the natural fixed-point of `r ∘ s`.

### Bug found + fixed (2026-05-30) — `overwrite` output ports cross the bridge

A composite that republishes a snapshot each tick (the `Publish`
pattern) was ACCUMULATING at the parent: a *persisting* system's
`map[float]` state DOUBLED every tick (`0.7071 → 1.4142 → 2.8284 → …`).
Root cause: `Schema::node_data_branches` — the self-exported data face
that a link node's `apply` traverses (`apply_update` routes every
`Link`/`ProcessLink`/`StepLink`/`CompositeLink` through `Tree { branches:
node_data_branches() }`) — filtered to **scalar** ports only
(`Float`/`Delta`/`Integer`). So a `map[float]` (or even
`overwrite[map[float]]`) output port fell through to the additive `Any`
passthrough, which sums numeric leaves — hence the doubling.

Fix (both in `prism-schema/src/schema.rs`): (a) `node_data_branches`
now also includes ports that declare an explicit `Overwrite` modifier,
so a snapshot port REPLACES; (b) `schema_at_path` types a link node's
self-slot (`%.port`) by its declared output face, so a *direct* leaf
apply honors it too. This is the symmetric-apply principle (slice 1)
made to hold for the OUTPUT face, uniformly local AND streamed — a
composite's declared `overwrite[T]` output port now replaces across the
bridge. Requires the surface annotation (`quantum-system.ys` output port
is now `overwrite[map[float]]`). Regression: the stability assertions in
`tests/quantum_auto_split.rs` + a `schema_at_path` unit test. The local
lifecycle hid this — it splits/merges every tick, so nothing persisted
to accumulate.

### Landed (2026-05-30) — split/merge over the `stream:` protocol

The lifecycle now runs with each `QuantumSystem` as a real `stream:`
child — `crates/chrysalis/ys/quantum-lifecycle-stream.ys`
(`map[StreamingQuantumSystem]`, the sole change from the local
`quantum-lifecycle.ys` being the `stream<QuantumSystem, …>` protocol
alias). Regression: `crates/chrysalis/tests/quantum_lifecycle_stream.rs`
(4 green). Confirmed real subprocesses via a `CHRYSALIS_BIN=/bin/false`
differential (child spawn fails with an Arrow IPC error) + an `strace`
`serve-process` exec count — not a silent local fallback.

This validates slice 8's premise (the merge protocol distributes) for
the **output direction**: the structural `_remove`/`_add` ride the
already-update-based output bridge and cross the wire unchanged — the
same path `grow_divide_stream.rs` proved for cell division. An `_add`
of a freshly-authored `StreamingQuantumSystem[state0: …]` Term spawns a
stream child (the spec lowers to `address: stream:…` + config + bridge;
discovery instantiates it).

**What it does NOT yet exercise**: the children are not load-bearing for
the *decision*. The parent `Lifecycle` reads each system's seeded
`state` and computes factorize/tensor itself; the children spawn and
republish but don't feed the lifecycle. The input wire is still
snapshot-flavoured (see slice 1's note below) — sending a *merge command
or reaction DOWN* to a remote child (slices 5/6) needs the symmetric
input wire, which is the remaining unification.

### Algebra question — idempotence at the fixed point

Open question for the schema algebra: should there be a law for
`r ∘ s ∘ r ∘ s ≡ r ∘ s` (encode/realize converge to a canonical form)?
Today's law 8 is the stricter `r ∘ s ≡ id` (round-trip). The
idempotence-at-fixed-point variant would accommodate types whose
"canonical form" is multi-step normalization — e.g. a composite whose
serialized state can be either raw inner-state OR a runnable spec,
both valid, with the spec as the fixed-point.

Not needed for the current implementation (we keep `r ∘ s ≡ id`
strict and require the sender to ship the canonical form). But the
idempotence variant is worth considering if we ever want
serialize/realize to NORMALIZE (e.g. always emit the spec) for types
where the canonical form is unambiguous.

### What the engine reading confirmed

After the grep-trap was identified, reading `composite.rs:200-274`,
`engine.rs:854-980`, and `schema.rs:1076-1110` confirmed:

- **Bridge OUTPUT is already update-based.** `bridge_out_deltas`
  accumulates inner writes as the composite's update each tick.
  `is_zero_delta` (`composite.rs:288`) filters silent updates.
- **Bridge INPUT is SET, not apply.** `composite.rs:204-208`:
  `engine.state_mut().set_path(internal_path, val.clone())`. This is
  the asymmetry to fix (slice 1 above).
- **`Schema::Any` apply is additive for floats** (`schema.rs:1076-1083`).
  That's why `state :: any` accumulated. Real types fix it.
- **Sibling propagation works.** A trivial Float-typed test confirms
  it. The user's mental model was correct all along.

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
