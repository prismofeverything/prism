//! The shared `run` command: compile a `.ys` program against the host's native
//! packages and run it. chrysalis owns the runner; a host invokes it with its
//! OWN packages (process factories, value-methods, importable modules) — so
//! there is one run path, parameterized, not one re-implemented per host.
//!
//! Layering: this depends only on chrysalis + prism (the engine). The natives a
//! program imports are supplied by the caller, keeping chrysalis independent of
//! any downstream package (e.g. spatio-flux).

use std::sync::Arc;

use prism_bigraph::{Engine, ProcessRegistry};
use prism_schema::{MethodRegistry, Value};

use crate::ast::Program;
use crate::compile::{compile_with_modules, CompileError, ModuleRegistry};

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
        Arc::clone(&result.registry),
    )
    .map_err(RunError::Engine)?;
    engine.discover_all_processes();
    engine.run(time);
    Ok(engine.state().clone())
}
