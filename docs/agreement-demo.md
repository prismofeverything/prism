# The agreement demo — three notions of "agree"

`crates/chrysalis/ys/agreement.ys` is the flagship process-contract
demonstration. It answers, operationally, a question SED-ML / KISAO leave open:
**when do two simulators agree?**

## The problem

Matching port *shapes* ("counts in, a time series out") does not make two
processes comparable. A deterministic mass-action ODE, an exact-stochastic (CME)
trajectory, and a well-mixed PDE collapse can all share that shape while
approximating *different mathematical objects*. Diffing them is a category error
the interface cannot see — and it is the gap underneath SED-ML/KISAO
"reproducibility" (see [`process-contracts.md`](process-contracts.md)).

## The contract layer

A `ProcessContract` states a process's *meaning*, on four axes — `target`
(which object is approximated), `method` (the KISAO-style realization), `claims`
(the sense in which two realizations agree), `advance` (time model). Two
processes are substitutable iff one contract `refines` the other, which is just
the schema join `resolve(a,b)==a` — **no new algebra op** (see
[`process-contracts-implementation.md`](process-contracts-implementation.md)).

The pivotal move: **the `claims` axis selects the comparison metric.**

## The demo — one model, two targets

`A → B` at `k = 0.7`, from 100 molecules of A. Two lanes, two contracts:

| Lane | Contract | Metric (gated by the contract) | Verdict |
|---|---|---|---|
| deterministic | `DeterministicMassAction` (claims: `Deterministic`) | trajectory MSE (`Compare`) | RK4 vs Euler **agree pathwise** |
| stochastic | `ExactCME` (claims: `Distributional`) | distributional distance (`DistCompare`) | ensembles **agree distributionally**; single paths diverge |

Each comparison step *demands* its contract (`a :: C`), so a metric physically
cannot touch the wrong data. **The teeth:** wiring an `ExactCME` path into the
deterministic `Compare` (or a deterministic trajectory into `DistCompare`) is a
**compile error** — proven in
`crates/chrysalis/tests/contract_enforcement.rs`.

## What it produces (verified)

```
deterministic-mse.csv         A,B = 3.59   # ~2% RMS of 100 → pathwise agreement (pure discretization)
distributional-distance.csv   A,B = 1.63   # z < 2 → distributional agreement
deterministic-overlay.svg                  # RK4 vs Euler: two coincident smooth curves
stochastic-overlay.svg                     # SSA seed 1 vs 2: two divergent jagged paths
```

The contrast between the two SVGs *is* the pitch: deterministic methods coincide
(comparing trajectories is meaningful), stochastic paths diverge (comparing
trajectories is meaningless — only the distribution agrees). The SSA ensemble
mean tracks the very ODE the deterministic lane integrates: the CME's mean-field
limit *is* mass-action — but that connection lives at the distributional level,
which the contract names and the compiler enforces.

## Implementation map

- **The algebra** — `refines` (`prism-schema/src/resolve.rs`); contracts lower to
  a `Tree` of nominal axes + an ordered `Enum`-downset for `claims`
  (`chrysalis/src/schema.rs`).
- **The library query** — `fulfillers(C)` over the program's `fulfills`-annotated
  definers (`chrysalis/src/contract.rs`).
- **Contracts survive wrappers** — `RunProcess` forwards its inner process's
  contract via a declarative rule in `chrysalis/src/check.rs` (`CONTRACT_FORWARDS`).
- **The math** — deterministic integrators, the Gillespie SSA + `ensemble` +
  `distributional_distance`, all over one `MassActionNetwork`
  (`prism-std/src/mass_action.rs`).
- **The surface** — `ys/agreement.ys` (unified) plus the focused
  `ys/integrator-comparison.ys` (deterministic lane) and `ys/cme-gillespie.ys`
  (stochastic lane).

## Not yet (informed by, not constrained by)

- **Auto-fan-out** — a comprehension over `fulfillers(C)` that *generates* the
  `RunProcess` children ("run every fulfiller of C") instead of hand-wiring
  them, plus a dt-refinement sweep showing the deterministic MSE → 0 as the
  timestep shrinks.
- **KISAO / SED-ML / OMEX export** (canonical task #33,
  [`kisao-export.md`](kisao-export.md)) — naming these contracts in the
  standard's vocabulary, the deliberate *export* layer. The legacy framing
  informs the design; it does not constrain the core.
