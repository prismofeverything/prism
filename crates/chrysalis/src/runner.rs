//! The shared `run` command: compile a `.ys` program against the host's native
//! packages and run it. chrysalis owns the runner; a host invokes it with its
//! OWN packages (process factories, value-methods, importable modules) — so
//! there is one run path, parameterized, not one re-implemented per host.
//!
//! Layering: this depends only on chrysalis + prism (the engine). The natives a
//! program imports are supplied by the caller, keeping chrysalis independent of
//! any downstream package (e.g. spatio-flux).

use std::collections::BTreeMap;
use std::io::Read;

use indexmap::IndexMap;
use prism_bigraph::{Core, Document, Engine, ProcessRegistry};
use prism_schema::{algebra, schema_to_value, value_to_schema, Key, MethodRegistry, Schema, Value};

use crate::ast::{CompositeDef, Def, Expr, Name, PortDecl, Program, SchemaExpr};
use crate::compile::{
    collect_top_level_bindings, compile_with_modules, CompileError, CompileResult, ModuleRegistry,
};
use crate::eval::Evaluator;
use crate::schema::{composite_inner_schema, lower_schema_in_program};

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("{0}")]
    Compile(#[from] CompileError),
    #[error("engine init: {0}")]
    Engine(String),
    #[error("invoke: {0}")]
    Invoke(String),
}

/// Compile `program` against the host's native packages (`registry` process
/// factories, `methods` value-methods, `modules` importable native modules) and
/// run it for `time`, returning the final engine state. The program's own
/// `Output` steps, if any, write artifacts during the run.
pub fn run(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
    time: f64,
) -> Result<Value, RunError> {
    let result = compile_with_modules(program, registry, methods, modules)?;
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .map_err(RunError::Engine)?;
    engine.discover_all_processes();
    engine.run(time);
    Ok(engine.state().clone())
}

/// Render a compiled program as a process-bigraph [`Document`] — the schema
/// rendered ALONGSIDE the state. The schema must travel with the state: without
/// it, import would lose apply-critical types (`Array`/`Delta`) and re-run wrong.
pub fn document_of(result: &CompileResult) -> Document {
    let mut doc = Document::new();
    doc.schema = Some(schema_to_value(&result.topology.state_schema));
    doc.state = result.initial_state.clone();
    doc
}

/// Compile `program` and render it as a [`Document`] (the `bigraph export` side).
pub fn to_document(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
) -> Result<Document, RunError> {
    Ok(document_of(&compile_with_modules(program, registry, methods, modules)?))
}

/// Run a process-bigraph [`Document`] directly against `core` (the `bigraph
/// import` side). The document carries schema + state; `core` supplies the
/// factories its addressed nodes reference. Invariant:
/// `run_document(document_of(compile(p)), p's core, t)` ≡ `run(p, …, t)`.
pub fn run_document(doc: &Document, core: Core, time: f64) -> Result<Value, RunError> {
    let schema = doc.schema.as_ref().and_then(value_to_schema).unwrap_or(Schema::Any);
    let mut engine =
        Engine::from_state(schema, doc.state.clone(), core).map_err(RunError::Engine)?;
    engine.discover_all_processes();
    engine.run(time);
    Ok(engine.state().clone())
}

// ── Compositional invocation (decision #24) ─────────────────────────────────

/// Read an input SOURCE spec to its raw serialized text. The connector is
/// **explicit** — the schema only ever drives `realize`, never where bytes come
/// from. Forms: bare text is a *literal*; `file:PATH` reads a file (bash
/// process-substitution `file:<(gen)` works, covering "pipe a huge value");
/// `-`/`stdin:` reads stdin (the one pipe); `stream:` is reserved for the
/// streaming layer; `lit:` forces a literal that would otherwise look like a
/// scheme.
fn read_source(spec: &str) -> Result<String, RunError> {
    let s = spec.trim();
    if s == "-" || s == "stdin:" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| RunError::Invoke(format!("read stdin: {e}")))?;
        Ok(buf)
    } else if let Some(path) = s.strip_prefix("file:") {
        std::fs::read_to_string(path).map_err(|e| RunError::Invoke(format!("read {path}: {e}")))
    } else if s.starts_with("stream:") {
        Err(RunError::Invoke("stream: sources not supported yet (batch I/O only)".into()))
    } else {
        Ok(s.strip_prefix("lit:").unwrap_or(s).to_string())
    }
}

/// Parse a source's text as the JSON-compatible [`Value`] that `serialize`
/// produces. A bare word that isn't valid JSON is taken as a string literal, so
/// both `--name foo` and `--name '"foo"'` work.
fn parse_encoded(text: &str) -> Value {
    let t = text.trim();
    serde_json::from_str::<Value>(t).unwrap_or_else(|_| Value::String(t.to_string()))
}

/// Bind one config param / input port `name` into `env`: from the command-line
/// SOURCE (decoded via the schema's `realize`) if present, else its default,
/// else an error. Defaults evaluate against the env built so far.
#[allow(clippy::too_many_arguments)]
fn bind_arg(
    env: &mut IndexMap<Name, Value>,
    ev: &Evaluator,
    program: &Program,
    args: &BTreeMap<String, String>,
    name: &str,
    schema_expr: &SchemaExpr,
    default: &Option<Expr>,
    kind: &str,
) -> Result<(), RunError> {
    let schema = lower_schema_in_program(schema_expr, program);
    let val = if let Some(spec) = args.get(name) {
        algebra::realize(&schema, &parse_encoded(&read_source(spec)?))
    } else if let Some(def) = default {
        ev.eval_value(def, env).map_err(CompileError::from)?
    } else {
        return Err(RunError::Invoke(format!("missing required {kind} `--{name}`")));
    };
    env.insert(name.to_string(), val);
    Ok(())
}

/// Resolve a program's entry as a `composite` (the only invokable kind so far).
fn resolve_entry(program: &Program) -> Result<CompositeDef, RunError> {
    match program.entry() {
        Some(Def::Composite(d)) => Ok(d.clone()),
        Some(other) => Err(RunError::Invoke(format!(
            "entry `{}` is a {}; only `composite` entries are invokable so far",
            crate::ast::def_name(other),
            entry_kind(other),
        ))),
        None => Err(RunError::Invoke(
            "no entry point: this file declares no composite/process/def to run".into(),
        )),
    }
}

/// Shared setup for both invocation paths: resolve the entry composite, bind its
/// `[config]` params and `~{inputs}` from `args` (the t=0 seed), evaluate the
/// body to the root state, and return a discovered engine ready to run plus the
/// entry (for output extraction).
fn engine_for(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
    args: &BTreeMap<String, String>,
) -> Result<(Engine, CompositeDef), RunError> {
    let entry = resolve_entry(program)?;
    let result = compile_with_modules(program, registry, methods, modules)?;
    let ev = &result.evaluator;

    // Seed env with top-level bindings, then bind config params + input ports.
    // Flags are the t=0 seed; per-tick input streams (stdin) are a later slice.
    let mut env = collect_top_level_bindings(program, ev)?;
    for p in &entry.params {
        bind_arg(&mut env, ev, program, args, &p.name, &p.schema, &p.default, "config")?;
    }
    for (name, port) in &entry.interface.inputs {
        bind_arg(&mut env, ev, program, args, name, &port.schema, &port.default, "input")?;
    }

    // Root state = the entry body evaluated with config + inputs in scope.
    let root = ev.eval_value(&entry.body, &env).map_err(CompileError::from)?;
    let schema = algebra::resolve(&Schema::infer(&root), &composite_inner_schema(&entry, program));
    let mut engine =
        Engine::from_state(schema, root, result.core.clone()).map_err(RunError::Engine)?;
    engine.discover_all_processes();
    Ok((engine, entry))
}

/// **Compositional invocation** — run a `.ys` file as its entry composite, with
/// the command line bound to that composite's interface (decision #24): each
/// `[config]` param and `~{input}` is filled from `args` (or its default), the
/// body becomes the root state, and after running `duration` each `->{output}`
/// is read back through its `@` bridge and `serialize`d into the returned record.
/// This is the **batch final frame** of the run; [`invoke_trace`] keeps the whole
/// time axis. Because input/output share the algebra codec, a run's output
/// round-trips as another run's input.
pub fn invoke(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
    args: &BTreeMap<String, String>,
    duration: f64,
) -> Result<Value, RunError> {
    let (mut engine, entry) = engine_for(program, registry, methods, modules, args)?;
    engine.run(duration);
    Ok(output_record(engine.state(), &entry, program))
}

/// **Compositional invocation as a trace** (decision #24, output→trace) — like
/// [`invoke`], but instead of keeping only the final frame it samples the entry's
/// `->{outputs}` every `sample_dt` over `duration` and returns the run as a
/// delta-log `Trace[T]` (`prism_trace`). The element `T` is the record of
/// output-port schemas (carried from the interface, never inferred). Serialize it
/// to the Arrow wire with `prism_trace::serialize_trace`; batch [`invoke`] is
/// exactly this trace's final frame.
pub fn invoke_trace(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
    args: &BTreeMap<String, String>,
    duration: f64,
    sample_dt: f64,
) -> Result<Value, RunError> {
    let (mut engine, entry) = engine_for(program, registry, methods, modules, args)?;
    let element = output_schema(&entry, program);

    let mut samples: Vec<(f64, Value)> = vec![(0.0, output_raw(engine.state(), &entry))];
    let steps = if sample_dt > 0.0 { (duration / sample_dt).round().max(0.0) as usize } else { 0 };
    for k in 1..=steps {
        engine.run(sample_dt);
        samples.push((k as f64 * sample_dt, output_raw(engine.state(), &entry)));
    }
    Ok(prism_trace::trace_of(&entry.name, &element, samples))
}

/// **Driven invocation** (decision #24, input→trace) — run the entry composite
/// with its `~{inputs}` *driven* by `input_trace` (an Arrow-decoded `Trace`):
/// each frame is injected into the input ports' bridge paths at successive ticks
/// (the stream's first frame is the t=0 seed; explicit `args` flags still win),
/// and the entry's `->{outputs}` are captured as the returned output `Trace[T]`.
/// At connect, the input trace's element must `refines` the entry's input
/// interface — the schema header makes this check mechanical. This closes the
/// pipe: `A.ys --trace | B.ys` is the composition `B ∘ A`.
pub fn invoke_driven(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
    args: &BTreeMap<String, String>,
    input_trace: &Value,
) -> Result<Value, RunError> {
    let entry = resolve_entry(program)?;

    // Connect-time type check: the producer's element must refine our inputs.
    let in_elem = prism_trace::element(input_trace);
    let want = input_schema(&entry, program);
    if !algebra::refines(&in_elem, &want) {
        return Err(RunError::Invoke(format!(
            "input stream does not fit this file's inputs: {in_elem:?} does not refine {want:?}"
        )));
    }

    // Seed the t=0 inputs from the stream's first frame (explicit flags win).
    let in_frames = prism_trace::frames(input_trace);
    let mut seed = args.clone();
    if let Some(f0) = in_frames.first() {
        for (name, _) in &entry.interface.inputs {
            if let Some(v) = f0.get_field(name) {
                seed.entry(name.clone())
                    .or_insert_with(|| serde_json::to_string(v).unwrap_or_default());
            }
        }
    }

    let (mut engine, entry) = engine_for(program, registry, methods, modules, &seed)?;
    let element = output_schema(&entry, program);

    // Drive: advance by each frame's actual time-delta (dynamic dt — the input
    // trace's `times` ARE the schedule, so the inner process sees the real
    // per-step interval; the first frame is the t=0 seed). Same per-step body as
    // the streaming filter.
    let in_times = prism_trace::times(input_trace);
    let mut out_samples: Vec<(f64, Value)> = Vec::with_capacity(in_frames.len());
    let mut prev_t = in_times.first().copied().unwrap_or(0.0);
    for (k, frame) in in_frames.iter().enumerate() {
        let t = in_times.get(k).copied().unwrap_or(prev_t);
        let out = drive_step(&mut engine, &entry, program, frame, t - prev_t, k > 0);
        out_samples.push((t, out));
        prev_t = t;
    }
    Ok(prism_trace::trace_of(&entry.name, &element, out_samples))
}

/// One driven step: inject an input `frame` at the input bridge paths (queueing
/// the changed paths so change-triggered steps fire), optionally advance the
/// engine by `sample_dt`, and read back the output record. Shared by
/// [`invoke_driven`] and [`serve_stream`].
fn drive_step(
    engine: &mut Engine,
    entry: &CompositeDef,
    program: &Program,
    frame: &Value,
    sample_dt: f64,
    run: bool,
) -> Value {
    let mut changed: Vec<Vec<Key>> = Vec::new();
    for (name, port) in &entry.interface.inputs {
        if let Some(v) = frame.get_field(name) {
            let path = output_path(name, port);
            let schema = lower_schema_in_program(&port.schema, program);
            engine.state_mut().set_path(&path, algebra::realize(&schema, v));
            changed.push(path);
        }
    }
    engine.queue_changes(changed);
    if run {
        engine.run(sample_dt);
    }
    output_raw(engine.state(), entry)
}

/// **Streaming filter** (decision #24 — the `stream:` protocol's child side): run
/// the entry composite as a *live* `Trace[In] → Trace[Out]` filter. Read input
/// frames from `input` (an Arrow trace stream), step one tick per frame, and emit
/// the output frame to `output` as a delta-log (first output absolute, the rest
/// diffs), flushing each (lock-step). Unbounded — runs until `input` hits EOF.
/// The connect-time `refines` check uses the stream header. This is what
/// `chrysalis run f.ys --serve-stream` runs, and what a parent `stream:` process
/// drives over OS pipes — proxying the program as a process.
pub fn serve_stream(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
    args: &BTreeMap<String, String>,
    input: impl std::io::Read,
    output: impl std::io::Write,
) -> Result<(), RunError> {
    let entry = resolve_entry(program)?;
    let mut reader = prism_trace::TraceReader::new(input)
        .map_err(|e| RunError::Invoke(format!("open input stream: {e}")))?;

    // Connect-time type check against the stream header.
    let in_elem = value_to_schema(reader.element()).unwrap_or(Schema::Any);
    let want = input_schema(&entry, program);
    if !algebra::refines(&in_elem, &want) {
        return Err(RunError::Invoke(format!(
            "input stream does not fit this file's inputs: {in_elem:?} does not refine {want:?}"
        )));
    }

    // Read the first frame to seed the t=0 inputs, then build the engine.
    let first = match reader.next() {
        Some(r) => r.map_err(|e| RunError::Invoke(format!("read frame: {e}")))?,
        None => return Ok(()), // empty stream: nothing to filter
    };
    let mut seed = args.clone();
    for (name, _) in &entry.interface.inputs {
        if let Some(v) = first.1.get_field(name) {
            seed.entry(name.clone()).or_insert_with(|| serde_json::to_string(v).unwrap_or_default());
        }
    }
    let (mut engine, entry) = engine_for(program, registry, methods, modules, &seed)?;

    let out_schema = output_schema(&entry, program);
    let mut writer = prism_trace::TraceWriter::new(output, &entry.name, &schema_to_value(&out_schema))
        .map_err(|e| RunError::Invoke(format!("open output stream: {e}")))?;

    // Each input row is folded back into a full frame (the stream is a delta-log:
    // first row absolute, the rest diffs — exactly `prism_trace::frames` done
    // incrementally), stepped, then the output emitted the same way (first
    // absolute, the rest diffs), one frame at a time (lock-step).
    let mut pending = Some(first);
    let mut cur_in: Option<Value> = None;
    let mut prev_out: Option<Value> = None;
    let mut prev_time: Option<f64> = None;
    loop {
        let (time, in_payload) = match pending.take() {
            Some(f) => f,
            None => match reader.next() {
                Some(r) => r.map_err(|e| RunError::Invoke(format!("read frame: {e}")))?,
                None => break,
            },
        };
        let frame = match &cur_in {
            None => in_payload.clone(),
            Some(prev) if matches!(in_payload, Value::None) => prev.clone(),
            Some(prev) => algebra::apply(&in_elem, prev, &in_payload),
        };
        cur_in = Some(frame.clone());

        // Advance by this frame's time-delta (dynamic dt; the `time`s are the
        // schedule). The first frame seeds — no advance. So the inner process
        // sees the real per-step interval, not a fixed rate.
        let dt = prev_time.map_or(0.0, |p| time - p);
        let out = drive_step(&mut engine, &entry, program, &frame, dt, prev_time.is_some());
        prev_time = Some(time);
        let out_payload = match &prev_out {
            None => out.clone(),
            Some(p) => algebra::diff(&out_schema, p, &out).unwrap_or(Value::None),
        };
        writer.push(time, &out_payload).map_err(|e| RunError::Invoke(format!("write frame: {e}")))?;
        writer.flush().map_err(|e| RunError::Invoke(format!("flush frame: {e}")))?;
        prev_out = Some(out);
    }
    writer.finish().map_err(|e| RunError::Invoke(format!("finish stream: {e}")))?;
    Ok(())
}

/// The carried element schema of the *input* interface: a `Tree` of the input
/// ports' schemas — the connect-time `refines` target.
fn input_schema(entry: &CompositeDef, program: &Program) -> Schema {
    let branches: IndexMap<Key, Schema> = entry
        .interface
        .inputs
        .iter()
        .map(|(name, port)| {
            (Key::from(name.as_str()), lower_schema_in_program(&port.schema, program))
        })
        .collect();
    Schema::Tree { branches }
}

/// The inner-state bridge path for an output port (`@ inner.path`, else `[name]`).
fn output_path(name: &Name, port: &PortDecl) -> Vec<Key> {
    port.bridge
        .clone()
        .unwrap_or_else(|| vec![name.clone()])
        .into_iter()
        .map(|s| Key::from(s.as_str()))
        .collect()
}

/// The entry's `->{outputs}` pulled through their bridges and `serialize`d into
/// one record — the batch final frame.
fn output_record(state: &Value, entry: &CompositeDef, program: &Program) -> Value {
    let mut record: IndexMap<Key, Value> = IndexMap::new();
    for (name, port) in &entry.interface.outputs {
        let val = state.get_path(&output_path(name, port)).cloned().unwrap_or(Value::None);
        let schema = lower_schema_in_program(&port.schema, program);
        record.insert(Key::from(name.as_str()), algebra::serialize(&schema, &val));
    }
    Value::Map(record)
}

/// The entry's `->{outputs}` pulled through their bridges as RAW values — the
/// per-frame element of the trace (the codec serializes them on the wire).
fn output_raw(state: &Value, entry: &CompositeDef) -> Value {
    let mut record: IndexMap<Key, Value> = IndexMap::new();
    for (name, port) in &entry.interface.outputs {
        let val = state.get_path(&output_path(name, port)).cloned().unwrap_or(Value::None);
        record.insert(Key::from(name.as_str()), val);
    }
    Value::Map(record)
}

/// The carried element schema of the output trace: a `Tree` of the output ports'
/// schemas (known from the interface — never inferred from data).
fn output_schema(entry: &CompositeDef, program: &Program) -> Schema {
    let branches: IndexMap<Key, Schema> = entry
        .interface
        .outputs
        .iter()
        .map(|(name, port)| {
            (Key::from(name.as_str()), lower_schema_in_program(&port.schema, program))
        })
        .collect();
    Schema::Tree { branches }
}

fn entry_kind(def: &Def) -> &'static str {
    match def {
        Def::Process(_) => "process",
        Def::Step(_) => "step",
        Def::Function(_) => "def function",
        _ => "definition",
    }
}
