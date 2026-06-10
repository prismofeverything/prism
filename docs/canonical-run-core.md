# Canonical run-Core — one Core through the `run()` boundary

> **Steward:** `unify`. **Surfaced by:** `synth` (2026-06-09, paused A6 Phase-2 on it).
> The #59 Core-threading rule reaching its LAST seam — the chrysalis `run()`/`compile`
> boundary — collapsing the 3-door split, adding the missing protocol door, so each
> domain exposes ONE `domain_core()`. Additive + small. Unblocks **synth A6 Phase 2** +
> **A8 `net:`**; the foundation the package/dependency layer (#67) sits on.
>
> **STATUS (`lang`, 2026-06-09): ✅ COMPLETE + GREEN — the 3-door split is RETIRED.**
> The canonical landed, ALL callers migrated, and the Felleisen gate is closed
> (`tests/run_core_one_door_guard.rs` ratchet is now `KNOWN_REMAINING = &[]`). Specifically:
> - `run` / `invoke` / `invoke_trace` / `invoke_driven` / `serve_stream` / `serve_process` /
>   `to_document` (runner.rs) take ONE `Core`; `cli::run_command` + the bin + codegen
>   template (`prelude::{core, modules}`) follow.
> - `compile_with_modules` / `compile_with_registry` / `compile_with_methods` **deleted**;
>   `compile_inner` **folded into `compile_with_core`** so the registries are LOCALS
>   decomposed from the Core — there is no `registry: ProcessRegistry` door anywhere.
> - The **protocol door** is real (a domain's protocols ride the Core; the hard-coded
>   `stream_protocols()` is gone). Enabler: `prism_bigraph::ProcessRegistry` is `Clone`
>   (`FactoryFn` Box→Arc, contained to `factory.rs`).
> - **spatio-flux's 6-fn prelude workaround collapsed** to `sf_core()` + the codegen
>   convention `core()` / `modules()` (a package = a Core); its bin + tests migrated.
> - Fixed `std_core()` (it had been missing the `Document` + quantum methods that
>   `std_methods()` carries — surfaced when callers moved off the 3-door).
>
> Green: chrysalis 245 / spatio-flux 37 / prism-bigraph 75. Proven by
> `tests/canonical_run_core.rs` (one Core threads to the engine; the protocol door carries
> the domain's protocols; a domain-native process reaches a `.ys` program through the one
> Core) + the `&[]` ratchet. The `names()`-from-Core import-surface refinement remains moot
> (`type_names()` already exists). **This unblocks synth A6 Phase 2 and A8 `net:`.**

## The diagnosis (verified 2026-06-09)

`prism_bigraph::Core` is the ONE unified runtime — types + processes + methods +
protocols — and `run_state` (`runner.rs:44`) / `run_document` (`:97`) ALREADY take it.
But the top-level path **splits it back apart**:

- `run(program, registry, methods, modules, time)` (`runner.rs:55`) — processes via
  `registry`, methods via `methods`, native **types** through a *third* door
  (`modules.type_`, the `ModuleRegistry`), and native **protocols have NO door**:
  `compile.rs:463` hard-codes `.with_protocols(stream_protocols())`, so a domain can't
  inject its own — **A8 `net:` blocked at the seam.**
- spatio-flux works around the split with a 6-function prelude (the #13 debt). synth hit
  the same wall on A6 Phase 2's `.ys` face (how does a `.ys` program reach prism-audio's
  native `Oscillator` etc.?) and refused to copy the workaround.

This is exactly the "registry-subset drift" the Core-threading rule forbids
([[project_core_unification]]), at the one boundary it never reached.

## The canonical (the fix)

**Thread the ONE Core through `run`/`compile`:**

```
run(program, domain_core: Core, modules, time)   // a domain passes its WHOLE Core
```

- Collapses **3 doors → 1**: procs + types + methods + protocols all ride the Core.
- Adds the missing **protocol door** — a domain injects its own protocols (retiring the
  `compile.rs:463` hard-code) → **A8 `net:` unblocked.**
- Each domain exposes **ONE `domain_core()`** (`audio_core()`, `sf_core()`); chrysalis
  merges the program's own defs in. spatio-flux's 6-function prelude dissolves to one
  `sf_core()`.
- **Additive** — `run_state`/`run_document` already take a Core, so the canonical `run`
  is a small additive sibling: land it beside the 3-door `run`, migrate callers (CLI +
  tests), then retire the split (keep-it-green).
- **Refinement (deferred):** deriving the import-surface FROM the Core (one source of
  truth for a domain's exported names) needs `ProcessRegistry::names()` — none today
  (only `contains()`, `factory.rs:41`; cf. `ProtocolRegistry::names()`, `protocol.rs:261`).
  A small additive core method, **deferred post-#71** so it does not pull `core` off the
  units arc.

## Recognition

This is **#59 (the generative-core Core-threading unification) reaching its last
boundary.** Same generator (one Core threaded everywhere; collapse the doors); new seam
(`run()`/`compile`). Felleisen-gated: it DELETES the 3-door split, the spatio-flux
prelude workaround, and the hard-coded protocols.

## Owners + sequencing (this unpauses the team)

- **lang** (leads the unblock): the canonical `run`/`compile` Core-threading (additive
  sibling → migrate callers → retire the split) + the **protocol door**. PLUS A6 Phase
  2's *other* lang ask — the **rich-kind constructor** (`Process[…]`/`Composite[…]` as
  value forms; today only `Reaction[…]`/`Pattern(…)` are value constructors) so a reactum
  can construct a module-type value inline. A6 is the consumer that finally justifies the
  bracket the homoiconic work deferred "for a consumer."
- **synth** (consumer): expose `audio_core()`; resume A6 Phase 2 against lang's progress
  (build a module value inline + reach prism-audio modules through the one Core). The
  dogfooding consumer that proves it.
- **core**: **STAYS on #71** (units, isolated worktree). The only core ask —
  `ProcessRegistry::names()` — is **deferred post-#71**; the run-Core lands with the
  existing `contains()`. This does NOT pull core off units.
- **simplify**: a one-door guard — *one Core threaded to the `run()` boundary, no
  registry-subset door* (the #45/#59 closure-guard extended to `run`/`compile`).
- **unify**: this recognition + the thin-layer discipline + the package relationship.

## Relationship to packages / dependencies

**Today** a `.ys` program *can* reach a native domain's modules — but via the fragmented
3-door path + a per-domain prelude workaround. **What exists:** `project.ys` (spatio-flux
has one), the explicit-origin import resolver, `chrysalis new` (#31 scaffolder),
per-domain codegen (#13 — spatio-flux is the first non-std `.ys` package). **What does
NOT exist:** `chrysalis add`, a package **registry**, a **lockfile**, semver, transitive
dependency resolution (**#67**, the "packages ecosystem" milestone).

The canonical run-Core is the **foundation**: **a package = a Core (procs + types +
methods + protocols) + an import surface.** Once a domain exposes one `domain_core()`,
"depend on package P" = "run against P's Core." `chrysalis add` / registry / lockfile
(#67) is the **management layer ON TOP** — a separate, bigger arc, **deferred** (do not
build it under A6 pressure; it's a deliberate future milestone). So: land the canonical
run-Core now (unblocks A6 + A8, founds packages); the full dependency-management
ecosystem is its own arc later.
