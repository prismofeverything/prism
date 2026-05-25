# Distributed execution at scale — patterns, prior art, and a plan

The goal: **giant colonies growing exponentially across an arbitrary number of
machines.** This doc answers "do we have our own Ray?", surveys how the fields
that already run planet-scale simulations do it, shows that prism's model
*already encodes the skeleton* of the right pattern, names the gaps, and drafts a
phased plan. Read `docs/execution-model.md` first (the `invoke→Defer→flush→collect`
seam this builds on).

## TL;DR

- We have the **execution seam** (Ray's *programming model* — distributed futures
  + actors-as-protocols + a BSP barrier), not the **runtime** (Ray's scheduler,
  object store, fault tolerance, membership). So "our own Ray": only the front
  half. The back half is a distributed-systems project — **adopt, don't rebuild.**
- The **fractal vision is the state of the art**: cells in a local environment =
  *spatial domain decomposition*; "diffuse between environments" = *halo / ghost
  exchange*; "a node that only cares about environments, not cells" = *Barnes-Hut /
  Fast-Multipole aggregation*; "recursively, at arbitrary scale" = an *adaptive
  forest-of-octrees* (AMR). prism's **composite-of-composites + protocol boundary +
  encapsulation + BSP tick** already map onto all four.
- **Biology is catching up, not hopeless**: BioDynaMo's TeraAgent ran **500 billion
  agents on 84,096 cores**; Biocellion does billions. The patterns are proven; the
  gap is a *substrate that makes them composable + model-agnostic* — which is
  exactly what a process-bigraph runtime is for.
- **What actually matters** (in priority): (1) locality — exchange *boundaries*,
  not state; (2) dynamic load balancing — exponential growth breaks any static
  partition; (3) hierarchical aggregation — the top must see summaries, not cells;
  (4) relaxed/local synchronization — global barriers cap scaling; (5) overlap of
  compute and communication.

## What Ray is (and what we already have)

Ray is a **distributed actor + task framework**. Its runtime has three pieces a
simulation runtime would otherwise have to build: a **bottom-up scheduler**
(per-node local schedulers + a global one; placement, resource-aware load
balancing), a **distributed object store** (shared-memory, zero-copy intra-node,
async transfer inter-node; results are addressable object refs), and a **global
control store** for system state + **lineage-based fault tolerance** (re-execute a
failed task from its inputs; restart a failed actor). Plus autoscaling/membership.
*Tasks* are stateless `@remote` functions returning futures; *actors* are stateful,
addressable objects with remote methods — "distributed actor" = location-
transparent stateful object with FT + scheduling. [Ray paper; Ray docs]

prism already has the **programming-model seam**: `Process::invoke → Defer`
(distributed futures), the **protocol boundary** (`local`/`rest`/`stream`/
`parallel` — a process is a stateful, addressable black box reached by a
transport = an actor), and the **BSP tick** (`invoke‖ → flush → collect/apply`
barrier — the consistency guarantee). What we lack is the *runtime plumbing*:
scheduler/placement, object store, fault tolerance, membership/autoscale. That
plumbing **is** Ray; rebuilding it is a multi-year detour. The leverage is to keep
the bigraph semantics on top and hang a real cluster backend underneath as *one
more protocol*.

## How the fields that already scale do it

| Field | Pattern | Prior art |
|---|---|---|
| **Molecular dynamics** | spatial **domain decomposition** + **halo (ghost) exchange** of boundary atoms; **dynamic load balancing** (local/geometric/graph re-partition) | LAMMPS (orthogonal), GROMACS (triclinic DD + DLB + redesigned halo exchange) |
| **N-body / astrophysics** | **octree** over space; treat a distant cell as one aggregate (center-of-mass / multipole) — `O(n log n)` Barnes-Hut, `O(n)` FMM | Barnes-Hut; Greengard FMM; extreme-scale tree codes |
| **Weather / climate / CFD** | **adaptive mesh refinement** on a **forest of octrees**; refine where the action is; MPI halo exchange + GPU interior | p4est, AMReX (Exascale Computing Project), DG-on-octree NWP |
| **Adaptive HPC runtime** | **over-decompose** into many small **migratable objects**; **measurement-based load balancing** migrates them as load shifts | Charm++ / AMPI |
| **AI / RL / irregular** | tasks→futures + stateful **actors** + object store + FT + autoscale | Ray |
| **Agent-based biology** | the above, applied to cells: spatial grid + diffusion (BioFVM-style) + parallel agents | Biocellion (billions); BioDynaMo / TeraAgent (**500B agents, 84k cores**) |

The convergent answer across all of them: **decompose space, keep interactions
local, exchange only boundaries, aggregate the far field hierarchically, and
rebalance dynamically.** Static decomposition is fine for fixed-N grids (weather);
**growing populations need dynamic rebalancing** (Charm++ migration, AMR
refinement) — that is the bit biology stresses hardest and the bit our exponential
colonies need most.

## prism already encodes the skeleton

The mapping is almost one-to-one — this is the encouraging part:

- **A "local environment" = a composite Engine** of cells over a shared pool
  (we have it: `environment.ys`). A *subdomain*.
- **"Diffuse between environments" = the bridge between sibling environments** —
  one environment's boundary output face → its neighbor's boundary input. That is
  **halo / ghost exchange**. (Today the bridge is parent↔child; neighbor↔neighbor
  wiring is the gap.)
- **"A node that only cares about environments, not cells" = a parent composite
  whose children are environments**, exchanging only boundary/aggregate fluxes.
  Our **encapsulation rule** (a composite's inner state is private; the parent sees
  only the exported face — memory `chrysalis_composite_rules`) gives **FMM-style
  aggregation for free**: the parent *cannot* see cells, only the environment's
  summary.
- **"Recursively, arbitrary scale" = composites of composites** — each nesting
  level a tree node; the **protocol boundary** makes any child remote (another
  machine) with no change to the model. That is the **forest-of-octrees**.
- **The superstep = the BSP tick** (`invoke‖ → flush → collect/apply`); **multi-
  rate** (environments tick coarser than cells) falls out of the **DES `fronts`**
  (different `next_time`/`interval` per process). Relaxed/local barriers (neighbors
  sync, not the globe) are expressible because each subengine has its own tick.

So we don't need to invent the patterns — we need to **realize them on this
substrate** and bolt on a cluster backend.

## The gaps (what it would take)

1. **Batched RPC (the `ray:` step, #21).** `invoke` enqueues instead of
   dispatching; a `ProtocolRuntime::flush_pending` issues **one packet per shard**,
   amortizing per-call overhead at scale. The seam already supports it (`parallel`
   exercises the flush path). *Smallest next step; the one genuinely new mechanism.*
2. **Halo / neighbor exchange.** Sibling-to-sibling boundary wiring + a
   *boundary-only* exported face (not the whole inner state). Diffusion across
   environment edges. Today bridges are parent↔child only.
3. **A spatial-partition protocol + the octree.** Place subdomains; build the
   recursive decomposition. Static first (fixed grid of environments across
   machines), then adaptive.
4. **Dynamic load balancing.** Split / migrate an overflowing subdomain as the
   colony grows (Charm++ measurement-based migration, or AMR refine-on-density).
   **The hard part — and the one exponential growth makes non-optional.**
5. **The cluster layer** — membership, placement, fault tolerance, an object store
   for big boundary buffers. **Build-vs-adopt decision below.**

## What actually matters (ranked)

1. **Locality / boundary-only exchange.** Co-locate interacting cells; ship only
   the halo. This is the single biggest scaling lever — it turns `O(N²)`
   all-to-all into `O(surface)` neighbor traffic. Our bridge + encapsulation are
   the mechanism.
2. **Dynamic load balancing.** A static partition dies the moment one region
   blooms. Over-decompose into many small environments and **migrate** them
   (Charm++) or **refine** the octree on density (AMR). This is *the*
   differentiator for growing colonies vs. fixed weather grids.
3. **Hierarchical aggregation.** The root must sync *summaries* (total biomass,
   boundary flux), never individual cells — or it bottlenecks. Encapsulation gives
   this; we just keep the exported face small (aggregate), à la FMM multipoles.
4. **Relaxed / local synchronization.** A global BSP barrier caps you around the
   slowest rank; neighbor-only barriers + multi-rate ticks scale to 10⁵ cores
   (the MD "local DLB" result). The `fronts` model permits it — don't trade the
   per-subdomain BSP guarantee, but don't impose a *global* one either.
5. **Overlap compute + communication.** The `invoke→Defer→collect` split already
   separates dispatch from gather; halo exchange should overlap interior compute
   (GROMACS splits local-only vs halo-dependent kernels — we can split interior vs
   boundary processes).

## The plan (phased, test-guarded — each rung a real consumer)

1. **`ray:` batched protocol (#21, now).** `flush_pending` collates a shard's
   enqueued invokes into one round-trip. Proof: a batched-vs-unbatched RPC-count
   test; reuse the `rest_engine`/`parallel_engine` harness. *Low effort, proves the
   batching seam.*
2. **Halo exchange + a 2-environment "diffusion across a boundary" demo.** Two
   `environment.ys` subdomains, a shared edge, glucose diffusing across it,
   conserved globally — over `local`, then `rest`/`stream` (two machines). Adds
   sibling-neighbor wiring + a boundary face.
3. **Static spatial-partition protocol (the octree, fixed).** An
   environment-of-environments across K machines; a placement table; the root syncs
   only aggregates. Realizes the fractal vision at modest scale. Conservation +
   weak-scaling test (2×, 4×, 8× machines, ~constant per-machine work).
4. **Dynamic load balancing.** Split/migrate an overflowing subdomain; measurement-
   based (cell count / wall-time per environment). The colony now grows past one
   machine's capacity. Test: exponential growth that stays balanced (no rank > X%
   over mean).
5. **Cluster backend — adopt, then wrap.** Pick a backend for membership /
   placement / FT / object store (below) and expose it as one protocol, keeping
   bigraph semantics on top.

## Build vs. adopt (the real decision)

We should **not** rebuild Ray's runtime. Options for the cluster layer, by fit:

- **Charm++ / AMPI** — the *closest conceptual match*: migratable over-decomposed
  objects + automatic measurement-based load balancing = exactly our migrating
  sub-environments. Mature, used at leadership scale. Cost: C++/niche, FFI.
- **Ray** — best for *dynamic, irregular, fault-tolerant* workloads (which growing
  colonies are): dynamic scheduling, actor restart, autoscale, object store. Cost:
  object-store/scheduler overhead for tight halo stencils; Python-centric.
- **MPI (+ AMReX / p4est)** — fastest halo exchange, the weather/MD standard. Cost:
  rigid, static-ish, no native dynamic spawn — load balancing is bolt-on.
- **Build minimal** — our protocols over a thin membership/placement layer. Most
  control, most work; only worth it if no backend's semantics fit.

**Recommendation:** treat the cluster as a **protocol with a pluggable backend**.
Start with the batched `ray:` protocol over our own transport (phase 1) to prove
the seam; for real multi-machine runs, wrap **Charm++-style migratable placement**
(the dynamic-load-balancing semantics we need) — or Ray if we want its FT/autoscale
sooner — behind the same `Protocol` interface. The bigraph stays the model; the
backend is swappable. This is the whole point of the protocol boundary: *the
simulation can't tell whether a subdomain runs in-thread, over HTTP, or on another
continent.*

## References

- Ray: [paper (arXiv 1712.05889)](https://arxiv.org/pdf/1712.05889) ·
  [Ray Core docs](https://docs.ray.io/en/latest/ray-core/walkthrough.html)
- Charm++ migratable objects + load balancing:
  [manual](https://charm.readthedocs.io/en/latest/charm++/manual.html) ·
  [Charm++ in Practice (PDF)](http://charm.cs.illinois.edu/newPapers/14-07/paper.pdf)
- MD domain decomposition + dynamic load balancing + halo:
  [adaptive DLB irregular DD](https://www.sciencedirect.com/science/article/abs/pii/S0010465515000181) ·
  [GROMACS halo redesign (arXiv 2509.21527)](https://arxiv.org/pdf/2509.21527) ·
  [GROMACS heterogeneous parallelization](https://pubs.aip.org/aip/jcp/article/153/13/134110/199476/)
- Hierarchical N-body (the "node only sees aggregates"):
  [Barnes–Hut](https://en.wikipedia.org/wiki/Barnes%E2%80%93Hut_simulation) ·
  [treecode + FMM on GPU (arXiv 1010.1482)](https://arxiv.org/pdf/1010.1482) ·
  [Berkeley N-body pattern](https://patterns.eecs.berkeley.edu/?page_id=193)
- Adaptive octree / AMR at exascale:
  [p4est (SISC)](https://epubs.siam.org/doi/10.1137/100791634) ·
  [AMReX (Exascale Computing Project)](https://www.exascaleproject.org/research-project/adaptive-mesh-refinement/) ·
  [AMR for NWP (arXiv 2404.16648)](https://arxiv.org/html/2404.16648v1)
- Agent-based biology at scale:
  [BioDynaMo (PMC)](https://pmc.ncbi.nlm.nih.gov/articles/PMC8723141/) ·
  [BioDynaMo HPC/TeraAgent (arXiv 2301.06984)](https://arxiv.org/pdf/2301.06984) ·
  [Biocellion (PMC)](https://pmc.ncbi.nlm.nih.gov/articles/PMC4609016/)
