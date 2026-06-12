# The bigraph viewer — render functors, navigation by `fold`/`unfurl`, transitions by natural transformation

> **Status:** design / RFC (2026-06-12, `mesh` ⋈ the human). The render + interaction
> design for the web viewer (`web-bigraphs.md` Slice 1+). Companions:
> [`web-bigraphs.md`](web-bigraphs.md) (the boundary), [`categorical-core.md`](categorical-core.md)
> §7b (the world-boundary face), [`functors.md`](functors.md) (`apply_functor`),
> [`bigraphs-all-the-way-down.md`](bigraphs-all-the-way-down.md) (`fold`/`unfurl`). **§3 (the
> natural transformation) is for `unify` to formalize.**

## The thesis

The viewer is **the bigraph's own two-graph structure made navigable and animated.**
Three operations, nothing else:

- **RENDER = a functor** `apply_functor : bigraph → SVG`. Multiple views = multiple
  functors over the *same* bigraph.
- **NAVIGATE = `fold`/`unfurl`** — on the **place** graph (enter/exit a *node*) and on
  the **link** graph (enter/exit a *link*). Same maneuver, two graphs.
- **TRANSITION between views = a natural transformation** between two render functors —
  *the animation*.

The human's words: *"we are using the same nodes, just translating them around between
the functors."* Exactly — one bigraph, many functor-images, morphed between.

---

## 1. The views are functors

Two characteristic views (mapk already ships both, as opaque/bespoke renders today):

| view | what it is | today | becomes |
|---|---|---|---|
| **side** | a layered **tree** (rank by depth, node-edge) | **Graphviz** (`dot.rs` → shell-out to the `dot` binary; an opaque string) | a native `apply_functor` layered-layout functor → structured SVG |
| **top-down** | nested **ovals** (containment = nesting) | `mol_svg.rs` (domain-specific: membrane/nucleus/ER) **or** `StructuralRender` (generic boxes) | a generic `apply_functor` nested-ellipse functor (SAMOCA) |

The unlock: make **both** views `apply_functor` render functors emitting **structured,
id'd** SVG (each `<g id="bigraph/path">`). Then they share node-identity — which §3
needs.

---

## 2. Navigation = `fold`/`unfurl` on both graphs

The **view state is a camera** — `{ focus: path, expand: set<path>, mode: side|top,
link_focus: name? }` — a **presentation** concern, *not* engine state. (So two viewers
hold different cameras on one running engine; the engine stays the single truth.) It
rides the request: `GET /state?focus=…&mode=…&expand=…`.

| gesture | operation | graph |
|---|---|---|
| **expand / collapse** a node | a depth/visibility param; a collapsed node draws a `⊕` affordance (has hidden children) | place |
| **enter** a node / **exit** | re-root the view at the node (**`unfurl`**) / pop to its parent (**`fold`**) + a **semantic** zoom | place |
| **enter / expand a link** | `resolve_link` → the link's **star** (all it connects) = **`unfurl`** on the link graph / **`fold`** to leave | link |

Node-navigation and link-navigation are the **same `fold`/`unfurl`** on the two graphs
— the place/link duality of `bigraphs-all-the-way-down.md`, as a UI.

---

## 3. Transition = a natural transformation (for `unify`)

`Side : Bigraph → Svg` and `TopDown : Bigraph → Svg` are two functors from the same
source. A **natural transformation** `η : Side ⟹ TopDown` is a family
`η_b : Side(b) → TopDown(b)`, natural in `b`. A *morphism in SVG-land* is a **continuous
morph** of one drawing into the other — so **η IS the animation.**

Why it is not a metaphor — both are `apply_functor` over the **same place graph**, so:

- the **components** `η_b` are free — match each node's side-shape to its top-down-shape
  **by id** (the bigraph path), and tween (CSS `transition: transform`, SMIL, or a small
  JS tween);
- **naturality** (a child morphs coherently *within* its parent's morph) is
  **structurally guaranteed** — the SVG group hierarchy *is* the place graph in *both*
  views, and transforms compose, so the naturality square commutes **by construction**
  (the same way `apply_functor` is functorial by construction).

**Open for `unify`:** the precise target category (what the markup prop's morphisms
formally are — a homotopy/continuous-deformation category over drawings) so `η` is a
theorem, not just an animation that happens to be coherent. The *structure* is real and
implementable; the formalization is the categorical-core §7b/§8 follow-on.

---

## 4. SAMOCA aesthetics + the legibility law

The target look is Milner's **SAMOCA**: nested **ovals** (regions in regions) with green
**hyperedges** (links).

- **Nested ovals — reuse `pattern.rs`.** `pattern.rs` is *already* a native, no-Graphviz,
  nested-**ellipse** layout: `layout`/`compartment_layout`/`position_children`/
  `forest_layout` lay a compartment out as an ellipse inscribing its children
  (`ELLIPSE_FIT_W/H`), ports as filled circles, with a forest fallback. It renders
  reaction-rule `Pattern`s today; the work is to **generalize its layout from `Pattern`
  to any bigraph `Value`** and re-seat it as an `apply_functor` Functor. The SAMOCA
  top-down is ~80 % already written here.
- **Green hyperedges — the missing half.** Neither current top-down draws the link graph
  generically. A link connects N ports (a hyperedge) → draw green curves from each
  connected node to the link's hub, via `resolve_link` (#56). This is the first real
  generic link-graph render (grows `project_link_drawing_scope`).
- **THE LEGIBILITY LAW: semantic zoom, never geometric.** *Never scale text with depth.*
  Nest by **containment at constant text size** (an oval grows to fit; text stays ~12 px);
  **entering re-renders at the focus level**, full-size, rather than scaling pixels. This
  is precisely why `StructuralRender` is legible and `mol_svg`'s top-down is not — it
  `scale`s the whole drawing and shrinks fonts with depth (13→11→10 px), so deep text
  vanishes. The fix is the law, not a tweak.

---

## 5. Graphviz → native SVG (the side view, its own push)

Graphviz is a **shell-out to the `dot` binary** (`dot.rs`; the crate's very reason for
being). Two costs: it is a **monster external dependency** (must be *installed* — mapk
falls back to in-process SVG when it is absent), and it emits an **opaque string** — no
ids, so it **cannot be morphed (§3) or clicked (§2)**.

Replace it with a **native layered layout → structured SVG**: rank nodes by place-graph
depth, order within a rank to reduce edge crossings (a Sugiyama-lite / barycenter pass),
position, draw edges as SVG paths. **How good?** Not pixel-perfect Graphviz, but: a clean
layered tree, **structured + id'd** (enables η + clicking), **no external binary**, and
`pattern.rs` already proves native SVG layout looks good. This is **its own push** — the
side-view *design* (layout + psychology/feel: *"you just have to see how they feel"*).
Graphviz can stay a dev-only fallback until the native side view feels right.

---

## 6. Architecture (the division)

- **Server (the functor):** `render(bigraph, view_state) → SVG` with **stable per-node
  ids** (= bigraph path) + the link hyperedges + the chosen view + constant text. A *pure
  function* of `(bigraph, view_state)`. Lives in `prism-viz` (on `apply_functor`),
  injected into the web boundary by chrysalis (the boundary cannot dep prism-viz).
- **Client (the camera + animation):** holds `view_state`; a gesture updates it,
  re-fetches the render, and **animates the transition by id** — `η` realized. Expand/
  collapse, enter/exit (semantic zoom), link enter/exit all just mutate the camera.
- **Engine (core):** unchanged — `view_state` ≠ engine state.
- **WASM later:** the functor runs *in the tab* → no round-trip per frame → buttery.

---

## 7. Build slices

- **A — foundation (unlocks everything):** stable `id = bigraph-path` on every SVG group
  · the **link graph** (green hyperedges via `resolve_link`) · the **SAMOCA oval restyle**
  (lift `pattern.rs`'s layout to `Value`) with **constant text**. → legible Milner ovals
  with hyperedges, *plus* the id substrate for clicking/animating/navigating.
- **B — camera:** `view_state` query params + **expand/collapse** (+ the `⊕` affordance).
- **C — enter/exit:** re-root + **semantic zoom** (the place-graph `fold`/`unfurl`).
- **D — the side functor + η:** native layered side-view functor (retire Graphviz) + the
  **natural-transformation animation** side ⇄ top-down (tween matched ids).
- **E — link navigation:** enter/exit/expand/collapse a **link** (the link-graph
  `fold`/`unfurl`).

---

## 8. Reuse map (don't clone — every piece exists)

| need | reuse |
|---|---|
| the functor lift | `prism_schema::functor::apply_functor` (core) |
| nested-ellipse layout (SAMOCA) | `prism-viz/pattern.rs` (`layout`/`compartment_layout`/…), generalized `Pattern → Value` |
| the SVG-as-`Value` target + serialize | `prism-viz/svg.rs` (`el`/`group`/`to_svg`) |
| the link graph + link navigation | `resolve_link` (#56) + the `_links` marker |
| navigation semantics (enter/exit) | `fold`/`unfurl` (S1/S2, `bigraphs-all-the-way-down.md`) |
| the boundary + intents | `prism-bigraph/protocols/web.rs` (mesh) |
| step / apply-time / fire | `Engine::tick`/`run` + `Engine::fire(rule, at)` (core, incoming) |

**Owner:** this is the **render-functor + interaction** front — viz/spatial's craft
(carried by `mesh` until a viz agent boots). The natural-transformation formalization
(§3) is `unify`'s.
