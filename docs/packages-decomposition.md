# Packages decomposition — the domains as a category of theories

> **Stewards:** `unify` (the spine) + `pkg` (the mechanism). Companion to
> [`packages-ecosystem.md`](packages-ecosystem.md) (the package *system* — pkg's 5
> phases) and [`grand-synthesis.md`](grand-synthesis.md) (M4, the one-engine demo).
> This doc is the *consumer* the package system was built for: the project's **domains
> become packages**, and that IS the concrete substrate for the ultimate demo.
>
> **Decisions taken with the human (2026-06-10):** **Phase-5-first** sequencing · a
> **top-level `packages/` dir** · **`bio` as an umbrella** depending on `spatio-flux` +
> `mapk`.

## 1. Thesis — the decomposition IS the one-engine substrate

Each domain is a **presented theory** (`categorical-core.md`). Making each a **package**
turns the project into a *navigable category of theories*:

| categorical | packaging |
|---|---|
| object (a presented theory) | a **package** (a `Core` + manifest) |
| morphism (a functor/inclusion) | a **dependency** edge |
| the shared base | the **`std`** package (the colimit's apex) |
| the colimit of the diagram | **`resolve_core`** (pkg's Phase-2 resolver) |

So the **M4 one-engine demo = a single `project.ys` that depends on `bio + quantum +
synth` and colimits their Cores into one running engine.** The decomposition is not
cleanup — it is the substrate for the ultimate demo. pkg's Phases 1–2 already built the
machine (resolve = colimit; the diamond linked once at the shared apex via
`own_over`-then-`merge`).

## 2. A package = a Core — three shapes

A package's `Core` (`types · processes · methods · protocols`) can come from `.ys`, from
native Rust, or both — **colimit'd together**. Native vs `.ys` is just *where a part's
Core comes from*; `Core::colimit` is uniform.

| shape | the Core comes from | example today |
|---|---|---|
| **pure-`.ys`** | `.ys` modules compiled by `resolve_core` | a `quantum` / `cells` package |
| **native** | a Rust crate's `domain_core()` fn | `../parsimony` → `spatial` |
| **mixed** | native `domain_core()` ⊔ `.ys` modules | `prism-audio` (`audio_core()`), `spatio-flux` (`sf_core()`) |

`spatio-flux` is the existing **template**: a native `sf_core()` + a `project.ys` + a
`ys/` subdir of modules built on its native types.

## 3. The native convergence (Phase 5) — the gate

**Today there are two unconverged manifest mechanisms** (`manifest.rs` even says so):

- **pkg's resolver** (`manifest.rs` / `resolver.rs`): a `.ys`-data manifest whose
  `dependencies` are **`.ys` path deps** → compiled + colimit'd. *No native dimension.*
- **the old `codegen.rs`**: a `package <name> [at <path>]` directive that links a **native
  crate** by generating a runner crate calling its `prelude::{core, modules}`. *This is how
  `spatio-flux`'s `.ys` reaches `sf_core()` today.* No `.ys` deps / version / transitive.

**Phase 5 unifies them onto the canonical run-Core:**

1. `manifest.rs` gains a **native dependency source** beside `path:` —
   `dependencies: { audio: { native: '../crates/prism-audio' } }`.
2. `resolve_core`, on a native dep, drives the **codegen path** (build the crate, obtain
   its `domain_core()`) and **colimits that Core in** — the same `Core::colimit` it uses for
   `.ys` parts. One resolver, two Core *sources*.
3. The native crate exposes one **`domain_core()`** (the convention the codegen template
   already calls: `prelude::{core, modules}`; concretely `audio_core()` / `sf_core()`).

After Phase 5, *"depend on package P"* = *"colimit P's Core,"* whether P is `.ys`, native,
or mixed. **This is the answer to "can packages include Rust?": yes — a mixed package is a
Cargo crate (kernels + `domain_core()`) + a `project.ys` + a `ys/` dir, and the resolver
colimits its native and `.ys` parts into one theory.**

### P5c — the codegen runner (the gate; `lang` ⋈ `pkg`)

chrysalis is a fixed binary — it cannot link an arbitrary domain crate in-process. So a
manifest with native parts **generates a runner crate** (the existing `codegen.rs` path,
EXTENDED) that links those crates, obtains each `prelude::core()`, and calls
`resolve_with_natives(manifest, modules, native_cores)` — the *same* resolver as in-process,
just with the native Cores supplied. **Reuse, don't clone:** the runner template already
calls `prelude::{core, modules}` + the canonical `run`; P5c generalizes it from ONE crate to
N (a `[dependencies]` entry + a `native_cores` entry per native part) and routes through
`resolve_with_natives`. The runner is a *transport* for native Cores, not new semantics.

**Two native shapes the manifest must express (the design question for the pair):**
- a package's **OWN** native crate — the co-located *mixed* case (`spatio-flux`'s `sf_core()`
  ⊔ its `ys/`; `prism-audio`'s `audio_core()`). *Recommend* a top-level `native: '<path>'`
  field (the package's own crate), distinct from `dependencies`.
- a native **dependency** — one package depending on another whose Core is native (`bio` →
  `spatio-flux`). P5a's `dependencies: { sf: { native: '<path>' } }`.

Both feed `native_cores`; a package's own `.ys` + own native core + its deps' Cores all
colimit. The legacy `package <name> [at <path>]` directive (one co-located native crate)
**migrates to the structured manifest's `native:` field** and retires (Felleisen — one
manifest, one codegen path).

**Consumer (no half-measures):** a REAL native crate via a generated runner — `spatio-flux`
or `prism-audio` as the first mixed package (`.ys` modules over a native `domain_core()`),
replacing P5b's hand-built Core. *That consumer IS the decomposition's first real package.*

**Seam split:** `pkg` drives the manifest→runner generation (package logic); `lang` owns the
`codegen.rs` template + `cli.rs` dispatch fit (the surface) + guards the canonical-run-core
path against regression. Coordinate-first; pair.

## 4. The map (roster → packages)

```
packages/
  std        prism-std + std_core               (base — everyone deps it)        native+ys
  quantum    quantum-*.ys (+ native effects)     deps: std                        mostly-ys
  manifold   kuramoto*.ys + ../manifold          deps: std, mesh                  mixed
  synth      prism-audio + ../gorgon + ys/       deps: std                        mixed
  spatial    ../parsimony                        deps: std                        native
  mesh       prism-bigraph protocols             deps: std                        native
  bio        cell/grow/divide/environment.ys     deps: std, spatio-flux, mapk     UMBRELLA
             └─ depends on → spatio-flux (native+ys) + mapk (native)
```

The monolith `crates/chrysalis/ys/` (≈50 files, already domain-clustered) **empties into
these packages**; the language-exercising demos (`homoiconic`/`ast`/`eval`/`alchemy`) go to
a `lang-demos` (or `examples`) package, not a domain.

## 5. `bio` — the umbrella (the "exercise everything" package)

`bio` is a package that **depends on** the `spatio-flux` + `mapk` packages + `std`, and
holds the cell-level `.ys` (`cell` / `grow` / `divide` / `environment` / `grow-divide-*` /
`mr` / `nuclear-shuttle`). As an umbrella it exercises, in one graph: **path deps +
transitive resolution + a native dep (post-P5) + a lockfile + version reqs + exports** —
which is exactly pkg's "a real package that exercises everything," by construction. It is
the natural shape (it mirrors the domain hierarchy) and the resolver's hardest local test.

## 6. Layout

- A new top-level **`packages/<domain>/`** holds each domain package: a `project.ys`
  manifest + a `ys/` dir (+ a `lib.ys` library entry, the resolver's `src/lib.rs` analogue).
- **Native crates stay in `crates/`** (and `../manifold` / `../parsimony` stay siblings) and
  simply **expose `domain_core()`**; a mixed package's `project.ys` declares a `native:` dep
  on that crate. Tool (`chrysalis`) stays cleanly separated from content (`packages/`).
- `spatio-flux` and `prism-audio` already co-locate native + `ys/`; their package
  `project.ys` moves to the new structured manifest (the legacy directive retires in P5).

## 7. Sequencing — Phase-5-first

1. **pkg — Phase 5 (the gate):** the native-dep convergence (`manifest.rs` `native:` source +
   `resolve_core` linking it via codegen, onto the canonical run-Core; retire the legacy
   directive). Lands with its consumer: a `.ys` package depending on `prism-audio` natively.
   *Touches `codegen.rs` / `cli.rs` — coordinate-first with `lang`.*
2. **Domain agents — expose `domain_core()`:** `std`/`audio`/`sf` have one; **`../manifold`,
   `../parsimony`, `mapk` add one** (a small additive `prelude` fn, the canonical convention).
3. **Break out ALL domains** into `packages/<domain>/` (pure-`.ys` + mixed together), `bio`
   as the umbrella; the monolith empties.
4. **M4:** a one-engine `project.ys` depending on `bio + quantum + synth`, colimit'd into one
   distributed/streaming engine (`grand-synthesis.md`).

## 8. Owners

- **pkg** (leads): Phase 5 (the native convergence) + the breakout + the `packages/` layout.
- **lang** (coordinate-first): the shared `codegen.rs` / `cli.rs` / import-resolver seams.
- **domain agents** (`synth`/`bio`/`spatial`/`manifold`/`quantum`/`mesh`): expose
  `domain_core()` + author their package's `project.ys` + move their `.ys` in.
- **unify** (steward): this doc + the spine (decomposition = the category of theories; the
  M4 substrate) + connecting the pieces across agents.
