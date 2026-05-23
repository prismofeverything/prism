Each line of the contract has a prism home:

| chrysalis form | prism mechanism |
|---|---|
| `contract DeterministicMassAction (…)` | a contract schema: today it lowers to a **`Tree` of nominal axes** (`chrysalis::schema::contract_ref_schema`); a first-class **`Custom`** type is the milestone (see NEXT-SESSION) |
| `fulfills C[…]` on a process | rides on the producer's **output-port schema**; `check::provider_contract` reads it back at wiring time |
| `:: C` on a port | the port **demands** a contract; substitutability = `algebra::refines` = `resolve(A,B)==A` (no new op), enforced by `check::check_contract` |
| `from integrators import rk4, euler` → `process Rk4 … ( rk4.integrate(…) )` | **prism-std** natives (`prism_std::integrator`) imported as a module and wrapped by a `.ys` `process` with its own ports + `fulfills` |
| `from core import RunProcess` | the native time-stepper (`prism_std::RunProcess`) imported wholesale — drives a `process` over `[0, runtime]` into a `TimeSeries` |
| `species_mse`, `overlay`, `.csv`, `.svg` | **`MethodRegistry`** value-methods on `TimeSeries` / `Figure` / `map` (analysis + effects); the `render_timeseries_svg` plotter lives in **`prism-viz`** |
| `composite … (DAG)` | a **`Composite` of `Step`s** — `RunProcess` drives each integrator, `Compare` / `Plot` / `Output` analyse; edges inferred from data deps and fired in dependency order |
| `def network :: CRN = {…}`, units | typed bindings; dimensioned ports the `target` *implies* (MassActionODE ⇒ intensive concentrations; CME ⇒ extensive counts), checked at compile and erased before run — contract and interface linked, not independent |

### Substitutability needs no new op — it is the existing join (RESOLVED)

A contract *is* a `Schema`: **one record whose fields are the four axes** —
not four loose top-level types, but four *fields of one type* (a `Tree`
envelope, as the chrysalis lowering emits, or a named `Custom` for a nominal
handle — `refines` works on either; both are covered by the tests). The
record (product) structure is load-bearing: it is exactly what lets "same
target, different method" be expressible; a single flat tag could not. Each
axis is the sort that gives it the right order under `resolve`:

| axis | sort | why |
|---|---|---|
| target, advance, method | parameter-free `Custom` (nominal) | `resolve` keeps a same-named `Custom` and prefers the update for a mismatched one → *nominal equality* (same ⇒ itself, different ⇒ not) |
| claims | `Enum` holding its **downset** (the claim + everything weaker it implies) | the existing Enum-union `resolve` makes a stronger claim's downset ⊇ a weaker one's → the refinement chain falls out |

"A (a fulfiller) may fill a slot demanding B" is the lattice order
`A ⊒ B`, which the join characterizes as the **derived predicate**
`refines(A, B) := resolve(A, B) == A`. It adds nothing to the algebra —
it is `resolve` + `==`. A demanded contract leaves `method` open (absent),
so any concrete method unions in and the join still equals the fulfiller;
a different *target* (FBA) makes the join differ, so the wire is rejected.
Proven on the real algebra in
[`prism-schema/tests/contract_substitutability.rs`](../crates/prism-schema/tests/contract_substitutability.rs)
(integrators accepted; FBA / wrong-target-right-label / underspecified /
pinned-method-mismatch rejected; claim chain directional).

The only genuinely new capability is **`admits`** (which methods are valid
realizations of a target) — and that is a `value-method` on the target
type (`MethodRegistry`), not a schema op. It isn't needed for the demo
(the comparison only checks fulfiller ⊒ demanded, not method↔target
admissibility).

**Status (2026-05-21): built and green.**
`prism_schema::algebra::refines` (derived, `resolve==`) +
`chrysalis::schema::contract_ref_schema` (lowering to a `Tree` of nominal
axes) + `chrysalis::check::check_contract` (enforcement: a producer wired
into a contract-demanding port must `refines` it, resolved through producer
paths like `r.trajectory`). Proven by
[`prism-schema/tests/contract_substitutability.rs`](../crates/prism-schema/tests/contract_substitutability.rs)
(7 cases) and
[`chrysalis/tests/contract_enforcement.rs`](../crates/chrysalis/tests/contract_enforcement.rs)
(fulfilling wire clean; wrong-target + uncontracted rejected). The parser
(`contract …`, `port :: C`, `fulfills C[…]`) compiles `.ys` source to this AST
— proven by
[`chrysalis/tests/parse_contract.rs`](../crates/chrysalis/tests/parse_contract.rs)
(parse → enforce; the wrong-target wire is a compile error). Build-sequence
step 3 is complete.

The teeth — what biocompose structurally cannot express:

```
# A comparison node DEMANDS the contract on its inputs (`:: C` on a port):
step Compare ~{a: TimeSeries :: DeterministicMassAction} ->{mse: map[float]} ( … )

# Rk4 fulfills DeterministicMassAction → the wire is accepted.
composite Good ( r = RunProcess[proc: Rk4[network: network], …]      | Compare ~{a: r.timeseries} )

# Gillespie fulfills StochasticCME (target = the CME, a DIFFERENT object) → rejected:
composite Bad  ( g = RunProcess[proc: Gillespie[network: network], …] | Compare ~{a: g.timeseries} )
#  ^ compile error:
#    contract mismatch: source fulfills `StochasticCME` but port demands `DeterministicMassAction`
#    Comparing across targets must NAME the bridge contract (a DeterministicLimit of the CME),
#    not silently MSE two incomparable trajectories.
```

These `Good` / `Bad` composites are the shape of
[`chrysalis/tests/parse_contract.rs`](../crates/chrysalis/tests/parse_contract.rs)'s
cases — the wrong-target wire is a compile error.
Substitutability-by-construction is the same move as tier-2's "illegal
programs unrepresentable," applied to *workflow composition* rather than
to evolving M/R bodies.

## Demonstration: the COPASI/Tellurium comparison (biocompose)

`biocompose` (vivarium-collective) is the Python process-bigraph
version of exactly this comparison — `CopasiUTCStep` ‖ `TelluriumUTCStep`
→ `CompareResults` computing `species_mse` over a shared model
(`copasi_tellurium_comparison.json`). It stalled at the **interface**:
the steps have typed I/O that wire up and run, but nothing certifies the
two trajectories are even the same mathematical object — `CompareResults`
will MSE any two series of matching shape. The chrysalis version adds
the missing layer: the comparison node is *typed by the shared
contract*, so an illegitimate comparison does not compile and the MSE
has a warrant.

### Rung 1 — native integrators (the buildable demo)

The two `DeterministicMassAction` fulfillers are two ODE integrators we
write natively (prism has none for chemical kinetics — its
`Deterministic` BRS mode is a rule-rewrite discipline, not numerical
integration; Gillespie/Stochastic target the CME; FBA/dFBA are
constraint-based). `Rk4` and `ForwardEuler` integrate
`dxᵢ/dt = Σ_r (νᵢ,prod − νᵢ,reac)·k_r·∏ⱼ xⱼ^{νⱼ,reac}` over the **same
mass-action network** — the network is the shared *model*; the two
processes are different *methods* realizing the same *target*. On a
coarse `dt` their trajectories diverge measurably (Euler lags); refine
`dt` and the MSE → 0 — "same target, different method, legitimate
comparison, method-induced divergence." Unit test: A→B at rate k has the
closed form `x_A(t) = x_A0·e^{−kt}`; verify each integrator against it,
then MSE the two.

Self-contained — no external simulators, no SBML parser — so it builds
the full contract machinery (including the negative test below) with
zero heavyweight dependencies. The contract layer is built in full;
nothing here is a shortcut.

### Where HiGHS / FBA fits — a different contract (the negative test)

prism's HiGHS solver is **Flux Balance Analysis**: `max cᵀv s.t.
S·v = 0, lb ≤ v ≤ ub` (`spatio-flux/processes/fba.rs`). That is a
*steady-state constraint optimization* — it returns a flux distribution
at steady state, not a time-course; its target semantics is
**constraint-based steady-state flux**, not the deterministic
mass-action ODE COPASI/Tellurium integrate. They do not even produce the
same kind of output (a flux vector vs. a trajectory x(t)). So HiGHS
*cannot* be one of the two `DeterministicMassAction` fulfillers — wiring
it into `Compare[under: DeterministicMassAction]` is precisely the
category error the contract layer is built to reject.

That makes FBA the **ideal negative test, using existing prism code**:
it fulfills a *different* contract (`ConstraintBasedFlux`: target =
steady-state flux polytope, claims = optimal-flux, advance =
steady-state-snapshot), so the compiler rejects it from the time-course
comparison — demonstrating the contract's teeth without a throwaway
process.

`dFBA` (`processes/dfba.rs`) is the interesting **bridge** case: it
*does* produce a time-course (re-solving the LP each tick with
Michaelis-Menten uptake bounds), but its target is dynamic
*quasi-steady-state* constraint flux, not the kinetic mass-action ODE.
Comparing dFBA against a kinetic integrator therefore requires an
**explicit cross-target bridge** — the quasi-steady-state approximation
with its regime of validity stated — a real systems-biology modeling
question (kinetic vs. constraint-based models), and a sharp illustration
that the contract *forces you to name* the approximation relating two
targets rather than silently MSE-ing them.

### Rung 2 — real COPASI/Tellurium (optional, deferred)

A BioSimulators bridge process (subprocess to the real tools — the
BioModels paper's own execution model — or to the existing biocompose
Python) swaps the native integrators for actual COPASI/Tellurium. It
proves real interop and reproduces biocompose exactly, but proves
nothing *new about contracts*, so it is **not required for the
demonstration** and is deferred.

### Rung 3 — KISAO export (contribution back)

Each fulfiller's `method:` maps to a KISAO term (`ForwardEuler` →
KISAO:0000030; explicit RK4 → KISAO:0000032; Gillespie-direct →
KISAO:0000029 — verify codes against the KiSAO source). A contract →
KISAO exporter lets a prism workflow round-trip to an OMEX/SED-ML
archive. The point: **KISAO is an export target, not the source of
truth** — internally prism keeps the target/method layering KISAO lacks
and emits KISAO only at the interop boundary. This is the "tell us your
target semantics and we'll tell you which KISAO methods are admissible
realizations" contribution back to the standard.

## Build sequence (rung 1)

1. **Native integrators** — *DONE; now in `prism-std`.* The mass-action
   integrators (`prism_std::integrator`): `rk4` / `euler` over a `CRN` reaction
   network, producing a `TimeSeries`, each tested against the A→B analytic
   solution. chrysalis imports them as the `integrators` module; a `.ys`
   `process` wraps each with its own ports + `fulfills`. *(This substrate is what
   the contract describes.)*
2. **Analysis methods** — *DONE.* `MethodRegistry` value-methods
   `TimeSeries::species_mse` / `overlay` (plus the `map` / `Figure` `.csv` /
   `.svg` effects); the `render_timeseries_svg` plotter now lives in `prism-viz`.
3. **Contract surface** — *DONE (2026-05-21).* AST (`Def::Contract`,
   `ContractRef`, `PortDecl.contract`), lowering (`schema::contract_ref_schema`
   → a `Tree` of nominal axes), enforcement (`check::check_contract`: a producer
   wired into a contract-demanding port must `refines` it), and the **parser**
   (`contract Name (…)`, `port :: C`, `fulfills C[…]` sugar → `::` on outputs).
   All on top of `prism_schema::algebra::refines` (= `resolve==`, no new op).
   Proven by `chrysalis/tests/contract_enforcement.rs` (AST fixtures) and
   `chrysalis/tests/parse_contract.rs` (parse `.ys` source → enforce; the
   wrong-target wire is a compile error).
4. **Workflow** — *DONE (2026-05-22).*
   [`crates/chrysalis/ys/integrator-comparison.ys`](../crates/chrysalis/ys/integrator-comparison.ys)
   runs end to end over chrysalis's bundled std library (prism-std) — no
   downstream package is involved, so the test lives in **chrysalis**
   ([`crates/chrysalis/tests/integrator_comparison.rs`](../crates/chrysalis/tests/integrator_comparison.rs)):
   parse → enforce contracts → `compile_with_modules` (the std registry + the
   injected `TimeSeries` methods + the `core` / `integrators` / `chem` / `io`
   modules) → `chrysalis::runner::run` → MSE + overlay `Figure`. `RunProcess`
   drives each imported integrator over `[0, runtime]`; `Compare` / `Plot` are
   ys-native (their bodies call the registered methods `a.species_mse(b)` /
   `a.overlay(b, …)`); `Output` writes the artifacts itself. Contracts flow
   through wirings to demanding slots (`check::collect_slot_contracts`). Steps
   1–4 complete; only rung 3 remains.

   **Running it.** The workflow is self-outputting — its `Output` step writes the
   files, so there is no Rust harness:

   ```sh
   cargo run -p chrysalis --bin chrysalis -- run crates/chrysalis/ys/integrator-comparison.ys
   # or, once installed (`cargo install --path crates/chrysalis`):
   #   chrysalis run crates/chrysalis/ys/integrator-comparison.ys
   ```

   writes to `outputs/integrator-comparison/`: `Rk4.csv` / `ForwardEuler.csv`
   (the two trajectories, each named from its `TimeSeries`), `mse.csv`
   (per-species MSE), and `overlay.svg` (the workflow's own `Figure`). On
   `A → B` at k = 0.7 (runtime 5.0, dt 0.2), RK4 tracks the analytic decay
   `e^{-kt}` while forward Euler lags — MSE ≈ 3.6e-4 per species: "same target,
   different method, warranted comparison, method-induced divergence."
5. **Rung 3**: contract → KISAO export; OMEX/SED-ML round-trip.

