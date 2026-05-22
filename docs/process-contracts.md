# Process Contracts

*Motivation: what SED-ML reproducibility actually delivers, where it
falls short, and how a typed `ProcessContract` in prism is the lever
to do better.*

## What SED-ML and KISAO are actually doing

SED-ML is a workflow description format: which model, what task (time
course / steady state / parameter scan / fit), what algorithm class,
what outputs, what plots. KISAO is a controlled vocabulary for the
"what algorithm" slot. The pitch — write `KISAO:0000029` (Gillespie
direct) instead of `copasi:gillespie` — is real and is the move that's
supposed to upgrade *repeatability* (same code, same machine → same
bytes) into *reproducibility* (same description across implementations
→ scientifically equivalent results).

Crucially, the implicit promise is method-level equivalence, not
bit-identical traces.

## Whether it works (~15 years in)

There's a concrete empirical answer from the 2025 BioModels curation
paper (Smith, Malik-Sheriff et al., PLOS Comp Bio). They re-ran 1055
curated ODE models through five SED-ML-conformant simulators (COPASI,
Tellurium, VCell, PySCeS, Amici) and found **88%** agree under
`allclose` (rtol 1e-4). That's after years of human curation and a
parallel infrastructure project (BioSimulators) to wrap each
simulator. So: it kind of works, on a curated subset, with non-trivial
human labor.

The failure modes are the interesting part:

- **The CVODE BDF divergence.** COPASI and Tellurium both claimed
  "CVODE BDF" with identical tolerance settings and disagreed on
  BioModel 1. COPASI scales the *absolute*-tolerance vector by initial
  conditions; Tellurium didn't. The community's response was not
  "those are different algorithms" — they minted a new KISAO term
  called *absolute tolerance adjustment factor* so SED-ML can
  disambiguate. Note what that term *is*: not an algorithm, a fudge
  factor about how an algorithm is parameterized. **The ontology grew
  to absorb an implementation detail.**

- **Stochastic methods are worse.** Two correct Gillespie-direct
  implementations produce different trajectories from different RNG
  streams. "Same algorithm" can only sensibly mean "same
  distribution," and SED-ML has no first-class machinery for
  distributional equivalence. The usual workaround (fix the seed) is
  repeatability, not reproducibility — it ties you to one RNG
  implementation.

The structural critique: **KISAO is a taxonomy of methods growing by
accretion** — a curated catalog, not a theory. L1V5 (April 2024) leans
further into KISAO-driven specification but doesn't change the shape
of the problem.

## Where the conceptual confusion lives

Two distinct things are smushed together in one ontology:

1. **Target semantics** — what mathematical object the process is
   approximating: chemical master equation, deterministic mass-action
   ODE, chemical Langevin, reaction-diffusion PDE.
2. **Method class** — which approximation realizes the target:
   SSA-direct, τ-leap, CVODE BDF, finite-volume PDE.

KISAO collapses these. "Gillespie direct" is a method; "exact CME
trajectory" is a target. SSA and τ-leap both *target the CME* (one
exactly, one approximately), and that's the *only* reason it's
coherent to swap them. Without explicit target semantics,
reproducibility is being defined at the wrong layer — you're asking
implementations to agree without ever stating which mathematical
object they're agreeing about.

This is also why **port signatures alone are insufficient for
substitutability.** Two processes with identical port signatures
(molecule counts in / time series out) can target the exact CME, an
approximate CME, the deterministic limit, or a well-mixed collapse of
a PDE. Same interface, incomparable outputs.

## Helpful or not — honest split

- As a **workflow archive format**: yes. OMEX + SED-ML is how
  BioModels delivers runnable experiments, and the curation paper
  proves it's tractable at the ~88% level. Real win.
- As a **reproducibility guarantee**: weaker than advertised. The
  community's reflex when two "identical" runs disagree has been to
  expand the ontology, not to articulate what equivalence is being
  claimed.
- As a **theory**: there isn't one. It's curation. The
  standardizing-before-stabilizing read is the right one.

## The proposal: `ProcessContract`

Port signatures are *interfaces* (Milner's outer face); they're
necessary but underspecified for substitutability. A `ProcessContract`
is the layer SED-ML is missing — and in prism it falls out of
homoiconicity for free (contracts are values, just like processes
themselves).

A contract carries:

- **Target semantics** — which mathematical object this process
  approximates (CME, RDE, PDE, deterministic limit). *This is what
  makes substitutability meaningful.*
- **Method class** — KISAO-style realization (SSA-direct, τ-leap,
  CVODE BDF, …).
- **Equivalence claim** — pathwise / distributional / weak-order /
  deterministic. *This is what "two processes agree" means for this
  contract.*
- **Time-advancement model** — continuous, discrete-event, fixed-step.

The payoff:

- Two processes with the same contract are substitutable, by
  construction. That's the local, compositional version of SED-ML
  reproducibility — and `discover_processes` can reflect on contracts
  and reject incompatible plugins at composition time.
- **KISAO becomes an export target, not the source of truth**: a
  contract → KISAO mapping for interop with COPASI / Tellurium / etc.,
  but internally prism has the target/method layering KISAO lacks.
- Chrysalis can make contracts first-class values — the tier-2
  dividend in [`chrysalis-design.md`](chrysalis-design.md): schemas
  that make illegal substitutions unrepresentable, rather than "caught
  at runtime."

This also reframes the question prism can pose externally: "tell us
your *target semantics*, and we'll tell you which KISAO methods are
admissible realizations." A contribution back, not just consumption of
the standard.

## Representing contracts in chrysalis

Now that chrysalis has a parser, a real schema layer (`crate::schema`),
units, and a `MethodRegistry`, the contract stops being a sketch —
every piece it needs already has a home.

**A contract is to an interface what a schema is to a value.** prism's
core principle is "schema is always present, inseparable from state."
The contract is that principle one level up: a port's *meaning* — which
mathematical object flows through it, and the sense in which two
same-shaped processes may disagree — is inseparable from its type. The
`~{} ->{}` interface is the *shape* (Milner's outer face); biocompose
stops here. The contract is the *meaning* that makes two same-shaped
processes substitutable. "Where a process fits" is its **coordinates in
a substitutability lattice**: COPASI and Tellurium sit at the *same*
point in target-space (mass-action ODE, deterministic) but *different*
points in method-space — and the comparison measures that method-space
distance at a shared target.

Surface form: a `contract` definer (lowercase meta-syntax, like
`process` / `pattern`) introduces it; `fulfills` annotates a process
with the contract it satisfies; `::` constrains a port to a contract.

```
contract TimeCourse (
  target  :: MathObject      # the WHAT  — which math object is approximated
  method  :: Method          # the HOW   — KISAO-style realization
  claims  :: Equivalence     # the sense in which two fulfillers agree
  advance :: TimeModel       # continuous / discrete-event / fixed-step
)

# One point in contract-space several simulators share. target + claim
# are pinned; method is left open to vary per fulfiller.
DeterministicMassAction = TimeCourse[
  target: MassActionODE, claims: Deterministic, advance: Continuous,
]

process Rk4
  ~{ network: MassActionNetwork, init: State, timespan: Time, dt: Time }
  ->{ trajectory: TimeSeries[Conc] }
  fulfills DeterministicMassAction[method: Rk4]          # → KISAO:0000032 (verify)
( rk4_integrate(network, init, timespan, dt) )           # MethodRegistry → native

process ForwardEuler
  ~{ network: MassActionNetwork, init: State, timespan: Time, dt: Time }
  ->{ trajectory: TimeSeries[Conc] }
  fulfills DeterministicMassAction[method: ForwardEuler] # → KISAO:0000030 (verify)
( euler_integrate(network, init, timespan, dt) )

step Compare[under: Contract]
  ~{ a: TimeSeries[Conc] :: under, b: TimeSeries[Conc] :: under }
  ->{ mse: Map[Float], figure: Figure }
(
  mse    = a.species_mse(b) |
  figure = a.overlay(b, title: 'Rk4 vs ForwardEuler') |
  { mse: mse, figure: figure }
)

workflow IntegratorComparison (
  net  = mass_action('A -> B' @ 0.7) |
  x0   = State[A: 1.0, B: 0.0] |
  span = Time[start: 0, end: 10, points: 200] |
  r = Rk4          ~{ network: net, init: x0, timespan: span, dt: 0.5 } |
  e = ForwardEuler ~{ network: net, init: x0, timespan: span, dt: 0.5 } |
  Compare[under: DeterministicMassAction] ~{ a: r.trajectory, b: e.trajectory }
)
```

Each line of the contract has a prism home:

| chrysalis form | prism mechanism |
|---|---|
| `contract C (…)` + `DeterministicMassAction = …` | a **`Custom` type** (NEXT-SESSION milestone #1): repr = the `{target,method,claims,advance}` record; value-methods `compatible`/`admits` |
| `fulfills C[…]` on a process | a **facet of the `ProcessLink` schema** (`crate::schema`, decision #13) — the contract rides with the port type |
| `a :: under` on a port | a **port-type parameter** carrying the contract; substitutability = the derived predicate `resolve(A,B)==A` (no new op), enforced by the existing wire-schema resolution |
| `species_mse`, `overlay` | **`MethodRegistry`** methods on `TimeSeries` / `Figure` (native analysis + the `render_timeseries_svg` plotter already in `spatio-flux/report.rs`) |
| `Rk4`, `ForwardEuler` | **native prism `Process`es** (`ProcessRegistry`, `discover_processes`) — the rung-1 integrators |
| `workflow … (DAG)` | **a Composite of `Step`s**, edges inferred from data deps (NEXT-SESSION #2); the `prism-mapk` workflow DAG is the existing pattern |
| `TimeSeries[Conc]` | **units** — `Conc = [substance]/[length]³`; and the contract's `target` *implies* the port units (MassActionODE ⇒ intensive concentrations; CME ⇒ extensive counts), so contract and interface are linked, not independent |

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
g = Gillespie ~{ network: net, init: x0, timespan: span }   # fulfills StochasticCME
Compare[under: DeterministicMassAction] ~{ a: r.trajectory, b: g.trajectory }
#  ^ compile error: g.trajectory's target is CME, not MassActionODE.
#    Comparing across targets must NAME the bridge:
#      Compare[under: DeterministicLimit(of: StochasticCME)] ~{ … }
```

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

1. **Native integrators** (`spatio-flux`): a `MassActionNetwork` model
   and two `Process`es — `Rk4`, `ForwardEuler` — producing a
   `TimeSeries`. Test each against the A→B analytic solution; MSE the
   two. *(This substrate is what the contract describes; build it
   first.)*
2. **Analysis methods** (`MethodRegistry`): `TimeSeries::species_mse`,
   `TimeSeries::overlay` (lift `render_timeseries_svg` into `prism-viz`).
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
   runs end to end (`chrysalis/tests/integrator_comparison.rs`, 2 green): parse →
   enforce contracts → `compile_with_methods` (native registry + injected
   `TimeSeries` methods) → Engine run → MSE produced. Native integrators run as
   one-shot `Process`es; `Compare` is ys-native (its body calls the registered
   methods). Contracts flow through wirings to slots
   (`check::collect_slot_contracts`). Steps 1–4 complete; only rung 3 remains.
5. **Rung 3**: contract → KISAO export; OMEX/SED-ML round-trip.

## Sources

- Smith, Malik-Sheriff et al., *Verification and reproducible curation
  of the BioModels repository* (PLOS Comp Bio, 2025) —
  <https://journals.plos.org/ploscompbiol/article?id=10.1371/journal.pcbi.1013239>
- SED-ML Level 1 Version 5 specification (2024) —
  <https://www.ncbi.nlm.nih.gov/pmc/articles/PMC11294059/>
- SED-ML Level 1 Version 4 specification —
  <https://www.ncbi.nlm.nih.gov/pmc/articles/PMC8560344/>
- Waltemath et al., *Reproducible computational biology experiments
  with SED-ML* (the original 2011 paper) —
  <https://www.ncbi.nlm.nih.gov/pmc/articles/PMC3292844/>
- Courtot et al., *Ontologies for use in Systems Biology: SBO, KiSAO
  and TEDDY* —
  <https://www.researchgate.net/publication/47541394_Ontologies_for_use_in_Systems_Biology_SBO_KiSAO_and_TEDDY>
- KiSAO GitHub repository — <https://github.com/SED-ML/KiSAO>
- BioSimulations / BioSimulators SED-ML conventions —
  <https://docs.biosimulations.org/concepts/conventions/simulation-experiments/>
