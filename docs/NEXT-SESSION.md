# Next session — launch prompt

Paste the block below as the first message of a fresh session to start the
**fresh-core schema-algebra rebuild**. It's self-contained: it points at the
durable spec (`docs/schema-algebra.md`), the `CLAUDE.md` conventions, and
the memories that auto-load, then states the goal, the invariant, and the
ordered plan.

Context for *why* this rebuild exists: across one long session we found that
prism's recurring pain was always an *operation outside the algebra* —
`ChrysalisBrs` (cloned the BRS), `infer_and_merge` + `overlay_apply_types`
(fake `resolve`), the schemaless `Composite`, a ported-but-dead `reconcile`.
The fix is a faithful, closed, **algebraic** core (an algebraic theory in
the Plotkin–Power sense) with the laws as executable axioms, then `Composite`
defined in terms of it. `docs/schema-algebra.md` is the spec.

Optionally prepend `/effort max`.

```
We're doing the fresh-core rebuild of prism's schema algebra. Start by reading
docs/schema-algebra.md (the spec/axioms) end to end, plus the two conventions in
CLAUDE.md (thin-layer; algebra-closure) and memories feedback_schema_algebra +
feedback_chrysalis_thin_layer. Skim docs/upstream-alignment.md. The upstream we
port faithfully is cloned at /home/pattern/code/bigraph-schema/bigraph_schema/methods/
and /home/pattern/code/process-bigraph/process_bigraph/composite.py.

GOAL: faithfully port the schema algebra and rebuild Composite *in terms of it*,
deleting every ad-hoc stand-in, so the core has real algebraic closure.

THE INVARIANT (non-negotiable): the schema layer is a closed algebra — an
algebraic theory (operations + laws). ALL schema/state transformation goes
through the algebra ops (default/check/infer/realize/apply/reconcile/merge/diff/
resolve/promote/generalize/coerce/divide/serialize/...). No hand-rolled
merge/diff/apply, no Schema::Any shortcut, no inline _add/_remove munging. A
behaviour the algebra can't express becomes a NEW named op with a signature +
laws (property tests) + the consumer that needs it — never a shortcut. Composite/
engine logic is *defined in terms of* the algebra (law #10).

APPROACH: axioms-first, op by op. For each op: write its laws as property tests,
implement faithfully against methods/<op>.py, make the laws pass, express the next
layer on it, delete the stand-in it replaces. Keep the periphery (bigraph matcher/
BRS, units, Value, divide, chrysalis, spatio-flux) — don't rewrite it. Keep the
whole workspace green at every step (336 tests are the behavioural net). cargo is
at ~/.cargo/bin (export PATH="$HOME/.cargo/bin:$PATH").

ORDER (docs/schema-algebra.md "Launch plan", tasks #20→#18→#19→#21):
0. #20 Phase-0 scaffolding FIRST — this is the closure guarantee: an `algebra`
   module that owns all schema/state transformation (make the raw mutators
   module-private so a shortcut won't compile); a laws/property-test harness
   (prism-schema/tests/algebra_laws.rs with (schema,value) generators); and a
   closure-guard test that fails if Schema::Any.apply_update / hand-rolled
   _add/_remove / bespoke merges reappear. Confirm green.
1. #18 schema lattice: generalize → resolve → promote (resolve.rs is a started,
   not-yet-wired WIP — pathless core + tests). Delete Schema::resolve stub,
   Schema::infer_and_merge, chrysalis overlay_apply_types.
2. #19 value/update ops: apply (faithful), reconcile, merge, diff. WIRE reconcile
   into the engine accumulation (delete the Schema::Any.apply_update spots ~
   engine.rs 876/895/1053) and use promote in apply (delete the
   apply_projections_to port-schema override). This fixes #14 — un-ignore
   spatio-flux/tests/chrysalis_port.rs::diffusion_composed_from_chrysalis as the proof.
3. #21 rebuild Composite::update = project → run inner → reconcile → apply → view,
   carrying its real inner schema (no Schema::Any).
4. Cut over + delete remaining stand-ins + encapsulate; closure-guard green.

DONE = every schema/state transform goes through the algebra; laws green;
closure-guard green; Composite defined via the algebra; all stand-ins deleted;
full workspace green; diffusion test un-ignored and passing.

Use max effort. Build it on bedrock, not around it.
```
