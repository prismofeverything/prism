# Next session — launch prompt & plan

## Where we are (2026-05-22, post language-foundation session)

The chrysalis **language foundation + the `chrysalis` build tool** are in, and the
architecture is realigned to **prism-std → chrysalis → spatio-flux**. Full
workspace green. The process-contract integrator-comparison demo runs end to end:

```sh
cargo run -p chrysalis --bin chrysalis -- run crates/chrysalis/ys/integrator-comparison.ys
# → outputs/integrator-comparison/{Rk4,ForwardEuler,mse}.csv + overlay.svg
```

### Landed this session
- **Import model** — `from <module> import <names>` (replaces `extern`): a
  `ModuleRegistry` the host populates; whole processes (`from core import
  RunProcess`), objects (`from integrators import rk4` → `rk4.method(…)`), and
  types (`from chem import CRN`) importable. Entry: `compile_with_modules`.
- **`def` (required) + first-class functions** — `def name :: Type = expr`,
  `def f(args) = body`; functions are values (pass/return/store) via `Expr::Call`.
  Bare `name = …` is now an error.
- **Process contracts** — `contract` / `fulfills` / `::`; substitutability =
  `algebra::refines` (no new op). The demo enforces it at compile time.
- **Self-outputting workflows** — effectful `Output` step (`->{}`), `Path` type,
  `/` path-join, `.name`, `.csv`/`.svg` writer methods; the `.ys` emits its own
  artifacts (no Rust harness).
- **Canonical formatting** — `fulfills` parses either side of the interface; the
  unparser emits canonical multi-line and no longer drops contracts.
- **`prism-std`** — prism's native standard library (`RunProcess`, the
  mass-action integrators, `CRN`, `TimeSeries` + methods); the SVG plotter →
  `prism-viz`. chrysalis bundles it via `chrysalis::prelude::std_{registry,
  methods,modules}`.
- **The `chrysalis` build tool** (`crates/chrysalis/src/bin/chrysalis.rs`) —
  `run` / `check` / `bigraph` live (std path, in-process via the prelude +
  `chrysalis::runner::run`).
- **Docs** — top-level `README.md`; `crates/chrysalis/ys/{README,GUIDE}.md`
  (GUIDE = how to write `.ys`); refreshed emacs README.

### Architecture orientation (read first)
- **Layering is prism-std → chrysalis → spatio-flux.** chrysalis bundles prism-std
  as its std library and **never deps spatio-flux**. spatio-flux is a downstream
  demo package (FBA/diffusion/particles). See memory `feedback_ys_layering`.
- **The run command is `chrysalis::runner::run(program, registry, methods,
  modules, time)`** — parameterized by the host's packages; never reimplemented.
- **`chrysalis` IS the build tool.** std `.ys` run in-process; non-std need the
  codegen path (#10).
- The schema layer is a closed algebra (`prism_schema::algebra`, single door,
  laws + closure-guard green) and the engine run-loop correctness arc is done
  (both historical now — see bottom).

## Next — the task tracker (durable; the harness task list is ephemeral)

🔧 **#10 — finish the `chrysalis` build tool's codegen path.** `run`/`check`/
`bigraph` are live for std `.ys`. Remaining: `chrysalis compile` + running `.ys`
that import **non-std** packages (e.g. spatio-flux). Model (rust-script style):
parse → resolve `from X import Y` to crates via a **module→crate manifest** →
generate a runner crate (deps = those crates; main = assemble registry + run) →
`cargo build` (cache by content hash) → run. This is also the **packages**
milestone. *Medium-large.*

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

📦 **#15 — convert spatio-flux to a `.ys` project (the downstream-package
endgame).** spatio-flux is Rust today; the goal is `chrysalis run
spatio-flux/ys/*.ys` over its native solvers (FBA/HiGHS, diffusion,
particles/rapier2d). This is the *consumer* that proves #10's codegen path and
forces the **project manifest**:
- a `project.ys` (uv-style) at the package root declaring the package's native
  crate(s) and the module→crate map (`from fba import …` → spatio-flux's FBA), so
  `chrysalis run` from a project root pulls the local package in as importable
  modules;
- `chrysalis compile` generates a runner crate (deps = those crates), builds it
  (cached by content hash), and runs — #10's rust-script model;
- port spatio-flux's processes to importable native modules + thin `.ys` wrappers
  (mirrors how prism-std exposes `core`/`integrators`/`chem`/`io`);
- move the spatio-flux demos to `spatio-flux/ys/*.ys`, run via `chrysalis`.
Layering stays prism-std → chrysalis → spatio-flux; the flip is that spatio-flux's
*demos* become `.ys` driven by the tool, not Rust examples. *Large; gated on #10 —
the marquee proof that build tool + import model + packages all compose.*

**Suggested order:** **#13 (quick cleanup) → #7 (quick win) → #14 (flagship teeth)
→ #6 (the headline) → #10 codegen → #15 (spatio-flux `.ys`, gated on #10) → #8 →
#12** — #6 is the science marquee, #15 the architecture marquee; both move whenever
there's appetite.

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
