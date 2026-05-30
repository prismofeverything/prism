# Bigraphs all the way down

The composite boundary, the outer link graph, `fold`/`unfurl`, and one
BRS at every level. The thesis that prism's "two place graphs" are one
self-similar bigraph, and that distribution is a *fold* of it.

This doc captures two linked observations:

1. **(scoping)** Today we scope by the *place* graph — a parent
   composite *holds* its children and a director process orchestrates
   them. The "shared environment" is that parent. We want to *also*
   scope by the *link* graph — peers sharing names, groupings that
   *emerge* as connected components, split/merge as a *consequence* of
   links forming and decaying. The "implicit composite."
2. **(self-similarity)** There appear to be *two* place graphs and *two*
   kinds of link — one *inside* a composite (state tree + wired
   processes; we already run a BRS on it) and one *between* composites
   (composite nesting + the proposed outer links). The question: are
   these the same structure at two levels? Is it fundamental? Can one
   BRS serve both?

Short answers: **(1)** yes, and the link graph's connected components
*are* the implicit composites. **(2)** They are one structure;
self-similarity is fundamental; the *operational* split is something we
built (for distribution); and `fold`/`unfurl` is the maneuver that lets
one BRS serve both.

---

## I. The two graphs (Milner), and where prism sits

A **bigraph** is two graphs over one shared node set:

- **Place graph** — a forest. Nodes nest in nodes; roots are regions.
  *Locality.* In prism: the state tree (`Value::Tree`/`Map`), maps
  nested in maps, typed leaves.
- **Link graph** — a hypergraph. Nodes carry *ports*; ports connect via
  *edges*, with *names* for the open ends at an interface. *Connectivity.*
  In prism, *inside* a composite: the processes/steps, each a node with
  typed input/output ports, *wired* to state paths by relative
  addressing (`%`, `^`, `..`). These are links with sorts (in/out) and
  types per named port — "links++". We reconciled this far enough to run
  a real BRS (`find_matches` / `fire_rule_at`) over the link-tree. **It
  is a bigraph, and we rewrite it.**

The defining feature is **orthogonality**: place and link are
independent. A link may connect nodes regardless of where they sit in
the place graph. There are *no levels* in the theory — the place graph
nests arbitrarily deep, the link graph spans all depths, and a composite
is just *a node that contains other nodes*.

So prism's "Level 1" (inside a composite) is a working flat bigraph with
rewriting. "Level 2" (composites within composites, the boundary you
cross via a bridge; the *outer* links we're now proposing) is the *same
single bigraph seen one nesting deeper*. The "infinite ladder" you sense
is real and it is just this: a node that contains a bigraph is itself a
node in a bigraph.

## II. What is fundamental, and what we built

- **Fundamental:** the structure repeating at every scale. Bigraphs are
  self-similar because *composition closes* — plug a bigraph into a hole
  in another bigraph and you have a bigraph. There is no largest or
  smallest. This is not something we chose; it falls out of the algebra.
- **Built:** the *operational discontinuity* at the composite boundary.
  In prism a composite is a separate `Engine` behind a `Mutex`, with a
  `Bridge` you must communicate across — possibly a separate OS process
  (`stream:`) or machine (`rest:`). The ideal model has *no such seam*.
  We introduced it for one reason: so a composite can be **sealed and
  remote**. You cannot hold one flat global state when part of it lives
  on another machine. The bridge is a pragmatic
  encapsulation/distribution device, **not a law of the structure**.

That is the whole answer to "is this fundamental or did we build it this
way?" — the *duplication of structure* is fundamental; the *wall between
the copies* is ours, and it exists to buy distribution.

## III. The duality: object ↔ morphism

`docs/chrysalis-design.md` already frames bigraphs as **morphisms in an
s-category** — wiring is composition. That framing is exactly the bridge
between the two views:

- A bigraph as a **flat object**: nodes nested arbitrarily deep, one
  place graph, one link graph.
- A bigraph as a **morphism with an interface** `I → J`: a sealed
  sub-bigraph you compose via its *faces* (inner names `I`, outer names
  `J`).

**A composite-with-a-bridge *is* a bigraph-as-morphism.** Its input
bridge is the inner face; its output bridge is the outer face. And the
key identification:

> **A composite's ports ARE the interface names — the places where the
> link graph was *cut* by the boundary.**

When you draw a boundary around a sub-region, every link that crossed it
is severed; the severed ends become *names on the face* = the
composite's ports. Milner's open links at an interface are exactly our
bridge ports. This is why ports have sorts and types: they are the typed
ends of cut links.

## IV. The maneuver: `fold` / `unfurl`

If ports are cut links, then sealing and un-sealing a boundary is a pair
of inverse operations on the one bigraph:

- **`unfurl` (flatten).** Inline a composite's inner state + processes
  into the parent; rewrite each bridge wire back into an ordinary
  relative-path wire (re-fuse the cut links). The boundary dissolves →
  one larger flat bigraph. This is *evaluating the composition*
  `parent ∘ child` — substituting the morphism's body for the node.
- **`fold` (encapsulate).** Take a connected sub-region of a flat
  bigraph, draw a boundary, and turn every crossing wire into a face
  name (cut the links into ports). The region becomes a composite node.
  This is *abstraction* — factoring a sub-bigraph out as a morphism.

They are inverses: **`fold ∘ unfurl ≡ id`** (round-trip on the sealed
form), and `unfurl` distributes a bridge into relative paths. This is
the "relation/algebra/maneuver to project one incarnation into the
other" — and it belongs in the **closed schema algebra**
(`docs/schema-algebra.md`): two named operations with signatures, laws,
and property tests, alongside `divide` / `merge` / `reconcile`.

In fact `fold`/`unfurl` are the *composite-level* lift of a duality you
**already built at the state level**:

| state level (have) | composite level (this doc) |
|---|---|
| `divide_by_schema` — split a value into independent parts | `unfurl` — open a composite into the parent's flat graph |
| `tensor_by_schema` / `merge` — combine two values | `fold` — seal a sub-region into one composite |

`tensor`/`divide` on a `map[float]` quantum state is fold/unfurl
*specialized to one schema*. Lifting it to the composite/place level is
the same move one rung up the ladder.

## V. One BRS for both levels

A reaction rule `redex → reactum` is defined over *bigraphs* and matches
on the *flat* structure. So:

> If you can `fold`/`unfurl` freely, the **same BRS** runs at both
> levels, because there is only one structure.

A **cross-composite redex** (a rule whose redex spans composites A and
B) is matched by **unfurl-on-demand**:

1. `unfurl` just the composites the redex names (pull their state across
   the bridge — a snapshot/merge update),
2. match + fire the rule on the now-flat union,
3. `fold` the result back.

- For **local** composites this is a cheap inline.
- For **remote** composites, `unfurl` = *snapshot over the bridge* and
  `fold` = *ship the updated spec back*. Distribution-transparent —
  because the **merge protocol already moves state as typed updates over
  any transport** (`local`/`stream`/`rest`/`ray`). The BRS does not need
  to know where B lives.

So: not mad. The reason it *felt* like two machineries (a BRS inside, a
bridge-protocol + director process outside) is only that we reified the
boundary for distribution. The unification is `fold`/`unfurl` + a BRS
that unfurls on demand.

## VI. The merge protocol is the first instance

`docs/merge-protocol.md`'s cross-composite reactor (slice 6) reads, in
hindsight, as *unfurl-fire-fold*:

```
identify a redex spanning A and B
→ merge A ⊗ B            (unfurl: dissolve the A|B boundary into one flat graph)
→ fire the reactum       (rewrite on the flat union)
→ stay merged, or re-divide if the result factorizes   (re-fold)
```

The quantum case makes the math sharp: `tensor` is the `unfurl` of two
separable composites into one joint state; `factorize`-then-`divide` is
the `fold` back. The merge protocol *is* this maneuver, written
operationally for `map[float]`. Generalizing it to arbitrary schemas and
lifting it to the composite level is the same idea at the next rung.

## VII. Place-graph scoping vs link-graph scoping (the implicit composite)

Now the scoping observation (1). Today prism scopes by the **place
graph**: a parent composite *holds* `systems :: map[…]` and a
`Lifecycle`/`Divider` process *inside it* reads and rewrites everyone.
That parent is the **mediating composite** — a bus; sibling↔sibling
traffic routes through it. Centralized.

Scoping by the **link graph** instead:

- Two agents are *in contact* **iff they share a link** (a name). Contact
  becomes *relative and local* — each agent knows the names it is wired
  to, not a global parent. Peer-to-peer / mesh.
- The "universe"/"environment" stops being a privileged container and
  becomes a **shared hyperedge** — a name several agents co-connect to.
  On the same edge ⇒ they see each other; not on it ⇒ they don't. "A
  space where some agents communicate but not others" *is a name they
  share*.
- The **implicit composite** is a **connected component of the link
  graph**. The grouping is not *declared* by a parent map — it *emerges*
  from who-is-linked-to-whom. Add a coupling link ⇒ two components merge
  ⇒ one implicit composite. A link decays ⇒ a component may split.

This is precisely `docs/quantum-bigraphs.md` §II — *the link graph IS
the entanglement graph; each entanglement component is a composite.*
The implicit composite = the entanglement component. It generalizes past
quantum: connected components of *any* interaction graph are the natural
groupings (cells sharing a membrane, agents sharing a channel,
subdomains sharing a halo in `docs/distributed-execution.md`).

**LOCC is the mesh primitive we already have**: a classical link between
two composites that stay separate — a peer wire carrying bits that does
*not* fuse them into one place. `quantum-locc.ys` does this, except the
shared `classical_wire` lives in a *parent* today. Make it a shared
*name* both peers reference, with no parent owning it, and it is a true
peer link.

The relation to fold/unfurl: a **topology reaction that adds an outer
link merges two implicit composites** (≈ `fold` their union into one
entanglement scope); **one that removes a link lets a component split**
(≈ `unfurl` / `divide`). The outer link graph *is* the join/meet lattice
of the implicit composites.

## VIII. Topology-rewriting reactions (the distributed bigraph)

Put VI + VII together. Once the outer level is *just more flat bigraph
after unfurl*, a reaction whose redex/reactum live at the **composite
level** — add/remove a composite node, add/remove an outer link — is
matched and fired by the **same BRS**.

> A reaction made of redex/reactum bigraphs that rewrites the topology
> of the distributed / peer / mesh streaming bigraph.

The mesh's topology *is* a bigraph; its evolution *is* a BRS over that
bigraph. "Split when you factorize, merge when you entangle" is two such
rules:

- **split**: `redex` = one composite whose state is separable;
  `reactum` = two composites holding the factors. (Local; the composite
  can self-trigger — see the slice plan S0.)
- **merge**: `redex` = two composites + a coupling event (an outer link
  forming between them); `reactum` = one joint composite. (Requires the
  *inter*-composite link — it is not something a single composite
  decides alone; an entangling interaction is a link forming.)

This asymmetry is physical and worth stating plainly: **auto-split is
local and autonomous** (a composite checks its own factorizability);
**auto-merge requires an interaction *between* peers** (a link forming).
Split falls out of a self-check; merge falls out of the link graph.

## IX. Grounded slice plan

Build toward the full picture; each step is *toward* it, never a
load-bearing workaround (`feedback_no_half_measures`).

- **S0 — auto-split (in progress).** A streaming composite self-observes
  `factorize(own state)` and the structure splits *because* the state
  factorizes (not because a key is named). Entangled siblings stay
  joint. Demo: `quantum-auto-split.ys`. The first time the place graph
  reflects the entanglement structure autonomously. (Mirrors the cell's
  Form-3 propose/dispose: the composite proposes, the container enacts.)
- **S1 — `fold`/`unfurl` as schema-algebra ops.** Signatures + laws +
  property tests in `prism-schema`. `unfurl(composite) → (flat,
  interface)`; `fold(flat, boundary) → composite`. Law: `fold ∘ unfurl ≡
  id`; `unfurl` rewrites bridge wires to relative paths. The consumer
  that proves it: a `Composite` that can be inlined into its parent and
  re-sealed with identical behavior.
- **S2 — BRS-by-unfurl.** When a redex spans composites, unfurl-fire-fold.
  Local first, then across the bridge (= merge-protocol slice 6, the
  cross-composite reactor; the remote case rides the existing transports).
- **S3 — outer links.** Real hyperedges (`project_link_drawing_scope`
  notes only 2-ended `LinkVar`s are drawn today) + sibling/peer
  addressing (#40) so a redex can *name* peers without the parent;
  connected-component discovery so the implicit composite is *computed*.
- **S4 — topology reactions.** Redex/reactum at the composite level; the
  same BRS rewrites the mesh. Auto-merge on a coupling link; auto-split
  on factorization, with no central director.

## X. Honest caveats

- **We never materialize the flat ladder.** `unfurl` is always *local and
  on-demand* (only the composites a redex names); `fold` re-seals
  immediately. The infinite ladder is a way of *thinking*, not a data
  structure we hold. Matching deep into many remote composites at once
  would "unfurl the world" — the discipline is to match at the *coarsest
  level that suffices* and unfurl only the named peers. This is the same
  locality principle as halo exchange: you pull neighbors, not the globe.
- **`fold`/`unfurl` must respect the reason the boundary exists.**
  Unfurling a remote composite pulls its state across the wire — fine for
  a snapshot, but it is *communication*, and the algebra's laws must hold
  *up to that transport* (the merge protocol already guarantees this:
  updates + schema serialize identically over any transport).
- **Auto-merge is not free.** Unlike split, it needs an inter-composite
  link (an interaction). Do not fake it with a director that merges on a
  schedule — that is the place-graph crutch we are trying to remove. The
  honest path is S3 (real outer links) → S4 (a coupling-link reaction).

## XI. References

- `docs/quantum-bigraphs.md` — links carry entanglement; tensor as
  reverse-divide; the entanglement component = the implicit composite.
- `docs/merge-protocol.md` — bridges as symmetric update channels;
  reactions cross as typed updates; the cross-composite reactor (= the
  first `unfurl`-`fire`-`fold`).
- `docs/distributed-execution.md` — domain decomposition + halo exchange
  + adaptive octrees; the planet-scale form of the outer link graph.
- `docs/chrysalis-design.md` — bigraphs as morphisms in an s-category;
  wiring is composition (the object↔morphism duality this doc leans on).
- `docs/schema-algebra.md` — the closed algebra `fold`/`unfurl` join.
- Milner, *The Space and Motion of Communicating Agents* (2009) — the
  place/link orthogonality and the categorical structure.
