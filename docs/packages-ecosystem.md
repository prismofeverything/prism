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
- ✅ **The manifest** (Phase 1) — `project.ys` reads as **`.ys`-as-data** (`manifest.rs`): a
  structured `def package = { name, version, dependencies, exports }` record parsed by the
  chrysalis parser, edited (Phase 3) through the same round-trip `coord set` uses. (The legacy
  one-line `package <name> [at <path>]` directive in `codegen.rs` still names a NATIVE crate to
  link; the two converge in Phase 5.)
- 🟡 **The codegen path (#13)** — `chrysalis run` finds `project.ys`, generates+builds+
  caches a runner crate; still on the OLD 3-door convention (`pkg::prelude::{registry,
  methods, modules}`) — to be rewritten onto `domain_core()`.
- 🟡 **Import resolution** — explicit-origin (bare = native/registry, `.name` = file). No
  package-name dimension yet.
- ✅ **`chrysalis new`** + verbs new/run/check/format/server/coord.
- ✅ **The linker** (Phase 1) — `Core::merge` (the idempotent join-semilattice of the four
  registries; conflict on incompatible same-name) **+ `Core::own_over`** (a package's own theory
  over the shared floor — the dual of merge), in `prism-bigraph` with prism-side laws
  (`tests/core_merge.rs`).
- ✅ **Path deps + resolver + import surface + `chrysalis run` auto-resolve** (Phase 1,
  `resolver.rs`): a `.ys` program depends on a path package and uses its exported processes.
- ✅ **Semver + transitive resolution + the lockfile** (Phase 2) — `version.rs` (an owned
  semver subset), version requirements on dependency edges, the **resolver-as-colimit**
  (`resolver.rs`, memoized — a diamond's shared apex resolved + linked once), `project.lock`
  (`lockfile.rs`, the colimit's chosen section, deterministic + reproducible).
- ❌ **Missing (Phase 3+):** the registry, `chrysalis add`/`remove`/`update`/`publish`/
  `install`, fetch/cache, publish, native-dep workspaces.

## The plan — 5 phases, each shippable with a consumer

**Phase 1 — manifest-as-data + a PATH dependency + `Core::merge` linking.** Grow
`project.ys` from a directive into a structured `.ys` manifest value (name, version,
`dependencies`, `exports`). Support a **path dependency** (one local `.ys` package depending
on another). Link by **merging the dependency's Core** (`Core::merge` — the algebra
key-union of the four registries; conflict on incompatible same-name). *Consumer:* a
2-package local project (`.ys` lib `foo` + a program depending on `foo`). **This is
"independent `.ys` projects with dependencies" in its highest-value form — and it de-risks
the linker before any registry complexity.**

> **✅ DONE (2026-06-10, `pkg`).** Shipped: `manifest.rs` (the `.ys`-data manifest +
> `find`/`load`/`parse`), `Core::merge` + **`Core::own_over`** in `prism-bigraph` (+ the four
> registry duals, prism-side laws `tests/core_merge.rs` 10/10), `resolver.rs` (`resolve` →
> the linked Core + the import surface), the `chrysalis run` auto-resolve hook
> (`bin/chrysalis.rs`), and the consumer `tests/package_path_dep.rs` (a program runs a path
> dependency's `Tick` to `n=5.0` — the split runs identically to the monolith). chrysalis
> suite 254/0, prism-bigraph core_merge 10/10, prism-schema 198/0.
>
> **Design finding (dogfooded).** A *compiled* `Core` is **not** a package's theory — it
> bundles the theory WITH the base it compiled against (the std/builtin floor + the
> per-compile generic `Composite`/`Brs` factories every compile re-creates with fresh
> closures, which false-conflict on `merge`). So linking goes through
> **`dep_core.own_over(base)`** — the package's own theory over the shared floor — and
> `Core::merge` joins THEORIES. `own_over` is the dual of `merge` (merge joins; `own_over`
> takes the part strictly above the floor); it is what makes "a package = a Core" literally
> true at the registry level.
>
> **Import surface.** Linking (`Core::merge`) provides the runtime FACTORY; for a `.ys`
> program to *use* a dependency's process in a composite, the compiler also needs the
> compile-time NAME, so `resolve` returns a `ModuleRegistry` declaring each dependency's
> exported processes — `from <dep> import <Name>`. (Type exports ride the merged
> `TypeRegistry` ambiently and need no declaration.)
>
> **Deferred (correctly) to later phases:** transitive resolution + the diamond's single-apex
> dedup need per-package memoization (Phase 2, where `own_over` is parameterized by
> `base ⊔ deps`); version constraints/solver + lockfile (Phase 2); surfacing TYPE/function
> exports through the module registry (when a consumer needs it).

**Phase 2 — the resolver + the lockfile (transitive + semver).** Version constraints;
transitive resolution; a **version solver** (semver); conflict detection (= `Core::merge`
conflicts); a **lockfile** (`project.lock`, the pinned graph, reproducible). *Consumer:* a
3-package graph with a shared transitive dep + a constraint → a deterministic lockfile.
*(The one genuinely-new algorithm.)*

> **✅ DONE (2026-06-10, `pkg`).** Shipped + green (chrysalis suite, all package tests):
> - **`version.rs`** — an owned semver subset (no `semver` crate dep): `Version`
>   (`major.minor.patch`, ordered) + `VersionReq` (`^`/`~`/`=`/comparators/`*`, bare =
>   caret per Cargo) + `matches`. The compatibility functor over the version poset.
> - **`resolver.rs` is the resolver-as-COLIMIT** — *iterated `own_over`-then-`merge` to the
>   shared apex* (`unify`'s framing). A recursive DAG walk **memoizes each package's own
>   theory by name**, compiling it once against `base ⊔ its-resolved-deps`; so a **diamond**
>   (`prog→A,B`, `A→D`, `B→D`) resolves + links `D` exactly ONCE at the apex — *dependency
>   resolution is confluence.* Each edge's `VersionReq` is checked against the resolved
>   version; a same-name package reached at two incompatible versions is a conflict (one
>   version per name ⇒ the apex is well-defined); cycles are detected.
> - **`lockfile.rs` — `project.lock`** — the colimit's chosen section, as deterministic
>   `.ys`-data (one entry per name: version + relative source + edges), written through the
>   parser/unparser + a re-parse gate, sources relativized to the project root (portable).
>   `chrysalis run` writes it (best-effort); re-resolve ⇒ byte-identical.
> - **Consumers:** `tests/package_transitive.rs` (the diamond resolves + links `D` once;
>   `prog` runs `ATick`+`BTick` to `n=10.0`; a requirement violation + an incompatible
>   transitive version both error), `tests/package_lockfile.rs` (a constrained 3-package
>   graph → a deterministic, reproducible lock). Verified on the real binary (`chrysalis
>   run` on a 4-package diamond writes a `project.lock` with `D` pinned once).
>
> **Deferred to Phase 3:** the registry (a `name → versions` index makes the version SOLVER
> *choose* among many — here each path is one fixed version, so the "solver" validates +
> dedups); `chrysalis add` editing the manifest + lock through the `coord set` homoiconic
> round-trip; the lockfile as an INPUT that pins re-resolution (matters once a registry can
> drift; path deps already re-resolve deterministically).

**Phase 3 — the registry + `chrysalis add` + fetch/cache.** A package **registry** — start
**local** (a dir-based index `name → versions → manifest + checksum`; a content-addressed
cache). `chrysalis add <pkg>` (resolve → update manifest + lock — reusing the `coord set`
homoiconic edit), `chrysalis install` (fetch), `remove`/`update`. The registry is a
**pluggable backend** (local now; remote later). *Consumer:* `chrysalis add foo` from a
local registry, then run.

> **🟡 IN PROGRESS (2026-06-11, `pkg`).** **P3a — `chrysalis add` DONE + green.** `chrysalis
> add <name> [--path <p> | --native <p>] [--version <v>]` edits the nearest `project.ys` through
> the **homoiconic round-trip** — `manifest::add_dependency` reuses `coord::set_path` (now
> `pub(crate)`) + parse → unparse → **re-parse gate**, so a stray brace/quote can never wedge
> the manifest, exactly like the board heartbeats. Header + other fields preserved; verified on
> the real binary (added path/native/versioned deps to a real `project.ys`). **NEXT:** P3b the
> local registry (a `name → versions` index so `chrysalis add foo` with no `--path` resolves
> from the registry) · P3c install/remove/update + lock integration · P3d the consumer.

**Phase 4 — publish + remote registry + the first published packages.** `chrysalis publish`
(version + upload); a **remote** backend (HTTP — generalizing Phase 3's index, the move the
mesh transport gets); integrity (checksums). *Consumer:* publish `prism-std`, `audio`
(prism-audio), `spatio-flux` as the first packages; a fresh project `chrysalis add audio` →
A6's module factory rides it from the registry. **The ecosystem goes live.**

**Phase 5 — native-dep + workspace polish.** The native-dependency dimension (a `.ys`
project depending on a native crate's `domain_core()` — bridges the registry + cargo,
**rewriting the #13 codegen path onto the canonical run-Core**, dissolving the last
spatio-flux 3-door debt). Workspaces (multi-package). Semver-aware `chrysalis update`.
*(Sequenced FIRST, ahead of Phase 3 — `docs/packages-decomposition.md`: most domains are
native/mixed, so the breakout is gated on it.)*

> **✅ DONE (2026-06-11, `pkg` ⋈ `lang`).** The native convergence is COMPLETE + verified on a
> real crate — *native vs `.ys` is just where a part's Core comes from; `Core::colimit` is
> uniform*:
> - **P5a — `manifest.rs` native shapes** — a `native:` DEPENDENCY source (`dependencies: {
>   audio: { native: '../crates/prism-audio' } }`) AND a package's OWN native crate (top-level
>   `native: '.'`, the co-located mixed shape). `DependencySource::Native` + `Manifest::native`.
> - **P5b — `resolver.rs` `resolve_with_natives`** — colimits SUPPLIED native crate Cores
>   (their `domain_core()`s, keyed by edge name) `own_over` the floor, exactly like a `.ys`
>   part, surfacing process exports. `tests/package_native.rs` (native Core + `.ys` dep → n=10).
> - **`ModuleRegistry::merge`** — composes import surfaces (`std_modules() ⊔ a crate's own
>   `modules()`), the own-native runner's need. `tests/module_merge.rs`.
> - **P5c — the codegen runner (`lang`) — DONE.** Codegen links the native crates: own-native →
>   `run_command(own::core(), std_modules.merge(own::modules()))` (no resolver); native-deps →
>   `resolve_with_natives`; own+deps deferred. **spatio-flux migrated to the structured
>   `native: '.'` and runs identically** (kinetics + diffusion verified) — the decomposition's
>   first real mixed package. Legacy `package <name>` kept as a fallback.
> - **Deferred:** the mixed `resolve_with_own_native` (own native + `.ys` deps) — no consumer
>   until `bio`. Next: the domain breakout + `bio` umbrella → M4.

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
