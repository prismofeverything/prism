# Prism

Rust workspace implementing Milner-style process bigraphs for
composable multi-scale simulation. Cellular biology is the driving
application domain.

## Where to look

- **`docs/prism-architecture.md`** — crate roles, the `Process` /
  `Step` / `BRS` / `Engine` primitives, the `discover_processes`
  reflection mechanism. Read this first.
- **`docs/chrysalis-design.md`** — design for **chrysalis**, the
  surface PL compiling to this runtime. Homoiconicity goal, the
  bigraph atoms to canonize, the implied-assembly decision (trees
  are bigraphs sugared), value methods via `MethodRegistry`, and
  the tiered benchmark examples: tier 1 (grow/divide, MAPK, static
  M/R) for patterns + reactions as first-class values; tier 2
  (evolving M/R, after Fontana's AlChemy) for process bodies as
  first-class values with schema-driven typed construction (illegal
  programs are unrepresentable, not "caught at runtime").
- **`crates/prism-mapk/src/rules.rs`** — canonical example of
  hand-written reaction rules in the Rust API. Source for chrysalis
  benchmark #2.
- **`chrysalis-example`** (file at repo root) — current grow/divide
  sketch in chrysalis surface syntax. Will be re-expressed in
  fully-homoiconic form (parent-installed `MassThresholdDivide`
  reaction) as part of the three-example acceptance test.

## Conventions

- Work from the workspace root (`/home/pattern/code/prism/`) — cargo
  expects it.
- prism is load-bearing infrastructure; design **on top** of it, not
  around it. Reflection via `discover_processes` is the substrate
  feature that makes higher-order computation already feasible.
- Bigraph state is `Value::Tree` (named parallel-of-nested) or
  `Value::List` (anonymous parallel). The tree-of-maps has an exact
  bigraph reading — see docs/chrysalis-design.md.
- For chrysalis design changes, update `docs/chrysalis-design.md` in
  the same commit as code changes.
