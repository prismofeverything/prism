# Upstream Alignment Plan

Prism was forked from `process-bigraph` + `bigraph-schema` on
**2026-03-28** (~2 months ago, at upstream `process-bigraph` v1.0.6
and `bigraph-schema` ~v1.2.x). The typed-schema-node foundation,
`plum.dispatch` multi-method machinery, and the basic protocol
abstraction (parallel / rest / socket) were **already present
upstream at fork time** — prism made the Rust-idiomatic choice to
encode schemas as enum variants with `match`-based dispatch instead
of porting the dataclass + multi-dispatch pattern.

Since the fork, upstream has added substantially (rough scope of new
work, both repos combined): ~9,000 LoC in bigraph-schema, ~6,500 LoC
in process-bigraph. The biggest pieces are the Ray protocol (with
sharded batching), Pool protocol, cluster protocols (EC2/SSM),
`bundle`/`nextflow`/`plumbing` modules, the REST server pulled in,
BRS moved into process-bigraph, and major reconcile/realize/serialize
method rewrites in bigraph-schema.

**The decision for prism is *not* a foundational rewrite.** prism's
enum-based `Schema` is a Rust-idiom equivalent of the upstream
dataclasses; the runtime gap is in (a) the missing protocol layer,
(b) the missing typed `Link` variants (`StepLink` / `ProcessLink` /
`CompositeLink` / `Bridge` / `Interface`), (c) the missing Composite
lifecycle hooks (`_flush_protocol_runtimes`, `Defer`/`invoke`/`get`),
(d) `SharedProcess`, (e) any specific upstream method-dispatch
improvements worth pulling in.

This document maps each missing piece onto prism's existing
architecture and lays out a phased path. **Chrysalis sits on top of
the aligned foundation**, so this work unblocks the homoiconic,
protocol-agnostic language design we want.

## Baseline at fork (2026-03-28, upstream v1.0.6)

Already present upstream when prism was created — prism inherited
these as concepts but encoded them as Rust enums + structs:

- Typed schema nodes (`Node`, `Link`, `StepLink`, `ProcessLink`, …)
- `plum.dispatch` method machinery (`default`, `realize`, `apply`, …)
- Basic protocol abstraction (`protocol_registry`, `parallel` / `rest`
  / `socket` protocols)
- `types/process.py` defining typed Link variants

So this part isn't a "missed update" — it's a deliberate Rust-idiom
choice. The work to "align" here is choosing whether to extend
prism's enum dispatch with new variants (Bridge, Interface,
CompositeLink, StepLink, ProcessLink) or to migrate dispatch to a
trait-object model. Either preserves the enum's perf in the common
case; we recommend extending the enum.

## Deltas since fork

### bigraph-schema (current: v1.3.x)

| Feature | Status in prism | Gap |
|---|---|---|
| Typed schema nodes | Encoded as Rust enums / structs | Add missing variants |
| Multi-method dispatch | Encoded as match arms per method | Extend for new variants |
| `Wires` as typed schema node | Partial (Vec<Key>) | Medium |
| `Quantity` / `Number._units` / wire-level unit conversion | No | Medium (post-tier-1) |
| Schema-agnostic JSON codec | No | Medium |
| `reconcile.py` (718 LoC, NEW since fork) | No | High |
| `realize.py` major rewrite (689 LoC delta) | Partial | Medium |
| `serialize.py` major rewrite (761 LoC delta) | Partial | Medium |
| `walk.py` (154 LoC NEW) | No | Low |
| `patch.py` (124 LoC NEW) | No | Low |
| `is_empty.py` (163 LoC NEW) | No | Low |
| `assembly.py` reactions (Pattern + ReactionRule + find_matches) | **Yes** — prism mirrors this | None |

### process-bigraph (current: v1.4.12)

| Feature | Status | Gap | Origin |
|---|---|---|---|
| `core` / `allocate_core()` unified registries | Scattered in prism (ProcessRegistry + TypeRegistry separate) | High | already in upstream at fork |
| `StepLink` / `ProcessLink` / `CompositeLink` typed variants | Prism has generic `Schema::Link` only | Medium (extend enum) | already in upstream at fork |
| `Bridge` / `Interface` as schema-typed nodes | Internal Rust structs in prism-bigraph | Medium (promote to schema types) | already in upstream at fork |
| `SharedProcess` / `SharedProcessRef` | No | Medium | already in upstream at fork |
| **Protocol abstraction** (`ProtocolRegistry`, address `{protocol, data}`) | No (single ProcessRegistry, local only) | **High** | parallel/rest/socket at fork; pool/ray/cluster after |
| `parallel` protocol (multiprocessing) | No | Medium | at fork |
| `rest` protocol (HTTP client) | No | Medium | at fork |
| `socket` protocol | No | Low | at fork |
| `pool` protocol (309 LoC) | No | Medium | **post-fork** |
| `ray` protocol with sharded batching (744 LoC) | No | Medium | **post-fork** |
| `session` protocol (130 LoC) | No | Low | **post-fork** |
| `clusters/ec2_ssm.py` (861 LoC) | No | Low | **post-fork** |
| `Composite._flush_protocol_runtimes` lifecycle hook | No | **High** (required for batching) | **post-fork** |
| `invoke()` returns `Defer`, `Defer.get()` collects later | No (direct `update()` only) | **High** | **post-fork** |
| `bundle.py` (484 LoC NEW) | No | Medium | **post-fork** |
| `nextflow.py` (438 LoC NEW) | No | Low (pipeline integration) | **post-fork** |
| `plumbing.py` (141 LoC NEW) | No | Medium | **post-fork** |
| `BigraphicalReactiveSystem` as a Process | **Yes — prism leads** (ControlStatus, structural-diff output) | None | upstream moved BRS here May 2026 |
| `composite.py` extensions (+2049 LoC) | Partial — prism has Composite but not all hooks | High | **post-fork** |
| `emitter.py` extensions (+437 LoC) | Partial | Medium | **post-fork** |
| `server/` REST server | No | Low | **post-fork** (we have the client side as `rest` protocol anyway) |

### rest-process (external, current: v1.1.0)

REST server exposing process classes as HTTP endpoints. Wire protocol:
- `POST /process/{class}/initialize` → returns `process_id`
- `GET /process/{class}/inputs/{process_id}` → port schema
- `GET /process/{class}/outputs/{process_id}` → port schema
- `POST /process/{class}/update/{process_id}` → run update, returns result
- `POST /process/{class}/end/{process_id}` → cleanup
- `GET /list-types`, `GET /list-processes`, `GET /process/{class}/config-schema` for discovery

Prism needs a Rust HTTP client wrapping this (the `rest` protocol). Prism
also could host its own Rust REST server compatible with the same wire
protocol, so prism-hosted processes can be consumed by anything speaking
the rest-process API.

## The interface-as-type insight, made concrete

The reason this alignment matters for chrysalis: upstream's `CompositeLink`
is literally a typed schema node carrying:

```python
@dataclass(kw_only=True)
class CompositeLink(ProcessLink):
    schema: Schema             # schema for the composite's internal state
    state: Node                # the inner state structure
    interface: Interface       # explicit inputs/outputs schema boundary
    bridge: Bridge             # wiring from internal state to interface
```

This is the "interface as type" idea you described, expressed as a
plain schema type. Anything declared as `CompositeLink` *is* a
composite, regardless of where its body runs. The bridge couples
internal state to external interface; the protocol decides where the
body lives.

A REST-protocol composite has the same `CompositeLink` shape — just
its `address.protocol == "rest"`. Pattern matching, bridging, lifecycle —
all uniform. That's the property chrysalis depends on for being
protocol-agnostic.

## Phased plan

The work is ~weeks of effort. Five phases. Each phase ends at a
checkpoint with passing tests.

### Phase 1 — Schema additions (no foundational rewrite)

Goal: extend prism's existing enum-based Schema with the typed Link
variants and the Bridge / Interface schema nodes upstream uses.

1. **New Schema variants** in `prism-schema::Schema`:
   - `StepLink { inputs, outputs, priority }`
   - `ProcessLink { inputs, outputs, interval }` (replaces / specializes
     existing `Link { temporal: Some(true), .. }`)
   - `CompositeLink { schema, state, interface, bridge, interval }`
   - `Bridge { inputs: Wires, outputs: Wires }` as a schema type
   - `Interface { inputs: Schema, outputs: Schema }` as a schema type
   - `SharedProcess` / `SharedProcessRef`
2. **`allocate_core()` + `Core`**: a single handle holding
   `link_registry` (process classes), `protocol_registry`,
   `type_registry`, `method_registry`. Replaces prism's scattered Arcs.
3. **Method extensions** in prism's existing match-based dispatch
   for each new variant: default / realize / apply / serialize / divide.
4. **Port `json_codec`**: schema-agnostic state-tree JSON
   serialization, needed for protocol boundaries.

This is **not a rewrite of prism's Schema enum dispatch model** —
just adding the missing variants and methods. Existing tests
unchanged.

**Checkpoint:** all prism-schema + prism-bigraph compat tests still
pass. New variants exist. `allocate_core()` returns the unified
registry handle.

### Phase 2 — Composite lifecycle + Defer pattern

Goal: protocol-friendly process lifecycle.

1. **`Defer`**: Process methods that return values become
   `invoke(state, interval) -> Defer<Update>`. For sync protocols
   (local), `Defer::get()` returns the update immediately. For async
   (Ray batching), `Defer` holds a runtime + proc_id; `get()` blocks
   until `flush_pending` has been called.
2. **`_flush_protocol_runtimes` hook**: in `Composite::update`,
   between the invoke pass and apply_updates, call
   `flush()` on every protocol runtime referenced by the active
   processes. Lets batching protocols submit one batch RPC instead of
   N per-process RPCs.
3. **`Bridge`/`Interface` first-class**: `Composite::input_bridge`
   and `output_bridge` become `Bridge` instances (typed schema
   nodes), not internal Rust-only structs. Serializable, inspectable,
   composable.

**Checkpoint:** existing `Composite` tests still pass. New BRS-driven
grow/divide test exercises the lifecycle (one cell, sync protocol).

### Phase 3 — Protocol registry + local + parallel

Goal: protocol dispatch works; multiprocessing protocol proves
protocol-agnosticism with simple infrastructure.

1. **`ProtocolRegistry`** in prism-bigraph: maps protocol name → a
   `Protocol` trait impl. Each impl has a `load(address, config) ->
   Box<dyn Process>` method (the upstream's `load_protocol` dispatch).
2. **Local protocol**: trivially delegates to `link_registry` /
   existing factories. No behavior change for current users.
3. **Address encoding**: `address` parsed from either `"protocol:data"`
   strings (legacy) or `{protocol: "...", data: "..."}` maps (new
   canonical form). Engine consults `ProtocolRegistry` instead of
   `ProcessRegistry` directly.
4. **Parallel protocol**: spawn a subprocess per remote Process;
   communicate via stdin/stdout JSON. Mirrors upstream's
   `parallel.py`. Validates the abstraction with the simplest
   real cross-boundary case.

**Checkpoint:** chrysalis grow/divide runs end-to-end with one cell
on `parallel` protocol (subprocess), rest on `local`. Same source code.

### Phase 4 — REST protocol

Goal: HTTP-based remote processes.

1. **REST client (`rest` protocol)**: reqwest-based, hits the
   rest-process API. Mirrors upstream `protocols/rest.py`.
2. **REST server (optional)**: prism-rest crate exposing prism
   processes through the rest-process wire protocol so other prism
   instances (or upstream Python `RestProcess`) can connect.
3. **End-to-end**: grow/divide with one cell on `rest` protocol,
   prism-rest server in another OS process.

### Phase 5 — Ray + cluster

Goal: distributed batched execution.

1. **Pool protocol**: round-robin actor pool, bounds memory. Reference:
   `protocols/pool.py`.
2. **Ray protocol with sharding**: shadow processes enqueue onto
   `RayProtocolRuntime`; `flush_pending` resolves batched updates.
   Reference: `protocols/ray.py` (744 LoC — the most complex one).
3. **Cluster protocol (EC2/SSM)**: optional. Reference:
   `protocols/clusters/ec2_ssm.py`.
4. **Validation**: 4096-cell grow/divide on a Ray cluster, sharded
   across N workers, matches single-process result modulo rng.

### Phase 6 — Chrysalis on the aligned foundation

Goal: tier 1 grow/divide working homoiconically.

1. Refactor chrysalis composites to lower to `CompositeLink`
   schema nodes.
2. `chrysalis::Rule` becomes a typed schema node alongside
   `ReactionRule`, with chrysalis Exprs for reactum/guard/rate.
3. `ChrysalisBrs` becomes a thin layer over the upstream-aligned
   `BigraphicalReactiveSystem` Process.
4. Tier 1 e2e tests: grow/divide, MAPK parity, M/R closure.

## Decisions made

- **Keep prism's BRS** (`prism-bigraph/src/brs.rs`). Two advantages
  over upstream: (a) `ControlStatus` honoring Milner Def 8.2
  active/passive/atomic activity; (b) structural-diff output emitting
  minimal `_add`/`_remove` deltas instead of overwrite-whole-subtree.
  Selectively pull upstream improvements (e.g. recent gillespie
  refinements) but don't rewrite from scratch.
- **Keep prism's Schema enum dispatch**. It's a Rust-idiom equivalent
  of upstream's plum-based dataclass dispatch; existing match arms
  perform fine. Extend with new variants for typed Link / Bridge /
  Interface / CompositeLink / SharedProcess; don't rewrite the dispatch
  model.
- **`local` is just a `Protocol` impl**, not a special case. Symmetry
  with `parallel`/`rest`/`ray` from day one.
- **Chrysalis composites always wrap (never expand)**. Same
  external shape regardless of protocol. The interface-as-type
  contract is preserved through `CompositeLink` schema.
- **Tier 1 chrysalis acceptance**: all three benchmark examples
  (grow/divide, MAPK, M/R) plus one of them running with a non-local
  protocol (parallel, then REST). That validates protocol-agnosticism
  beyond a single transport.

## Open questions

- **`Quantity` / units priority**: useful for biology, not blocking
  chrysalis tier 1. Schedule for after tier 1 is green.
- **Tier 2 (Expr as first-class value) ordering**: depends on phase 1
  schema nodes being in place (Expr would be a schema-typed value).
  Likely fits between phase 4 and 5 if we want chrysalis tier 2 before
  Ray.
- **Effort estimate (revised)**: phase 1-2 (additions to schema + Composite
  lifecycle + Defer) is ~1 week. Phase 3 (protocol registry + local +
  parallel) is ~1 week. Phase 4 (REST) is ~few days assuming hyper/reqwest
  go smoothly. Phase 5 (Pool, Ray) is the biggest unknown — ~1-2 weeks
  depending on how clean a Rust analogue of ray.py's sharded runtime
  ends up. Phase 6 (chrysalis tier 1) on top is ~1 week. **Total: ~4-6
  weeks** for the full vertical including chrysalis. Phase 6 alone (on
  current prism, no protocol layer) is ~1 week if we cut scope to local
  only.

## References

Paths are relative to the prism repo root; the upstream Python repos
(`process-bigraph`, `bigraph-schema`, `rest-process`) are assumed
cloned alongside prism.

- `../process-bigraph/process_bigraph/types/process.py` — typed schema nodes
- `../process-bigraph/process_bigraph/protocols/{parallel,pool,ray,rest,session}.py` — protocol implementations
- `../process-bigraph/process_bigraph/processes/bigraphical_reactive_system.py` — upstream BRS
- `../bigraph-schema/bigraph_schema/{schema,methods,json_codec,units}.py` — typed schema + multi-dispatch + JSON + units
- `../rest-process/rest_process/server.py` — REST server wire protocol
- `../process-bigraph/process_bigraph/composite.py` — Composite lifecycle reference
