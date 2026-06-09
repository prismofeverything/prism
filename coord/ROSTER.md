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

## Two regimes — monotone vs shared-invariant (the CALM rule, dogfooded)

The board collisions and the *build breaks* are the SAME lesson at two levels, and
it is the project's own (CALM; `categorical-core.md` §5): **coordination-free ⟺
MONOTONE.** Two kinds of change, two disciplines:

- **Monotone / per-source** (your own `coord/<role>.ys`; your own crate's code) —
  conflict-free by ownership. **Act freely; sync eventually** (the board is
  anti-entropy — others catch up). No coordination needed.
- **Non-monotone / SHARED INVARIANT** (the workspace must BUILD; a shared crate's
  public API; a `Cargo.toml` / dependency; a shared schema or `.ys` vocabulary) —
  *any* agent can break it for *everyone*, **immediately and globally**, and CALM
  says you cannot make it conflict-free by ownership. Eventual sync is too late
  here (the break already stopped everyone's `cargo test`). So:
    1. **Keep-it-green (default, local — prevents most breakage).** Never leave the
       workspace un-buildable between steps: add a manifest `[[example]]`/member
       entry only once its file/crate exists (stub first); fix a dep path before
       saving. `cargo metadata` is a cheap check before you step away. *This needs
       no coordination — it just doesn't break the invariant.*
    2. **Coordinate-FIRST (for unavoidable shared breakage).** Before changing a
       SHARED-IMPACT file — a `Cargo.toml`, the workspace manifest, a shared crate's
       public API, a shared schema/vocabulary — **READ the board, then CLAIM it in
       your `touching` + drop a `note`** ("changing X's API; rebuild/pull after").
       Read-then-act, *not* act-then-eventually-sync — because the blast radius is
       everyone, now.
    3. **Isolate (big restructures).** A `git worktree` / branch builds
       independently; merge when green.

Rule of thumb: **if your change can break someone else's `cargo test`, it is
non-monotone → keep it green, or coordinate-first. If it touches only your own
files, it is monotone → go.**

## Persistence — the fractal (`coord/<agent>.next`)

`NEXT-SESSION.md`, fractalized: each agent keeps a durable `coord/<agent>.next`
alongside its live `coord/<agent>.ys`. The pair mirrors the project's own convention
(`NEXT-SESSION.md : harness-panel`):

- **`<agent>.ys`** — the LIVE heartbeat (`task`/`touching`/`status`/`note`/`tick`),
  gossiped through the board, overwritten each tick. The ephemeral "now."
- **`<agent>.next`** — the DURABLE resume (⏯️ ON BOOT · Domain · Now/next · Done).
  Survives reboot; **on boot you rebuild your `.ys` from it.**

Own **only your own pair** — monotone, per-source, no collision. The structure grows
to arbitrary N: a new agent = a new `(.ys, .next)` pair, nothing shared.

**The overall lives in `coord/unify.next`** — the unify agent stewards the whole: the
cross-agent index (a line + pointer per agent), the convergence map (how the domains
are one engine), and the M0–M5 state. It is the fractal's ROOT / front door, backed by
the deep docs (`docs/grand-synthesis.md`, `docs/categorical-core.md`,
`docs/NEXT-SESSION.md`). It is in `unify` — not a shared file everyone edits — because
the overall is just unify's domain, so it stays single-writer/per-source: the same
monotone discipline, applied to the overall itself.
