# Cells as composites — bridge-bounded, distributable, schema-first division

Design for the **one** cell representation and the division-through-reaction
feature, **respecting the composite boundary** so it works when a composite runs
on another machine. Foundation for #9 (dynamic structure), #27 (boundary-crossing
ops), #28/#29 (retire `Schema::Any` / unify code paths).

> **The context we keep losing — write it down:** *a composite is an enforced
> boundary, possibly on another machine.* The outer engine **never** reads or
> writes a composite's internals. **All** interaction crosses the **bridge**
> (state in / updates out). Every decision below follows from this.

---

## ✅ RESOLVED (2026-05-24) — the implemented model

The sections below were the design exploration. What was actually built + proven:

- **The bridge forwards the inner UPDATE (delta) intact — `diff(pre,post)` is
  deleted.** "Updates ARE deltas," so a composite emitting its update is the same
  operation as a `stream:` delta-frame, a `rest:` delta, a trace entry, a pipe.
  **local bridge == stream wire == rest == trace.** The old diff was lossy (dropped
  structural `_remove`) and inconsistent with streaming. Engine taps the reconciled
  delta at every output-bridge root (`set_bridge_out_paths`, accumulated over inner
  ticks); a **conduit** (output port with no inner slot, e.g. the division output)
  is forwarded-only (not applied inner); an inner-slot face (`mass`) is forwarded
  AND applied. (memory `composite_bridge_forwards_updates`; conduit-declaration is
  a wart → design task #13.)
- **Division is Form 3, canonical** (1 & 2 are derived): the cell **proposes** by
  setting its `%.divide` self-node face (an inner Trigger), the environment
  **enacts** via the `_divide` sentinel (a Divider step), and the schema-holding
  `apply` performs the split (`apply_divide_sentinel` → `divide_by_schema`). Neither
  reaches across the boundary — the cell exposes a face, the *container* rewrites
  its own membership (place-graph law: siblinghood is the parent's).
- **A cell is an addressed `CompositeLink` node** (`{address, config, mass, …}`),
  not a `{_type,mass,body}` container. `cells: Map{CompositeLink}`. The **`%`
  self-node wire** (engine, `["%",…]` bases on the process's own node) exports the
  `mass`/`divide` face onto `cells.N.*` so a parent can match it without knowing
  the dynamic key.
- **`divide_by_schema(CompositeLink)`** halves the on-node face (extensive `Delta`)
  and **shares `address`** → daughters re-realize on the SAME protocol (division is
  cross-protocol for free).
- **Algebra holes fixed (were the real bugs):** `resolve(Tree, Map{Link})` now
  preserves the Map (was collapsing → daughters `Schema::Any`);
  `reconcile(CompositeLink)` merges node fields (was last-wins → dropped the `mass`
  delta = conservation leak); instance removal checks the node's OWN path (was
  parent → zombie mother).
- **Guard:** `Engine::add_process` panics on structural explosion (a generous
  backstop, not a size limit; `set_max_nodes` lowers it for fast-fail).

**Proven (prism-bigraph tests/cells_division.rs + growth_division.rs):** a
mass-balanced grow-divide-glucose cell (uptake → biomass + acetate, `glucose +
Σmass + acetate = const` every tick) divides via Form 3, bounded; **identical over
local AND `rest:` (a real HTTP wire)**; `growth_division` does true division
(1→2→4) at 0.01s (was a 46s zombie explosion).

## ✅ RESOLVED (2026-05-24, part 4) — the `stream:` half, over a real wire

The cell now divides + conserves over the **`stream:` protocol** too — a separate
OS process per cell, Arrow frames over pipes — proven by
`crates/chrysalis/tests/grow_divide_stream.rs` (conservation `glucose+Σmass+acetate=41`
EVERY tick across growth AND division; terminates at a stable 16 cells of 1.5625 pg;
terminal numbers IDENTICAL to the proven local/rest run). Only the address changed
(`local:Composite` → `stream:cell.ys`); the cell, its bridge, and the division are
the same. `crates/chrysalis/ys/cell.ys` is the streamable cell (its ENTRY is the
`Cell` composite — no trailing `Cell[]`, so `chrysalis run cell.ys` invokes it as a
composite, not a one-shot script).

**The load-bearing fix — the stream child became a delta-FORWARDER.** The `stream:`
protocol's child was still a *delta-LOG filter*: `serve_stream` synthesized the wire
frame by `diff`-ing absolute output snapshots (`output_raw` + `algebra::diff`).
That double-counts a shared pool the instant two cells draw on it (and drops
structural `_add`/`_remove`) — it had never received the "forward the inner UPDATE"
fix the **local** `Composite::update` got (memory `composite_bridge_forwards_updates`).
New `runner::serve_process` (CLI `--serve-process`; `StreamProcess` now spawns it)
runs the entry as a real `Composite` and forwards `Composite::update`'s reconciled
delta each tick — byte-identical to local / the rest server. No `refines` handshake
(like rest), no one-tick lag (the parent stamps cumulative time *after* the step, so
the first frame is a full interval). `serve_stream` stays as the `A | B` trace
**filter** (absolute, replayable delta-log) — a genuinely different transport.

**Remaining:** only the surface sugar — `chrysalis run env.ys` with the whole
environment in `.ys` (thread `map[Cell] → Map{CompositeLink}`, retire the
`composite_instance_schema` dodge, the `%` surface wire, and surface stream
addressing). The proof above builds the environment in Rust (mirroring the rest
test); the `.ys` env is a convenience, not a capability gap (#20).

---

## 1. The problem

There are two cell representations, and one violates the boundary:

- **Form A — addressed composite (internal division), correct.**
  `Cell[id, mass] ->{…}` → `{_type: Cell, address: "local:Composite", config, …}`.
  The cell is a process node behind a bridge; it divides itself internally.
- **Form B — homoiconic container, wrong.** `{_type: Cell, mass, body: <subengine>}`
  conflates two things that must be separate: the **exported face** (`mass`) and
  the **running instance** (`body`), and it tempts the outer engine to reach into
  `body` — which breaks the moment the composite is remote.

The fix: a cell is **just a composite** that **exports its `mass`** across the
bridge; division is the composite's own operation, triggered from outside.

---

## 2. The representation

A cell's **outer** value is its spec plus its **exported face** — nothing else:

```text
cells.0 = {                       # what the OUTER engine sees
  _type:   Cell,                  # matchable face
  address: "local:Composite",    # a process node → instantiate (local OR remote)
  config:  { … },                # how to (re)realize the inner engine
  mass:    1.2,                  # the exported face: inner `mass`, bridged OUT
}
```

- The **inner state** (`mass`, `grow`) lives **inside** the composite (its
  subengine), **behind the boundary** — never in the outer tree.
- The **running instance** is a runtime concern: prism tracks instances **by
  path** (a live engine can't sit in a `Value`); deleting `cells.0` drops them.
  It is **not** in the schema. (This is the role the old `body` was confusedly
  playing — process-bigraph's `_instance`.)
- `schema(cells.0) = CompositeLink { inputs, outputs: {mass: Delta, …}, inner_schema: {mass: Delta, grow: ProcessLink} }`.
  **`outputs`** = the exported face the outer matches/observes; **`inner_schema`**
  = the internals, used **by the composite itself** to divide. The outer uses
  only `outputs`; the inner is opaque to it.

### The Cell, written normally

```
composite Cell[mass: Mass] ->{mass: Mass} (
  mass: mass |                            # inner mass — a PEER of grow
  grow: Grow ~{mass: mass} ->{mass: mass} # grow links to mass NORMALLY (internal wire)
)
```

The inner wires are ordinary (`grow ↔ mass`, peers). The output port `mass`
**bridges the inner mass OUT** to the cell's outer node `cells.N.mass` — the
exported face. That output wire **defaults** to the cell's own node, is
**explicit/overridable**, and is **relative**, so daughters inherit it on
re-realization (see §4).

---

## 3. Every operation, respecting the boundary

| Op | How it works |
|---|---|
| **Discover** | `cells = Map{CompositeLink}` → `cells.N` is a `Link` + address → instantiate (local or **remote**). Schema-first, no scan. |
| **Grow** | Happens **inside** the composite; the exported `mass` bridges out → `cells.N.mass` updates. The outer never sees `grow`. |
| **Match** | The reaction matches the **exported face**: `?cid : ?cell::Cell where ?cell.mass > threshold` — `_type` + `cells.N.mass`. The outer only ever sees the face. |
| **Divide (boundary-crossing)** | `?cell.divide(?cid)` **triggers `divide()` on the composite**. The composite — which alone knows its internals — splits its inner state into **two complete composites** (`divide_by_schema(inner_schema)`: `mass` halves as `Delta`, `grow` re-realizes as `ProcessLink`) and returns them. The BRS removes the mother (`?cid`) and `_add`s the daughters. **The outer never touches the internals** → identical whether the composite is local or remote. |
| **Re-discover** | Daughters land in `cells` (`Map{CompositeLink}`) → instantiated (local/remote), grow, divide again. |

**The user's framing, confirmed:** the cell has an address before it divides;
`divide()` does everything by the schema *inside the composite*; daughters are
addressed composites with the same address, re-realized per `inner_schema`.

---

## 4. Mechanisms

1. **The bridge exports the face → `cells.N.mass`.** `mass` is a normal **output
   port**; the bridge carries it out, so **`cells.N.mass` always exists on the
   node, populated from the bridge each tick** — never read from the composite's
   internal state. The outer matches/divides this *bridged face*, not the
   internals. The output wire **defaults to the cell's own node** and is
   **relative** — so `divide_by_schema` re-realizing daughters carries the same
   wire, re-targeting to each daughter's node (no per-key rewrite). Cross-refs
   (`environment: @`) are `@`/`..`-relative, inherited the same way. *(Earlier
   "self-wire into the engine's state" overcomplicated it — it's a plain output
   bridge.)*
2. **Division rides the EXISTING bridge seam — no new op, no protocol-specific
   code.** The outer can't reach inside, so it doesn't: when a cell divides, the
   composite **emits a structural update through its normal output bridge** —
   `{cells: {_remove: [self], _add: {daughters}}}` out its `->{environment}` port.
   That update is carried by the *same* `invoke`→`Defer` path (#27) over **any**
   protocol (`local`/`stream`/`rest`), so division "works across distributed
   protocols without modification" for free. `divide_by_schema(inner_schema)` runs
   **inside** the composite (on its hidden state) to make the daughters; the outer
   only ever sees the `_add`/`_remove` cross the bridge. *(So there is no `divide()`
   RPC to add and no outer-side `divide_by_schema`.)* The **trigger** is a reaction
   on the cell's state — cleanest as the cell's OWN internal divide reaction (fires
   on its threshold). The homoiconic "parent BRS reaches into `cells`" form violates
   the boundary; it **unifies onto internal self-division** — same mechanism,
   boundary-safe, already what the internal form (`grow_divide.rs`) does.
3. **`divide_by_schema` over `inner_schema`** (the whole body), not just the
   exported scalars: `mass: Delta` halves, `grow: ProcessLink` re-realizes → two
   complete composites. (Core split exists; retarget it to `inner_schema`.)
4. **Cycle detection in schema lowering** — done (`composite_link` + `building`
   stack): a composite reference builds its real `inner_schema`, back-edges marked.
5. **Map preservation in `resolve`** — TODO: `resolve(Tree, Map{value})` keeps the
   `Map` when `value` is a process-node `Link` (a dynamic collection), so daughters
   inherit the value schema; keeps the `Tree` for data maps (`fields: map[array]`).
6. **`apply(CompositeLink)`** — applies the **exported face** (`outputs`) at the
   node; the inner is the composite's own (across the bridge).
7. **Enforce remoteness with `stream` (#19) — a top-priority test.** Run the
   dividing cell over the `stream` protocol (a real serialized wire). If the
   daughters still cross correctly, the boundary is genuinely respected and division
   works on any machine. This is *the* test that makes "the composite is remote"
   impossible to forget; it must pass with the cell's protocol set to `stream`
   *unchanged*. It also **doubles as the distribution proof that real parallelism
   (#27) builds on** — so finish this design (#9) **before** resuming #27, then
   #27's `rest`/`stream` concurrent dispatch is validated by the same
   grow-divide-over-`stream`.

---

## 5. Code-path simplification (the payoff)

- **`composite_instance_schema` → the `CompositeLink` itself**: the exported face
  is `outputs`; the divisible body is `inner_schema`. One source.
- **The address scan → deleted.** One schema-first discovery walk: recurse
  non-`Link` nodes, instantiate `Link` nodes (which carry their address, local or
  remote).
- **Two discovery passes → one.**
- **The container form → gone.** No `{_type, mass, body}`; a cell is a composite.
- **`Schema::Any` at cell nodes → gone** (#28).

---

## 6. Implementation order (test-guarded; each green)

Gating tests: `grow_divide_pipeline_runs` (internal) + `homoiconic_grow_divide_runs`
(division through a reaction) + the **`stream` division test** (§4.7 — the boundary
proof) + workspace.

1. **`divide_by_schema(CompositeLink)` divides `inner_schema`** (whole body), not
   the outputs. Unit-test: a `CompositeLink` value splits into two with `mass`
   halved + `grow` present.
2. **Map preservation** in `resolve` (value `is_link` → keep `Map`).
3. **The Cell exports `mass`** via an output port; the wire defaults to the cell's
   node, relative. Test: `cells.N.mass` is populated from the bridge each tick; a
   daughter's wire targets its own node.
4. **Division as a structural bridge update**: the cell's internal divide reaction
   runs `divide_by_schema(inner_schema)` and emits `{cells: {_remove: [self],
   _add: {daughters}}}` through its `->{environment}` bridge; the outer applies it.
   No new op, no outer-side divide. Test: mother gone, two daughters, `mass` halved.
5. **cells element → `CompositeLink`** (cycle-detected); `homoiconic` rewritten to
   addressed composites that self-divide via an internal reaction; both grow/divide
   tests green.
6. **Run the division test over `stream`** (§4.7): set the cell's protocol to
   `stream` *unchanged* and assert the daughters cross the wire. The boundary proof.
7. **Delete the scan + collapse `composite_instance_schema` + unify discovery** —
   workspace green proves completeness (#29).

Stop-loss: if a step can't go green, it reveals a missing mechanism (likely §4.1
the export wire, or §4.2 the bridge-update division) — fix *that* in the core,
honoring the boundary; never reach across it.
