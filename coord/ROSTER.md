# coord/ROSTER.md — the agent roster (the "regions of expertise")

prism is large and **convergent**: many domains whose value is in how they
*connect* (the categorical spine). So we partition the work into agent **roles**
that let each focus its context **without siloing**. An agent boots into ONE role,
reads this roster for its purview, reads `coord/board.ys` for who else is active,
and coordinates through the shared mesh board.

**This roster is the project's own structure applied to us** — a fractal of prism:
the **domains** are the specializations; **`unify`** is the functors that connect
them; **`core`/`lang`** are the shared substrate; **`simplify`** is the reduction
to normal form; and the **mesh board** is the link that keeps us converged. Role =
purview = lens; the lens shapes the work (that is the emergent "personality").

Not all active at once — roles boot in and out as the work calls. A role can split
(e.g. `reaction` out of `core`, `perf` out of `simplify`) or merge when small.

## Cross-cutting roles — the connective tissue (span every domain)

These are the anti-silo mechanism. They keep the convergence real.

| role | purview | owns (≈ NEXT-SESSION cat) |
|---|---|---|
| **unify** | the categorical SPINE — actively CONNECT the domains: cross-domain functors/couplings, the one-engine thesis, the generative-core program. The "look at everything and connect it" role. | `categorical-core.md`, `grand-synthesis.md`, `generative-core.md`; #59, #64 (cat E) |
| **simplify** | REDUCE — remove redundant paths, find the unifying mechanism, "one way to do each thing" (wei-qi / Felleisen gate), the unification audit. `/simplify` + `/code-review` as a standing role. | the audit #46; cross-tree (cat E/G) |
| **core** | the SUBSTRATE everyone builds on — the schema algebra + the engine + the reaction/BRS machinery. Builds new algebra ops / engine capabilities domains need. | `prism-schema`, `prism-bigraph` (engine, reaction). #5/#15/#30/#60 (cat E). *(may split: `reaction` = the BRS/AlChemy rewriting layer)* |
| **lang** | the chrysalis SURFACE — parse / eval / compile / check, language semantics, diagnostics. The surface every domain writes `.ys` against. | `crates/chrysalis`; #10/#14/#16/#17 (cat F) |

## Domain roles — the verticals (own an application / capability area)

These mirror `grand-synthesis.md` §1's table (one engine, many specializations).

| role | purview | owns (≈ NEXT-SESSION cat) |
|---|---|---|
| **mesh** | distribution — the peer/mesh protocol, the HPC ladder, **coordination (this board)**. | `prism-bigraph/protocols`; #62, #25–29 (cat A) |
| **manifold** | adaptive networks — oscillators, Kuramoto, plastic/learning topology, dynamical convergence. | `../manifold`; #66, #70 (cat I) |
| **synth** | audio — streaming synthesizers, the `Signal` sort, modules, the device boundary. | `crates/prism-audio`; #63 (cat D) |
| **bio** | cellular biology — cells, CRN, SBML, dFBA/spatio-flux, the science demos. | `prism-mapk`, `spatio-flux`; #8/#13/#33 (cat B) |
| **quantum** | quantum bigraphs — algebraic effects/handlers, entanglement, teleportation. | quantum `.ys`, effects; #35/#36 (cat C) |
| **spatial** | geometry — the octree, placement, 3D/4D rendering (the "eye"). | `../parsimony`, viz; #65 (cat H) |

## Active now

- **mesh** — #62 distribution + this coordination system. *(me)*
- **manifold** — #70 composite multi-scale intervals.
- **synth** — #63 A5 (a live BRS rewriting a synth patch).

## How it works

- **Boot:** the human assigns you a role. Read this roster + `coord/board.ys`.
  Create/own `coord/<role>.ys` (`def <role> = { task, touching, status, note, tick }`).
  **Edit ONLY your own file.**
- **Coordinate:** through the mesh board — `coord/board.ys` merges every
  `coord/<role>.ys` through a `mesh` link (per-source = no write collisions). Run
  `chrysalis run coord/board.ys --time 1` for the converged view. The live socket
  form (no file) is `chrysalis coord` (push/pull over the mesh).
- **Don't silo:** `unify` connects the domains; the board keeps everyone aware;
  `core`/`lang` are common ground. Convergence is structural, not optional.
