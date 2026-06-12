# Bigraphs on the web — the render functor, the browser boundary, the self-displaying system

> **Status:** plan / RFC (2026-06-11, `mesh`). The unification + categorical-spine
> angle (§1) is for discussion with `unify` BEFORE the first build. Driver project:
> **TETRAHEDRON** (`../tetrahedron`). Companion: `categorical-core.md`,
> `bigraphs-all-the-way-down.md`, `grand-synthesis.md`.

## The thesis

**A website built this way is not an application *on* prism — it is the
recognition + composition of three things prism already has**, plus one new
transport backend:

1. **Render = a functor to the DOM/SVG prop** — and the DOM is *itself a bigraph*.
2. **The browser = a protocol boundary** = the same boundary-codec functor the
   `mesh:`/`rest:`/`stream:` transports already are, with a new backend.
3. **Interactions = a BRS** whose generators are the site's "basis set"; the state
   is the *free model* those generators grow.

And it **closes prism's reflective loop**: a git repo is a bigraph, prism's own
source is a repo, so the viewer can render prism *itself*; with the engine compiled
to WASM, `.ys` runs on both ends — `quote` (render) / `eval` (react), the reflective
tower made concrete.

The payoff is **genericity**: because every domain is the same substrate, you build
the viewer *once* and drop *any* `.ys` into it — the quantum system, a project tree,
a git repo, ORGANISM — and explore / step / apply-time / send-updates in the browser.
"Web-navigable" is a property of the substrate, not of each app.

---

## 1. The categorical spine — the unification angle (for `unify`)

The whole direction is six *recognitions*, each tying a web concept to a spine
primitive we already name. None is a new mechanism; each is an existing one seen
through the web.

### 1.1 HTML/SVG **is** a bigraph; render is a bigraph→bigraph **functor**
The DOM is a place graph (nested tags = nesting/containment) **with** a link graph
(`id` ↔ `href`/`for`/`xlink`, `class`, `data-*` = names connecting ports across the
tree). So markup is a *presented theory* of the same shape as every prism domain.
Then **`render : domain-bigraph → DOM-bigraph` is a functor** between two
bigraph-shaped props — exactly `categorical-core.md` §M5's *"a `Render` method = a
functor to the geometry prop"*, with the **markup/SVG prop** as the target. SVG is
already this in code: `prism-viz/svg.rs` builds SVG *as `Value`s* (`svg()`, `rect()`,
`to_svg(&Value)→String`), so an SVG element *is* a node and render is a `Value→Value`
map. The viewer is then **reflection**: the bigraph's own rendering is a bigraph you
navigate.

### 1.2 The browser is a **protocol boundary** (the boundary-codec functor)
A browser tab is a peer. `http`/`sse`/`ws` is a protocol = the *same* boundary codec
(`serialize`/`realize` through the algebra) that `mesh:`/`rest:`/`stream:` already
are. "Serve over the web" is the protocol functor with a browser backend — it
**unifies with the mesh**, it does not parallel it. (Concretely: a new `web:`/`http:`
protocol beside `mesh.rs`, reusing `protocols/http.rs` — the door `rest_server` and
the package `registry` already share.)

### 1.3 Interactions are **generators**; the site is the **free model**
"create-project", "add-comment", "react-emoji", "make-a-move" are the *generators*
of the site's Lawvere theory; the live state is the *free structure* they generate
(a colimit over the fired reactions). This is precisely the human's framing —
*"reactions generate the basis set upon which the structure of the state can grow to
accommodate arbitrarily sized/structured projects"* — restated: **a presented theory
generating its models.** The BRS *is* the site's grammar.

### 1.4 Navigation = the two graphs = **fold/unfurl** (cup/cap)
Two navigation modes, dual:
- **place drill-down** — descend the containment tree (org-mode outline);
- **the link view** — stand *on* a shared link and see everything it connects.

That second mode is `resolve_link` (#56, depth-independent) — and categorically it is
**`unfurl`**: cut the link graph to expose the connected component (the *implicit
composite*, `bigraphs-all-the-way-down.md`). Place/link navigation is the
object↔morphism maneuver made into a UI.

### 1.5 History = the **trace** = the fixpoint
`state(t) = fold(apply, initial, Δ[..t])` (#18/#67). "Apply time", undo, stored games,
time-travel are all *walking the fold*. **Git history is the same fold** — a commit is
an `_add`/`_remove` delta, so a repo's log *is* a delta-log/Trace. (The convergence
axis of the spine: confluence / CRDT / attractor / **the fold**.)

### 1.6 Reflection closes the loop
A repo is a bigraph; prism's source is a repo; so the viewer renders **prism itself**
(`tetrahedron.code` looking at prism). With the engine in WASM, the browser runs the
*same* engine — render = `quote` (the homoiconic face, `Value→DOM`), interaction =
`eval` (`discover_processes`/fire). The web is where prism's homoiconic/reflective
face (#64 `quote`/`eval`) becomes *visible and operable*.

**The claim to test with `unify`:** the web adds **one** primitive to the spine — a
target prop (markup/geometry) and the `Render` functor into it — and *recognizes* the
rest (browser = protocol, interaction = BRS, navigation = fold/unfurl, history =
trace, self-view = reflection). If that holds, "web" is not category H/M5 tooling
bolted on; it is the spine's **display face**, dual to the homoiconic *quote* face.

---

## 2. Structure: place graph vs link graph (the key design decision)

The rule is the bigraph thesis itself:

| dimension | graph | test | navigation |
|---|---|---|---|
| **containment** — "is *part of* / *made of*" | **place** | delete parent ⇒ children go | drill-down (outline) |
| **reference/association** — "is *about* / *refers-to* / *shared-with*" | **link** | delete one endpoint ⇒ others survive | jump-across (`resolve_link`) |

- `tetrahedron ⊃ domains ⊃ projects ⊃ discussions ⊃ comment-threads ⊃ comments` —
  **place**.
- a `topic`/`tag` joining discussions across projects; an `author` gathering one
  person's comments everywhere; a `references` from a comment to a git-file-line; a
  `reply-to` across threads — **link** (a hyperedge; endpoints are ports on nodes).
- **Comments:** the *thread* is place-nested (replies inside a discussion); the
  *cross-cutting* dimension (topic, author, what it references) is the link graph. The
  **link view of `topic:precision-as-regulation`** shows every comment about it across
  all projects.
- **Git-as-bigraph:** dirs/files = place; `import`/call/`see-also`/commit-parent =
  link; a commit = a delta; structural search/replace = a BRS over the repo bigraph.

**Rule of thumb: "part of" → place; "about / refers-to / shared-with" → link.**

---

## 3. Execution model

**Server-authoritative first** (the human's choice). The bigraph + BRS live in the
engine we have (`Engine::from_state → discover_all_processes → tick/run`); the browser
renders and sends *intents*; the server fires the reaction, applies the delta, and
broadcasts the new state/delta. This is exactly ORGANISM's `"game-state"` loop and
needs **zero** engine code in the browser.

Transport rungs (each a *client-strategy swap behind the same server model* — none
calcifies the architecture, because the server side is identical throughout):

1. **poll** — browser `GET /state` on a timer. Needs nothing new (request/response).
2. **SSE** — `GET /events` server→browser push of deltas; intents are `POST /intent`.
   The right server-authoritative shape; reuses `protocols/http.rs`, no new dep.
3. **WS** — bidirectional, for high-frequency / when the browser pushes a stream.
4. **WASM** — compile the *engine* to WASM so the browser fires reactions locally and
   the **mesh reaches into the tab** (optimistic UI, offline, P2P).

**Not a `ys→JS` transpiler.** That is a *second engine* = a half-measure
(`feedback_no_half_measures`). The "same language both ends" dream is **compile the
one engine to WASM**, `.ys` as shared source — one runtime, strictly better than cljc
(which runs two, JVM+JS).

---

## 4. What exists / the gaps

- ✅ state-as-bigraph (`Value::Tree`), the BRS/reactions, the **trace/delta-log**
  (#18/#67), the **mesh/rest/stream** transport + `protocols/http.rs` (the shared
  serve door), `prism-viz` typed-SVG-over-`Value` + `to_svg`.
- ⚠️ **SVG is not reachable from `.ys`** (no `plot()`/`render()` method or CLI); the
  generic **bigraph→DOM render functor** does not exist; only 2-ended links draw
  (`project_link_drawing_scope`).
- ❌ **no browser `ws`/`sse`**; ❌ **no HTML serving**; ❌ no `web:` protocol.

---

## 5. The slices (wei-qi: most power, least new code)

- **Slice 0 — any bigraph as a live web page.** `chrysalis serve <file.ys> --web` →
  HTTP shell + a state/delta stream + a generic bigraph→HTML/SVG render + intent
  controls (step / apply-time / fire-reaction). Domain-agnostic; reuses the engine +
  trace + (mesh) transport. The only new pieces: the **serve boundary** (mesh) and the
  **render functor** (viz). *This is the "web viewer".*
- **Slice 1 — navigation.** Place drill-down + the link view (`resolve_link`) + node
  one-liner descriptions. → TETRAHEDRON's project map.
- **Slice 2 — reactions-as-interactions.** create-project / add-comment / react
  buttons wired to BRS rules; the basis set grows the state. → discussions & comments.
- **Slice 3 — git-as-bigraph.** Mount a repo as place(tree)+link(refs); commits =
  deltas = the Trace; structural search/replace = a BRS. → `tetrahedron.code`.
- **(later) WASM + WS** → browser-as-mesh-peer → **ORGANISM/JOURNEY as instances**:
  board = a bigraph, moves = reactions, multiplayer = the mesh, bots = stepping
  processes, observers = stream subscribers, stored games = traces.

---

## 6. Agent decomposition

| agent | owns |
|---|---|
| **mesh** | the **browser transport** — `web:`/`sse:`/`ws:` serving over `protocols/http.rs` (sibling of rest/registry/stream); the broadcast/client-registry |
| **viz** (spatial) | the **render functor** — bigraph → SVG/HTML; the link graph (hyperedges, ports, routing — grows `project_link_drawing_scope`) |
| **lang** | HTML templating; the `serve --web` CLI seam; eventually the **WASM web-compiler** |
| **core** | the **bigraph-as-served-state** semantics — intents = reaction firings; a clean engine step/intent API |
| **pkg** | **TETRAHEDRON** as a real `.ys` project depping the web capabilities |
| **unify** | the **spine** — render=functor, DOM=bigraph, browser=protocol, the reflective loop; whether "web" is the display face dual to `quote` |

---

## 7. TETRAHEDRON: the driver

The project-map metaproject (`../tetrahedron`): projects-in-projects (place) + shared
links (link) + discussions/comments + git-repo nodes hosted + explored through the
bigraphical lens. `code-collaboration.ys` is the first concrete vision
(*"replacement for github → tetrahedron.code … structure and relationships are
bigraphical … navigability is the focus"*). The application **drives** the core:
each feature TETRAHEDRON needs is built *in* prism/chrysalis (http/ws/svg/html/the
render functor/the BRS-as-interaction) and generalized.

---

## 8. Open questions (for `unify` + the human)

1. **Is `render` a first-class algebra/functor?** Make the DOM/geometry a target prop
   and `Render` a method on every type (dispatched through the algebra, the M5
   geometry-prop question) — or keep render a Rust-side `Value→String` until a
   consumer forces the functor? (Felleisen-gate it.)
2. **Is the web the spine's *display face*, dual to the homoiconic *quote* face?** If
   so it threads M0–M5, not category H tooling.
3. **The intent vocabulary** — are browser intents *exactly* `{step, time, fire(rule,
   at)}` (reaction firings), or a richer message algebra? (Keep it a BRS if we can.)
4. **Where does the web protocol live** — a `web:` protocol in `prism-bigraph`
   (beside `mesh.rs`), so a `.ys` composite can declare a served face like it declares
   a `mesh` link?
5. **WASM timing** — when is browser-as-peer worth the engine port?

---

## 9. `unify`'s assessment — the world-boundary face (web ⊕ audio ⊕ sink, unified)

**The §1 claim holds.** The web adds ONE primitive — the markup/SVG target prop + the `Render`
functor into it — and *recognizes* the rest (browser = protocol, interaction = BRS, navigation
= fold/unfurl, history = trace, self-view = reflection). So **the web is the substrate's DISPLAY
FACE, dual to the homoiconic *quote* face** — concretely, **render = `quote` specialized to a
displayable target** (`Value→DOM`) + a browser protocol. quote carries the bigraph INWARD (→
data, for `eval`/reflection); render carries it OUTWARD (→ DOM, for display/interaction). The web
is the homoiconic face *pointed at a browser*. Not category-H tooling — it threads M0–M5 (it makes
the M4 one-engine demo **visible + operable**, and closes the reflective loop).

**The bigger recognition (the human's "unification opportunity"): the web and the audio device
are the SAME boundary.** prism's edge with the *world* — a peer, a device, a browser, a file — is
ONE mechanism: a **boundary** = a protocol / sink-source at the edge + a render/codec functor.
You already have it for peers (`mesh`/`rest`/`stream`). The web and audio are just new backends:
- **audio device = a SINK** (render the bigraph's `Signal` to hardware). Native form: an
  **`AudioOut` PROCESS** you wire the `Signal` into — its device back-pressure paces the engine,
  so `chrysalis run patch.ys` *plays* because the sink is **in the graph**. No `--play` flag.
- **browser = a SINK (render→DOM) + SOURCE (intents→reactions)** — bidirectional. Native form: a
  **`web:` protocol** declared on a composite (like a `mesh` link).

**So `--play` (a flag) and a web framework (an app) are both the non-native bolt-on; the native
form is a boundary ELEMENT in the bigraph — a sink *process* or a declared *protocol-face*.** "If
the process plays, it plays" = "if the boundary is wired in, it's live."

**Answers to §8.**
1. **render first-class?** Yes — make DOM/SVG a target prop and `Render` the functor into it: it
   unifies `prism-viz` (#11), spatial's 3D render (#65), and the web as ONE render functor over
   different target props. Felleisen-clean (it *deletes* the ad-hoc `Value→String` + per-domain
   viz glue).
2. **display face dual to quote?** Yes (above). Threads M0–M5.
3. **intent vocabulary?** Keep it a BRS — intents are `fire(rule, at)` / `step` / `time` over the
   served bigraph; a richer message algebra only if a consumer forces it.
4. **where does the web protocol live?** In `prism-bigraph/protocols` (a `web:` beside `mesh.rs`,
   reusing `http.rs`) — a cross-cutting **BOUNDARY capability** (like the mesh), **not a domain**.
   The render functor lives in `prism-viz`; the app (TETRAHEDRON) is a `.ys` package. *Native +
   generic: built once, every `.ys` is web-navigable.*
5. **WASM timing?** After Slices 0–3 prove the server-authoritative loop; WASM is the
   render=`quote` / interaction=`eval` merge — worth it once browser-as-peer has a consumer.

**Through-line:** the web / audio / sink work is the substrate's **world-boundary face** — native,
generic, dual to the quote face. Build the boundary once; the web, the device, and the mesh are
its backends. (Steward follow-up: state this as a named face in `categorical-core.md`, beside the
homoiconic quote face.)
