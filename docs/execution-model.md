# Execution model — parallel & distributed (the foundation)

How prism runs a simulation, and how it should parallelize / distribute. The
guiding result: **the tick is a BSP gather→apply barrier; parallelism is a
pluggable transport hung on one seam — `invoke() → Defer`.** Get that seam right
and `local` / `parallel` / `rest` / `stream` / a batched `ray` backend all
parallelize through the *same* code, with the simulation semantics unchanged.

## prism's tick today (already BSP — `engine.rs:822` `advance_to_next_event`)

The run loop is **invoke → advance → apply → trigger**, faithful to upstream
process-bigraph's `Composite.run`:

1. **invoke** — every *due* process (`next_time <= time`) runs `update()` against
   the **same immutable snapshot**, stashing its result in `front.pending`.
   *Nothing is applied yet* — so no process sees another's mid-step mutation. This
   is the core correctness property (BSP snapshot consistency).
2. **advance** `time` by the smallest interval to any next event (front-of-queue).
3. **apply** — all stashed updates reconciled + applied **together** (deltas sum,
   `_add`/`_remove` batch) via `apply_reconciled`.
4. **trigger** dependent Steps (a topological DAG cascade) + discover new processes.

The invoke pass is **embarrassingly parallel** (same snapshot in, independent
updates out). But today it's a **sequential `for` loop** and `Process::update`
**blocks** — so nothing runs concurrently, not even `parallel:`/`rest:`/`stream:`.

## The upstream's proven model (confirmed, `process-bigraph/composite.py`)

The base `Composite` holds **zero** Ray/threading code; it provides the *rhythm*,
and delegates actual concurrency to a pluggable transport via three hooks:

- **The seam — `invoke()` returns a `.get()`-able handle** (`composite.py:1172`,
  `Defer.get` at `1377`). Local → instant (`SyncUpdate`); remote → blocks on a
  future. The view/project (wiring) stays *local* in the Composite; only
  `update(state, interval)` crosses the boundary. This is what lets a remote/
  batched backend slot in without touching the run loop.
- **The collect — `apply_updates`** (`composite.py:3080`): phase 1 resolves all
  deferred updates (`defer.get()`), phase 2 reconciles + applies once.
- **The flush hook — `_flush_protocol_runtimes()`** (`composite.py:1620`), run
  *between* invoke and apply: a batching runtime dispatches its enqueued calls
  concurrently here.

Two batching forms, both opt-in, both in `protocols/` (not the Composite):

- **Form A — `flush_pending` (batched RPC).** A `RayShadowProcess.invoke`
  *enqueues* (doesn't run) and returns a `_RayDefer`; `flush_pending` issues one
  `batch_update.remote(...)` per shard actor and `ray.get`s them all at once, then
  scatters results. N processes → grouped onto a fixed shard pool → **one RPC per
  shard per tick** (the doc's 4096 cells → 16 RPCs).
- **Form B — `tick_lifecycle` (collapse the lifecycle).** The runtime takes over
  the *entire* invoke+apply for its group and returns one combined packet —
  collapsing N port-views + N applies into 1. Pure perf for huge homogeneous N
  (the framework's per-process `view`×N + `apply`×N overhead dominated `ray.get`
  at 64×64). Composite-side hook complete; the consumer lives downstream.

Resource layering for distribution: **cluster ⊃ pool ⊃ session ⊃ tick** (Ray/EC2
cluster (minutes) ⊃ warm actor pool (seconds) ⊃ per-Composite session (ms) ⊃ tick
dispatch). Concurrency primitives used: `ThreadPoolExecutor` (gated on
`parallel_processes`/`parallel_steps`), Ray (actor-per-call + sharded), and
`multiprocessing`. No asyncio. Thread pools are safe only when `update()` releases
the GIL — a constraint Rust does **not** have (threads parallelize CPU work).

## The execution-pattern landscape

| Pattern | Essence | Where prism sits |
|---|---|---|
| **BSP** (bulk-synchronous) | superstep: parallel compute → communicate → barrier | the tick *is* a superstep (invoke‖ → apply → advance-barrier); snapshot consistency = the BSP guarantee |
| **DES** (discrete-event) | event queue; advance to next event; heterogeneous rates | the per-process `fronts` (`next_time`) — already DES-style |
| **Ray / task-graph futures** | each call a task; dispatch graph, await futures; batch calls | the `invoke→Defer→flush→collect` seam + batched RPC |
| **Actor model** | message-passing, no shared state | the protocol boundary (`local`/`rest`/`stream`/`parallel`) |
| **Dataflow / Kahn networks** | fire when inputs ready; channels | the port wiring *is* the dataflow graph (path to async, no global barrier) |
| **CSP** | channels + synchronization | the `stream:` pipe |

Deep tension: **BSP (synchronous barrier — simple, correct)** vs **async
dataflow/DES (different rates, more parallelism, harder consistency)**. prism's
hybrid — BSP barrier *per tick* over DES `fronts` — keeps correctness while
allowing heterogeneous rates. Keep it.

## The gap in prism

- `Process::update` **blocks** (no `Defer` seam).
- The invoke pass is a **sequential** `for` loop (`engine.rs:842`).
- `protocols/parallel.rs` spins a worker thread per process, but `update()`
  sends-then-waits → no gain (the file's own "Future" note names the fix).
- prism **already has**: the BSP tick (`engine.rs`), a `Defer`/`DeferSlot`
  scaffolding, and the protocol seam (`local`/`rest`/`stream`/`parallel`).

## Proposed design (port the proven model; minimal semantic change)

1. **The seam:** `Process::invoke(&self, state, interval) -> Box<dyn Defer>` where
   `Defer::get(self) -> Update`. Default `invoke` = `SyncDefer(self.update(...))`,
   so **every existing process is unchanged** and runs identically.
2. **The engine:** the invoke pass calls `invoke()` (collecting Defers, *not*
   blocking) → a **flush hook** (`flush_protocol_runtimes`) → the apply pass
   resolves each `Defer::get()` → reconcile + apply (unchanged). Snapshot
   consistency is preserved (all invokes read the pre-step snapshot).
3. **Protocol runtimes** (a per-`Core` batching context):
   - `local` → `SyncDefer` (instant).
   - `parallel` → `invoke` enqueues on a shared thread pool (e.g. rayon); `.get()`
     awaits. CPU parallelism, no GIL caveat.
   - `rest` → `invoke` fires the HTTP call returning a future; flush awaits all
     concurrently.
   - `stream` → `invoke` writes the input frame; flush reads all output frames.
   - `ray`/batched → `invoke` enqueues; flush issues one batched call per shard
     (Form A).
4. **(Later, perf) `tick_lifecycle`** for huge homogeneous N — collapse N views +
   N applies into one vectorized packet (Form B). See #20.
5. **(Later, distribution) the lifecycle layering** (cluster⊃pool⊃session⊃tick)
   for HPC. See #17.

**The payoff:** a process author writes `update()`; parallelism is free and
uniform across transports. `report.ys`'s six independent sections, each a
`stream:`/`parallel:`/`rest:` process, run concurrently with no change to the
sections.

## Features — the evaluation

- **MUST**: the `Defer` seam + flush hook + concurrent invoke (the core — it
  parallelizes everything through one path).
- **MUST**: protocol runtimes (`local` sync / `parallel` thread-pool / `rest` +
  `stream` concurrent).
- **SHOULD**: a batched/`ray` protocol (collate → one packet per shard) to
  amortize RPC at scale.
- **LATER (perf)**: `tick_lifecycle` (collapse views+applies for huge N).
- **LATER (distribution)**: the cluster/pool/session lifecycle for HPC (#17).
- **KEEP**: BSP snapshot consistency + DES `fronts` — never traded for speed.

## Implementation path (incremental, test-guarded)

1. ✅ `Process::invoke → Defer` (default = sync `update`); engine invoke pass uses it.
   *Pure refactor — results byte-identical.* (The highest-leverage, lowest-risk step.)
2. ✅ `parallel:` → thread-pool Defer + flush (`ParallelPool` + `ParallelProcess`).
3. ✅ **step 2.5 — `Protocol::runtime()` + auto-registration.** A protocol exposes its
   batching runtime (`ParallelProtocol::runtime()` → the shared pool); the top-level
   engine registers every `core.protocols.runtimes()` in `from_state_core` (NOT in
   subengines — one barrier at the top; a nested flush over-synchronizes / can deadlock
   a fixed pool). Proven: `prism-bigraph/tests/parallel_engine.rs` (4 × 50ms sleeping
   `parallel:` processes finish in ~50ms, built via a real typed schema).
4. ✅ **step 3 (stream) — `stream:` concurrent dispatch.** `StreamProcess::invoke` is
   now non-blocking: it WRITES the input frame eagerly and returns a `Defer::lazy`
   that READS the output in the collect pass. So the engine hands every stream child
   its input first (all separate OS processes overlap), then gathers outputs —
   wall-clock ≈ slowest child. Proven concurrent + correct by
   `chrysalis/tests/grow_divide_stream.rs` (16 stream cells; runtime 0.40s → 0.15s
   after the change).
5. ⏳ **step 3 (rest) — `rest:` concurrent dispatch.** `RestProcess::update` still
   blocks on the HTTP round-trip; mirror the stream change (fire the request in
   `invoke`, await in the `Defer`) so `rest:` nodes also overlap.
6. ⏳ A batched `ray:`-style protocol (collate → one packet per shard, Form A).
7. (Later) `tick_lifecycle` + the cluster/pool/session lifecycle.
