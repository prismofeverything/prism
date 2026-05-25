# Next session — launch prompt & plan

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 4 — division proven over STREAM; parallelism next)

> Continue prism (Rust process-bigraphs + the `.ys` language). Workspace green
> (chrysalis suite all-pass incl. the new `grow_divide_stream`). Read
> `docs/cells-and-division.md` §"RESOLVED (part 4)" + memory
> `composite_bridge_forwards_updates` first.
>
> **DONE THIS SESSION — cell division over the `stream:` protocol, conserving mass.**
> The `stream:` child was still a delta-LOG *filter* (`serve_stream` `diff`ed absolute
> `output_raw` snapshots — double-counts a shared pool once ≥2 cells draw on it, drops
> structure). Fixed: new **`runner::serve_process`** (CLI `--serve-process`;
> `StreamProcess` spawns it) runs the entry as a real `Composite` and **forwards
> `Composite::update`'s delta** — the pipe mirror of the rest server (no `refines`
> handshake, no one-tick lag: parent stamps cumulative time *after* the step).
> `serve_stream` stays the `A|B` trace FILTER. New `crates/chrysalis/ys/cell.ys` (a
> streamable Cell; its ENTRY is the composite — no trailing `Cell[]`). Proof:
> `crates/chrysalis/tests/grow_divide_stream.rs` — a `stream:cell.ys` cell (separate
> OS process, Arrow over pipes) grows + divides + **conserves `glucose+Σmass+acetate=41`
> EVERY tick**, terminating at a stable 16 cells, terminal numbers IDENTICAL to
> local/rest. The 2 `stream_protocol.rs` echo tests were rewritten to a delta-emitting
> Counter (a passthrough forwards nothing — correct, same as local).
>
> **NEXT: real parallelism (#27 / task #4)** — the user's stated goal, rides this same
> protocol seam. Steps 1+2 done; pick up at step 2.5 (register the pool as a
> `ProtocolRuntime` via `Protocol::runtime()`), an engine-level wall-clock test, then
> step 3 (`rest`/`stream` concurrent dispatch — the `stream:` cells can now run
> truly concurrently) and step 4 (batched ray). See `docs/execution-model.md`.
>
> **ALSO available (not blocking):** the env.ys surface (#20 / task #20) — thread
> `map[Cell]→Map{CompositeLink}`, retire the `composite_instance_schema` dodge, the `%`
> surface wire, surface stream addressing — so the WHOLE environment runs as
> `chrysalis run env.ys` (today the proof builds the env in Rust). A convenience, not a
> capability gap.

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 3 — bridge unified, division proven over rest)

> Continue prism (Rust process-bigraphs + the `.ys` language). Workspace green
> (94 test groups; one *rare pre-existing* flake `law_reconcile_coherence` — fold
> into #11). Read `docs/cells-and-division.md` **§"✅ RESOLVED"** (top) + memory
> `composite_bridge_forwards_updates` + the task list first.
>
> **THE FOUNDATIONAL SHIFT THIS SESSION:** the composite bridge now **forwards the
> inner UPDATE (delta) intact — `diff(pre,post)` deleted**. Updates ARE deltas, so
> **local bridge == `stream:` wire == `rest:` == trace == command** (one way across
> every boundary). This is what makes protocol-equivalence real. Division is **Form
> 3 canonical** (cell proposes via `%.divide` self-node face → env enacts via the
> `_divide` sentinel → schema splits via `divide_by_schema(CompositeLink)`; daughters
> inherit the cell's protocol via shared `address`). Proven: a mass-balanced
> grow-divide-glucose cell divides + conserves (`glucose+Σmass+acetate=const` every
> tick), **identical local AND over `rest:` (real HTTP)** — `prism-bigraph/tests/
> cells_division.rs`. `growth_division` is true division now (1→2→4, 0.01s, was a
> 46s zombie explosion). Algebra holes fixed: `resolve` Map-preservation,
> `reconcile` node-field merge, own-path instance removal, the `%` self-node wire,
> the `add_process` explosion backstop (generous; tests set `set_max_nodes(64)`).
>
> **FIRST: the `stream:` half = `chrysalis run cell.ys --serve-stream` (#6→#7→#8).**
> The rest half is proven at the prism level; stream needs the `.ys` cell. (#6)
> Thread the chrysalis lowering `map[Cell] → Map{CompositeLink}` (retire the
> `composite_instance_schema` dodge at `chrysalis/src/schema.rs:80`; cell value is
> already an addressed CompositeLink node via `build_composite_outer`) + the `%`
> self-node wire at the **surface** (engine side done — `["%",…]`); update the
> dependent `schema.rs` tests. (#7) Write the grow-divide-glucose `.ys` (Form 3:
> cell + Divide reaction + environment; mass-balanced uptake). (#8) `chrysalis run`
> it over local + `stream:`, asserting the SAME division + conservation as rest —
> the same cell, only the address changes.
>
> **THEN:** #11 complete the law generators to cover node sorts (so node-holes fail
> `cargo test` — would have caught this session's `reconcile`/`resolve` holes) +
> fix the flaky law deterministically, THEN re-apply the *uniform node-data
> handling* (reverted this session — apply/divide/reconcile for ALL link sorts via
> `node_data_branches`, validated by the now-complete laws). #13 design explicit
> bridge **conduits** (today inferred by inner-slot absence — a wart). #9 cleanup
> (#29 collapse the address-scan / two discovery walks now that cells are
> schema-first). Then real parallelism (#27) rides the same seam.

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 2)

> Continue prism (Rust process-bigraphs + the `.ys` language). Workspace is green
> (530/0); read this section + the task list first.
>
> **FIRST: finish the cells/division work (#9).** It's the prerequisite for
> distributed/parallel execution AND it's currently *incorrect*:
> `divide_by_schema(CompositeLink)` divides the exported outputs, but must divide
> `inner_schema` (the whole body). Implement `docs/cells-and-division.md` §6 in
> order; the **grow-divide-over-`stream`** test is the boundary proof and a top
> priority — getting it green validates the composite boundary (a cell divides
> itself and exports daughters across the bridge, working unchanged over a real
> wire).
>
> **THEN: real parallelism (#27)** — the original goal. Steps 1–2 are done (the
> engine runs invoke→Defer→flush→collect; `ParallelPool` + blocking `Defer::slot`;
> pool wall-clock test green). Pick up at **step 2.5** (register the pool as a
> `ProtocolRuntime` via `Protocol::runtime()`), an engine-level wall-clock test
> (build via `Topology`/instances, NOT `Schema::Any`), then **step 3** (`rest`/
> `stream` concurrent dispatch) and **step 4** (batched `ray`) — and **test it with
> the working grow-divide-over-`stream`** (same boundary seam). See
> `docs/execution-model.md`.
>
> Also: #28 (Schema::Any: static done, cells part = #9), #29 (core unification,
> before perf #20). Keep the composite-boundary principle front of mind (memory
> `feedback_composite_boundary`): a composite may be remote; never reach into its
> internals; all I/O via the bridge.

## This session (2026-05-24 part 2 — parallelism + extern + schema-threading + cells/division design)

Started as "real parallelism (#27)"; the no-half-measures/dogfooding loop pulled in
extern retirement, `Schema::Any` threading, and a foundational cells/division
redesign. **Workspace green: 530/0.** Changes uncommitted (user commits).

- **#27 real parallelism — steps 1+2 DONE (resume AFTER #9).** Engine runs
  invoke→Defer→flush→collect (`ProcessFront.pending: Option<Defer<Update>>`; invoke
  pass calls `p.invoke()`; `flush_protocol_runtimes()` between invoke+collect;
  collect resolves `.get()` — byte-identical for local `Defer::immediate`).
  `Defer::slot()` → blocking one-shot (`sync_channel`; dead `Slot` variant removed).
  `ParallelPool` (shared worker pool + `flush_pending` barrier) + `ParallelProcess`
  (`invoke` enqueues → slot-`Defer`); pool wall-clock test green (4×50ms < 150ms).
  REMAINING: step 2.5 register pool as a `ProtocolRuntime` (`Protocol::runtime()`);
  engine-level wall-clock test (build via `Topology`/instances); step 3 rest/stream
  concurrent; step 4 batched ray. (`engine.rs`, `defer.rs`,
  `protocols/parallel.rs`, `protocol_runtime.rs`; `docs/execution-model.md`.)
- **#14 extern — FULLY RETIRED.** Removed from grammar/AST/parse/eval/compile/
  check/unparse/repl; `dish.ys` → `from diffusion import DiffusionAdvection`
  (`sf_modules` already exports it); converted parse_import/chrysalis_port/
  composite_process/parse_contract.
- **#28 Schema::Any — STATIC schema-first DONE.** chrysalis `branch_schema` emits a
  base `Link` MARKER for native controls (no compile-time `Any`); the engine stamps
  each node's REAL Link from its INSTANCE into `self.schema` at `add_process`
  (`stamp_instance_link` / `is_stampable_node_path`); cycle detection in chrysalis
  lowering (`composite_link` + `building` stack); `extract_processes` infer-keys
  deleted; `scan_for_processes` schema-first-primary (address fallback only for
  dynamic/homoiconic). Test `native_node_found_by_address_…`. Dynamic (cells) → #9.
- **#9 cells & division — DESIGN WRITTEN (`docs/cells-and-division.md`); DO FIRST.**
  Currently INCORRECT: `divide_by_schema(CompositeLink)` divides the exported
  outputs — it must divide `inner_schema` (the whole body; §6 step 1). It's also
  the prerequisite for testing distributed/parallel execution (grow-divide-over-
  `stream`). A cell is just a composite that EXPORTS its `mass` across the bridge
  (`cells.N.mass`,
  populated from the bridge — the matchable/divisible face). Division-through-a-
  reaction is a STRUCTURAL BRIDGE UPDATE the composite emits itself
  (`{cells:{_remove:[self], _add:{daughters}}}` out `->{environment}`), carried by
  invoke→Defer over ANY protocol — no division-specific code; `stream` is the
  boundary-proof test. `divide_by_schema(inner_schema)` runs INSIDE the composite.
  Implement §6 in order. (Memory `feedback_composite_boundary`.) The cells-as-
  `CompositeLink` attempt bounced on the homoiconic representation → became this
  design; cells currently reverted to the instance schema (green).
- **#29 core unification** created — collapse `composite_instance_schema` vs the
  `CompositeLink`, the address scan, the two discovery walks, the ~150 remaining
  `Schema::Any` — gated before the perf sweep (#20).

## Where we are (2026-05-24 — spatio-flux-report-as-.ys + delta-motion + .ys-modules)

The **full-circle challenge landed: the entire spatio-flux report is regenerable
from `.ys`.** All 6 simulation families run as `.ys` sections through the codegen
path, each plotting itself BY ITS SCHEMA. The dogfooding loop (turn a sim into
`.ys`, find the gap, fix it IN the core — memory `feedback_integrate_into_core`)
drove every fix below. Full workspace green throughout.

### The report, as `.ys` (the 6 families)
`crates/spatio-flux/ys/{kinetics,diffusion,dfba,brownian,newtonian,comet}-section.ys`
— kinetics·line, diffusion·heatmap, dFBA(HiGHS)·line, brownian·scatter,
newtonian·scatter, comet·heatmap+particle-overlay. Each is just
`Simulate[proc: <sim>] → trace → traj.plot → figure.svg`.

### Core capabilities folded in (each surfaced by the challenge)
- **delta-traces kernel** — `prism_trace` (Trace = `initial` + `deltas`,
  `state(t)=fold(apply,…)`; Arrow IPC codec) + `prism_std::Simulate`, the
  engine-faithful runner (same-name wiring, folds the inner's REAL output schema).
  `RunProcess` is the integrator special case; `Simulate` is the general one.
- **`stream:` protocol** — a `.ys` proxied as a `Process` over the Arrow trace wire
  (parent `StreamProcess` ⇄ child `--serve-stream`), lock-step or bulk.
- **dynamic timesteps** — interval read from state (`overwrite[T]`); `MinimalGillespie`
  recreated (Rust test + `ys/gillespie.ys`).
- **particle DELTA-MOTION model** — movers emit Δposition/Δvelocity, `apply` SUMS
  (additive `Array[[2]]`), so Brownian + drift + interactions + boundary-correction
  SUPERPOSE. Retired the particle `Map[Any]` dodge (`motion_schema`; boundaries →
  corrective Δ). Proven: `motion_deltas_superpose`.
- **schema-driven views** (`prism_viz`) — line / heatmap (`View::Field`) / scatter
  (`View::Particles`) / comet overlay (`View::Spatial`), all place-graph SVG values;
  `first_field` digs nested ports.
- **retired 3 `Schema::Any` leaks** — diffusion output, particle-mover outputs, AND
  composite outputs (`composite_inner_schema` now types the interface ports →
  `Composite.outputs()` honest → composites-via-`Simulate` plot by schema).
- **codegen staleness → cargo-driven** (prism edits rebuild the cached runner).

### `.ys` FILE MODULES + unified modules
- `from <pkg>.<sub>.<file> import <Def>` → `<ys_root>/<sub>/<file>.ys` (package-rooted,
  recursive; the module's host imports + type vocabulary ride along). Parser accepts
  hyphenated/dotted paths (`spatio-flux.composites`). `resolve_file_modules` (cli.rs),
  `parse_module_path` (parse.rs).
- Sections DRYed into shared modules: `report/section.ys` (Trace/Figure/Plot/Output) +
  `composites/comets.ys` (the Comet composite), imported by all 6 sections.

### Next (half-done → finish, then new)
- **Report workflow DONE**: `report.ys` runs all 6 sections in one pass (`chrysalis run
  report.ys` → each self-outputs its SVG); `.ys` imports now pull TRANSITIVE value-deps,
  so `report.ys` is just the 6 section imports. REMAINING: a single COMBINED artifact
  (index / stacked SVG); `spatioflux_reference_demo`; parallel section execution (#27).
- **Complete `.ys` modules**: transitive value-deps DONE; REMAINING: the `import
  <module>` namespace forms (`comets.Comet`) + `import <module>.Def`; validate the
  package segment vs `project.ys`.
- **Retire `extern` (#14) is a refactor, not a cleanup**: woven through ~9 src files AND
  the old-import-form fixtures (`ys/culture.ys` + `ys/dish.ys` + `parse_import`, whose
  `diffusion_natives()` registry is keyed on the `Diffusion` extern). No `.ys` *uses* the
  `extern` declaration except that fixture. One focused pass.
- **SVG-as-place-graph everywhere (#12)**: retire `render_dot` (graphviz bigraph viz) →
  place-graph SVG; retire `render_timeseries_svg` (→ plot's `line_chart`).
- **Grow the command**: `chrysalis new <name>` scaffolder (#21); multi-line config
  unparse + emacs mode (#24); the `.ys` vs process-bigraph-JSON vs Python comparison (#23).
- **Then** the perf sweep (#20, AFTER features).

## Where we are (2026-05-22 — post runtime + toolchain session)

The language foundation (import model, `def` + first-class functions, process
contracts, units, prism-std as the std library) is in. THIS session built out the
**unified runtime core, the distribution boundary, and the `chrysalis`
toolchain**. Full workspace green. The CLI is now a real toolchain:

```sh
chrysalis run | check | bigraph  <file.ys>                  # compile+run / check / emit doc
chrysalis bigraph export f.ys out.json | import out.json    # import(export(f)) ≡ run(f)
chrysalis server [--port P]                                 # serve a Core over REST
chrysalis repl                                              # interactive homoiconic prompt
# `chrysalis compile` (codegen for NON-std packages) is the remaining piece — #10
```

### Landed this session
- **Unified `Core`** (`prism_bigraph::Core` = types + processes + methods +
  protocols) — replaces the engine's four separate registry fields; threaded
  through the engine AND every subengine (`Composite::from_config(&Core)`), so a
  `Custom`-typed / method-using / `rest:`-addressed process works inside a
  composite. `from_state(…, impl Into<Core>)` keeps registry-only callers
  compiling. (memory `project_core_unification`.)
- **Incremental steps** (`prism_bigraph::StepCache`) — a workflow's steps skip when
  their on-disk output is fresh; `--steps a,b` forces; staleness cascades the step
  DAG. Per-composite via `config.cache` (config-as-data, not a Rust handle). The
  substrate for the report-as-expirable-DAG.
- **The protocol boundary, made real** — `RestProcessServer` serves a `Core` over
  the rest-process wire protocol with full lifecycle cleanup (`end` deletes, no
  leaks); discovery now resolves `rest:` addresses in state (`address_class`), so a
  `rest:` process/composite is discovered + driven over HTTP indistinguishably from
  local (tests: `grow_divide_over_rest`).
- **Missing-reference validation** — a doc referencing a process the core can't
  build is rejected (`Core::missing_process_refs` / `Engine::check_references` /
  server 400), not silently run partial. The seed of the diagnostics goal.
- **CLI build-out** — `chrysalis server` (serves `std_core`); `chrysalis bigraph
  export/import` (the document is a runnable artifact; `runner::{document_of,
  run_document}`; `import(export(f)) ≡ run(f)`); `chrysalis repl` (Session eval,
  `:type`/`:env`/`:reset`, **rustyline with live syntax highlighting + completion +
  history hints + multi-line continuation**, redefinition overwrites).
- **spatio-flux as a package (slices 1–2)** — dep chain completed (spatio-flux now
  deps chrysalis + prism-std normally); `spatio_flux::prelude::{sf_core,
  sf_registry, sf_methods, sf_modules}` expose its natives as a Core / run-path
  packages; the `sf` bin proves the run path in-tree (the hand-written twin of what
  the codegen will emit).
- **Defaults** — confirmed `algebra::default` is ported + faithful; config/port
  defaults already parse; completing the surface is a task.

### Architecture orientation (read first)
- **Layering: prism-std → chrysalis → spatio-flux.** chrysalis NEVER deps
  spatio-flux; spatio-flux now deps chrysalis (the package half). memory
  `feedback_ys_layering`.
- **One `Core` threads everywhere** (engine + every subengine) — the root-cause fix
  for subengines losing types/methods/protocols. The schema layer is a closed
  algebra. The composite boundary is real + **protocol-mediated** (local / rest /
  parallel); cross-boundary config travels as DATA, never a Rust handle.
- **Single run path.** `chrysalis::runner::run(program, registry, methods, modules,
  time)` (compile+run) and `runner::run_document(doc, core, time)` (import).
  `run`, `server`, and `import` MUST share package resolution + `Core` assembly —
  the server is not a separate code path; the codegen (#10) builds that one path.
- **The remaining gap is the codegen (#10).** chrysalis (one binary) can't link a
  package's natives (HiGHS/rapier2d) and must never dep spatio-flux, so
  `chrysalis run <non-std>.ys` must **generate + cargo-build + cache a runner
  crate** that links the package, then run it (invisible to the user). The `sf` bin
  is the hand-written prototype; `sf_*`/`sf_core` are what the generated runner
  calls. See the codegen entry below for the `project.ys` manifest design.

## Next — the task tracker (durable; the harness task list is ephemeral)

✅🔧 **#10 — the codegen path (`chrysalis run` on NON-std packages) — MVP WORKING
(2026-05-23).** A `.ys` importing a non-std package (spatio-flux) can't run in the
fixed `chrysalis` binary (can't link HiGHS/rapier2d; must never dep spatio-flux),
so `chrysalis run` **codegens + builds + caches** a runner crate linking the
package. DONE + validated end-to-end:
- `crates/chrysalis/src/codegen.rs` — `find_manifest` (walk up for `project.ys`),
  render a runner crate (`Cargo.toml` path-deps chrysalis [baked
  `CARGO_MANIFEST_DIR`] + the package [manifest dir]; `main` = the shared CLI),
  build with `CARGO_TARGET_DIR` = the **workspace target** so compiled deps are
  reused, cache by content hash, exec.
- `crates/chrysalis/src/cli.rs` — `run_command(args, registry, methods, modules)`:
  the ONE run path, parameterized by packages. The chrysalis bin calls it with
  std; the generated runner calls it with the package's
  `prelude::{registry, methods, modules}` (added to spatio-flux as the codegen
  convention). **Single path** — std and packages run identical code.
- `crates/spatio-flux/project.ys` (`package spatio-flux`); `ys/diffusion.ys` +
  `ys/kinetics.ys` (fresh demos using the real `DiffusionAdvection` /
  `MonodKinetics`). `chrysalis run ys/diffusion.ys` builds the runner once (~34s,
  then **cached → 0.008s**); diffusion conserves mass (Σ=45), kinetics grows
  biomass 0.1→4.1 / consumes glucose. CLI input override (`--field '…'`) flows
  through the runner via compositional invocation. Tests: `codegen.rs` units
  (manifest parse, render).

*REMAINING:* (a) **dogfood the report** — port the ~18 `report.rs` sims
(`CANONICAL_ORDER`: dFBA family [HiGHS], particles [rapier2d], comets, newtonian)
to `.ys` and run each via the tool, validating every process family; (b) the
stale `culture.ys`/`dish.ys` (still `extern Diffusion`) → align to the package or
delete; (c) wire `compile`/`server`/`import` through the same codegen (`server` =
`run` ending in serve-over-REST — the runner gets a `server` subcommand);
(d) published-crate path deps (today path-deps assume the dev workspace).
*The hard part is done; the rest is breadth.*

⏳ **#6 — SBML→CRN importer / repressilator (the original goal).** Native subset
SBML/MathML reader (roxmltree + ~6-operator evaluator + one-time assignment-rule
param precompute) → a `CRN`; generalize the integrators to an arbitrary ODE RHS
(an `OdeSystem` trait both `MassActionNetwork` and an `SbmlOde` implement — the
repressilator BIOMD0000000012 is Hill-kinetics, not mass-action). Add
`CRN.from_sbml(path)` (or `from models import repressilator`). The model + Python
reference are at `../biocompose/biocompose/{models/BIOMD0000000012_url.xml,
experiments/copasi_tellurium_comparison.py, processes/}`. NOT a COPASI port —
this model has no events/rules/function-defs/piecewise. *Large.*

⏳ **#7 — dt-refinement convergence sweep.** Run the comparison at several
timesteps; show MSE → 0 as dt shrinks (the two methods converge to the same
target — divergence is pure discretization). Emit a convergence plot/CSV.
*Small — good warm-up.*

⏳ **#8 — comment-preserving parse/unparse.** Retain comments through
`parse→unparse` so all `.ys` regenerate losslessly. Leading block comments
(before a def) are easy; trailing/inline comments need node-level trivia (type
aliases, ports, body items). Then regenerate every hand-written `.ys` canonically
(~190 comment lines to preserve). *~half-day.*

⏳ **#12 — prism-svg: SVG as place-graph values.** Types for SVG nodes
(svg/g/rect/line/polyline/text); a `Figure` becomes a tree of typed nodes
(homoiconic); rendering = a `serialize`. Replaces the plotters string in
`prism-viz::render_timeseries_svg`. *Medium.*

🧹 **#13 — fully retire `extern`.** The import model (`from … import`) replaced it,
but the construct is still live: `Tok::Extern` / `Def::Extern` / `parse_extern_def`
with `compile`/`eval`/`check`/`unparse` arms, the grow-divide fixtures
(`src/fixtures/grow_divide*.rs`), and five tests (`parse_contract.rs`,
`contract_enforcement.rs`, `parse_use_import.rs`, `compile_imports.rs`,
`grow_divide_homoiconic.rs`). Migrate those to imports, then delete the token + AST
variant + every arm. The emacs mode already de-highlights `extern` (2026-05-22), so
the grammar is the current laggard. *Small-medium; mechanical, ~13 files.*

🦷 **#14 — real contract enforcement through `RunProcess`.** The flagship
`integrator-comparison.ys` only *declares* `fulfills` on the integrators; its
`Compare` takes plain `TimeSeries`, so nothing is rejected in that file (the teeth
live in the dedicated enforcement tests). To make its "a different target would not
compile" comment literally true: (a) add `:: DeterministicMassAction` to
`Compare`'s input ports, and (b) propagate the wrapped process's output contract
through `RunProcess`'s `timeseries` output — today `RunProcess` is a generic driver
that drops it, so `check::provider_contract` finds no contract at `rk4_traj`. The
real capability: a contract a process carries must survive a generic wrapper. Fix
in prism (`RunProcess` forwards its inner process's output contract) + a test that
a wrong-target inner process is rejected at the demanding port. *Medium.*

📦 **#15 — spatio-flux as a `.ys` project (slices 1–2 DONE; this arc IS #10's
slices).** `spatio_flux::prelude::{sf_core, sf_registry, sf_methods, sf_modules}`
expose its natives (FBA/diffusion/particles) as a Core / run-path packages — slices
1–2 ✓, the `sf` bin proves the run path in-tree. Remaining = **#10's slices 3–4**:
port the demos to `.ys` (the stale `dish.ys`/`culture.ys` first — they use
`extern Diffusion`/wrong names) and the codegen so `chrysalis run spatio-flux/ys/*`
works with no per-package bin. Layering: spatio-flux → chrysalis (the package half);
chrysalis never deps spatio-flux. *Large; gated on #10's codegen.*

🩺 **#16 — chrysalis diagnostics: comprehensible `.ys` error messages.** A
first-class diagnostics subsystem so EVERY way a `.ys` program can go wrong gives a
clear, located, actionable, *educational* error (rustc/Elm-quality), in
surface-language terms (not prism internals). Three parts: (a) a survey/taxonomy of
failure modes — parse errors, unknown imports, unknown process/type refs (the new
`document references unregistered process(es): …` is the first instance),
contract mismatches, wiring/port errors (unbound port, cross-wire type mismatch),
unbound vars, unit-dimension errors, schema-resolution failures, malformed
addresses; (b) a structured diagnostic (code + source span + message + why +
how-to-fix) threaded parse→eval→compile→check→run and surfaced by the CLI; (c) a
test suite of deliberately-wrong `.ys`, each asserting its exact diagnostic, so
error *quality* is regression-guarded. *Medium-large; high-DX-value; generalize
the missing-ref pattern.*

✅ **#17 — `chrysalis bigraph export`/`import` (DONE).**
`chrysalis bigraph export f.ys out.json` writes a process-bigraph `Document`
(**schema rendered alongside state** — else import loses `Array`/`Delta`);
`chrysalis bigraph import out.json [--time T]` reads it back and runs it. Built:
`runner::{document_of, to_document, run_document}` + the CLI dispatch; the
invariant **`import(export(f))` ≡ `run(f)`** is proven by
`chrysalis/tests/export_import.rs` (round-trip via the program's own core).
*Remaining:* a doc with USER-defined processes needs that program's core — the
CLI's `std_core` import runs std-self-contained docs and **clearly rejects**
others (the missing-ref diagnostic, e.g. "document references 5 unregistered
process(es): …"). Full self-containment — capturing user-process *bodies* as
homoiconic values, or naming packages — is the follow-on (ties to #10 codegen +
schema-as-state #20).

✅ **#18 — `chrysalis server` (DONE).** `chrysalis server [--port P]` serves the
std `Core` over the rest-process protocol (`prelude::std_core` — std factories +
the `Composite` factory + std methods; CLI parks the main thread). Lifecycle
cleanup + missing-ref rejection come from `RestProcessServer`. Proven by
`chrysalis/tests/server.rs` (serves a composite over REST with cleanup; rejects an
unknown-process doc) + the `grow_divide_over_rest` tests (a `rest:` node is
discovered + driven). Remaining polish: a named-package core (needs #10), graceful
shutdown. → **distributed simulation**: a `.ys` with `rest:` nodes pointing at
`chrysalis server`s.

✅ **#19 — `chrysalis repl` (DONE).** Session eval (defs/functions accumulate;
bindings + bare exprs print with inferred type), `:type`/`:env`/`:reset`/`:help`/
`:quit`, error-resilient, **redefinition overwrites** (a corrected `process`/`def`
wins, no dead duplicate). **rustyline** editor with **live syntax highlighting**
(ys tokenizer → ANSI by kind), **completion** (session names + keywords),
**history hints**, and **multi-line continuation** (write a multi-line `process` at
the prompt). `crates/chrysalis/src/repl.rs` (5 unit tests). *Follow-ons:*
`:load`/`:export`, stepping a workflow tick-by-tick over the step cache, completion
of methods/imported names, `reedline` if we want richer interaction.

🧬 **#20 — schema-as-state: a meta-schema; operate on the schema, reconcile state
to match.** Make the schema itself first-class operable STATE (a `Value`;
`schema_to_value`/`value_to_schema` already round-trip it), governed by a
META-SCHEMA so schema operations are themselves type-checked. The behavior:
mutating the schema (add a field, retype, add a `Custom` variant) **generates /
fills in the corresponding state to match** — new field → its `default`, removed →
dropped, retyped → `coerce`d — via the algebra (`default`/`coerce`/`apply`/
`reconcile`), **transactionally** (schema change + state reconciliation commit
atomically; state always satisfies its schema; roll back on failure). The
meta-circular completion of the schema-is-state principle (memories
`project_schema_as_state`, `project_schema_state_unity`); the substrate for
self-modifying simulations (a process that evolves its own type, illegal states
unrepresentable throughout). *Deep/foundational; a new named algebra op + laws,
not ad-hoc munging.*

🧩 **#21 — complete surface-language defaults.** `algebra::default` is ported +
faithful; config params + port decls already parse `name: T = default`. Extend:
type-field defaults (`type Cell = {mass: float = 1.0}`); schema-driven fill-in at
construction (a partial record/term gets its missing fields from the schema's
`default`); defaults through composite config / contract pins / the REPL; a
value-method to ask a schema for its default. Where a default exists, missing →
filled (not an error; coordinate with diagnostics #16 for genuinely-required
fields). The op is ported — this surfaces it through parse→eval→compile.
**INCLUDE the process `interval` wire default** (noted 2026-05-24): a process's
`interval` is an automatic wire to `%.interval` (its own local interval slot) —
inconsistent with other same-name auto-wires (which bind to the enclosing scope).
Make it an *explicit, overridable* default in the process schema (`interval`
defaults to `%.interval`), so a process can instead wire its interval to a
**shared clock** driving many processes at once. This rides the **dynamic-timestep
engine mechanism shipped 2026-05-24**: the scheduler re-reads `[name, "interval"]`
from state each tick (a step can rewrite it — a Gillespie τ), and nested
`Overwrite` outputs are now honored in `apply_reconciled` (the per-port schema is
nested back into the store, not dropped). Proven by the `MinimalGillespie` port in
`crates/prism-bigraph/tests/gillespie.rs` (event Process + interval Step → the
engine schedules by the dynamic τ = 1/(k·a)).
**Principled shape (2026-05-24):** there is no separate "default wire" mechanism —
a port's default *is* its type's `default`. The `Link`/edge `default` (upstream
`default_wires`) gives every port its default wiring, today uniformly same-name
`[port]`. Make `interval` a port of an **inner-wire type** whose `default` is
`%.port` (self-local) instead of `[port]` — so `default` *earns its keep* (the
self-wire is a `default` output, consulted per-port in the `Link` arm), `interval`
is "just another type" with no engine/chrysalis special-case, and an **override**
is an ordinary explicit wire (`~{interval: clock}` → a shared clock driving many
processes). The engine then reads the **resolved** `interval` input each tick
(self default → `[name,"interval"]`; override → the wired target), replacing the
current engine special-case (which only sees the self default).
**STATUS (2026-05-24):** the OVERRIDE half shipped — the engine reads `interval`
as the *resolved input* (overridable to a shared clock; `[name,"interval"]`
fallback), `.ys` gained `overwrite[Float]` (`SchemaExpr::Overwrite`), and
`crates/chrysalis/ys/gillespie.ys` + the Rust port (`prism-bigraph/tests/
gillespie.rs`) prove the dynamic τ = 1/(k·A) (full workspace green). REMAINING:
the **inner-wire type** producing `interval`'s `%.port` default wire via
`default_wires`, retiring the `[name,"interval"]` engine side-channel. *Medium.*

✍️ **#22 — enforce `::`=type / `:`=value / `fulfills`-contract** (decided
2026-05-23; chrysalis-design.md resolved decision #22). `::` ascribes a TYPE
everywhere (def + return, params `f(x :: T)`, ports `~{state :: map[float]}`,
config `[out :: Path]`, pattern sorts `?c :: Cell`); `:` binds a VALUE (map
entries, call args, wirings); the contract is the keyword **`fulfills`** on both
sides (`process Rk4 fulfills Det` / port `~{a :: TimeSeries fulfills Det}`,
replacing `a: T :: Contract`). Migration: parser flips type-position `:`→`::` +
port `:: C`→`fulfills C` (consider a lenient phase then tighten); unparser emits
the new forms; regenerate every `.ys` + GUIDE/design examples; update parse tests.
*Medium; do while the syntax is young.*

✅ **#23 — explicit composite-bridge syntax `@` + self → `%`** (DONE
2026-05-23; chrysalis-design.md resolved decision #23). A composite's
interface ports map to inner-state paths (the *bridge*); this was assumed
(name-inference) and is now *declarable*: `name :: Type [@ inner.path]
[fulfills C] [= default]` (e.g. `~{values :: Map[Float] @ fields.values}`).
Absent ⇒ name-inference, backward-compatible. `@` (formerly the self/here
sigil) is now the bridge operator; **self/here moved to `%`** (`->{env: %}`,
`%.volume`). Lexer `%`→`Tok::Percent`; `PortDecl.bridge: Option<Vec<Name>>`;
`eval::build_composite_outer` reads the explicit wire (else `[port]`);
unparser emits `@ path` (and `%` self), in parser order (bridge, `fulfills`,
`= default` — fixed a latent default-before-contract roundtrip bug). The 8
`@`-as-self uses across grow-divide / nuclear-shuttle `.ys` migrated to `%`;
emacs `ys-mode` sigils updated. Tests: `crates/chrysalis/tests/composite_bridge.rs`
(parse, name-infer fallback, unparse-fixpoint, **eval lands in `config.bridge`**).

🚧 **#24 — a file IS a composite (invocation = `Trace[In] → Trace[Out]`)** (batch
shipped + streaming DESIGNED 2026-05-23; chrysalis-design.md decision #24; harness
task #21). The rule: **a file's value is its last top-level term**; defs are
importable vocabulary; the *last* `composite`/`process`/`def` is the runnable
interface; a trailing headless `[config] ~{} ->{} ( body )` is anonymous-composite
sugar; no interfaced term ⇒ pure package. **THE MODEL** (designed this session): an
invocation is the morphism **`Trace[In] → Trace[Out]` seeded by config** — the CLI
is one *transport* of the same port boundary as `~{}`/`->{}` internally and `rest:`
over the network; a pipe `A.ys | B.ys` *is* `B ∘ A`. Config = the **t=0 seed** (set
once, picks the morphism); input = the **t>0 stream**. You don't need two kinds of
input: **config can only be seeded (flags); input can be seeded (flag=t=0) OR
driven (stdin=stream)** — time-invariance is what makes a value config. **Batch
("set a config and run") is the degenerate t=0→t=final collapse.**
DONE (the batch collapse): `Program::entry`; `compile` no-`main` fallback inlines a
bare-runnable composite; `runner::invoke` binds `[config]`+`~{inputs}` from flags
via `realize`, serializes the final-frame `->{outputs}` record to stdout /
`--out FILE`; `chrysalis run f.ys --<port> SOURCE`; `--time` = duration (engine
`interval` = per-step dt). Tests: `tests/file_entry.rs`, `tests/invoke.rs`.
**OPEN DECISIONS — RESOLVED:** wire = **delta-log/Arrow straight off** (not
JSON-first — streaming/distributed needs it regardless; JSONL stays a
human-readable projection); name mismatch = **`--map out=in`** rename sugar now,
**`--adapt adapter.ys`** (a real adapter composite) later; the stream **schema
header is MANDATORY** (check `refines` at connect, before data flows; mirrors
`document_of`); flag↔stdin = **flag is t=0**, a stream frame overrides from that
frame on (a stream frame naming a config param ⇒ diagnostic #16).
SLICES (each additive; batch preserved): (1) **output→trace** — per-tick output
delta-log (Arrow, schema header first), keep last-frame for a TTY (hooks
`Simulate`/`Trace[T]`, #25); (2) **input→trace** — feed stdin frames per tick,
flags the t=0 seed; (3) **schema header + `refines`** at connect; (4) **`--map`**
then **`--adapt`**; (5) **process/def entries** (uniform boundary; `def` = the
zero-time pure-function collapse). NOTE: legacy `.ys` ending in `env.run(t)`
(grow-divide-unbounded) never ran via the bin (`run` isn't a method); migrate to
end in the composite + `--time`.

🎨 **#25 — type-driven canonical outputs: `plot(schema, trace)` + report sections
as self-outputting workflows** (STARTED 2026-05-23). The crystallized vision (co-
designed): **a report section is a complete `.ys` workflow that derives its
canonical outputs from the composite's TYPES** — not a hand-written plot per sim.

THE UNIFICATION: *everything is a single-item type extended through time = a
**trace** (`list[state]`)*, and **`plot(schema, trace)` is a schema-driven
operation** (dispatches on schema like the algebra's `serialize`/`apply`/`divide`):
- scalars (`float`/`map[float]`/tree-of-scalars) → **line plot** (the integrator
  `TimeSeries` is just this case);
- a **field** (`array`/`map[array]`) → **heatmap / animation**;
- the **structure** (the state `Tree`) → a **"4D" bigraph-viz**: the structure
  shown at its change-points. The t=0 tree is just one snapshot; the structural
  trace is naturally stored as **diffs** (`algebra::diff`, reconstructed via
  `apply` — change-only), so structure is *not special* — it's another type
  whose trace is plotted.

**THE FULL SYNTHESIS → [`docs/delta-traces.md`](delta-traces.md)** (co-designed
2026-05-23): the entire system is a **procession of schema-deltas in time**
(event sourcing); `apply` = replay, `diff` = compute-delta; the **brutality
lattice** (additive → overwrite → `_remove`/`_add` structural); a fixed-shape
trace **is a tensor** `[time × dims]` (Arrow-friendly) that **structural deltas
reshape** → a *piecewise tensor*; value-deltas vs structural-deltas = the
bigraph's **two graphs** (state/tensor vs place/topology). Read that doc first.

DONE: `prism_viz::plot(schema, trace, title) -> Value` returns a **place-graph
SVG** (not a string): scalars → line chart, field → **animated** heatmap (SMIL
`<animate>` — the animation is place-graph data), else → note. `prism_viz::svg`
(SVG-as-place-graph + `to_svg` + `el/rect/line/text/polyline/animate`).
`RunProcess` emits the full `trace` carrying its element schema `Trace[T]` (no
infer). `Trace.plot` uses the carried `T`. `Figure` carries the place-graph under
`root`; `figure.svg` serializes via `to_svg`. `report-section.ys` — the reusable
`Section[sim, …]` template (run → trace → plot → self-output) renders a real line
chart end-to-end. Tested across prism-viz/prism-std/chrysalis.

REMAINING (ordered): (1) **`Simulate`** — the engine-faithful **event-source
runner** (vs `RunProcess`, matched to full-output integrators): feed the inner
its full state, `apply` its update via the state schema (kinetics deltas add,
diffusion fields replace — schema decides), capture `Trace[T]`. Storage: frames →
**delta-log + replay** → **hybrid keyframes + Arrow**. (2) **4D structural** plot
(bigraph-viz per `_add`/`_remove`) + expose a bigraph-viz writer to `.ys`.
(3) the `Section` template uses `Simulate` so **spatio-flux sections** run via the
codegen tool. (4) apply down `CANONICAL_ORDER`, validating each family.
*The abstraction is settled (see delta-traces.md); the rest is the runner + the
per-type renderers + the template + serialization.*

⚡ **#26 — performance sweep (do LAST, after the feature set).** The user will use
`.ys` as their primary language for ~everything, so the runtime must be performant
+ direct. Once features are in: establish benchmarks, profile the hot paths — eval
(ExprProcess body interpretation per tick; consider compiling bodies vs re-walking
the AST), the engine step loop (`advance_to_next_event`), `algebra`
apply/diff/reconcile + `Value` cloning, the delta-log/Arrow codec + streaming +
stream-protocol overhead, discovery — then optimize **without** sacrificing the
principled design (closed algebra, no half-measures). Honor the zero-cost
principle (check once, erase, run raw). Gated on features; not started. See memory
`project_ys_primary_language`.

🌐 **HORIZON — distributed bigraphs across HPC → [`docs/distributed-bigraphs.md`](distributed-bigraphs.md)**
(vision, 2026-05-23). prism IS already a distributed actor system (place graph =
partition, link graph = channels, composite = actor, delta = message, algebra =
sync, trace = durable log). Working backwards, the whole HPC picture reduces to
**ONE seam: the engine stepping a `rest`-addressed subengine like a local one**
(send input state, get output delta, `apply`) — then a chrysalis `composite`
reaches an HPC by flipping a subengine's address `local`→`rest`, *`.ys`
unchanged*. FIRST MOVE: split one composite across two REST processes (parent
engine ↔ `RestProcessServer` child) exchanging bridge deltas once per step (BSP
barrier of 2); builds on the REST server we shipped. Protocols are a **ladder**
(REST control-plane → Arrow Flight data-plane → MPI/UCX collectives, where
`all-reduce` = commutative-delta `merge`), pluggable behind the protocol seam.
The parallelize-vs-synchronize lines are schema-derived (commutative deltas →
async; non-commutative → barrier). Far horizon; the linchpin is small.

**Suggested order.** **Immediate — the delta-traces kernel** (`Simulate` +
delta-log/Arrow codec, prism-side): the linchpin just designed — the start of #25
AND the prerequisite for #24 streaming (we chose straight-to-Arrow, no JSON
half-measure; see chrysalis-design.md decision #24 + docs/delta-traces.md). Once it
lands the work **forks in parallel**: **#24 streaming** (output→trace, input→trace,
schema-header/`refines`) ∥ **#25 viz** (plot per-type, 4D structural, the `Section`
template). **#24's surface** (`--map`/`--adapt`, process/def entries) needs nothing
from the kernel — anytime. The other marquee is **#10 codegen** — it unblocks #15
(spatio-flux as `.ys`), full export/import self-containment, and `server`/`import`
on packages (the single path). Then **#16 diagnostics → #21 defaults → #6 SBML (the
science headline) → #20 schema-as-state**. Quick wins anytime: **#7** dt-sweep,
**#13** retire-extern, **#8** comment retention, **#12** prism-svg (folds into the
kernel's svg-as-place-graph), **#14** real-enforcement-through-RunProcess. (Done
this session: Core unification, StepCache, rest server + discovery, per-composite
cache, missing-ref, **#17/#18/#19** CLI + REPL.)

## Long-run milestones (beyond the tracker)
- **First-class `Custom` types** — `type Name = <repr> with { op = …,
  method(args) = … }` (the algebraic-effects handler model): make a `Custom`
  indistinguishable from a built-in sort by threading a registry through the
  algebra ops. `CRN`/`Path` are synthetic types today; this makes them truly
  first-class. The contract layer + import-types are the substrate.
- **KISAO export (contract rung 3)** — contract → KISAO term mapping; OMEX/SED-ML
  round-trip. "Tell us your target semantics and we'll tell you the admissible
  methods" — a contribution back to the standard, not just consumption.
- **dFBA cross-target bridge** — comparing a kinetic integrator vs dFBA needs an
  explicit, *named* cross-target contract (the contract forces you to name the
  approximation rather than silently MSE-ing incomparable things).
- **More demos as `.ys`** — convert remaining examples; optional rung-2 real
  COPASI/Tellurium via a BioSimulators subprocess bridge (interop, not new
  contract content).
- **Tier-2: evolving M/R** — process bodies as first-class values (after
  Fontana's AlChemy); schema-driven typed construction so illegal programs are
  unrepresentable.
- **A packages ecosystem** — #15 makes spatio-flux the first non-std `.ys`
  package; generalize so any native crate ships importable `.ys` modules (a
  `project.ys` manifest + a resolver/registry), plus a fundamental-type catalog.

## Build / test loop
Build is slow (links many binaries). Use `cargo check -p <crate>` for "does it
build", `cargo test -p <crate> --test <name>` for the touched area, full
`cargo test --workspace` only at checkpoints. Wrap runs in `timeout` — prism
tests run <1s, so a slow run means an infinite loop / structural explosion, not
patience. No `.cargo/config.toml` linker override (rustflags change ⇒ whole-tree
rebuild). See memories `feedback_test_timeout`, `feedback_no_sleep_polling`.

## Pointers
- `README.md` — the port + chrysalis + commands.
- `crates/chrysalis/ys/GUIDE.md` — how to write `.ys` (the tutorial).
- `docs/prism-architecture.md`, `docs/schema-algebra.md`,
  `docs/chrysalis-design.md`, `docs/process-contracts.md`.
- Memories: `feedback_ys_layering` (the architecture), `feedback_chrysalis_thin_layer`,
  `feedback_no_half_measures`, `feedback_consult_upstream`, `feedback_test_timeout`.

## Historical (done — provenance in git)
- The fresh-core **schema-algebra rebuild** — closure achieved; `prism_schema::
  algebra` is the single door; 13 laws + closure-guard green.
- The **engine execution-correctness arc** — invoke/apply separation, `reconcile`
  wired into apply, dependency-layered step firing, schema always inferred.
- The **process-contract demo** — built and now runs via `chrysalis run`.
