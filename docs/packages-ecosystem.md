# The packages ecosystem — dependency management as theory composition (#67)

> **Steward:** `unify`. **Foundation:** the canonical run-Core (`docs/canonical-run-core.md`,
> landed + in use — prism-audio's `ys` feature exposes `audio_core()`): *a package is a
> Core.* This is the plan for the full package / dependency surface (#67, the "packages
> ecosystem" milestone): the manifest, the resolver, the lockfile, the registry,
> `chrysalis add`/`publish`, semver, transitive + native deps.
>
> **Thesis: package management = theory composition.** Every piece reuses an existing
> generator; the only genuinely-new content is a version solver, a registry index +
> transport, a lockfile, and CLI verbs.

## The principled spine (why this is not a bolt-on)

A `.ys` module = a **presented theory** (`categorical-core.md`). So:

| Packaging concept | = prism generator (reused, not rebuilt) |
|---|---|
| a **package** | a **`Core`** (procs + types + methods + protocols) + import surface — the canonical run-Core (landed) |
| **linking** a dependency | **`Core::merge`** — union the four registries; a same-name incompatible def = a conflict. The *same* key-union/reconcile the schema algebra + the mesh already use. NOT a bespoke linker. |
| the **manifest** (`project.ys`) | **`.ys`-as-data** — `chrysalis add` edits it through the homoiconic face (read→Value→algebra apply→unparse→round-trip gate→write), the **same correct-by-construction edit `coord set` already designs** |
| the **registry** | a **registry of theories** — name → versions → manifest + Core/modules + checksum |
| **semver** | **theory compatibility** — patch/minor preserve the theory's morphisms; major may break them |
| **fetch transport** | a **pluggable backend** (local dir → HTTP/relay) — the same generalization the mesh transport gets |

The new content is small and bounded: a **version solver**, a **registry index +
transport**, a **lockfile**, and the CLI verbs. Everything else is composition of parts we
have.

### The categorical semantics are BINDING (settled with the human, 2026-06-09)

Not just motivation — design constraints on the resolver + `Core::merge`:

- **`Core::merge` is an idempotent join-semilattice** on theories (commutative + associative +
  idempotent) — *the same CALM property `algebra::mesh_safety` already gates for the mesh.* So
  merging an identical `(name@version)` twice = once.
- **A diamond dependency** (A→D, B→D, prog→A,B) therefore merges D **once, at the shared
  apex** — *not* a naive double-union that false-conflicts D with itself. **Transitive
  resolution = a colimit (pushout) over the dependency DAG.**
- A same-name **different-version** def is the genuine **conflict** (the version is part of
  the object's identity). So the **registry is a poset over `name × version`**; the
  **resolver picks one version per name** so the colimit exists conflict-free; the
  **lockfile is that chosen section.** *Dependency resolution is confluence; the registry is
  a CRDT of theories.*
- **Already running:** `coord/board.ys` (`from .mesh import mesh … from .simplify import
  simplify` merged through a `mesh` link) is a path-import + merge-link of N `.ys` packages
  converging with no coordinator — **Phase 1's working prototype.** pkg recognises +
  generalises it; it does not invent.

## Current state (the starting line)

- ✅ **Foundation — the canonical run-Core** (`compile_with_core`/`run_with_core` + the
  protocol door). In use: prism-audio `audio_core()`.
- 🟡 **The manifest** — `project.ys` exists but is a one-line *directive* (`package <name>
  [at <path>]`, `codegen.rs::Manifest`). No version, deps, or exports.
- 🟡 **The codegen path (#13)** — `chrysalis run` finds `project.ys`, generates+builds+
  caches a runner crate; still on the OLD 3-door convention (`pkg::prelude::{registry,
  methods, modules}`) — to be rewritten onto `domain_core()`.
- 🟡 **Import resolution** — explicit-origin (bare = native/registry, `.name` = file). No
  package-name dimension yet.
- ✅ **`chrysalis new`** + verbs new/run/check/format/server/coord.
- ❌ **Missing:** version, deps, resolver, lockfile, registry, `chrysalis
  add`/`remove`/`update`/`publish`/`install`, semver, transitive resolution, `Core::merge`.

## The plan — 5 phases, each shippable with a consumer

**Phase 1 — manifest-as-data + a PATH dependency + `Core::merge` linking.** Grow
`project.ys` from a directive into a structured `.ys` manifest value (name, version,
`dependencies`, `exports`). Support a **path dependency** (one local `.ys` package depending
on another). Link by **merging the dependency's Core** (`Core::merge` — the algebra
key-union of the four registries; conflict on incompatible same-name). *Consumer:* a
2-package local project (`.ys` lib `foo` + a program depending on `foo`). **This is
"independent `.ys` projects with dependencies" in its highest-value form — and it de-risks
the linker before any registry complexity.**

**Phase 2 — the resolver + the lockfile (transitive + semver).** Version constraints;
transitive resolution; a **version solver** (semver); conflict detection (= `Core::merge`
conflicts); a **lockfile** (`project.lock`, the pinned graph, reproducible). *Consumer:* a
3-package graph with a shared transitive dep + a constraint → a deterministic lockfile.
*(The one genuinely-new algorithm.)*

**Phase 3 — the registry + `chrysalis add` + fetch/cache.** A package **registry** — start
**local** (a dir-based index `name → versions → manifest + checksum`; a content-addressed
cache). `chrysalis add <pkg>` (resolve → update manifest + lock — reusing the `coord set`
homoiconic edit), `chrysalis install` (fetch), `remove`/`update`. The registry is a
**pluggable backend** (local now; remote later). *Consumer:* `chrysalis add foo` from a
local registry, then run.

**Phase 4 — publish + remote registry + the first published packages.** `chrysalis publish`
(version + upload); a **remote** backend (HTTP — generalizing Phase 3's index, the move the
mesh transport gets); integrity (checksums). *Consumer:* publish `prism-std`, `audio`
(prism-audio), `spatio-flux` as the first packages; a fresh project `chrysalis add audio` →
A6's module factory rides it from the registry. **The ecosystem goes live.**

**Phase 5 — native-dep + workspace polish.** The native-dependency dimension (a `.ys`
project depending on a native crate's `domain_core()` — bridges the registry + cargo,
**rewriting the #13 codegen path onto the canonical run-Core**, dissolving the last
spatio-flux 3-door debt). Workspaces (multi-package). Semver-aware `chrysalis update`.

## Owners

- **pkg** (OWNS, solo — the dedicated agent; charter `coord/pkg.next`): the manifest,
  resolver, lockfile, registry, CLI verbs, the codegen rewrite — the bulk. The soul is
  **theory-composition (Phases 1–2)**; the ecosystem (3–5) is thin reach on top. *(Supersedes
  the earlier "lang leads" plan — we spun up `pkg` to run the full ecosystem as one principled
  arc.)*
- **lang** (coordinate-first adjacent): the shared chrysalis seams `pkg` builds on —
  `cli.rs` dispatch, `codegen.rs`, the import resolver. Non-monotone → claim-before-touch.
- **core**: `Core::merge` (linking = algebra key-union of the four registries) + registry
  introspection (`names()`/`type_names()`). Small; after #71.
- **mesh**: the remote-registry **transport** (Phase 4 — a pluggable backend; address +
  auth = capability refs, the mesh-transport generalization).
- **simplify**: guards — the manifest round-trips as `.ys`-data; the resolver is
  deterministic; one linker door (`Core::merge`).
- **unify** (me): the "registry of theories" recognition + the spine (linker = the algebra
  merge; manifest = `.ys`-data; semver = theory compatibility — keep it thin).

## Sequencing recommendation

**MVP-first: Phases 1–2** (manifest-as-data + path-deps + resolver + lockfile) deliver
modular `.ys` projects with dependencies — the core value — and de-risk the `Core::merge`
linker. **Then pause to consume it** (spatio-flux + a synth `.ys` lib as the first
multi-package projects) before the registry ecosystem (Phases 3–5). It is lang-heavy, so it
**queues after lang's current A6-unblock work** — unless we spin up a dedicated packaging
agent to run it in parallel.
