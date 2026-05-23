//! The shared `run` command: compile a `.ys` program against the host's native
//! packages and run it. chrysalis owns the runner; a host invokes it with its
//! OWN packages (process factories, value-methods, importable modules) — so
//! there is one run path, parameterized, not one re-implemented per host.
//!
//! Layering: this depends only on chrysalis + prism (the engine). The natives a
//! program imports are supplied by the caller, keeping chrysalis independent of
//! any downstream package (e.g. spatio-flux).

use prism_bigraph::{Core, Document, Engine, ProcessRegistry};
use prism_schema::{schema_to_value, value_to_schema, MethodRegistry, Schema, Value};

use crate::ast::Program;
use crate::compile::{compile_with_modules, CompileError, CompileResult, ModuleRegistry};

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("{0}")]
    Compile(#[from] CompileError),
    #[error("engine init: {0}")]
    Engine(String),
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
