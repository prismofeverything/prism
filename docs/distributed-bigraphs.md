# Distributed bigraphs: prism across many nodes

*Status: vision + roadmap (2026-05-23), co-designed in session. The thesis is
that distribution is **not a new system** — it is the existing bigraph
primitives (composite, bridge, protocol, delta) extended across a network
boundary. Read alongside [`delta-traces.md`](delta-traces.md) (deltas are the
messages) and [`prism-architecture.md`](prism-architecture.md).*

## The thesis

**prism is already a distributed actor system; it just doesn't span nodes yet.**
A bigraph is *the* model for this, because a bigraph already carries the two
graphs distribution needs:

| bigraph | distributed-systems role |
|---|---|
| **place graph** (nesting/locality) | the **partition** — what runs *where* |
| **link graph** (wiring) | the **channels** — who talks to whom |
| **composite** (subengine, typed black box) | an **actor** (the unit of placement + parallelism) |
| **bridge** (typed interface ports) | the actor's **mailbox / channel ends** |
| **protocol** (`local`/`rest`/…) | **location transparency** (the transport seam) |
| **delta** (an `update`) | the **message** on a channel |
| **schema algebra** (`apply`/`merge`/`diff`) | the **synchronization** logic |
| **delta-trace** (event log) | durable history → **checkpoint / replay / recovery** |

So we are not reinventing Ray. Ray has an *implicit* actor graph and untyped
messages; ours is the **explicit, typed, reconfigurable bigraph** with typed
deltas as messages. The topology is first-class data (homoiconic), and it
*reconfigures over time* via structural deltas (`_add`/`_remove`) — a
**dynamic-schema, temporally-reconfiguring tensor of traces.** That combination
does not exist off the shelf (see Prior art).

## The algebra decides what parallelizes ("along which lines")

The deep point, and the answer to "is this part of our algebra?": **yes — the
delta's place in the brutality lattice determines its synchronization
requirement.**

- **additive / commutative deltas** (`Float`/`Delta`) — order-free. Linked
  actors can exchange them **asynchronously**, merge with `apply`, and never
  barrier. This is the CRDT-safe, embarrassingly-parallel direction.
- **overwrite / structural deltas** (`Overwrite`, `_add`/`_remove`) —
  non-commutative. They need **ordering** → a barrier / consensus across the
  link.

So "decomposable along certain lines, synchronize on others" is **derived, not
declared**: the *place-graph cuts* give the partition; the *link-graph edge
types* (read off the schema) give the sync requirements. Parallelization is
schema-driven, exactly like `plot`, `apply`, and `divide`. Co-locate tightly,
commutatively-linked actors; cut the graph where links are sparse or additive.

## What's already here (the seams to extend)

- **Composites as subengines** — the actor + partition unit (`Composite::from_config`).
- **The bridge** — the typed channel boundary (`@ inner.path`), input/output ports.
- **The protocol boundary** — `local` vs `rest`, `address_class` (local string vs
  remote `.process`), and a working **`RestProcessServer`** (a process served over
  HTTP). This is location transparency for *one* remote process.
- **The exchange-bridge** — a *synchronized cross-boundary delta flow* already
  works locally (glucose depletes across a composite boundary; see memory
  `project_exchange_bridge_bug`). This is the **local prototype of a distributed
  link.**
- **The delta-trace** — deltas are the messages; the log is the durable,
  replayable history (recovery for free).

## What's required (the gaps)

1. **Cluster addressing + registry** — an address resolvable to a *node*
   (`local | rest | node@cluster`), and a registry answering "where does process
   X live?" so a wire can cross a node boundary. (Generalizes `address_class`.)
2. **Delta transport over links** — when a link crosses nodes, stream **deltas**
   (Arrow-IPC / Arrow Flight / gRPC) over the channel, not full state. The
   exchange-bridge goes network; the REST protocol carries deltas. (Cheap,
   because deltas ≪ state — see delta-traces.md.)
3. **A distributed orchestrator** — coordinate firing across nodes. Start with
   **BSP** (bulk-synchronous-parallel): each superstep = compute locally →
   exchange deltas at a barrier → repeat. The barrier *is* the timestep boundary.
   Refine: only **non-commutative** links barrier; commutative links exchange
   async with bounded staleness (the algebra says which).
4. **Placement / partitioning** — map the place graph → physical nodes
   (declared annotations first — e.g. a chrysalis `@node` placement — then
   automatic graph-partitioning to minimize cross-node links, METIS-style).
5. **Fault tolerance / elasticity** — a failed node's subgraph **replays from its
   delta-log** (event sourcing pays off here); elastic = re-partition the place
   graph as nodes join/leave.

## The roadmap (here → there)

Each step is a working milestone, and each is the *previous* primitive crossing
one more boundary:

0. **Now** — composite subengines (local), `RestProcessServer`, delta-traces,
   the local exchange-bridge.
1. **Remote composite** — a composite addressed on another node (generalize
   `rest` → cluster addressing). One actor, elsewhere.
2. **Delta-streaming protocol** — links carry Arrow-IPC delta-streams; the
   exchange-bridge over the network. (Two nodes exchange deltas across a wire.)
3. **BSP orchestrator** — two nodes step in lockstep, exchanging deltas at the
   barrier. The "hello, distributed world."
4. **Placement** — chrysalis `composite X @ node` (or a policy); the place graph
   maps to nodes.
5. **Scale + FT** — N nodes, async where the algebra allows, replay-recovery,
   elastic re-partition.

**The ultimate test** (your "composite in chrysalis"): a chrysalis `composite` —
a whole organism, nested composites at many scales — deployed across HPC nodes,
each scale an actor, all communicating through wires across protocols. The
composite *is* the distributable unit; nothing about its `.ys` changes — only its
placement.

## Where to start (the smallest real step)

The smallest thing that proves the *whole shape*: **two composites, each served
by `RestProcessServer` (even two local OS processes), exchanging deltas across a
single wire once per step** — a distributed exchange-bridge with a BSP barrier in
miniature. It builds directly on the REST-process server we already have, and
from there "more nodes" is quantitative, not qualitative.

So: **we start here.** Every piece of "there" is a piece of "here" pushed across
one boundary — the protocol seam, carrying deltas, coordinated by a barrier the
algebra tells us when to drop.

## Working backwards: the critical path

Start from the goal and ask, at each level, "what *one* thing must be true just
before this?" The chain converges on a single seam.

- **Organism across N HPC nodes** ⇐ placement + N-node orchestration + fault
  tolerance. *All quantitative — scaling.* Strip them:
- **Any composite running on a different node, in step** ⇐ a wire that crosses a
  node boundary carrying per-step deltas, with a barrier. *The transport is just
  "which node"; the qualitative core is the crossing.* Strip the node specifics:
- **A wire crossing a process boundary, exchanging per-step deltas** ⇐ (a) the
  protocol routes a remote-addressed subengine to a server; (b) the bridge
  exchanges state-in / delta-out across it; (c) a barrier steps both sides. We
  already have **(b)** [the bridge is asymmetric state-in/updates-out — memory
  `reference_bridge_state_in_updates_out`] and the server half of **(a)**
  [`RestProcessServer`]. The missing piece:
- **THE LINCHPIN — the engine steps a `rest`-addressed subengine exactly like a
  local one**: each step, send its input-port state, receive its output delta,
  `apply` it. ⇐ engine wire-resolution + step-loop routing a remote address
  through the REST client (*rest-discovery — a known TODO,
  `project_core_unification`*).
- ⇐ **the remote serves a composite and advances it** — `RestProcessServer` (have).
- ⇐ **deltas serialize across the wire** — `serialize`/`realize` + JSON (have).
- ⇐ **NOW**: local composites + bridge + REST server + the algebra.

**Everything reduces to one seam.** Make the engine treat a `rest`-addressed
subengine as a remote actor (step it, `apply` its delta), and a chrysalis
`composite` reaches HPC by **changing one subengine's address from `local` to
`rest`** — *the `.ys` is unchanged.* Above the seam (placement, N nodes, Arrow
delta-transport, async-where-commutative, replay-recovery) is all additive; below
it is already built.

**First move:** split *one* composite across *two* OS processes over REST —
parent engine ↔ `RestProcessServer` child — exchanging bridge deltas once per
step (a distributed exchange-bridge; a BSP barrier of size 2). That single test
proves the whole shape; "more nodes" is then quantitative.

## Protocols: a ladder, not a choice

REST is the right *start* — universal, debuggable, language-agnostic, perfect for
the **control plane** (lifecycle, discovery) and the first proof. But it's a poor
**data plane** for per-step delta exchange: HTTP/JSON overhead and text encoding
are heavy when you're crossing a barrier every timestep with large numeric
tensors. The fix isn't "pick a better protocol" — it's that the protocol seam
already in the `Core` (`local`/`rest`/…) generalizes to a **ladder chosen per
link by locality + the delta's commutativity:**

| reach | transport | why |
|---|---|---|
| same address space | direct call | already `local` — no serialization |
| same node | **shared memory / Arrow IPC** (Plasma-style) | zero-copy; no network at all |
| same cluster (data plane) | **Arrow Flight** (Arrow over gRPC) | purpose-built for streaming columnar/tensor data at high throughput — and our deltas already serialize to Arrow (delta-traces.md), so it's the *matched* transport |
| same cluster (typed RPC / many channels) | **gRPC** (HTTP/2, protobuf) | bidirectional streaming, binary, typed, multiplexed — the direct step up from REST. (Rust: `tonic`, `arrow-flight`.) |
| HPC tight inner loop | **MPI / UCX / libfabric** (RDMA, InfiniBand) | lowest latency; collectives for the dense, tightly-coupled numeric parts |
| control plane / loose / debug | **REST** (keep it) | discovery, lifecycle, the proof; also HTTP/2–3 (QUIC) if staying HTTP |
| many-channel pub/sub | **ZeroMQ / NATS** | brokerless/lightweight channel patterns; lighter than Kafka |

Two connections make this *part of the algebra*, not a bolt-on:

1. **Arrow Flight is the matched data plane.** Because a delta serializes to
   Arrow, "stream the delta-blocks between nodes" *is* Arrow Flight. The
   serialization format and the transport are the same decision.
2. **MPI collectives *are* algebra operations.** An `all-reduce` of **additive
   (commutative)** deltas across nodes is exactly the distributed `merge`; a
   `barrier` is the BSP sync. So the algebra picks the collective: commutative
   deltas → `all-reduce` (async-safe), non-commutative → ordered/barrier. The HPC
   primitive and the schema operation are the same thing.

So the transport is **another schema-/locality-derived choice**, pluggable behind
the seam: the `.ys` and the algebra never change; only which rung a given link
uses. Don't over-build — REST/JSON to prove it, **Arrow Flight** when throughput
bites, **MPI/UCX** only at true HPC scale. (Ray, by contrast, is a *runtime*, not
a transport — its Arrow/Plasma object store is worth stealing for the same-node
zero-copy rung, but its topology is implicit where ours is the typed bigraph.)

## Prior art (and why this is novel)

| system | actors | topology | messages | dynamic? | gap |
|---|---|---|---|---|---|
| **Ray** | yes | implicit | untyped (Python objs) | yes | Python-only; topology not first-class; untyped |
| **Akka / Erlang** | yes | implicit | untyped | yes | no bigraph, no schema, no multi-scale nesting |
| **Flink / Beam** | operators | **fixed** dataflow | typed-ish | no | static topology; no structural reconfiguration |
| **MPI** | ranks | manual | bytes | no | hand-wired, static, no types |
| **Repast HPC / FLAME GPU** | agents | domain-specific | domain | partial | ABM-specific; not a general typed substrate |

None combine **typed, schema-driven** + **dynamic bigraph topology**
(reconfiguring via structural deltas) + **event-sourced delta-traces** +
**multi-scale composites**. That is the thing to build — and the design is just
the bigraph, distributed. We don't invent a new model; we run the one we have
across the network.
