# Delta-traces: a simulation is a procession of schema-deltas in time

*Status: design synthesis (2026-05-23), co-designed in session. Foundations
exist (`prism_schema::diff`/`algebra::apply`/`serialize`, `prism_viz::plot`,
`RunProcess` trace capture); `Simulate` (the event-source runner) + the
delta-log storage + Arrow serialization are the build-out. Read alongside
[`schema-algebra.md`](schema-algebra.md) (the operations this rests on) and
NEXT-SESSION #25.*

## The thesis

**The entire system is a procession of *deltas* moving forward in time.** State
at any moment is `fold(apply, initial, deltas[0..t])` — *time is the fold*, the
history *is* the delta-stream, and a simulation is the **event source** that
emits it. This is event sourcing, and it is also exactly Milner's reaction
sequence (a bigraph evolving by rewrites): same shape, two names.

One currency (the **schema-delta**), one fold (**`apply` = replay**), one
operation set (the schema algebra). Everything else — traces, plots,
serialization, the 4D structural view — falls out of that.

## One currency: the delta brutality lattice

A process's `update` *is* a delta/event. Deltas form a lattice by how violently
they change the state — and the **schema** decides which applies where:

| delta | effect | schema |
|---|---|---|
| additive `+δ` | `current + δ` (commutative, order-free) | `Float`/`Integer`/`Delta` |
| overwrite | replace the value | `Overwrite`, `List` (replace-wholesale) |
| `_add` | **create** a node (ex nihilo) | structural |
| `_remove` | **annihilate** a node — the data ceases to exist | structural |

`_remove` is the most brutal: it doesn't replace a value, it deletes the node
from the place graph. `_add` is its dual (creation). These structural deltas are
exactly what `divide` and the composite bridge already speak — so **one currency
spans value-deltas *and* structural deltas.** `apply` interprets them all,
dispatched on the schema.

## Event sourcing: the trace is the delta-log

A **trace** is the event-sourced history of a single item — `Trace[T]`, where
`T` is the item's schema (carried from the producer, **never re-inferred** — see
memory `feedback_carry_dont_infer_schema`).

- **Capture**: a `Simulate` step runs an inner process and records each step's
  `update` (the event/delta). The inner's `update` IS the event.
- **Replay**: `state(t) = fold(apply, initial, deltas[0..t])`. The algebra's
  `apply` is the replay operator; you don't re-run the (expensive) simulation,
  you replay the (cheap) log.
- **Diff**: `diff(schema, a, b)` computes a delta — the inverse direction, for
  turning two snapshots back into an event.

### Storage tradeoffs

| model | pro | con |
|---|---|---|
| **delta-log** (events) | compact when changes ≪ state; source-of-truth; replayable/auditable; composable | random access to time *t* is `O(t)` replay |
| **snapshots** (frames) | `O(1)` random access; trivial to plot | copies the whole state every step (bad for large fields) |
| **hybrid** (keyframes + deltas) | bounded replay (from nearest keyframe) + compactness — the general optimum | two code paths |

The hybrid is the destination (cf. video keyframes, Redis RDB+AOF). The natural
keyframe boundary is a **structural event** (see below).

## The tensor duality

Ask "is a trace a tensor?" and the answer is *yes, between structural changes*:

- **Fixed structure ⇒ a tensor.** A fixed-shape state traced over time is
  `[time × dims]`: `N` scalars → a `T×N` matrix (literally the timeseries
  columns); a `H×W` field → a `T×H×W` 3-tensor. This is why the line-plot
  transpose works and why **columnar/Arrow** serialization fits perfectly — a
  fixed-schema run is a dense tensor with time as the leading axis.
- **Structural deltas reshape it.** `_add`/`_remove` change the *index set* (a
  cell divides → `N` grows; dies → `N` shrinks). The dimensions jump at
  structural events. So it is not *one* tensor — it is a **piecewise tensor**:
  dense rectangular blocks, punctuated by topological reshapes.

This closes the loop with the bigraph: **the two delta kinds are the two graphs.**

| delta kind | evolves | character | bigraph aspect |
|---|---|---|---|
| value (additive/overwrite) | the state | continuous, dense, **tensor**-shaped | link/state |
| structural (`_add`/`_remove`) | the structure | discrete, **topological** | place graph |

Continuous dynamics + discrete rewrites is precisely what a process bigraph *is*;
the delta-stream makes the split fall out for free.

## `plot(schema, trace)` — viz over the stream

Plotting is a **schema-driven operation** returning a place-graph SVG value (see
[`prism-viz/src/plot.rs`](../crates/prism-viz/src/plot.rs); `_remove`/`_add`
aware later):

- scalars → a **line chart** (the `T×N` tensor, drawn);
- a field → an **animated heatmap** (the `T×H×W` tensor; each cell's fill cycles
  through time via SMIL `<animate>` — the animation is itself place-graph data);
- the structure (a `Tree`) → the **4D structural view** (the bigraph at each
  *structural* change — a new block per `_add`/`_remove`).

`plot` is replay-then-render *within a block*; a structural event starts a new
block. So the same op covers numeric series, fields, and evolving structure —
because they are all "a type extended through time."

## Serialization (the open challenge)

Dense **Arrow** tensor-blocks *between* structural events; the structural deltas
are the segment boundaries (the keyframes that reset the shape). Streaming large
traces to disk = a sequence of columnar blocks + structural-event markers. This
is the "interesting challenge" — but the structure above tells you the answer:
store dense where the shape is fixed, segment where it reshapes.

## What exists / what's next

- **Exists**: `prism_schema::diff`/`algebra::apply`/`serialize` (the delta
  algebra); `RunProcess` captures a `Trace[T]` (frames + carried `T`);
  `prism_viz::plot` (place-graph line chart + animated heatmap) and
  `prism_viz::svg` (SVG as place-graph data + `to_svg`).
- **Next**: `Simulate` — the **engine-faithful event-source runner**: feed the
  inner its full state, `apply` its update via the state schema (so kinetics
  deltas add and diffusion fields replace — the schema decides), capture the
  trace. Storage starts as frames, moves to the **delta-log + replay**, then the
  **hybrid + Arrow** blocks. (`RunProcess` stays the matched runner for
  full-output integrators; `Simulate` is the general one.)
- **Then**: the `Section` template runs any sim through `Simulate` → `plot` →
  self-output, dogfooded down `CANONICAL_ORDER`.

## Tooling survey: what aligns

Roles and *honest* fit. prism is **Rust** and **local-first** (one engine), but
scales to many engines over the **REST-process protocol** — and that boundary is
exactly where the heavier streaming tools start to earn their weight.

### Dense tensor blocks (the fixed-shape segments)
- **Apache Arrow (`arrow-rs`)** — in-memory columnar; a `RecordBatch` *is* a
  tensor-block. Zero-copy, the analytics interchange standard, Rust-native. The
  natural carrier for a value-delta run and for the wire (**Arrow IPC**). *Best
  fit — reach for first.*
- **Parquet (`arrow-rs`)** — Arrow-on-disk: compressed columnar + predicate
  pushdown. The disk form of a trace block. *Strong.*
- **Zarr / NetCDF / HDF5** — chunked N-d arrays on disk/cloud; the scientific
  array standards (climate/sim) for big field tensors (`T×H×W`). *Strong on the
  data side; Python/C ecosystems → interop, not in-engine.*

### Labeled & ragged arrays (analysis / read side)
- **xarray (+ Zarr)** — labeled N-d arrays with named dims + coords — *exactly*
  the fixed-shape view (time × species, time × grid), the premier tool for
  slicing/plotting scientific tensors. *Strong, but Python → a consumer: export
  Arrow/Zarr, explore in xarray.*
- **Awkward Array** — ragged/jagged nested arrays (variable-length per record).
  Built for "N particles per event" = **N cells per timestep** — the
  *structural*, topology-changing side that rectangular Arrow/xarray can't hold.
  The closest match for the piecewise/ragged part. *On-point; Python, niche.*
- **Polars** — Rust, Arrow-backed dataframes; the scalar timeseries as a fast
  dataframe, in-engine. *Strong + Rust-native.*
- **DuckDB** — embedded analytical SQL over Arrow/Parquet (no server); ad-hoc
  trace queries. *Strong + embeddable.*

### The event log / streaming
- **Plain append-only Arrow-IPC file** — for a *local* trace this simply *is* the
  event-sourced log: append delta-blocks, replay by scanning. No infrastructure.
  *Reach for this first locally.*
- **Kafka** — distributed durable log. Your instinct is right: **overkill for a
  single local sim** (a cluster + ops just to run a `fold`). It earns its weight
  only when *many* producers/consumers share a replayable stream — i.e.
  **multiple distributed engines over the REST-process protocol**, or live
  dashboards. There it (or lighter **Redis Streams / NATS JetStream**) fits. So:
  *not* for the local trace; *yes* for the distributed-engines case.
- **EventStoreDB** — purpose-built event store (streams + replay + projections);
  more aligned than Kafka *if* event-sourcing is the primary model and you want
  it off-the-shelf. *A server dependency.*

### Philosophical kin (the immutable / time-travel model)
- **Datomic / XTDB** — immutable, accretive, bitemporal DBs where a *datom*
  (E-A-V-T) **is** a delta and "the database as of time *t*" **is** replay. The
  closest conceptual match to "the system is a procession of deltas;
  `state(t) = fold`." **XTDB** is the open, reachable one. *Strong conceptual
  alignment; a DB dependency.*
- **CRDTs** — the additive/commutative deltas are CRDT-like (order-free merge);
  the formalism if simulation ever goes concurrent/distributed-merge. *Fits the
  additive layer of the lattice.*
- **Git** — content-addressed snapshots + diffs + history: the diff/apply/replay
  model in the large. A cousin; prism's `bigraph export/import` could dedup
  content-addressably.

### What I'd actually reach for (Rust + local-first → REST)
1. **Local trace**: `arrow-rs` RecordBatches as the value-delta tensor-blocks +
   an **Arrow-IPC append-only file** as the event log; **Parquet** to persist;
   Polars/DuckDB to query. (No Kafka.)
2. **Structural changes**: segment at `_add`/`_remove` (piecewise Arrow) + a
   small structural-event log; Awkward-style list/struct columns for the variable
   nesting.
3. **Analysis / interop**: export Arrow/Zarr → **xarray** / **DuckDB**.
4. **Distributed / live (later)**: a shared log — **EventStoreDB**, or
   Kafka/Redis-Streams/NATS — *only* when multiple engines or consumers share the
   stream (the REST-process angle); **XTDB** if you want time-travel queries as a
   first-class DB.

The throughline: **the schema-delta is the unit, and Arrow is its dense
serialization** — everything else is a consumer or a scaling choice.
