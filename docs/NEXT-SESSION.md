# Next session — launch prompt & plan

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
fields). The op is ported — this surfaces it through parse→eval→compile. *Medium.*

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

🚧 **#24 — a file IS a composite (compositional invocation)** (in progress
2026-05-23; chrysalis-design.md resolved decision #24; harness task #21). The
rule: **a file's value is its last top-level term** (like a body block's last
`|`-less line). Defs are importable vocabulary; the *last* `composite`/`process`/
`def` is the file's runnable interface; a trailing headless `[config] ~{} ->{}
( body )` is anonymous-composite sugar; no interfaced term ⇒ pure package. DONE
(composite entries): `Program::entry`; `compile` no-`main` fallback inlines a
*bare-runnable* composite; `runner::invoke` binds `[config]`+`~{inputs}` from the
command line with **explicit connectors** (bare=literal, `file:PATH`, `-`/stdin,
`stream:` reserved, `lit:` force) — the schema only drives `realize`, never the
source (the type-based file/literal guess was rejected); `->{outputs}`
`serialize`d to one JSON record on stdout / `--out FILE`; `chrysalis run f.ys
--<port> SOURCE` wired. Run duration is `--time` (the engine's `interval` is the
per-step dt). Round-trips (a run's output is another run's input). Tests:
`tests/file_entry.rs`, `tests/invoke.rs` + manual CLI (literal/file/stdin/--out/
error). NEXT SLICES: `process` + `def`-function entries (function = pure CLI
transform); the headless top-level form (parser). The codec already existed
(`serialize`/`realize`/`deserialize`, law `deserialize∘serialize≡id`); rich types
carry their own (`CRN`↔SBML, `Figure`↔SVG). LATER: streaming ports = a channel on
the `Emitter`/`Output` substrate (batch first). NOTE: legacy `.ys` ending in
`env.run(t)` (e.g. grow-divide-unbounded) never ran via the bin (`run` isn't a
method); migrate them to end in the composite + `--time`.

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

DONE: `crates/prism-viz/src/plot.rs` — `plot(schema, trace, title) -> svg` +
`characteristic(schema) -> View{Lines,Field,Snapshot}`, dispatching over the
existing prism-viz renderers (`render_timeseries_svg`; snapshot for field/tree as
a placeholder). Tested (schema→view; scalar trace→line SVG; nested scalars
flatten to `substrates.glucose`).

REMAINING (ordered): (1) **trace capture** — extend `RunProcess` (today: flat
`map[float]`→scalar columns only) to emit the **full state trace** ("arbitrary
outputs"); store as diffs. (2) a real **field heatmap/animation** renderer
(port report.rs's plotters heatmap into prism-viz) + the **4D structural** plot
(bigraph-viz at change-points). (3) **expose** `plot` + a bigraph-viz writer to
`.ys` (method/step). (4) the **`Section` template** — a composite param'd by
`(sim, title, description, out)` that runs the sim, calls `plot(output_schema,
trace)` per output port (schema-driven), and self-outputs to `outputs/<name>/`.
(5) apply down `CANONICAL_ORDER`, validating each family via the codegen tool.
*The abstraction (`plot(schema,trace)`) is settled + seeded; the rest is the
trace plumbing + the per-type renderers + the template.*

**Suggested order:** **#10 codegen** is the architecture marquee — it unblocks #15
(spatio-flux as `.ys`), full export/import self-containment, and `server`/`import`
on packages (the single path). Then **#16 diagnostics → #21 defaults → #6 SBML
(the science headline) → #20 schema-as-state**. Quick wins anytime: **#7** dt-sweep,
**#13** retire-extern, **#8** comment retention, **#12** prism-svg, **#14** flagship
real-enforcement-through-RunProcess. (Done this session: Core unification, StepCache,
rest server + discovery, per-composite cache, missing-ref, **#17/#18/#19** CLI +
REPL.)

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
