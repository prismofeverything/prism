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

## Smallest next step

Sketch `ProcessContract` as a value type and write out what its fields
would say for the tier-1 examples (grow/divide, MAPK, static M/R).
That stress-tests whether the target/method split is real or
hand-wavy before baking it into chrysalis's schema design.

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
