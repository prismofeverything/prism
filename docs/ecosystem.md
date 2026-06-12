# The ecosystem — generative ⋈ selective (the organism's second half)

> **Steward:** synth + the human (2026-06-12). Companion to `docs/organism.md`.
> The organism so far (Slice 0) is the **generative** half — an M/R metabolism that
> *produces* variation (cells metabolize, self-heal, divide, patch). An **ecosystem**
> is that **plus selection** — and the two **carve each other**. This note records the
> design so it is not lost; it slots into the spine beside the M/R closure.

## 1. Thesis — an ecosystem is two forces interwoven

> generative (makes variation) ⋈ selective (judges + filters it)

Neither alone is alive. Pure generation diverges into noise; pure selection has nothing
to act on. An ecosystem is the **intermixing** — variation produced, variation judged,
the survivors shaping the substrate the next variation is produced *from*. The forces
**carve each other**: selection sculpts the generative population, and the population
reshapes what selection has to choose between. (Variation + selection = Darwin; the M/R
reactions are the variation operator, the selective functions are the fitness operator.)

## 2. The rendered phenotype IS the fitness signal

The organism already has a **render functor** `Structure → Signal` (a cell's sound is its
F/Φ/B blend; `MembraneVoice`/`ColonyVoice`, generalised by `apply_functor` in Slice 1).
That rendered output — the **WAV / the Signal block** — is the **phenotype**. So a
**selective functor** `Signal → fitness` *evaluates* it:

- spectral (centroid/brightness, harmonicity, roughness),
- temporal (periodicity, density, novelty vs. its own past),
- relational (consonance with the rest of the colony — fitness is *contextual*).

The render we built to *hear* the organism is the same surface **selection reads to judge
it**. Phenotype = what the render functor emits; fitness = what a selective functor folds
it to. Both are functors out of the same bigraph — render is `Structure → Signal`,
select is `Signal → ℝ` (a reduction). Composing: `fitness = select ∘ render`.

## 3. Selection = fitness-gated reactions (same BRS substrate)

Selection is not new machinery — it is the **rates**. Each cell carries a `fitness`
scalar (computed by §2 from its rendered sound). The generative reactions become
**fitness-weighted**:

- **divide** rate ∝ `fitness` — the consonant/interesting cells reproduce;
- **degrade / prune** rate ∝ `1 − fitness` — the noisy/dead ones are culled;
- **patch** (Slice 1) biased toward wirings that raised fitness last tick.

The BRS already takes per-rule rates; making a rate read a `fitness` field is the whole
of it. Generation (the M/R cycle) and selection (the weighting) run on **one engine**,
interwoven every tick — §1 made literal.

## 4. The selectors themselves evolve (the open-ended move)

A **fixed** fitness function converges the population to its optimum and stops — a dead
end (the optimum is a wall). The living move is that **the selective functions evolve
too**:

- a selector is a **process / reaction as DATA** (the homoiconic face) — so it can be
  **mutated, recombined, and *itself* selected**;
- the criteria of "better" are then **endogenous**: a population of organisms *and* a
  population of selectors, **coevolving** — each the other's environment.

This is the **keystone (rules-as-products) applied to SELECTION**: not only does the
synth write the rules that write the synth (M/R closure, §`organism.md` Slice 2) — it
writes **the judges that judge the synth**. Evolving fitness is the known route to
**open-ended evolution** (endless novelty instead of convergence): the target keeps
moving because the population moves it.

## 5. The closure, extended — efficient cause → final cause

Rosen's M/R closure is **closure to efficient causation**: every *maker* is made within
the system (F made by Φ, Φ by β; `organism.md` §2). The ecosystem extends it to **closure
of final causation**: every *criterion* — what counts as fit, what the system is *for* —
is also defined within. No external fitness function = no external judge = a genuinely
**autonomous** system (it defines its own relevance — Rosen's anticipation, von Uexküll's
*umwelt*, autopoiesis's self-defined viability). The organism makes its **metabolism**,
its **rules**, *and* its **ends**. That is the whole of the closed category alive.

## 6. The prism mapping — all existing mechanisms

| ecosystem piece | prism mechanism |
|---|---|
| phenotype | the render functor `Structure → Signal` (Slice 1 `apply_functor`) |
| fitness | a **selective functor** `Signal → ℝ` (a fold; `select ∘ render`) |
| selection | **fitness-weighted reaction rates** in the same BRS |
| evolving selectors | selectors as **reaction/process DATA** (the homoiconic face, #60/#61) — mutated + selected (Slice 2 machinery) |
| coevolution | two populations (organisms ⋈ selectors) on one bigraph, each the other's environment |

The ecosystem runs on the **same substrate** as the organism — render functor + BRS +
homoiconic rules — with selection as a rate term and selectors as data. No new engine.

## 7. The build — slices

- **E0 — fixed selection (audible).** One selective functor (e.g. consonance-with-colony,
  or spectral-centroid-toward-a-target) → a per-cell `fitness` → fitness-weighted
  `divide`/`prune`. You *hear* the colony get more consonant / sparser / denser over a run.
  *Reuses the render functor + the BRS rates.*
- **E1 — the selector as data.** The fitness function is a value (a reaction/fold-as-data),
  swappable + mutable. Two runs with different selectors diverge audibly.
- **E2 — coevolution (open-ended).** A population of selectors carved by a meta-criterion
  (novelty / diversity of the ecosystems they produce). Organisms and selectors carve each
  other; the music never settles.

The standard (`organism.md` §6) holds: **one object, not two analogies.** Selection must
be the *same* BRS + render + homoiconic substrate, only with a fitness term — if it needs
a parallel mechanism, it is a half-measure.
