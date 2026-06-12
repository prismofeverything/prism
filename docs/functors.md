# The `functor` definer — structure-preserving maps, first-class (render is the first)

> **Steward:** `unify`. Realizes `categorical-core.md` §4 (the one new definer) — now being
> BUILT, with **render** as the forcing consumer (the human, 2026-06-12: *"if we don't have a
> consumer, make one — the functor is a key part of the spine; start applying it with render"*).
> Companion: `web-bigraphs.md` (the display face), `categorical-core.md` §7b (the world-boundary).

## 1. What a `functor` is (the definer)

A **functor** `F : A → B` maps the **generators** of a source theory/prop `A` to **morphisms**
(constructions) in a target `B`, lifted to all of `A` by preserving `⊗` (parallel) and `∘`
(nesting/sequencing). First-class in `.ys` (surface is lang's call):

```
functor Name : Source -> Target (
  generatorA ↦ <a construction in Target>,
  generatorB ↦ <…>,
)
```

The Felleisen gate (§4): a first-class `functor` **deletes** hand-rolled glue (the render
`Value→String`, bespoke cross-domain bridges) and **names** the rest as instances. The forcing
consumer is **render** — and specifically that there are *many* render functors, which a one-off
generic render cannot express.

## 2. render — the first consumer, and why it's plural

The target is the **markup prop** — DOM/SVG, *itself a bigraph* (nesting = place, `id`/`href` =
link; `prism-viz` already builds SVG as `Value`s). A **render functor** is `View : Source →
Markup`. There are MANY, for two reasons:

- **different views of the same bigraph** — a **structural** view (nested boxes + edges: the
  place+link graph, navigable) vs a **semantic** view (the organism board: cells as circles in a
  field; a quantum circuit; a synth waveform);
- **different domains** — bio's organism board, quantum's circuit, synth's waveform.

So two tiers:

- **the generic STRUCTURAL functor** (substrate, built-in) renders ANY bigraph as its own
  structure → every `.ys` is navigable for free (`web-bigraphs.md` Slice 0). It is the bigraph
  displaying *itself* (reflection).
- **domain VIEW functors** are **declared** (via this definer, in each domain's library):
  `Waveform : Signal → Svg` (synth), `OrganismBoard : Colony → Svg` (bio), `Circuit : Qubits →
  Svg` (quantum). **This is why the definer is needed — many named, composable views.**

### How they compose / get chosen
- **compose + delegate** — a view is PARTIAL: customize the generators it cares about, **delegate
  to the structural functor** for the rest (`Board ∘ structural`). Functors compose; a view is a
  method-override-with-a-default.
- **choose the lens** — the display face applies a chosen functor: the structural default, or a
  named domain view. The *same* running bigraph renders many ways (structure ↔ organism board) —
  in a browser, switch the functor, re-render the same state.

## 3. The substrate-first split

| substrate (built once, generic) | domain library (declares its specifics) |
|---|---|
| the `functor` definer · the markup prop · the **generic structural** render functor · the `web:` protocol | each domain's **view functors** (organism board, waveform, circuit) |

A domain's library *includes how it renders* (its `Render` functor — `categorical-core.md` §M5).
The one-engine M4 organism is then viewable as its **structure** (the engine's bigraph) AND as the
**organism board** (bio ⊕ synth ⊕ quantum views composed) — two functors over one running engine.

## 4. The build
- **lang ⋈ core** — the `functor` definer (parse / ast / eval; *applying* a functor = map a
  bigraph through `generator ↦ morphism`, lifted by functoriality). The new language construct.
- **viz / mesh** — the markup prop (extend `prism-viz` SVG-as-`Value`) + the **generic structural**
  render functor (any bigraph → DOM/SVG). mesh's web Slice 0 rides this as its first render.
- **domains** — declare their view functors in their libraries (later, as each wants a custom view).
- **unify** — this design + the §4/§7b spine + the recognition tail.

## 5. The recognition tail (after render proves the definer)
Name the existing glue as `functor` instances **where it cleanly fits** (recognition, Felleisen —
not forced): unit/context conversion (the dimension functor), the boundary codec
(`mesh`/`rest`/`stream`/`web` serialize-realize), `chrysalis::compile` (syntactic → runtime).
render proves the shape; these fall in behind it as the shape generalizes — no speculative
deletion.
