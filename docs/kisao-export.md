# KISAO IDs, SED-ML / OMEX export

Design notes for putting KISAO identifiers in prism's process contracts and
exporting prism programs as SED-ML / OMEX artifacts. The reverse direction
(SED-ML → prism) is also covered. Pairs with canonical task **#33** and
sits on top of the contract layer (#12).

---

## What KISAO is

KISAO (Kinetic Simulation Algorithm Ontology) is the systems-biology
controlled vocabulary for **simulation algorithms**. Every method has a
stable identifier:

| KISAO ID | Algorithm |
|---|---|
| `KISAO:0000019` | CVODE |
| `KISAO:0000030` | Forward Euler |
| `KISAO:0000032` | Runge-Kutta 4 |
| `KISAO:0000064` | Gillespie SSA |
| `KISAO:0000086` | Implicit-tau leaping |

Curated by the Biomodels / BioSimulators community; consumed by COPASI,
Tellurium, libSBMLSim, and the SED-ML standard. When you write
`<algorithm kisaoID="KISAO:0000032"/>` in a SED-ML file, every conforming
simulator knows you mean RK4.

KISAO names *what* algorithm to run. It does **not** carry what the
algorithm targets (which mathematical idealization) or what it claims
(determinism, conservation laws, convergence). Those are prism's contracts.

---

## Where KISAO fits in prism's contract layer

prism contracts (see `integrator-comparison.ys`, `docs/process-contracts.md`)
already structure simulation methods richer than KISAO does:

```ys
contract DeterministicMassAction (
  target:  MassActionODE,    # WHAT the method approximates
  claims:  Deterministic,    # WHAT it guarantees
  advance: Continuous        # HOW it steps
)

process Rk4 fulfills DeterministicMassAction[method: Rk4] (...)
```

Four axes — `target / claims / advance / method`. KISAO essentially names
the `method` axis (with some terms covering the others, less expressively).
The mapping is **many-to-one in our favor**: several prism contracts can
collapse to the same KISAO term when their differences are below KISAO's
granularity, but one KISAO term explodes into multiple admissible prism
contracts.

That asymmetry is the **value-add**. KISAO names the algorithm; prism
contracts name what it's *for* and what it *guarantees*. Exporting to KISAO
is lossy outward (claims/advance/target don't translate); importing from
KISAO requires picking plausible contracts.

---

## Putting KISAO IDs in contracts

KISAO is a **labeling**, not a fifth axis. Two reasonable shapes:

### Option A (recommended): on the method-axis contract

```ys
contract Rk4         (kisao: 'KISAO:0000032')   # Runge-Kutta 4
contract ForwardEuler (kisao: 'KISAO:0000030')
contract Cvode        (kisao: 'KISAO:0000019')

process Rk4Integrator
  fulfills DeterministicMassAction[method: Rk4]
  (...)
```

Each KISAO ID lives once, on the method that has that identity. All
`fulfills` sites carry it transitively. Matches how the standards world
thinks: "Rk4 IS KISAO:0000032 everywhere."

### Option B: on each `fulfills` site

```ys
process Rk4Integrator
  fulfills DeterministicMassAction[method: Rk4, kisao: 'KISAO:0000032']
```

More repetitive, harder to keep consistent. Defer.

---

## What "exportable" concretely means

Two layers:

### Layer 1 — SED-ML emission

`chrysalis bigraph export f.ys out.sedml --as sedml` would:

- Map the entry composite's structure to SED-ML's `<listOfModels>` +
  `<listOfSimulations>` + `<listOfTasks>` skeleton.
- Each `fulfills DeterministicMassAction[method: Rk4]` becomes a
  `<uniformTimeCourse algorithm="KISAO:0000032">` element.
- The CRN target maps to an SBML `<model>` reference (inline or by URL).
- The composite's `interval` becomes SED-ML's `outputStartTime` / step
  size; `--time` becomes `outputEndTime`.

Result: an existing BioSimulators-compatible runner can take that SED-ML
and re-execute the simulation in COPASI, Tellurium, libSBMLSim, anything
that handles KISAO terms. **prism becomes a first-class authoring tool in
the systems-biology ecosystem.**

### Layer 2 — OMEX archives

OMEX wraps SED-ML + SBML + metadata into one `.omex` file (a ZIP with a
manifest). `chrysalis bigraph export-omex` produces a reproducible
artifact: model + simulation spec + manifest, runnable on the
BioSimulators public registry.

---

## The reverse direction — SED-ML → prism

`chrysalis bigraph import-sedml` reads a SED-ML file, looks up each
`algorithm kisaoID=…` in the prism contract registry, and instantiates the
matching prism integrator.

**Lossy.** KISAO doesn't carry our `claims` / `advance` axes, so the
imported program is a best-effort:

- If exactly one prism contract maps to that KISAO term, use it.
- If multiple do (because prism contracts are finer-grained), use the
  most-specific one whose claims/advance are compatible with whatever
  SED-ML metadata is available.
- Otherwise, emit a `Generic` fulfills with the KISAO term as the method
  identifier and let the user refine.

---

## The contribution back

Today's standards consumers (COPASI, Tellurium, …) read KISAO and run.
Producers (Antimony, libsbml) emit KISAO but don't reason about it — the
algorithm choice is made by a human.

prism contracts let you ask:

- "Given target = `MassActionODE` + claims = `Deterministic`, which
  KISAO terms qualify?"
- "Which prism methods refine which others under the same KISAO term?"
- "Can I substitute this method for that one without losing claims?"

The `refines` algebra computed over contracts is the engine; exporting to
KISAO is just naming the results in the standard's vocabulary. This is
the **inverse direction** existing standards tooling doesn't have, and
the natural contribution back: a contracts-aware advisor over SED-ML.

---

## Suggested slicing (canonical task #33)

- **Slice A — `kisao` field on contracts.** Optional field on `contract`
  defs (option A above). Pure additive; nothing else changes. ~50 LOC +
  parser hook + contract-display tweak. Pre-req for B/C/D.

- **Slice B — `chrysalis bigraph export --as sedml`.** Walk the entry
  composite, emit SED-ML with KISAO references. Small if the program is
  just `RunProcess`-wrapped; gets interesting for multi-process composites.

- **Slice C — `chrysalis bigraph import-sedml`.** Read SED-ML, look up
  each `algorithm kisaoID=…` in the contract registry, instantiate.

- **Slice D — OMEX round-trip.** Archive wrapper around B/C.

Slice A is the prerequisite for everything else and is the natural
checkpoint to start at.

---

## Pointers

- KISAO: <https://bioportal.bioontology.org/ontologies/KISAO>
- SED-ML L1V4: <https://sed-ml.org/>
- OMEX: <https://identifiers.org/combine.specifications/omex>
- BioSimulators: <https://biosimulators.org/>
- prism contracts: `docs/process-contracts.md`,
  `crates/chrysalis/ys/integrator-comparison.ys` (the flagship example).
