# Next session — launch prompt & plan

## ⏯️ ON BOOT — paste this to rebuild the task list

> Resume prism (Rust process-bigraphs + the `.ys` language). Read this prompt +
> the latest dated `⏯️ NEXT-SESSION PROMPT` below + the `MEMORY.md` index. Then
> **rebuild the harness task list** from "## Task list (durable canonical)" below:
> `TaskCreate` one task per entry, `status: completed` for ✅, `pending` for ⏳/🧩.
> Then continue from the NEXT items in the latest dated prompt. (The harness task
> panel is EPHEMERAL — it does not survive a restart; THIS file + MEMORY are the
> durable record. The IDs below match the panel so reconstruction is faithful.)

## Task list (durable canonical — rebuild on boot)

- ✅ #1 standalone cell.ys + failing stream conservation test
- ✅ #2 serve_stream / serve_process delta-forwarder
- ✅ #3 grow-divide over stream == rest (boundary proof)
- ✅ #4 real parallelism — Defer seam, ParallelPool, stream concurrent (steps 2.5/3/4)
- ⏳ #5 complete law generators over node sorts + fix flaky law + uniform node-data
- ⏳ #6 explicit bridge conduits (today inferred by inner-slot absence)
- ✅ #7 cleanup — collapse address-scan + the two discovery walks (DONE 2026-05-25 — `extract_processes` + `discover_all_processes` collapsed into one `scan_for_processes` path; the address-fallback is EXPUNGED — discovery is schema-first via `is_link`, with explicit `_type: "process"|"step"|"link"|"composite"` (upstream process-bigraph convention) as the bootstrap for dynamic adds. Producers workspace-wide migrated.)
- ⏳ #8 SBML→CRN importer / repressilator (original science goal)
- ⏳ #9 dt-refinement convergence sweep
- 🧩 #10 comment-preserving parse/unparse + retire extern — extern FULLY RETIRED (no `Tok::Extern`/`Def::Extern`); REMAINING: retain comments through `parse → unparse` (the lexer skips `#`-to-EOL; roundtrip tests verify AST, not comments)
- 🧩 #11 prism-svg: SVG as place-graph values — typed SVG nodes DONE (`crates/prism-viz/src/svg.rs`: `el`/`svg`/`rect`/`line`/`text`/`group`/`polyline`/`animate`); `prism_viz::plot` emits typed SVG. REMAINING: retire `render_timeseries_svg`'s plotters-string path
- ⏳ #12 real contract enforcement through RunProcess
- 🧩 #13 spatio-flux as a `.ys` project (codegen slices 3-4) — codegen MVP DONE; `culture.ys`/`dish.ys` migrated to imports; 6 `*-section.ys` files cover one representative per family + `report.ys` composes them. REMAINING: 12/18 of `CANONICAL_ORDER` not yet ported (the rest of the dFBA family, comets variants, spatioflux_reference_demo)
- ⏳ #14 chrysalis diagnostics: comprehensible `.ys` errors
- ⏳ #15 schema-as-state: meta-schema, reconcile state to match — paired with #30 (the unified entity registry that this operates on)
- 🧩 #16 surface-language defaults — input defaults + default harness + dynamic-τ OVERRIDE DONE (`composite_param_env`, `cli::harness_process_entry`, `bench_run.rs`, `overwrite[Float]`, `gillespie.ys`); REMAINING: an inner-wire-typed `interval` whose `default` is `%.port`, retiring the engine's `[name,"interval"]` side-channel
- 🧩 #17 enforce `::`=type / `:`=value / `fulfills`-contract syntax — lenient phase LIVE (`accept_type_sep` accepts `::` or `:`; `accept_contract_sep` accepts `fulfills` or `::`); REMAINING: migrate every `.ys` + GUIDE/design to canonical `::`/`fulfills`, then tighten the parser
- 🧩 #18 file-as-composite streaming slices + type-driven outputs — Trace kernel, delta-log/Arrow codec, `chrysalis run --<port> SOURCE`/`--in TRACE`/`--serve-stream`, schema-header + `refines` check at connect, `prism_viz::plot(schema, trace)` (line/animated-heatmap), `report-section.ys` ALL DONE. REMAINING: `--map out=in` + `--adapt adapter.ys`; 4D structural plot per `_add`/`_remove`; apply down `CANONICAL_ORDER` validating each family
- ⏳ #19 performance sweep (LAST, after features)
- ✅ #20 run parallel stream cells from env.ys (end-to-end surface)
- 🧩 #21 parallelism follow-ups — rest-concurrent + `ParallelPool`/`flush_pending` DONE (`rest_engine.rs`, `protocols/parallel.rs`); REMAINING: batched ray (a `ray.rs` protocol with one packet/shard — same as canonical #25)
- ✅ #22 protocols as Custom types + composite IS-A type (one program-aware lowering; `composite_is_a_type`)
- ✅ #23 modernize `.ys` files — DONE: (a) width-aware unparser with pipes-at-end-of-line (matching composite/process bodies), break threshold `MAX_WIDTH = 100`; `mapk.ys` longest line 326 → 118; (b) `grow-divide-unbounded.ys` migrated off legacy `env.run(t)` — now a trailing `Environment[...]` entry, runs clean via `chrysalis run --time`; (c) `grow-divide-glucose.ys` migrated off `env.run(t)` as well — still in `ys_files_run::SKIP` because its reaction's `?c.divide()` reactum is a homoiconic method call on a sited cell (blocker rolled into #30); (d) `mapk.ys` regenerated readably + its SKIP reason refreshed to the real blocker (reactions aren't yet resolvable as values, also #30); (e) new `chrysalis format <file.ys> [-w]` subcommand — `parse → unparse`, prints to stdout or rewrites in-place. Warns + refuses `-w` on commented files until #10 lands (comment preservation). The remaining two SKIPs are both pre-reqs for the unified entity registry.
- ✅ #24 RETIRE branch_schema — derive inner schema via the algebra
- ⏳ #25 distributed: batched `ray:` protocol (Form A — `flush_pending`, one packet/shard) — `docs/distributed-execution.md` phase 1
- ⏳ #26 distributed: halo/neighbor exchange + 2-environment diffusion-across-a-boundary demo — phase 2
- ⏳ #27 distributed: static spatial-partition protocol (the octree; environment-of-environments across machines) — phase 3
- ⏳ #28 distributed: dynamic load balancing (split/migrate overflowing subdomains) — phase 4
- ⏳ #29 distributed: adopt a cluster backend (Charm++ / Ray / MPI) behind the `Protocol` interface — phase 5
- 🧩 #30 unified entity registry — "a type is a control with extras" (homoiconic unification, paired with #15). Every capitalized name (`Cell`, `Compartment`, `MEK`, `Grow`, `BRS`, `CRN`, …) is ONE `EntityDef` in one registry, with optional slots: `{schema, methods, process, composite, reactions, ports}`. The lowercase definers (`process`/`step`/`composite`/`reaction`/`pattern`/`type`/`control`) are SUGAR that fills slots; the same name carries the same identity across all of them. "Bare" controls (used in `mapk.ys` for `Compartment`/`Cytoplasm`/`MEK`/etc.) become the minimal case — a label with no extras, valid bigraph atoms with no declaration required. An optional `control Foo` declaration is the explicit form (enables typo detection, declared ports). Pairs with #15: once unified, schema-as-state mutates slots uniformly. Pre-req for tier-2 evolving M/R. *Large; foundational. Sliced:*  
  - ✅ **Slice 1A — reactions as values.** `Expr::Var(name)` lookup unified into one match-over-Def so each definer kind contributes ONE arm naming how its identity becomes a value (`Composite`/`Process`/`Step`/`Reaction` all flow through a no-arg `Term`). Unblocked `mapk.ys`'s `rules: [phosphorylate, dissociate, …]`; off `ys_files_run::SKIP`, runs end-to-end.  
  - ✅ **Slice 1B — bare controls as values.** Confirmed already working: a capitalized name with no Def parses as `Expr::Term { control, args: [], body: None }` (not `Var`), which `eval_term_value` resolves via the free-control / `build_plain_map_value` path. So `kind: Cytoplasm` etc. already evaluates correctly. No code change required.  
  - ✅ **Slice 2 — `EntityView` (unified lookup).** `ast::EntityView<'a>` + `Program::entity(name)` collapse every Def-kind into one borrowed view with optional slots (`function` / `process` / `step` / `composite` / `reaction` / `pattern` / `type_def` / `contract` / `protocol` / `unit` / `context` / `binding`). The view is the eval-side spine: `Expr::Var` resolution and `eval_term_value`'s dispatch are now ONE entity lookup followed by priority-ordered slot dispatch (composite > process > step > reaction > protocol > function-error). Scattered `match self.program.lookup(name) { Def::X | Def::Y | ... }` collapsed into one access pattern; future Def kinds slot in by adding a field, not by editing every match. Tests in `chrysalis/tests/entity_view.rs`.  
  - ✅ **Slice 2b — owning `EntityDef` + AST-as-value (summary).** `ast::EntityDef` is the OWNED form of the view (`EntityDef::new(name).with_process(d).with_reaction(r)…`); `Program::owned_entities()` / `entity_owned(name)` produce / look up the registry as standalone values. `EntityDef::to_value()` + `Program::to_value()` emit the homoiconic summary shape `{_type: "EntityDef", name, slots, process: {inputs, outputs}, …}`. `load()` stashes `_entities` alongside `_source` in the Document Value so a `.ys` caller can inspect the program's named contributions WITHOUT re-parsing. The user's mental model — "start with an empty Entity, then add things to it until it's whatever ys program" — is *literally* this struct + chainable `with_…` slot-setters. Tests in `chrysalis/tests/entity_as_data.rs`; the homoiconic-demo `.ys` now also emits `entities:` alongside `via_loaded`/`via_manual`.  
  - ⏳ **Slice 3 — methods on sited cells + `?c.divide()`.** Pre-req: `?c :: Cell` binds the cell as a typed value; method dispatch on that. Unblocks `grow-divide-glucose.ys`.  
  - ⏳ **Slice 4 — `control Foo` declarative form** (typo detection, declared ports).
- 🧩 #32 programs-as-data: in-language `load()` + demos — the homoiconic principle made visible.  
  - ✅ **JSON round-trip demo** (`programs_as_data.rs`): `grow-divide-unbounded.ys` runs identically as a Program (parsed source) AND as a Document VALUE (`document_of` → JSON → `from_str` → `run_document`).  
  - ✅ **`from io import load` + `Document.run(time)`** wired through compile/eval. Native FUNCTION imports now flow through `ResolvedImports::functions` → `Evaluator::imported_functions` → `Expr::Call`'s dispatch (previously errored with `bare-function import is not supported yet`). `prelude::load(path) → Value` exposed publicly. Loaded Documents carry `_source` so `.run` can re-compile for the right Core. Tests in `chrysalis/tests/load_in_language.rs` exercise the Rust API.  
  - ✅ **Hand-built `==` loaded** (`load_in_language.rs::document_run_dispatches_for_loaded_and_hand_built_alike`): runs BOTH `load(path)` AND a HAND-BUILT `Value::Map` with the same shape through `Document.run(5.0)`; both produce `count: 5.0`. The Lisp parallel made concrete — load ~ read, Document.run ~ eval, map literals are the data-form cons/list.  
  - ✅ **Surface-`.ys` demo** (`load_in_language_ys.rs`): a `.ys` file uses `from io import load` + map literals to express both paths and runs through the `chrysalis` bin. The composite's two output ports (`via_loaded`, `via_manual`) both emit 5.0. The homoiconic identity is visible to anyone running `chrysalis run`.  
  - ✅ **Shipped `homoiconic-demo.ys`** in `crates/chrysalis/ys/`: a permanent demo file (+ `homoiconic-demo-tick.ys` companion) showing `load(path).run(time)`, hand-built `{_type: 'Document', _source: ...}.run(time)`, AND reading `loaded_doc._source` to see the resolved path — all in one composite. Runs via `chrysalis run crates/chrysalis/ys/homoiconic-demo.ys`. Picked up by `ys_files_run` auto-smoke. Output: `{"via_loaded": 5.0, "via_manual": 5.0, "source_path": "..."}`.  
  - ✅ **`load(path)` resolves relative to the entry file's dir** via `std_modules_at(ys_root)` (the same convention `from … import` already uses). The bin's `cmd_run` computes ys_root from the entry path; `Document.run`'s re-compile path uses the source file's parent. So demo `.ys` files can reference siblings without absolute paths.  
  - ⏳ **REMAINING**: (a) `Document.run` returns full state (incl. internal process specs); a variant returning only the OUTPUT FACE would let loaded results compose into outer programs without extracting fields. (b) The bin's entry-rule for trailing scalar/method-chain expressions — today `chrysalis run` needs a composite/process entry, so the surface demo wraps in `composite HomoiconicDemo`.  
  - ✅ **STRETCH (one-way) — Expr → Value** (the AST as walkable data). Every `Expr` variant serializes via `Expr::to_value()` to `{_type: "<Variant>", …fields}`; recursive — `Term` → ports/body recurse; `BinOp`/`If`/`Let`/etc. recurse into their sub-Exprs. `EntityDef::to_value()` now includes the body in each slot summary (`process.body` / `composite.body` / `reaction.{redex, reactum}` / `function.body`). Combined with `load(path)._entities`, the WHOLE program's AST is visible as data from inside `.ys` — the homoiconic-demo now shows the loaded program's parallel composition + KeyedEntry + Var wires under `entities`. Tests in `entity_as_data.rs::expr_to_value_serializes_every_variant_the_parser_emits` and `…loaded_documents_carry_their_entities_as_walkable_ast`.  
  - ✅ **STRETCH (round trip) — Value → Expr.** `Expr::from_value(&Value) -> Result<Expr, ExprFromValueError>` covers the common body variants (literals / Var / Path / Term / Parallel / KeyedEntry / Map / Record / List / Let / Block / If / BinOp / UnaryOp / Method / Field / Call / Unbound / LinkVar) plus the supporting types (StringLit / PlacePath / TermArg / PortBindings / BinOp / UnaryOp). Rare/pattern-only variants (Comprehension / Rule / Site / ReplaceWith / Where) return an explicit error. Round-trip identity verified: `to_value(from_value(to_value(expr))) ≡ to_value(expr)`. Tests in `entity_as_data.rs`.  
  - ✅ **TIER-2 STARTER — `eval(ast)` callable from `.ys`.** `from meta import eval` exposes `chrysalis::prelude::eval(value)`: take any Expr-shape Value, `from_value` it to a real `Expr`, evaluate against an empty env, return the result. The chrysalis `(eval '(+ 2 3))` — Lisp's data-as-program made callable. Shipped demo `crates/chrysalis/ys/eval-demo.ys`: assembles sub-expressions as named values (`def two = {_type: 'Float', value: 2.0}`, `def plus = {_type: 'BinOp', op: 'Add', lhs: two, rhs: three}`), then composes them into a compound and runs `eval(compound_ast)`. Output: `{"add_result": 5.0, "sub_result": 6.0, "compound": 11.0}`. Programs ARE data, runnable from inside `.ys` — the homoiconic identity is now end-to-end visible in surface code.  
  - ✅ **TIER-2 — `eval(value, env)`**: the two-arg form takes a bindings Map (`{x: 5.0, …}`); hand-built `Var("x")` references resolve through it. `chrysalis::prelude::eval_with(value, Some(env))` is the Rust entry; the HostFn picks one-arg vs two-arg based on args. Shipped demo `eval-demo.ys` now also shows `eval({Var(x) * 2.0}, {x: 5.0}) → 10.0`. Tests in `entity_as_data.rs::eval_with_env_resolves_var_references`.  
  - 🎯 **North-star milestone** (user's): *run the distributed/streaming environment with a HAND-CONSTRUCTED Cell.* Build the Cell composite — mass + glucose ports + Grow body + Divide step — as a `Value::Map` (no parser), splice it into `environment.ys`'s cells, run. Pre-reqs: (a) `EntityDef::from_value` (round-trip whole entities, not just Exprs); (b) a `compile_entity(value) -> CompositeDef`-style entry the runner accepts; (c) the stream protocol consumes a hand-built composite spec the same as a parsed one. Each piece is straightforward; the end-state is the full tier-2 demonstration.  
- ✅ #31 `chrysalis new <dir>` project scaffolder — creates `<dir>/project.ys` + `<dir>/main.ys` (flat layout: chrysalis's `ys_root = entry_file_dir` convention makes flat natural; growing projects can move sources into `ys/` later). `project.ys` is a comments-only manifest with the `package <name>` directive commented out — so the scaffolded project is std-only by default and runs in-process without triggering the codegen path. `main.ys` is a minimal Tick + Main composite that produces `count: 5.0` after `--time 5`. `--force` to overwrite; package-name sanitization for non-identifier dir names. Tests in `chrysalis/tests/scaffold.rs` (scaffold runs end-to-end; refuses to overwrite without `--force`).
- ⏳ #33 KISAO IDs + SED-ML/OMEX export (contract rung 3) — see `docs/kisao-export.md`. Slices: (A) `kisao: 'KISAO:…'` field on method-axis contracts; (B) `chrysalis bigraph export --as sedml` emits `<uniformTimeCourse kisaoID=…>`; (C) `chrysalis bigraph import-sedml` reads SED-ML, looks up KISAO in contract registry, instantiates the matching prism integrator; (D) OMEX archive round-trip. Contribution back to the standard: contracts-aware advisor over SED-ML (the inverse direction tooling doesn't have).
- 🧩 #35 algebraic effects + handlers — **goal: quantum bigraphs through algebraic effects** (paired-design with `docs/exploring-the-computational-unknown.md`'s quantum-bigraphs entry). See `docs/effects-and-handlers.md` for the full spec — recognizes today's 7 implicit effect systems (protocols, dispatch, apply, scheduling, discovery, schema check, trace) as instances of one Plotkin-Pretnar pattern, plus §XI quantum case study (state-rep swap, categorical constraints, composition). Slices:  
  - ✅ **Slice 1 — eval-time `handle(expr, handlers)`** + breadth. `chrysalis::prelude::handle(expr_value, handlers_value)` materializes each `{params, body}` handler as a synthetic `Def::Function` in a scoped Program; the existing call-resolution path dispatches `Call(name, args)` through the matching handler body. Four tests in `effects_first_slice.rs`: (1) same `choose(2.0,3.0)` returns 2.0/3.0/5.0 under three handlers; (2) handler-body computes via BinOp expression; (3) **handlers scope through nested expressions** (`add(choose(2,3), 10)` → 13); (4) **multiple effects in one bundle** (choose+pick+combine all handled at once). Surface `.ys` demo `effects-handle-demo.ys` ships in `ys/`. Registered as `from meta import handle`.  
  - ✅ **Quantum interference demonstration** (`crates/chrysalis/ys/quantum-interference.ys`). Same expression `flip(flip(initial_state))` evaluates under TWO handlers. Quantum handler implements Hadamard `H` (amp_0' = (a+b)/√2, amp_1' = (a-b)/√2); applied twice → returns to |0⟩ via destructive interference (output `{amp_0: 1.0..., amp_1: 0.0}`). Classical handler implements uniform randomization; two flips stay uniform (`{amp_0: 0.5, amp_1: 0.5}`). Two qualitatively different physics through the SAME expression.  
  - ✅ **Bell state — entanglement** (`crates/chrysalis/ys/quantum-bell-state.ys`). Two-qubit state representation as a Map keyed by basis-bitstring (`b00`/`b01`/`b10`/`b11`). Expression: `cnot_01(hadamard_q0(init_00))` with two gates dispatched through the handler bundle. Output: `(|00⟩ + |11⟩)/√2`, the canonical maximally-entangled Bell state. **`prob_same = 1.0`** (measure-same-basis correlation; uniform random would give 0.5). Real entanglement through eval-time effects + the homoiconic substrate, with NO engine refactoring. Proves the §XI thesis end-to-end at the eval layer — including the multi-qubit / entanglement structure.  
  - ✅ **Quantum measurement** (`crates/chrysalis/ys/quantum-bell-measure.ys` + `meta::sample` HostFn). Computes Born probabilities (`|amp|²` per basis state) from the Bell state, then samples four times with different seeds. Every observed outcome is `b00` or `b11` — never `b01` or `b10`. **Real entanglement OBSERVED**, not just computed. `sample(distribution, seed)` is deterministic given seed — pre-quantum-handler-bundle work; slice 9 will fold this into a `measure` handler with proper state-collapse semantics.  
  - ✅ **Engine-level quantum bigraph** (`crates/chrysalis/ys/quantum-engine.ys`). `Hadamard` and `CNOT` as real chrysalis `process` definers with `~{state :: overwrite[any]} ->{state :: overwrite[any]}` ports. Composite `BellCircuit` chains them across two state slots (`s_init → s_after_h → s_after_cnot`); the engine's BSP tick propagates state across them. **Bell state emerges after 2 ticks via the same engine that runs `hand-built-cell.ys`'s mass-balanced cells**. Entanglement structurally visible: the wire `s_after_h` shared between Hadamard's output and CNOT's input IS the correlation. The §XI engine-level thesis: same engine, classical OR quantum semantics, distinguished by which processes are wired in. No handler bundle needed for this slice — the overwrite-typed `apply` is generic enough to carry quantum gates.  
  - ✅ **Full engine-level quantum experiment** (`crates/chrysalis/ys/quantum-engine-measure.ys`). Extends the engine demo with a `Measure[seed]` process that calls `meta::sample` inside its body to collapse the state to an observed basis outcome. Four state slots; three BSP ticks; the full pipeline `init → H → CNOT → measure` runs through the engine. Output: `observed: "b11"` — a real measurement of an entangled state, deterministic given the seed. **The complete quantum experiment as a bigraph that the engine schedules.** Made `meta::sample` tolerate zero-weight distributions (warmup ticks).  
  - ✅ **3-qubit GHZ state through the engine** (`crates/chrysalis/ys/quantum-ghz.ys`). H + CNOT(q0,q1) + CNOT(q0,q2) + Measure as four processes, five state slots, 4 BSP ticks. Final state `(|000⟩ + |111⟩)/√2`; observed outcomes always `b000` or `b111` (perfect 3-way correlation). Generalizes the Bell-state engine pattern to 3 qubits.  
  - 📐 **Design: `docs/quantum-bigraphs.md`** — extends §XI with the *tensor as reverse-divide* framing: bigraph link graph IS the entanglement graph; separable qubits can live in different composites/processes, entangled ones must share a joint composite; `tensor` is the inverse of `divide`; LOCC (Local Operations + Classical Communication) corresponds to classical-typed bridges between composites with no quantum coupling; gate operations across composites trigger auto-merge. Six follow-on slices (Q1-Q6 in `#36`) ranging from independent quantum subprocesses up to quantum teleportation.  
  - ⏳ Slices 2-6 (eval-time): wrap protocol/method/apply as effects; add `log` as a side-effect effect.  
  - ⏳ Slices 7-8 (engine-time): state-representation parameter on effects; categorical-constraint typing.  
  - ⏳ Slices 9-10 (quantum): quantum handler bundle + Bell state demo. The end-state: a `.ys` constructs a 2-qubit Bell state from map literals, runs through the same chrysalis engine that runs classical mass-balanced cells, gets correct quantum statistics.
- 🧩 #34 north star: run streaming env with a hand-constructed Cell. **Mechanism complete; the Cell-scale demo is now just "exercise the mechanism more."**  
  - ✅ **Slice A — `EntityDef::from_value`** + lossless slot serialization (params + per-port schema/default/contract/bridge + body). `Program::from_value` round-trips whole programs. SchemaExpr ↔ string via `unparse_schema` ↔ `parse_schema_expr`. Tests in `entity_as_data.rs::entity_def_round_trips_through_value`.  
  - ✅ **Slice B — `compile_value(program_value) → Document Value`.** The in-memory sibling of `load(path)`. Stashes `_program` in the returned Document so `Document.run(time)` can re-compile for the right Core (user-defined factories from the hand-built entities). Hand-built Tick + Main composite test in `chrysalis/tests/hand_built_program.rs` — pure-data program reaches `count: 5.0` after 5 ticks.  
  - ✅ **Slice C — Surface `.ys` demo** `crates/chrysalis/ys/hand-built-demo.ys`: uses `from meta import compile_value`, assembles a program with `def` named map literals (`def tick_entity = {...}`, `def main_entity = {...}`, `def my_program = {entities: [...]}`), then runs `compile_value(my_program).run(5.0).count`. Picked up by `ys_files_run` smoke. Output: `{"count": 5.0}`. **No parser involvement for the simulation logic.**  
  - ✅ **Slice D — AST-builder helper library** (`crates/chrysalis/ys/ast.ys`). A pure-`.ys` library of `def` functions wrapping raw map literals: `float(v)`, `var(n)`, `add(a,b)`, `mul(a,b)`, `gt(a,b)`, `if_(c,t,e)`, `record(fields)`, `parallel(items)`, `keyed(k,v)`, `term(control, ports)`, `ports(in, out)`, `mk_process(name, params, in, out, body)`, `mk_composite(name, params, in, out, body)`, `program(entities)`, etc. (Renamed off keyword collisions: `process`/`step`/`composite`/`unit` are reserved.) Demo `hand-built-with-helpers.ys` builds the same Tick + Main program but reads as an EDSL — `def tick = mk_process('Tick', [], {count: float_port}, {count: float_port}, record({count: float(1.0)}))`. Output: `{"count": 5.0}`.  
  - ✅ **Slice E — THE HAND-BUILT CELL.** `crates/chrysalis/ys/hand-built-cell.ys` constructs `Grow` (mass-balanced metabolism — `block` expression with BinOp arithmetic, If-clamps, Record output: `u·yield`, `−u`, `u·(1−yield)`), `Divide` (If-threshold record), and `Cell` (composite wiring 4 state slots + 2 process instantiations with `term_with_args` + `named_arg`) — three entities, ~100 lines, all from map-literals via the `ast` library. Output after 5 ticks: `{mass: 7.0, glucose: 0.0, acetate: 4.0, divide: true, total: 11.0}` — mass+glucose+acetate = initial total *exactly*. **The hand-built program conserves mass identically to a parsed one.** Streaming over this Cell is a follow-on (same `_program` mechanism, same protocol layer); the simulation logic is fully reflective today.
- 🧩 #36 quantum bigraphs — links carry entanglement; `tensor` as reverse-divide. The structural / categorical view of distributed quantum: bigraph place graph holds qubit nodes, bigraph link graph IS the entanglement graph. Separable systems live in different composites; entangled systems must share a joint composite. `tensor` is the inverse of `divide`. LOCC = classical-typed bridges between composites with no quantum coupling. Design in `docs/quantum-bigraphs.md`. Slices:  
  - ✅ **Q1 — independent quantum subprocesses** (`crates/chrysalis/ys/quantum-two-bells.ys`). Two `BellPair` composites side-by-side, no link between them, each measures with its own seed. The bigraph place graph is two disjoint sub-bigraphs. Demonstrates: separable quantum systems CAN be distributed.  
  - ✅ **Q2 — classical wire (LOCC)** (`crates/chrysalis/ys/quantum-locc.ys` + `tests/quantum_locc.rs`). Two composites — `Alice` (H + measure) and `Bob` (conditional prepare) — wired via a shared `classical_wire :: any` slot in the outer `LOCC` composite. Alice's measurement outcome ('b0' or 'b1') flows through the slot as a String; Bob reads the bit, fires `ConditionalPrepare` (an `if bit == 'b0' then |0⟩ else |1⟩` body), prepares matching basis state. After 3 ticks, Bob's qubit is classically-correlated to Alice's outcome (no entanglement). Demonstrates: LOCC as a chrysalis pattern — typed classical channel = primitive for distributed quantum protocols.  
  - ✅ **Q3 — `meta::tensor`** + demo + tests. The inverse of `divide`: takes two separable single-qubit/multi-qubit states (`{bitstring → amplitude}` maps) and combines them into one joint state by cross-product + amplitude multiplication. Registered as `from meta import tensor`. Demo `crates/chrysalis/ys/quantum-tensor.ys`; regression `tests/quantum_tensor.rs` covers two-`|+⟩` tensoring, separable-embedding `|+⟩⊗|0⟩`, norm preservation (Σ|amp|² multiplicative), and cross-product cardinality (m·n joint keys).  
  - 🧩 **Q4 — auto-merge on cross-composite gate.** Compile-time form remains future work. **Runtime structural-lifecycle form** shipped as `crates/chrysalis/ys/quantum-lifecycle.ys` + `tests/quantum_lifecycle.rs`: user's "starts entangled → splits → joins back" as a single `.ys` file with one `Lifecycle` process that picks the structural intent (`_remove`+`_add`) each tick based on factorizability. Three deterministic phases at three `--time` snapshots: 0 → `{ab}` (joint), 1 → `{a, b}` (split), 2+ → `{ab2}` (merged). Discovered: parallel `step`s on the same slot RACE in BSP (non-deterministic); collapsing into one `process` with sequential `if/else` fixes it. **Design captured**: `docs/merge-protocol.md` lays out the principle (composites stay sealed, merge moves state across the bridge, reactions are bigraphs that can be transmitted) and the 6 slices (`tensor_by_schema` → `_add` w/ composites → local merge primitive → reaction-send over bridge → cross-composite reactor → distributed merge). **Probe**: `crates/chrysalis/ys/quantum-cross-cnot.ys` is the failing-test entry point — surfaced 3 concrete blockers: (a) composite-output leak (bridge tap exposes schema/spec tree, not just slot value); (b) `Schema::Any` + `overwrite` doesn't replace (Hold process accumulates instead of replacing); (c) no sibling-addressing syntax (`alice.state` from Lab level). Fix these 3 (engine work) and the reactor work outlined in merge-protocol.md becomes implementable. **REMAINING**: 3 engine fixes + 6 design slices from merge-protocol.md. (Streaming variant + distributed merge are slice 6.)  
  - 🧩 **Q5 — auto-divide on factorizability.** Substrate complete: `meta::factorize(joint_state, split_k)` HostFn shipped (`from meta import factorize`) — rank-1 detection over the m×n amplitude matrix; returns `{separable: bool, a, b}`. Tests in `tests/quantum_factorize.rs` cover separable `|+⟩⊗|+⟩` factoring, Bell + GHZ entangled-state detection, exact round-tripping (`factorize ∘ tensor = id` on separable inputs), and 3-qubit splits at variable `k`. Demo `crates/chrysalis/ys/quantum-factorize.ys`. **Runtime use**: `quantum-self-observe.ys` shows `FactorizeCheck` calling `factorize` from a process body — two side-by-side composites (Bell vs `|+⟩⊗|+⟩`) self-observe + report their separability ("the composite knows whether it's entangled"). **REMAINING**: a Divider-style step (analogous to `environment.ys`'s `Divider`) that converts the observation into a structural `_divide` intent, so a separable child composite splits into independent sub-composites reflecting the post-measurement physics.  
  - ✅ **Q6 — quantum teleportation.** The canonical demo, shipped: `crates/chrysalis/ys/quantum-teleportation.ys` + regression `tests/quantum_teleportation.rs`. Five processes — `CnotA1A2`, `HadamardA1`, `MeasureA1A2`, `ExtractBobState`, `BobCorrect` — chain across six state slots. Initial state |ψ⟩=0.6|0⟩+0.8|1⟩ on Alice's qubit A1, pre-shared Bell pair on (A2, B). After 6 BSP ticks (process chain depth), Bob's qubit `bob_final` matches |ψ⟩ exactly within float tolerance — regardless of which measurement outcome occurred. The conditional Pauli correction `Z^a1 · X^a2` is a nested `if/then/else` over the 4 possible bit pairs ('m00'/'m01'/'m10'/'m11'). Only 2 classical bits cross from Alice; entanglement does the rest. No-cloning preserved (Alice's measurement destroys her copy). Exercises Q1-Q5 together — the canonical quantum protocol running end-to-end through the prism engine.

A side-quest doc captured the broader landscape: `docs/exploring-the-computational-unknown.md` — survey of reflective towers, meta-circular interpreters, macros, Futamura projections, staging, algebraic effects, probabilistic / differentiable / reversible / quantum / unconventional computing, and the axes that compose into the space of methods.

## ⏯️ NEXT-SESSION PROMPT (2026-05-27 — quantum bigraphs Q2/Q3/Q5/Q6; #36 nearly complete)

> Workspace GREEN (582 workspace tests pass, 0 fail). This session pushed **#36
> quantum bigraphs** from design + Q1 to a near-complete five-slice arc, with
> only Q4 (compile-time auto-merge) remaining.
>
> **Q2 — LOCC (`crates/chrysalis/ys/quantum-locc.ys` + `tests/quantum_locc.rs`).**
> Two composites — `Alice` (H + Measure) and `Bob` (ConditionalPrepare) — wired
> via a shared `classical_wire :: any` slot. Only the bit ('b0'/'b1') crosses;
> no amplitudes. `ConditionalPrepare` uses a nested `if bit == 'b0' then |0⟩
> else |1⟩` body. After 3 ticks, Bob's qubit is classically-correlated to
> Alice's outcome — the canonical LOCC primitive.
>
> **Q3 — `meta::tensor` (`crates/chrysalis/src/prelude.rs` + `quantum-tensor.ys`
> + `tests/quantum_tensor.rs`).** The inverse of `divide` for quantum: cross-
> product over bitstring keys, amplitude multiplication. Registered as `from
> meta import tensor`. Four tests: |+⟩⊗|+⟩ → uniform 0.5, |+⟩⊗|0⟩ → separable
> embedding, norm preservation (Σ|amp|² multiplicative), and m×n cross-product
> cardinality.
>
> **Q5 substrate — `meta::factorize` (`prelude.rs` + `quantum-factorize.ys` +
> `tests/quantum_factorize.rs`).** The dual of `tensor`. Builds the m×n joint-
> amplitude matrix indexed by (left-bits, right-bits); rank-1 detection by
> deriving a candidate factorization from any nonzero row and verifying every
> other entry matches `a[i]·b[j]`. Returns `{separable: bool, a, b}`. Six
> tests: separable factoring, Bell + GHZ correctly identified as non-separable,
> exact `factorize ∘ tensor = id` round-trip, 3-qubit splits at variable `k`.
>
> **Q5 runtime use — `quantum-self-observe.ys`.** Shows `FactorizeCheck`
> calling `factorize` from inside a process body. Two side-by-side composites
> (Bell vs `|+⟩⊗|+⟩`) self-observe + report their separability — the
> composite literally knows whether it's entangled.
>
> **Q6 — Quantum teleportation (`quantum-teleportation.ys` +
> `tests/quantum_teleportation.rs`).** The canonical demo. Five processes
> chained across six state slots: CnotA1A2 → HadamardA1 → MeasureA1A2 →
> ExtractBobState → BobCorrect. Initial state |ψ⟩=0.6|0⟩+0.8|1⟩ on Alice's
> A1, pre-shared Bell pair on (A2, B). After 6 BSP ticks (process chain
> depth), Bob's `bob_final` reconstructs |ψ⟩ = (0.6, 0.8) EXACTLY within
> float tolerance — regardless of which measurement outcome (m00/m01/m10/m11)
> happened. The conditional Pauli correction `Z^a1 · X^a2` is a nested
> `if/then/else` over the 4 outcomes. Only 2 classical bits cross from
> Alice; entanglement does the rest. No-cloning preserved (Alice's
> measurement destroys her copy).
>
> **Q4-spirit — Structural LIFECYCLE (`quantum-lifecycle.ys` +
> `tests/quantum_lifecycle.rs`).** The user's "starts entangled → splits
> → joins back" vision as a single `.ys` file. A Lab composite holds
> `systems :: map[any]`; one `Lifecycle` process picks the structural
> intent each tick based on factorizability: `_remove: [ab]` +
> `_add: {a, b}` (split via `meta::factorize`), then `_remove: [a, b]`
> + `_add: {ab2}` (merge via `meta::tensor`). Three deterministic phases
> visible at `--time` snapshots: 0 → joint `ab`, 1 → split `a`+`b`,
> 2+ → merged `ab2`. **Discovered along the way**: two parallel `step`s
> on the same slot RACE in the BSP cycle and give non-deterministic
> results; collapsing into one `process` with sequential `if/else` is the
> fix. This race is the kind of issue that motivates explicit ordering
> primitives for future work. Lifecycle uses raw data maps for each
> system (not real sub-composites or stream children); the streaming
> variant (`map[StreamingQuantumSystem]`) surfaces the open question of
> how `_add` instantiates sub-composites from override maps — saved for
> Q4 follow-on.
>
> **Docs.** `docs/quantum-bigraphs.md` §IX slice statuses updated;
> `docs/NEXT-SESSION.md` canonical-list entry for #36 reflects
> Q1✅ Q2✅ Q3✅ Q4⏳ Q5🧩 Q6✅.
>
> **Q4 deep-dive (design + probe).** Captured the principle, the merge
> protocol, and the first-probe findings in `docs/merge-protocol.md`.
> Wrote `crates/chrysalis/ys/quantum-cross-cnot.ys` as the failing-test
> entry point — two sibling `QuantumSystem` composites in a
> `map[QuantumSystem]` slot, with the reaction definer commented out.
> Three concrete blockers surfaced before the reactor work can even
> start:
>
>   1. **Composite-output leak**: the `systems` output is deeply nested
>      (`alice.state.state.…`) and includes `_type: "Any"` schema
>      descriptors. Bridge tap is not projecting to slot value only.
>      The "_process leak" pattern from `project_composite_execution_gap`
>      surfacing for `any`-typed slots. Fix: trace bridge-snapshot
>      logic in `engine.rs`.
>   2. **`any` + `overwrite` doesn't replace**: a trivial Hold process
>      (`{state: state}` with `overwrite[any]`) produces `{0: 2.8284}`
>      at `--time 2` — the value is being SUMMED per tick, not
>      overwritten. `overwrite` modifier not being honored for
>      `Schema::Any`. Fix: audit `apply_with_schema(Any, …)` in
>      `apply.rs` / `reconcile.rs`.
>   3. **No sibling-addressing syntax** (`alice.state` from Lab
>      level). `%.foo` (self) and `^.foo` (parent) exist; no sibling
>      form. Small parser slice.
>
> See `docs/merge-protocol.md` for the full design, the 6 slices, and
> the protocol-level vision (composites stay sealed; merge moves state
> across the bridge; reactions are bigraphs that can be transmitted
> across the bridge — all distribution-transparent).
>
> **UNCOMMITTED at break**: `crates/chrysalis/ys/quantum-teleportation.ys`,
> `crates/chrysalis/ys/quantum-self-observe.ys`,
> `crates/chrysalis/ys/quantum-lifecycle.ys`,
> `crates/chrysalis/ys/quantum-cross-cnot.ys`,
> `crates/chrysalis/tests/quantum_teleportation.rs`,
> `crates/chrysalis/tests/quantum_lifecycle.rs`,
> `docs/merge-protocol.md` (NEW),
> `docs/NEXT-SESSION.md` (this entry + #36 status update),
> `docs/quantum-bigraphs.md` (Q4-spirit + Q5/Q6 status). Plus the usual
> spatio-flux/out/ regen drift from running the test suite. Suggested
> commit messages: *"teleport"* / *"lifecycle"* / *"merge protocol"*
> (matches the one-word convention of recent quantum commits).
>
> **NEXT (open work, in suggested order):**
> 1. **Q4 blocker fixes (engine work)**: in priority order,
>    (a) bridge-output projection (composite outputs should be just the
>    slot value, not the schema/spec tree); (b) `Schema::Any` + `overwrite`
>    semantics (overwrite must replace regardless of inner schema kind);
>    (c) sibling-addressing parser slice. Each is small + concrete; together
>    they unblock the cross-composite reactor work outlined in
>    `docs/merge-protocol.md`.
> 2. **`tensor_by_schema`** — the schema-driven dual of
>    `divide_by_schema` (slice 1 of merge-protocol.md). Schema-algebra
>    work; mirrors divide.
> 3. **Reaction-send over the bridge** — extend the stream protocol's
>    command vocabulary to carry "fire this reaction at this position"
>    in addition to state deltas. Enables transparently-distributed
>    reactions (slice 4 of merge-protocol.md).
> 4. **Q5 trigger half** — a Divider-style step (mirroring
>    `environment.ys`'s `Divider`) that watches child composites'
>    `observation.separable` flags and emits a structural `_divide` intent
>    when a child reports separable. Closes the auto-divide loop: the
>    composite's verdict becomes structural change.
> 5. **Other tracks ready to go**: #6 explicit bridge conduits (related
>    to Q4 blocker 1), #9 dt-refinement sweep (small warmup), #21 batched
>    ray (the only piece keeping #21 from done), #25-29 distributed
>    phases (the planet-scale arc — Q4's merge protocol IS phase 1 in
>    spirit), #15+#30 schema-as-state + entity registry (#30 slices 3-4
>    remain — methods on sited cells + `control Foo` declarative form).

## ⏯️ NEXT-SESSION PROMPT (2026-05-25 — schema-first discovery + canonical-list audit)

> Workspace GREEN (101 groups, 546 passed, 0 failed). This session finished **#7
> cleanup** end-to-end — both halves of "collapse address-scan + the two discovery
> walks":
>
> **Part A (the discovery walks).** Collapsed `extract_processes` (schema-only,
> init-time) + `discover_all_processes` (full-state) into ONE path through
> `scan_for_processes` via `discover_processes`. `discover_all_processes` is now a
> thin "from-root + settle_steps" wrapper. `merge_schema` goes through the same
> path. Net −179 lines in `crates/prism-bigraph/src/engine.rs`.
>
> **Part B (the address-fallback).** **EXPUNGED address-as-hint from discovery.**
> Previously `scan_for_processes` recognised a node as a process via `has_address`
> — but many non-process values legitimately carry an `address` field (a contact
> card, a webhook config, an OAuth grant). The principled fix: a node is a process
> iff (a) the schema declares it as a `Link`-kind, or (b) the state value carries
> an explicit `_type: "process"|"step"|"link"|"composite"` marker (upstream
> process-bigraph's convention, already consumed by `Schema::infer`). Address is
> for *instantiation only*. Producer migration: `chrysalis::eval::build_spec_value`
> (the canonical chrysalis emitter) now takes a `kind` and threads `_type`;
> `build_native_spec` emits `_type: "link"` (kind decided by the registry at
> instantiation); ~10 Rust test fixtures + 17 spatio-flux JSON fixtures (778
> `_type` additions) updated. Re-probe is empty: zero legacy `has_address && !is_link`
> hits across the workspace.
>
> **Canonical-list audit.** Walked every still-open task and corrected statuses
> against the code (deltas reflected in the list above): #10 extern is FULLY
> RETIRED (only comment-preservation remains); #11 typed SVG nodes are DONE in
> `prism-viz/src/svg.rs` (only `render_timeseries_svg`'s plotters string is left);
> #13 codegen MVP + 6 representative `*-section.ys` are done (the rest of the
> 18-member `CANONICAL_ORDER` is the remaining work); #16 dynamic-τ OVERRIDE half
> shipped (the inner-wire-type half is the cleanup); #17 lenient `::`/`fulfills`
> parsing is LIVE (tighten after migrating the `.ys` corpus); #18 Trace kernel +
> `--in TRACE` + plot dispatch + Section template are all landed (only
> `--map`/`--adapt` + the 4D structural plot remain); #21 `ParallelPool` +
> `flush_pending` shipped (ray protocol is the remaining piece); #23 `mapk.ys`'s
> `phosphorylate` is BOUND (corrected — only `grow-divide-unbounded/glucose`
> legacy `env.run` remains).
>
> UNCOMMITTED at break: `engine.rs`, `chrysalis/src/eval.rs`, ~10 Rust test
> fixtures, 17 spatio-flux JSON fixtures (`spatial_many_dfba.json` +
> `spatioflux_reference_demo.json` are the bulk), `NEXT-SESSION.md` (this entry
> and the refreshed canonical list).
>
> **NEXT (open work, in suggested order):** #9 dt-refinement convergence sweep
> (small warm-up; uses the existing integrator-comparison + the type-driven plot
> pipeline that #18 left ready) → #5 law generators over node sorts (the audit
> confirmed `Link`/`StepLink`/`ProcessLink`/`CompositeLink` are absent from
> `arb_additive()` — adding them would mechanically catch the kind of holes #7
> just papered over) → #16 inner-wire type for `interval` (retire the engine
> side-channel) → #10 comment-preserving unparse OR #21/#25 batched ray (depending
> on whether you want a `.ys` polish vs. the first concrete distributed step).

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 6 — rest-concurrency + the defaults/harness fix; distributed plan drafted)

> Workspace GREEN (69 groups + doctests). This session, after the part-5 milestone
> (committed): **rest-concurrency** (#21) — `RestProcess::invoke` fires the POST on a
> thread + the server releases the map-lock before `update()` (`rest_engine.rs`: 4×50ms
> remote ≈ one tick); the **default harness** — any `process`/`step` runs standalone
> (`chrysalis run grow.ys --mass 1 --glucose 5`; `cli::harness_process_entry`); and the
> **input-defaults fix** — `composite_param_env` is the one eval-side binding rule, so
> `cell.ys --glucose 5` drives its metabolism (`bench_run.rs`). Then drafted
> **`docs/distributed-execution.md`** — the plan for planet-scale colonies (your
> octree/fractal vision = domain decomposition + FMM aggregation + adaptive octrees;
> prism's composite-of-composites + protocol boundary + encapsulation + BSP tick already
> encode the skeleton). UNCOMMITTED at break: rest.rs, rest_server.rs, rest_engine.rs,
> bench_run.rs, the defaults/harness edits (eval.rs/cli.rs/cell.ys), execution-model.md,
> distributed-execution.md.
>
> **NEXT:** #25 (batched `ray:` protocol — finishes #21, smallest concrete step toward
> scale) → #26/#27 (halo exchange + the static octree — the fractal vision at modest
> scale). Or a cleanup pass (#5 laws / #7 discovery). See `docs/distributed-execution.md`
> for the full phased plan + the build-vs-adopt decision (cluster = a protocol with a
> pluggable backend; do NOT rebuild Ray's runtime).

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 5 — env.ys divides over STREAM in parallel; schema-name machinery unified)

> Continue prism (Rust process-bigraphs + the `.ys` language). Workspace GREEN
> (67 test groups across chrysalis + prism-schema + prism-bigraph + prism-std, 0
> failures). Read memory `composite_is_a_type` + `using_injection_sibling_wire` +
> `docs/chrysalis-design.md` §"Type names resolve through one registry" first.
>
> **DONE THIS SESSION — `chrysalis run environment.ys` runs PARALLEL STREAM cells
> that divide and conserve mass, end-to-end in the surface language.** The cell
> model is now a modular `.ys` library: `grow.ys` (←Metabolism), `divide.ys`
> (←Trigger, the propose step), `cell.ys` (imports both), `environment.ys`
> (stream-addressed cells via `protocol StreamingCell = stream<Cell, path:'cell.ys'>`
> + a `.ys` `Divider` step). 32 parallel `stream:cell.ys` children grow, divide,
> terminate, conserve `glucose+Σmass+acetate=42.0`. `.ys` gained **map
> comprehensions** (`{k: v for k, x in m if p}`) so the env-side `Divider` can scan
> the cells map in surface syntax.
>
> **THE UNIFICATION (the real work):** the schema-name machinery was collapsed to one
> principle — *a named type is one registry entry whose representation is its real
> schema; ONE program-aware lowering; the algebra/engine resolve `Custom→
> representation` at use; nothing infers/promotes/tweaks the schema at runtime.*
> Concretely: composites register as types (`Cell→CompositeLink`), type aliases are
> real `Def::Type` (carry across imports; `Mass→Delta`), `lower_schema_in_program`
> is the single name lowering (incl. `ExprProcess`/`ExprStep` ports), the
> `composite_link` cycle guard wraps ports. This killed a chain of latent "we
> weren't using the real schema" bugs (mass not halving; promote-degrade;
> cross-file alias `lookup=NONE`; infinite recursion). Also fixed a latent
> units/context break: `using ctx(f: @.slot)` injected an own-node `%` wire instead
> of a container sibling (`sibling_wire`; `units_engine.rs` guards it).
>
> **ALSO this session — every definer runs standalone (the default harness) + the
> input-defaults fix.** A bare `process`/`step` entry is wrapped in a synthesized
> harness (`cli::harness_process_entry`): a slot per port, self-wired (read-modify-
> write ports accumulate), `--port` seeds, every slot an output — so `chrysalis run
> grow.ys --mass 1 --glucose 5` runs it "on a bench." `.ys` also gained MAP
> comprehensions. And the defaults gap closed: input-port defaults were honored
> runner-side (`bind_arg`) but NOT eval-side, so a composite body referencing an
> input (`glucose: glucose`) was unbound → `Evaluator::composite_param_env` (config
> + input defaults) is now the ONE eval-side rule (shared by `eval_top_level` +
> `build_composite_outer`). cell.ys now runs standalone (`--glucose 5` drives its
> metabolism, conserves 6) AND driven (env). Guard: `tests/bench_run.rs`. Full suite
> green (68 groups + doctests). See memory `defaults_and_binding_paths`,
> `composite_is_a_type`, docs/chrysalis-design.md §"Every definer runs standalone".
>
> **NEXT (pick up #21 parallelism follow-ups, or the cleanups):** #21 rest-concurrent
> dispatch + batched ray (the stream cells already overlap via the invoke/Defer
> seam; rest is the remaining concurrent transport). Also ripe: #5/#11 (complete law
> generators over node sorts — would have caught the schema-name holes mechanically),
> #7 (#29 collapse the address-scan / two discovery walks now cells are schema-first),
> #6 (#13 explicit bridge conduits). The schema layer is now genuinely
> single-sourced — a good moment to lean on the laws.

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 4 — division proven over STREAM; parallelism next)

> Continue prism (Rust process-bigraphs + the `.ys` language). Workspace green
> (chrysalis suite all-pass incl. the new `grow_divide_stream`). Read
> `docs/cells-and-division.md` §"RESOLVED (part 4)" + memory
> `composite_bridge_forwards_updates` first.
>
> **DONE THIS SESSION — cell division over the `stream:` protocol, conserving mass.**
> The `stream:` child was still a delta-LOG *filter* (`serve_stream` `diff`ed absolute
> `output_raw` snapshots — double-counts a shared pool once ≥2 cells draw on it, drops
> structure). Fixed: new **`runner::serve_process`** (CLI `--serve-process`;
> `StreamProcess` spawns it) runs the entry as a real `Composite` and **forwards
> `Composite::update`'s delta** — the pipe mirror of the rest server (no `refines`
> handshake, no one-tick lag: parent stamps cumulative time *after* the step).
> `serve_stream` stays the `A|B` trace FILTER. New `crates/chrysalis/ys/cell.ys` (a
> streamable Cell; its ENTRY is the composite — no trailing `Cell[]`). Proof:
> `crates/chrysalis/tests/grow_divide_stream.rs` — a `stream:cell.ys` cell (separate
> OS process, Arrow over pipes) grows + divides + **conserves `glucose+Σmass+acetate=41`
> EVERY tick**, terminating at a stable 16 cells, terminal numbers IDENTICAL to
> local/rest. The 2 `stream_protocol.rs` echo tests were rewritten to a delta-emitting
> Counter (a passthrough forwards nothing — correct, same as local).
>
> **NEXT: real parallelism (#27 / task #4)** — the user's stated goal, rides this same
> protocol seam. Steps 1+2 done; pick up at step 2.5 (register the pool as a
> `ProtocolRuntime` via `Protocol::runtime()`), an engine-level wall-clock test, then
> step 3 (`rest`/`stream` concurrent dispatch — the `stream:` cells can now run
> truly concurrently) and step 4 (batched ray). See `docs/execution-model.md`.
>
> **ALSO available (not blocking):** the env.ys surface (#20 / task #20) — thread
> `map[Cell]→Map{CompositeLink}`, retire the `composite_instance_schema` dodge, the `%`
> surface wire, surface stream addressing — so the WHOLE environment runs as
> `chrysalis run env.ys` (today the proof builds the env in Rust). A convenience, not a
> capability gap.

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 3 — bridge unified, division proven over rest)

> Continue prism (Rust process-bigraphs + the `.ys` language). Workspace green
> (94 test groups; one *rare pre-existing* flake `law_reconcile_coherence` — fold
> into #11). Read `docs/cells-and-division.md` **§"✅ RESOLVED"** (top) + memory
> `composite_bridge_forwards_updates` + the task list first.
>
> **THE FOUNDATIONAL SHIFT THIS SESSION:** the composite bridge now **forwards the
> inner UPDATE (delta) intact — `diff(pre,post)` deleted**. Updates ARE deltas, so
> **local bridge == `stream:` wire == `rest:` == trace == command** (one way across
> every boundary). This is what makes protocol-equivalence real. Division is **Form
> 3 canonical** (cell proposes via `%.divide` self-node face → env enacts via the
> `_divide` sentinel → schema splits via `divide_by_schema(CompositeLink)`; daughters
> inherit the cell's protocol via shared `address`). Proven: a mass-balanced
> grow-divide-glucose cell divides + conserves (`glucose+Σmass+acetate=const` every
> tick), **identical local AND over `rest:` (real HTTP)** — `prism-bigraph/tests/
> cells_division.rs`. `growth_division` is true division now (1→2→4, 0.01s, was a
> 46s zombie explosion). Algebra holes fixed: `resolve` Map-preservation,
> `reconcile` node-field merge, own-path instance removal, the `%` self-node wire,
> the `add_process` explosion backstop (generous; tests set `set_max_nodes(64)`).
>
> **FIRST: the `stream:` half = `chrysalis run cell.ys --serve-stream` (#6→#7→#8).**
> The rest half is proven at the prism level; stream needs the `.ys` cell. (#6)
> Thread the chrysalis lowering `map[Cell] → Map{CompositeLink}` (retire the
> `composite_instance_schema` dodge at `chrysalis/src/schema.rs:80`; cell value is
> already an addressed CompositeLink node via `build_composite_outer`) + the `%`
> self-node wire at the **surface** (engine side done — `["%",…]`); update the
> dependent `schema.rs` tests. (#7) Write the grow-divide-glucose `.ys` (Form 3:
> cell + Divide reaction + environment; mass-balanced uptake). (#8) `chrysalis run`
> it over local + `stream:`, asserting the SAME division + conservation as rest —
> the same cell, only the address changes.
>
> **THEN:** #11 complete the law generators to cover node sorts (so node-holes fail
> `cargo test` — would have caught this session's `reconcile`/`resolve` holes) +
> fix the flaky law deterministically, THEN re-apply the *uniform node-data
> handling* (reverted this session — apply/divide/reconcile for ALL link sorts via
> `node_data_branches`, validated by the now-complete laws). #13 design explicit
> bridge **conduits** (today inferred by inner-slot absence — a wart). #9 cleanup
> (#29 collapse the address-scan / two discovery walks now that cells are
> schema-first). Then real parallelism (#27) rides the same seam.

## ⏯️ NEXT-SESSION PROMPT (2026-05-24 part 2)

> Continue prism (Rust process-bigraphs + the `.ys` language). Workspace is green
> (530/0); read this section + the task list first.
>
> **FIRST: finish the cells/division work (#9).** It's the prerequisite for
> distributed/parallel execution AND it's currently *incorrect*:
> `divide_by_schema(CompositeLink)` divides the exported outputs, but must divide
> `inner_schema` (the whole body). Implement `docs/cells-and-division.md` §6 in
> order; the **grow-divide-over-`stream`** test is the boundary proof and a top
> priority — getting it green validates the composite boundary (a cell divides
> itself and exports daughters across the bridge, working unchanged over a real
> wire).
>
> **THEN: real parallelism (#27)** — the original goal. Steps 1–2 are done (the
> engine runs invoke→Defer→flush→collect; `ParallelPool` + blocking `Defer::slot`;
> pool wall-clock test green). Pick up at **step 2.5** (register the pool as a
> `ProtocolRuntime` via `Protocol::runtime()`), an engine-level wall-clock test
> (build via `Topology`/instances, NOT `Schema::Any`), then **step 3** (`rest`/
> `stream` concurrent dispatch) and **step 4** (batched `ray`) — and **test it with
> the working grow-divide-over-`stream`** (same boundary seam). See
> `docs/execution-model.md`.
>
> Also: #28 (Schema::Any: static done, cells part = #9), #29 (core unification,
> before perf #20). Keep the composite-boundary principle front of mind (memory
> `feedback_composite_boundary`): a composite may be remote; never reach into its
> internals; all I/O via the bridge.

## This session (2026-05-24 part 2 — parallelism + extern + schema-threading + cells/division design)

Started as "real parallelism (#27)"; the no-half-measures/dogfooding loop pulled in
extern retirement, `Schema::Any` threading, and a foundational cells/division
redesign. **Workspace green: 530/0.** Changes uncommitted (user commits).

- **#27 real parallelism — steps 1+2 DONE (resume AFTER #9).** Engine runs
  invoke→Defer→flush→collect (`ProcessFront.pending: Option<Defer<Update>>`; invoke
  pass calls `p.invoke()`; `flush_protocol_runtimes()` between invoke+collect;
  collect resolves `.get()` — byte-identical for local `Defer::immediate`).
  `Defer::slot()` → blocking one-shot (`sync_channel`; dead `Slot` variant removed).
  `ParallelPool` (shared worker pool + `flush_pending` barrier) + `ParallelProcess`
  (`invoke` enqueues → slot-`Defer`); pool wall-clock test green (4×50ms < 150ms).
  REMAINING: step 2.5 register pool as a `ProtocolRuntime` (`Protocol::runtime()`);
  engine-level wall-clock test (build via `Topology`/instances); step 3 rest/stream
  concurrent; step 4 batched ray. (`engine.rs`, `defer.rs`,
  `protocols/parallel.rs`, `protocol_runtime.rs`; `docs/execution-model.md`.)
- **#14 extern — FULLY RETIRED.** Removed from grammar/AST/parse/eval/compile/
  check/unparse/repl; `dish.ys` → `from diffusion import DiffusionAdvection`
  (`sf_modules` already exports it); converted parse_import/chrysalis_port/
  composite_process/parse_contract.
- **#28 Schema::Any — STATIC schema-first DONE.** chrysalis `branch_schema` emits a
  base `Link` MARKER for native controls (no compile-time `Any`); the engine stamps
  each node's REAL Link from its INSTANCE into `self.schema` at `add_process`
  (`stamp_instance_link` / `is_stampable_node_path`); cycle detection in chrysalis
  lowering (`composite_link` + `building` stack); `extract_processes` infer-keys
  deleted; `scan_for_processes` schema-first-primary (address fallback only for
  dynamic/homoiconic). Test `native_node_found_by_address_…`. Dynamic (cells) → #9.
- **#9 cells & division — DESIGN WRITTEN (`docs/cells-and-division.md`); DO FIRST.**
  Currently INCORRECT: `divide_by_schema(CompositeLink)` divides the exported
  outputs — it must divide `inner_schema` (the whole body; §6 step 1). It's also
  the prerequisite for testing distributed/parallel execution (grow-divide-over-
  `stream`). A cell is just a composite that EXPORTS its `mass` across the bridge
  (`cells.N.mass`,
  populated from the bridge — the matchable/divisible face). Division-through-a-
  reaction is a STRUCTURAL BRIDGE UPDATE the composite emits itself
  (`{cells:{_remove:[self], _add:{daughters}}}` out `->{environment}`), carried by
  invoke→Defer over ANY protocol — no division-specific code; `stream` is the
  boundary-proof test. `divide_by_schema(inner_schema)` runs INSIDE the composite.
  Implement §6 in order. (Memory `feedback_composite_boundary`.) The cells-as-
  `CompositeLink` attempt bounced on the homoiconic representation → became this
  design; cells currently reverted to the instance schema (green).
- **#29 core unification** created — collapse `composite_instance_schema` vs the
  `CompositeLink`, the address scan, the two discovery walks, the ~150 remaining
  `Schema::Any` — gated before the perf sweep (#20).

## Where we are (2026-05-24 — spatio-flux-report-as-.ys + delta-motion + .ys-modules)

The **full-circle challenge landed: the entire spatio-flux report is regenerable
from `.ys`.** All 6 simulation families run as `.ys` sections through the codegen
path, each plotting itself BY ITS SCHEMA. The dogfooding loop (turn a sim into
`.ys`, find the gap, fix it IN the core — memory `feedback_integrate_into_core`)
drove every fix below. Full workspace green throughout.

### The report, as `.ys` (the 6 families)
`crates/spatio-flux/ys/{kinetics,diffusion,dfba,brownian,newtonian,comet}-section.ys`
— kinetics·line, diffusion·heatmap, dFBA(HiGHS)·line, brownian·scatter,
newtonian·scatter, comet·heatmap+particle-overlay. Each is just
`Simulate[proc: <sim>] → trace → traj.plot → figure.svg`.

### Core capabilities folded in (each surfaced by the challenge)
- **delta-traces kernel** — `prism_trace` (Trace = `initial` + `deltas`,
  `state(t)=fold(apply,…)`; Arrow IPC codec) + `prism_std::Simulate`, the
  engine-faithful runner (same-name wiring, folds the inner's REAL output schema).
  `RunProcess` is the integrator special case; `Simulate` is the general one.
- **`stream:` protocol** — a `.ys` proxied as a `Process` over the Arrow trace wire
  (parent `StreamProcess` ⇄ child `--serve-stream`), lock-step or bulk.
- **dynamic timesteps** — interval read from state (`overwrite[T]`); `MinimalGillespie`
  recreated (Rust test + `ys/gillespie.ys`).
- **particle DELTA-MOTION model** — movers emit Δposition/Δvelocity, `apply` SUMS
  (additive `Array[[2]]`), so Brownian + drift + interactions + boundary-correction
  SUPERPOSE. Retired the particle `Map[Any]` dodge (`motion_schema`; boundaries →
  corrective Δ). Proven: `motion_deltas_superpose`.
- **schema-driven views** (`prism_viz`) — line / heatmap (`View::Field`) / scatter
  (`View::Particles`) / comet overlay (`View::Spatial`), all place-graph SVG values;
  `first_field` digs nested ports.
- **retired 3 `Schema::Any` leaks** — diffusion output, particle-mover outputs, AND
  composite outputs (`composite_inner_schema` now types the interface ports →
  `Composite.outputs()` honest → composites-via-`Simulate` plot by schema).
- **codegen staleness → cargo-driven** (prism edits rebuild the cached runner).

### `.ys` FILE MODULES + unified modules
- `from <pkg>.<sub>.<file> import <Def>` → `<ys_root>/<sub>/<file>.ys` (package-rooted,
  recursive; the module's host imports + type vocabulary ride along). Parser accepts
  hyphenated/dotted paths (`spatio-flux.composites`). `resolve_file_modules` (cli.rs),
  `parse_module_path` (parse.rs).
- Sections DRYed into shared modules: `report/section.ys` (Trace/Figure/Plot/Output) +
  `composites/comets.ys` (the Comet composite), imported by all 6 sections.

### Next (half-done → finish, then new)
- **Report workflow DONE**: `report.ys` runs all 6 sections in one pass (`chrysalis run
  report.ys` → each self-outputs its SVG); `.ys` imports now pull TRANSITIVE value-deps,
  so `report.ys` is just the 6 section imports. REMAINING: a single COMBINED artifact
  (index / stacked SVG); `spatioflux_reference_demo`; parallel section execution (#27).
- **Complete `.ys` modules**: transitive value-deps DONE; REMAINING: the `import
  <module>` namespace forms (`comets.Comet`) + `import <module>.Def`; validate the
  package segment vs `project.ys`.
- **Retire `extern` (#14) is a refactor, not a cleanup**: woven through ~9 src files AND
  the old-import-form fixtures (`ys/culture.ys` + `ys/dish.ys` + `parse_import`, whose
  `diffusion_natives()` registry is keyed on the `Diffusion` extern). No `.ys` *uses* the
  `extern` declaration except that fixture. One focused pass.
- **SVG-as-place-graph everywhere (#12)**: retire `render_dot` (graphviz bigraph viz) →
  place-graph SVG; retire `render_timeseries_svg` (→ plot's `line_chart`).
- **Grow the command**: `chrysalis new <name>` scaffolder (#21); multi-line config
  unparse + emacs mode (#24); the `.ys` vs process-bigraph-JSON vs Python comparison (#23).
- **Then** the perf sweep (#20, AFTER features).

## Where we are (2026-05-22 — post runtime + toolchain session)

The language foundation (import model, `def` + first-class functions, process
contracts, units, prism-std as the std library) is in. THIS session built out the
**unified runtime core, the distribution boundary, and the `chrysalis`
toolchain**. Full workspace green. The CLI is now a real toolchain:

```sh
chrysalis run | check | bigraph  <file.ys>                  # compile+run / check / emit doc
chrysalis bigraph export f.ys out.json | import out.json    # import(export(f)) ≡ run(f)
chrysalis server [--port P]                                 # serve a Core over REST
chrysalis repl                                              # interactive homoiconic prompt
# `chrysalis compile` (codegen for NON-std packages) is the remaining piece — #10
```

### Landed this session
- **Unified `Core`** (`prism_bigraph::Core` = types + processes + methods +
  protocols) — replaces the engine's four separate registry fields; threaded
  through the engine AND every subengine (`Composite::from_config(&Core)`), so a
  `Custom`-typed / method-using / `rest:`-addressed process works inside a
  composite. `from_state(…, impl Into<Core>)` keeps registry-only callers
  compiling. (memory `project_core_unification`.)
- **Incremental steps** (`prism_bigraph::StepCache`) — a workflow's steps skip when
  their on-disk output is fresh; `--steps a,b` forces; staleness cascades the step
  DAG. Per-composite via `config.cache` (config-as-data, not a Rust handle). The
  substrate for the report-as-expirable-DAG.
- **The protocol boundary, made real** — `RestProcessServer` serves a `Core` over
  the rest-process wire protocol with full lifecycle cleanup (`end` deletes, no
  leaks); discovery now resolves `rest:` addresses in state (`address_class`), so a
  `rest:` process/composite is discovered + driven over HTTP indistinguishably from
  local (tests: `grow_divide_over_rest`).
- **Missing-reference validation** — a doc referencing a process the core can't
  build is rejected (`Core::missing_process_refs` / `Engine::check_references` /
  server 400), not silently run partial. The seed of the diagnostics goal.
- **CLI build-out** — `chrysalis server` (serves `std_core`); `chrysalis bigraph
  export/import` (the document is a runnable artifact; `runner::{document_of,
  run_document}`; `import(export(f)) ≡ run(f)`); `chrysalis repl` (Session eval,
  `:type`/`:env`/`:reset`, **rustyline with live syntax highlighting + completion +
  history hints + multi-line continuation**, redefinition overwrites).
- **spatio-flux as a package (slices 1–2)** — dep chain completed (spatio-flux now
  deps chrysalis + prism-std normally); `spatio_flux::prelude::{sf_core,
  sf_registry, sf_methods, sf_modules}` expose its natives as a Core / run-path
  packages; the `sf` bin proves the run path in-tree (the hand-written twin of what
  the codegen will emit).
- **Defaults** — confirmed `algebra::default` is ported + faithful; config/port
  defaults already parse; completing the surface is a task.

### Architecture orientation (read first)
- **Layering: prism-std → chrysalis → spatio-flux.** chrysalis NEVER deps
  spatio-flux; spatio-flux now deps chrysalis (the package half). memory
  `feedback_ys_layering`.
- **One `Core` threads everywhere** (engine + every subengine) — the root-cause fix
  for subengines losing types/methods/protocols. The schema layer is a closed
  algebra. The composite boundary is real + **protocol-mediated** (local / rest /
  parallel); cross-boundary config travels as DATA, never a Rust handle.
- **Single run path.** `chrysalis::runner::run(program, registry, methods, modules,
  time)` (compile+run) and `runner::run_document(doc, core, time)` (import).
  `run`, `server`, and `import` MUST share package resolution + `Core` assembly —
  the server is not a separate code path; the codegen (#10) builds that one path.
- **The remaining gap is the codegen (#10).** chrysalis (one binary) can't link a
  package's natives (HiGHS/rapier2d) and must never dep spatio-flux, so
  `chrysalis run <non-std>.ys` must **generate + cargo-build + cache a runner
  crate** that links the package, then run it (invisible to the user). The `sf` bin
  is the hand-written prototype; `sf_*`/`sf_core` are what the generated runner
  calls. See the codegen entry below for the `project.ys` manifest design.

## Next — the task tracker (durable; the harness task list is ephemeral)

✅🔧 **#10 — the codegen path (`chrysalis run` on NON-std packages) — MVP WORKING
(2026-05-23).** A `.ys` importing a non-std package (spatio-flux) can't run in the
fixed `chrysalis` binary (can't link HiGHS/rapier2d; must never dep spatio-flux),
so `chrysalis run` **codegens + builds + caches** a runner crate linking the
package. DONE + validated end-to-end:
- `crates/chrysalis/src/codegen.rs` — `find_manifest` (walk up for `project.ys`),
  render a runner crate (`Cargo.toml` path-deps chrysalis [baked
  `CARGO_MANIFEST_DIR`] + the package [manifest dir]; `main` = the shared CLI),
  build with `CARGO_TARGET_DIR` = the **workspace target** so compiled deps are
  reused, cache by content hash, exec.
- `crates/chrysalis/src/cli.rs` — `run_command(args, registry, methods, modules)`:
  the ONE run path, parameterized by packages. The chrysalis bin calls it with
  std; the generated runner calls it with the package's
  `prelude::{registry, methods, modules}` (added to spatio-flux as the codegen
  convention). **Single path** — std and packages run identical code.
- `crates/spatio-flux/project.ys` (`package spatio-flux`); `ys/diffusion.ys` +
  `ys/kinetics.ys` (fresh demos using the real `DiffusionAdvection` /
  `MonodKinetics`). `chrysalis run ys/diffusion.ys` builds the runner once (~34s,
  then **cached → 0.008s**); diffusion conserves mass (Σ=45), kinetics grows
  biomass 0.1→4.1 / consumes glucose. CLI input override (`--field '…'`) flows
  through the runner via compositional invocation. Tests: `codegen.rs` units
  (manifest parse, render).

*REMAINING:* (a) **dogfood the report** — port the ~18 `report.rs` sims
(`CANONICAL_ORDER`: dFBA family [HiGHS], particles [rapier2d], comets, newtonian)
to `.ys` and run each via the tool, validating every process family; (b) the
stale `culture.ys`/`dish.ys` (still `extern Diffusion`) → align to the package or
delete; (c) wire `compile`/`server`/`import` through the same codegen (`server` =
`run` ending in serve-over-REST — the runner gets a `server` subcommand);
(d) published-crate path deps (today path-deps assume the dev workspace).
*The hard part is done; the rest is breadth.*

⏳ **#6 — SBML→CRN importer / repressilator (the original goal).** Native subset
SBML/MathML reader (roxmltree + ~6-operator evaluator + one-time assignment-rule
param precompute) → a `CRN`; generalize the integrators to an arbitrary ODE RHS
(an `OdeSystem` trait both `MassActionNetwork` and an `SbmlOde` implement — the
repressilator BIOMD0000000012 is Hill-kinetics, not mass-action). Add
`CRN.from_sbml(path)` (or `from models import repressilator`). The model + Python
reference are at `../biocompose/biocompose/{models/BIOMD0000000012_url.xml,
experiments/copasi_tellurium_comparison.py, processes/}`. NOT a COPASI port —
this model has no events/rules/function-defs/piecewise. *Large.*

⏳ **#7 — dt-refinement convergence sweep.** Run the comparison at several
timesteps; show MSE → 0 as dt shrinks (the two methods converge to the same
target — divergence is pure discretization). Emit a convergence plot/CSV.
*Small — good warm-up.*

⏳ **#8 — comment-preserving parse/unparse.** Retain comments through
`parse→unparse` so all `.ys` regenerate losslessly. Leading block comments
(before a def) are easy; trailing/inline comments need node-level trivia (type
aliases, ports, body items). Then regenerate every hand-written `.ys` canonically
(~190 comment lines to preserve). *~half-day.*

⏳ **#12 — prism-svg: SVG as place-graph values.** Types for SVG nodes
(svg/g/rect/line/polyline/text); a `Figure` becomes a tree of typed nodes
(homoiconic); rendering = a `serialize`. Replaces the plotters string in
`prism-viz::render_timeseries_svg`. *Medium.*

🧹 **#13 — fully retire `extern`.** The import model (`from … import`) replaced it,
but the construct is still live: `Tok::Extern` / `Def::Extern` / `parse_extern_def`
with `compile`/`eval`/`check`/`unparse` arms, the grow-divide fixtures
(`src/fixtures/grow_divide*.rs`), and five tests (`parse_contract.rs`,
`contract_enforcement.rs`, `parse_use_import.rs`, `compile_imports.rs`,
`grow_divide_homoiconic.rs`). Migrate those to imports, then delete the token + AST
variant + every arm. The emacs mode already de-highlights `extern` (2026-05-22), so
the grammar is the current laggard. *Small-medium; mechanical, ~13 files.*

🦷 **#14 — real contract enforcement through `RunProcess`.** The flagship
`integrator-comparison.ys` only *declares* `fulfills` on the integrators; its
`Compare` takes plain `TimeSeries`, so nothing is rejected in that file (the teeth
live in the dedicated enforcement tests). To make its "a different target would not
compile" comment literally true: (a) add `:: DeterministicMassAction` to
`Compare`'s input ports, and (b) propagate the wrapped process's output contract
through `RunProcess`'s `timeseries` output — today `RunProcess` is a generic driver
that drops it, so `check::provider_contract` finds no contract at `rk4_traj`. The
real capability: a contract a process carries must survive a generic wrapper. Fix
in prism (`RunProcess` forwards its inner process's output contract) + a test that
a wrong-target inner process is rejected at the demanding port. *Medium.*

📦 **#15 — spatio-flux as a `.ys` project (slices 1–2 DONE; this arc IS #10's
slices).** `spatio_flux::prelude::{sf_core, sf_registry, sf_methods, sf_modules}`
expose its natives (FBA/diffusion/particles) as a Core / run-path packages — slices
1–2 ✓, the `sf` bin proves the run path in-tree. Remaining = **#10's slices 3–4**:
port the demos to `.ys` (the stale `dish.ys`/`culture.ys` first — they use
`extern Diffusion`/wrong names) and the codegen so `chrysalis run spatio-flux/ys/*`
works with no per-package bin. Layering: spatio-flux → chrysalis (the package half);
chrysalis never deps spatio-flux. *Large; gated on #10's codegen.*

🩺 **#16 — chrysalis diagnostics: comprehensible `.ys` error messages.** A
first-class diagnostics subsystem so EVERY way a `.ys` program can go wrong gives a
clear, located, actionable, *educational* error (rustc/Elm-quality), in
surface-language terms (not prism internals). Three parts: (a) a survey/taxonomy of
failure modes — parse errors, unknown imports, unknown process/type refs (the new
`document references unregistered process(es): …` is the first instance),
contract mismatches, wiring/port errors (unbound port, cross-wire type mismatch),
unbound vars, unit-dimension errors, schema-resolution failures, malformed
addresses; (b) a structured diagnostic (code + source span + message + why +
how-to-fix) threaded parse→eval→compile→check→run and surfaced by the CLI; (c) a
test suite of deliberately-wrong `.ys`, each asserting its exact diagnostic, so
error *quality* is regression-guarded. *Medium-large; high-DX-value; generalize
the missing-ref pattern.*

✅ **#17 — `chrysalis bigraph export`/`import` (DONE).**
`chrysalis bigraph export f.ys out.json` writes a process-bigraph `Document`
(**schema rendered alongside state** — else import loses `Array`/`Delta`);
`chrysalis bigraph import out.json [--time T]` reads it back and runs it. Built:
`runner::{document_of, to_document, run_document}` + the CLI dispatch; the
invariant **`import(export(f))` ≡ `run(f)`** is proven by
`chrysalis/tests/export_import.rs` (round-trip via the program's own core).
*Remaining:* a doc with USER-defined processes needs that program's core — the
CLI's `std_core` import runs std-self-contained docs and **clearly rejects**
others (the missing-ref diagnostic, e.g. "document references 5 unregistered
process(es): …"). Full self-containment — capturing user-process *bodies* as
homoiconic values, or naming packages — is the follow-on (ties to #10 codegen +
schema-as-state #20).

✅ **#18 — `chrysalis server` (DONE).** `chrysalis server [--port P]` serves the
std `Core` over the rest-process protocol (`prelude::std_core` — std factories +
the `Composite` factory + std methods; CLI parks the main thread). Lifecycle
cleanup + missing-ref rejection come from `RestProcessServer`. Proven by
`chrysalis/tests/server.rs` (serves a composite over REST with cleanup; rejects an
unknown-process doc) + the `grow_divide_over_rest` tests (a `rest:` node is
discovered + driven). Remaining polish: a named-package core (needs #10), graceful
shutdown. → **distributed simulation**: a `.ys` with `rest:` nodes pointing at
`chrysalis server`s.

✅ **#19 — `chrysalis repl` (DONE).** Session eval (defs/functions accumulate;
bindings + bare exprs print with inferred type), `:type`/`:env`/`:reset`/`:help`/
`:quit`, error-resilient, **redefinition overwrites** (a corrected `process`/`def`
wins, no dead duplicate). **rustyline** editor with **live syntax highlighting**
(ys tokenizer → ANSI by kind), **completion** (session names + keywords),
**history hints**, and **multi-line continuation** (write a multi-line `process` at
the prompt). `crates/chrysalis/src/repl.rs` (5 unit tests). *Follow-ons:*
`:load`/`:export`, stepping a workflow tick-by-tick over the step cache, completion
of methods/imported names, `reedline` if we want richer interaction.

🧬 **#20 — schema-as-state: a meta-schema; operate on the schema, reconcile state
to match.** Make the schema itself first-class operable STATE (a `Value`;
`schema_to_value`/`value_to_schema` already round-trip it), governed by a
META-SCHEMA so schema operations are themselves type-checked. The behavior:
mutating the schema (add a field, retype, add a `Custom` variant) **generates /
fills in the corresponding state to match** — new field → its `default`, removed →
dropped, retyped → `coerce`d — via the algebra (`default`/`coerce`/`apply`/
`reconcile`), **transactionally** (schema change + state reconciliation commit
atomically; state always satisfies its schema; roll back on failure). The
meta-circular completion of the schema-is-state principle (memories
`project_schema_as_state`, `project_schema_state_unity`); the substrate for
self-modifying simulations (a process that evolves its own type, illegal states
unrepresentable throughout). *Deep/foundational; a new named algebra op + laws,
not ad-hoc munging.*

🧩 **#21 — complete surface-language defaults.** `algebra::default` is ported +
faithful; config params + port decls already parse `name: T = default`. Extend:
type-field defaults (`type Cell = {mass: float = 1.0}`); schema-driven fill-in at
construction (a partial record/term gets its missing fields from the schema's
`default`); defaults through composite config / contract pins / the REPL; a
value-method to ask a schema for its default. Where a default exists, missing →
filled (not an error; coordinate with diagnostics #16 for genuinely-required
fields). The op is ported — this surfaces it through parse→eval→compile.
**INCLUDE the process `interval` wire default** (noted 2026-05-24): a process's
`interval` is an automatic wire to `%.interval` (its own local interval slot) —
inconsistent with other same-name auto-wires (which bind to the enclosing scope).
Make it an *explicit, overridable* default in the process schema (`interval`
defaults to `%.interval`), so a process can instead wire its interval to a
**shared clock** driving many processes at once. This rides the **dynamic-timestep
engine mechanism shipped 2026-05-24**: the scheduler re-reads `[name, "interval"]`
from state each tick (a step can rewrite it — a Gillespie τ), and nested
`Overwrite` outputs are now honored in `apply_reconciled` (the per-port schema is
nested back into the store, not dropped). Proven by the `MinimalGillespie` port in
`crates/prism-bigraph/tests/gillespie.rs` (event Process + interval Step → the
engine schedules by the dynamic τ = 1/(k·a)).
**Principled shape (2026-05-24):** there is no separate "default wire" mechanism —
a port's default *is* its type's `default`. The `Link`/edge `default` (upstream
`default_wires`) gives every port its default wiring, today uniformly same-name
`[port]`. Make `interval` a port of an **inner-wire type** whose `default` is
`%.port` (self-local) instead of `[port]` — so `default` *earns its keep* (the
self-wire is a `default` output, consulted per-port in the `Link` arm), `interval`
is "just another type" with no engine/chrysalis special-case, and an **override**
is an ordinary explicit wire (`~{interval: clock}` → a shared clock driving many
processes). The engine then reads the **resolved** `interval` input each tick
(self default → `[name,"interval"]`; override → the wired target), replacing the
current engine special-case (which only sees the self default).
**STATUS (2026-05-24):** the OVERRIDE half shipped — the engine reads `interval`
as the *resolved input* (overridable to a shared clock; `[name,"interval"]`
fallback), `.ys` gained `overwrite[Float]` (`SchemaExpr::Overwrite`), and
`crates/chrysalis/ys/gillespie.ys` + the Rust port (`prism-bigraph/tests/
gillespie.rs`) prove the dynamic τ = 1/(k·A) (full workspace green). REMAINING:
the **inner-wire type** producing `interval`'s `%.port` default wire via
`default_wires`, retiring the `[name,"interval"]` engine side-channel. *Medium.*

✍️ **#22 — enforce `::`=type / `:`=value / `fulfills`-contract** (decided
2026-05-23; chrysalis-design.md resolved decision #22). `::` ascribes a TYPE
everywhere (def + return, params `f(x :: T)`, ports `~{state :: map[float]}`,
config `[out :: Path]`, pattern sorts `?c :: Cell`); `:` binds a VALUE (map
entries, call args, wirings); the contract is the keyword **`fulfills`** on both
sides (`process Rk4 fulfills Det` / port `~{a :: TimeSeries fulfills Det}`,
replacing `a: T :: Contract`). Migration: parser flips type-position `:`→`::` +
port `:: C`→`fulfills C` (consider a lenient phase then tighten); unparser emits
the new forms; regenerate every `.ys` + GUIDE/design examples; update parse tests.
*Medium; do while the syntax is young.*

✅ **#23 — explicit composite-bridge syntax `@` + self → `%`** (DONE
2026-05-23; chrysalis-design.md resolved decision #23). A composite's
interface ports map to inner-state paths (the *bridge*); this was assumed
(name-inference) and is now *declarable*: `name :: Type [@ inner.path]
[fulfills C] [= default]` (e.g. `~{values :: Map[Float] @ fields.values}`).
Absent ⇒ name-inference, backward-compatible. `@` (formerly the self/here
sigil) is now the bridge operator; **self/here moved to `%`** (`->{env: %}`,
`%.volume`). Lexer `%`→`Tok::Percent`; `PortDecl.bridge: Option<Vec<Name>>`;
`eval::build_composite_outer` reads the explicit wire (else `[port]`);
unparser emits `@ path` (and `%` self), in parser order (bridge, `fulfills`,
`= default` — fixed a latent default-before-contract roundtrip bug). The 8
`@`-as-self uses across grow-divide / nuclear-shuttle `.ys` migrated to `%`;
emacs `ys-mode` sigils updated. Tests: `crates/chrysalis/tests/composite_bridge.rs`
(parse, name-infer fallback, unparse-fixpoint, **eval lands in `config.bridge`**).

🚧 **#24 — a file IS a composite (invocation = `Trace[In] → Trace[Out]`)** (batch
shipped + streaming DESIGNED 2026-05-23; chrysalis-design.md decision #24; harness
task #21). The rule: **a file's value is its last top-level term**; defs are
importable vocabulary; the *last* `composite`/`process`/`def` is the runnable
interface; a trailing headless `[config] ~{} ->{} ( body )` is anonymous-composite
sugar; no interfaced term ⇒ pure package. **THE MODEL** (designed this session): an
invocation is the morphism **`Trace[In] → Trace[Out]` seeded by config** — the CLI
is one *transport* of the same port boundary as `~{}`/`->{}` internally and `rest:`
over the network; a pipe `A.ys | B.ys` *is* `B ∘ A`. Config = the **t=0 seed** (set
once, picks the morphism); input = the **t>0 stream**. You don't need two kinds of
input: **config can only be seeded (flags); input can be seeded (flag=t=0) OR
driven (stdin=stream)** — time-invariance is what makes a value config. **Batch
("set a config and run") is the degenerate t=0→t=final collapse.**
DONE (the batch collapse): `Program::entry`; `compile` no-`main` fallback inlines a
bare-runnable composite; `runner::invoke` binds `[config]`+`~{inputs}` from flags
via `realize`, serializes the final-frame `->{outputs}` record to stdout /
`--out FILE`; `chrysalis run f.ys --<port> SOURCE`; `--time` = duration (engine
`interval` = per-step dt). Tests: `tests/file_entry.rs`, `tests/invoke.rs`.
**OPEN DECISIONS — RESOLVED:** wire = **delta-log/Arrow straight off** (not
JSON-first — streaming/distributed needs it regardless; JSONL stays a
human-readable projection); name mismatch = **`--map out=in`** rename sugar now,
**`--adapt adapter.ys`** (a real adapter composite) later; the stream **schema
header is MANDATORY** (check `refines` at connect, before data flows; mirrors
`document_of`); flag↔stdin = **flag is t=0**, a stream frame overrides from that
frame on (a stream frame naming a config param ⇒ diagnostic #16).
SLICES (each additive; batch preserved): (1) **output→trace** — per-tick output
delta-log (Arrow, schema header first), keep last-frame for a TTY (hooks
`Simulate`/`Trace[T]`, #25); (2) **input→trace** — feed stdin frames per tick,
flags the t=0 seed; (3) **schema header + `refines`** at connect; (4) **`--map`**
then **`--adapt`**; (5) **process/def entries** (uniform boundary; `def` = the
zero-time pure-function collapse). NOTE: legacy `.ys` ending in `env.run(t)`
(grow-divide-unbounded) never ran via the bin (`run` isn't a method); migrate to
end in the composite + `--time`.

🎨 **#25 — type-driven canonical outputs: `plot(schema, trace)` + report sections
as self-outputting workflows** (STARTED 2026-05-23). The crystallized vision (co-
designed): **a report section is a complete `.ys` workflow that derives its
canonical outputs from the composite's TYPES** — not a hand-written plot per sim.

THE UNIFICATION: *everything is a single-item type extended through time = a
**trace** (`list[state]`)*, and **`plot(schema, trace)` is a schema-driven
operation** (dispatches on schema like the algebra's `serialize`/`apply`/`divide`):
- scalars (`float`/`map[float]`/tree-of-scalars) → **line plot** (the integrator
  `TimeSeries` is just this case);
- a **field** (`array`/`map[array]`) → **heatmap / animation**;
- the **structure** (the state `Tree`) → a **"4D" bigraph-viz**: the structure
  shown at its change-points. The t=0 tree is just one snapshot; the structural
  trace is naturally stored as **diffs** (`algebra::diff`, reconstructed via
  `apply` — change-only), so structure is *not special* — it's another type
  whose trace is plotted.

**THE FULL SYNTHESIS → [`docs/delta-traces.md`](delta-traces.md)** (co-designed
2026-05-23): the entire system is a **procession of schema-deltas in time**
(event sourcing); `apply` = replay, `diff` = compute-delta; the **brutality
lattice** (additive → overwrite → `_remove`/`_add` structural); a fixed-shape
trace **is a tensor** `[time × dims]` (Arrow-friendly) that **structural deltas
reshape** → a *piecewise tensor*; value-deltas vs structural-deltas = the
bigraph's **two graphs** (state/tensor vs place/topology). Read that doc first.

DONE: `prism_viz::plot(schema, trace, title) -> Value` returns a **place-graph
SVG** (not a string): scalars → line chart, field → **animated** heatmap (SMIL
`<animate>` — the animation is place-graph data), else → note. `prism_viz::svg`
(SVG-as-place-graph + `to_svg` + `el/rect/line/text/polyline/animate`).
`RunProcess` emits the full `trace` carrying its element schema `Trace[T]` (no
infer). `Trace.plot` uses the carried `T`. `Figure` carries the place-graph under
`root`; `figure.svg` serializes via `to_svg`. `report-section.ys` — the reusable
`Section[sim, …]` template (run → trace → plot → self-output) renders a real line
chart end-to-end. Tested across prism-viz/prism-std/chrysalis.

REMAINING (ordered): (1) **`Simulate`** — the engine-faithful **event-source
runner** (vs `RunProcess`, matched to full-output integrators): feed the inner
its full state, `apply` its update via the state schema (kinetics deltas add,
diffusion fields replace — schema decides), capture `Trace[T]`. Storage: frames →
**delta-log + replay** → **hybrid keyframes + Arrow**. (2) **4D structural** plot
(bigraph-viz per `_add`/`_remove`) + expose a bigraph-viz writer to `.ys`.
(3) the `Section` template uses `Simulate` so **spatio-flux sections** run via the
codegen tool. (4) apply down `CANONICAL_ORDER`, validating each family.
*The abstraction is settled (see delta-traces.md); the rest is the runner + the
per-type renderers + the template + serialization.*

⚡ **#26 — performance sweep (do LAST, after the feature set).** The user will use
`.ys` as their primary language for ~everything, so the runtime must be performant
+ direct. Once features are in: establish benchmarks, profile the hot paths — eval
(ExprProcess body interpretation per tick; consider compiling bodies vs re-walking
the AST), the engine step loop (`advance_to_next_event`), `algebra`
apply/diff/reconcile + `Value` cloning, the delta-log/Arrow codec + streaming +
stream-protocol overhead, discovery — then optimize **without** sacrificing the
principled design (closed algebra, no half-measures). Honor the zero-cost
principle (check once, erase, run raw). Gated on features; not started. See memory
`project_ys_primary_language`.

🌐 **HORIZON — distributed bigraphs across HPC → [`docs/distributed-bigraphs.md`](distributed-bigraphs.md)**
(vision, 2026-05-23). prism IS already a distributed actor system (place graph =
partition, link graph = channels, composite = actor, delta = message, algebra =
sync, trace = durable log). Working backwards, the whole HPC picture reduces to
**ONE seam: the engine stepping a `rest`-addressed subengine like a local one**
(send input state, get output delta, `apply`) — then a chrysalis `composite`
reaches an HPC by flipping a subengine's address `local`→`rest`, *`.ys`
unchanged*. FIRST MOVE: split one composite across two REST processes (parent
engine ↔ `RestProcessServer` child) exchanging bridge deltas once per step (BSP
barrier of 2); builds on the REST server we shipped. Protocols are a **ladder**
(REST control-plane → Arrow Flight data-plane → MPI/UCX collectives, where
`all-reduce` = commutative-delta `merge`), pluggable behind the protocol seam.
The parallelize-vs-synchronize lines are schema-derived (commutative deltas →
async; non-commutative → barrier). Far horizon; the linchpin is small.

**Suggested order.** **Immediate — the delta-traces kernel** (`Simulate` +
delta-log/Arrow codec, prism-side): the linchpin just designed — the start of #25
AND the prerequisite for #24 streaming (we chose straight-to-Arrow, no JSON
half-measure; see chrysalis-design.md decision #24 + docs/delta-traces.md). Once it
lands the work **forks in parallel**: **#24 streaming** (output→trace, input→trace,
schema-header/`refines`) ∥ **#25 viz** (plot per-type, 4D structural, the `Section`
template). **#24's surface** (`--map`/`--adapt`, process/def entries) needs nothing
from the kernel — anytime. The other marquee is **#10 codegen** — it unblocks #15
(spatio-flux as `.ys`), full export/import self-containment, and `server`/`import`
on packages (the single path). Then **#16 diagnostics → #21 defaults → #6 SBML (the
science headline) → #20 schema-as-state**. Quick wins anytime: **#7** dt-sweep,
**#13** retire-extern, **#8** comment retention, **#12** prism-svg (folds into the
kernel's svg-as-place-graph), **#14** real-enforcement-through-RunProcess. (Done
this session: Core unification, StepCache, rest server + discovery, per-composite
cache, missing-ref, **#17/#18/#19** CLI + REPL.)

## Long-run milestones (beyond the tracker)
- **Distributed execution at scale (`docs/distributed-execution.md`)** — giant
  colonies growing exponentially across arbitrary machines. The fractal/octree
  vision = the SOTA HPC pattern (spatial domain decomposition + halo exchange +
  Barnes-Hut/FMM aggregation + adaptive forest-of-octrees). prism's
  composite-of-composites (= the octree), protocol boundary (= a remote subdomain),
  bridge (= halo exchange), encapsulation (= FMM aggregation), and BSP tick (= the
  superstep) already encode the skeleton. Gaps: batched RPC (#25), halo/neighbor
  exchange (#26), spatial-partition protocol (#27), dynamic load balancing (#28),
  a cluster backend (#29). Key decision: the cluster is a **protocol with a
  pluggable backend** — adopt Charm++ (closest fit: migratable objects + auto load
  balancing) / Ray (FT + autoscale) / MPI (raw halo speed); do NOT rebuild Ray's
  runtime. What matters most: locality, dynamic load balancing, hierarchical
  aggregation, relaxed/local sync, compute/comm overlap.
- **First-class `Custom` types** — `type Name = <repr> with { op = …,
  method(args) = … }` (the algebraic-effects handler model): make a `Custom`
  indistinguishable from a built-in sort by threading a registry through the
  algebra ops. `CRN`/`Path` are synthetic types today; this makes them truly
  first-class. The contract layer + import-types are the substrate.
- **KISAO export (contract rung 3)** — contract → KISAO term mapping; OMEX/SED-ML
  round-trip. "Tell us your target semantics and we'll tell you the admissible
  methods" — a contribution back to the standard, not just consumption.
- **dFBA cross-target bridge** — comparing a kinetic integrator vs dFBA needs an
  explicit, *named* cross-target contract (the contract forces you to name the
  approximation rather than silently MSE-ing incomparable things).
- **More demos as `.ys`** — convert remaining examples; optional rung-2 real
  COPASI/Tellurium via a BioSimulators subprocess bridge (interop, not new
  contract content).
- **Tier-2: evolving M/R** — process bodies as first-class values (after
  Fontana's AlChemy); schema-driven typed construction so illegal programs are
  unrepresentable.
- **A packages ecosystem** — #15 makes spatio-flux the first non-std `.ys`
  package; generalize so any native crate ships importable `.ys` modules (a
  `project.ys` manifest + a resolver/registry), plus a fundamental-type catalog.

## Build / test loop
Build is slow (links many binaries). Use `cargo check -p <crate>` for "does it
build", `cargo test -p <crate> --test <name>` for the touched area, full
`cargo test --workspace` only at checkpoints. Wrap runs in `timeout` — prism
tests run <1s, so a slow run means an infinite loop / structural explosion, not
patience. No `.cargo/config.toml` linker override (rustflags change ⇒ whole-tree
rebuild). See memories `feedback_test_timeout`, `feedback_no_sleep_polling`.

## Pointers
- `README.md` — the port + chrysalis + commands.
- `crates/chrysalis/ys/GUIDE.md` — how to write `.ys` (the tutorial).
- `docs/prism-architecture.md`, `docs/schema-algebra.md`,
  `docs/chrysalis-design.md`, `docs/process-contracts.md`.
- Memories: `feedback_ys_layering` (the architecture), `feedback_chrysalis_thin_layer`,
  `feedback_no_half_measures`, `feedback_consult_upstream`, `feedback_test_timeout`.

## Historical (done — provenance in git)
- The fresh-core **schema-algebra rebuild** — closure achieved; `prism_schema::
  algebra` is the single door; 13 laws + closure-guard green.
- The **engine execution-correctness arc** — invoke/apply separation, `reconcile`
  wired into apply, dependency-layered step firing, schema always inferred.
- The **process-contract demo** — built and now runs via `chrysalis run`.
