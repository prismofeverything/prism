//! Lower a chrysalis [`Program`] into prism runtime artifacts.
//!
//! Output:
//!
//! ```text
//! CompileResult {
//!     topology:      prism_bigraph::Topology,
//!     registry:      Arc<prism_bigraph::ProcessRegistry>,
//!     initial_state: prism_schema::Value,
//!     evaluator:     Arc<Evaluator>,
//! }
//! ```
//!
//! Caller wires the result into an [`Engine`]:
//!
//! ```ignore
//! let result = chrysalis::compile::compile(&program)?;
//! let engine = prism_bigraph::Engine::from_state(
//!     result.topology.state_schema.clone(),
//!     result.initial_state.clone(),
//!     Arc::clone(&result.registry),
//! )?;
//! ```

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use prism_bigraph::composite::{Bridge, Composite};
use prism_bigraph::{Engine, ProcessNode, ProcessRegistry, Topology};
use prism_schema::{MethodRegistry, Schema, Value};

use crate::ast::{CompositeDef, Def, Name, Param, ProcessDef, Program, StepDef};
use crate::eval::{brs_config_from_value, EvalError, Evaluator};
use crate::runtime::brs::ChrysalisBrs;
use crate::runtime::expr_process::ExprProcess;
use crate::runtime::expr_step::ExprStep;

/// Output of compiling a chrysalis [`Program`].
pub struct CompileResult {
    pub topology: Topology,
    pub registry: Arc<ProcessRegistry>,
    pub initial_state: Value,
    pub evaluator: Arc<Evaluator>,
    pub methods: Arc<MethodRegistry>,
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("eval error: {0}")]
    Eval(#[from] EvalError),

    #[error("compile error: {0}")]
    Other(String),
}

/// Compile a chrysalis program into prism runtime artifacts.
///
/// Entry conventions: the program must contain a top-level
/// `main = <expr>` binding. `<expr>` evaluates to a [`Value::Map`]
/// (typically a single process spec) that becomes the engine's
/// initial state.
pub fn compile(program: &Program) -> Result<CompileResult, CompileError> {
    let methods = Arc::new(MethodRegistry::new());
    let program_arc = Arc::new(program.clone());
    let evaluator = Arc::new(Evaluator::new(Arc::clone(&program_arc), Arc::clone(&methods)));

    let registry_handle: Arc<OnceLock<Arc<ProcessRegistry>>> = Arc::new(OnceLock::new());

    let mut registry = ProcessRegistry::new();

    // Register a factory per user-defined composite / process / step.
    for def in &program.defs {
        match def {
            Def::Composite(composite_def) => {
                register_composite_factory(
                    &mut registry,
                    composite_def,
                    Arc::clone(&evaluator),
                    Arc::clone(&registry_handle),
                );
            }
            Def::Process(process_def) => {
                register_process_factory(
                    &mut registry,
                    process_def,
                    Arc::clone(&evaluator),
                );
            }
            Def::Step(step_def) => {
                register_step_factory(&mut registry, step_def, Arc::clone(&evaluator));
            }
            // Reactions and Patterns are values constructed at call sites,
            // not separate process types. Bindings (including `main`) are
            // top-level values evaluated at compile time.
            Def::Reaction(_)
            | Def::Pattern(_)
            | Def::Unit(_)
            | Def::Context(_)
            | Def::Binding { .. } => {}
        }
    }

    // The chrysalis BRS is a built-in.
    register_chrysalis_brs_factory(&mut registry, Arc::clone(&evaluator));

    let registry = Arc::new(registry);
    let _ = registry_handle.set(Arc::clone(&registry));

    // Evaluate `main`. If main is a call to a user-defined composite,
    // *inline* it: the top-level engine becomes that composite, so its
    // inner state slots (cells, sub-processes) live at engine root
    // and the outer state is directly inspectable. Otherwise treat
    // main's value as the initial state directly.
    let main_expr = match program.lookup("main") {
        Some(Def::Binding { value, .. }) => value.clone(),
        _ => {
            return Err(CompileError::Other(
                "program must contain a top-level `main = ...` binding".into(),
            ));
        }
    };
    let env: IndexMap<Name, Value> = collect_top_level_bindings(program, &evaluator)?;

    // Evaluate `main` to its outer-map form. For a composite call
    // site, this produces `{_type, observable slots, _process: spec}`
    // — same shape regardless of protocol. The engine discovers the
    // `_process` spec on the first tick and instantiates the
    // composite as a wrapped sub-engine. Crucially: data slots are
    // siblings of `_process` at the root, so the composite's bridge
    // can project to / read from them through normal wire resolution.
    let initial_state = evaluator.eval_value(&main_expr, &env)?;

    let topology = Topology {
        state_schema: Schema::Any,
        initial_state: initial_state.clone(),
        processes: IndexMap::new(),
    };

    Ok(CompileResult {
        topology,
        registry,
        initial_state,
        evaluator,
        methods,
    })
}

/// Evaluate top-level `name = expr` bindings into a starter env so
/// that compile-time references resolve.
fn collect_top_level_bindings(
    program: &Program,
    evaluator: &Evaluator,
) -> Result<IndexMap<Name, Value>, CompileError> {
    let mut env: IndexMap<Name, Value> = IndexMap::new();
    for def in &program.defs {
        if let Def::Binding { name, value } = def {
            // `main` is handled separately at the top of compile().
            if name == "main" {
                continue;
            }
            let v = evaluator.eval_value(value, &env)?;
            env.insert(name.clone(), v);
        }
    }
    Ok(env)
}

// ===============================================================
// Factory registration helpers
// ===============================================================

fn register_composite_factory(
    registry: &mut ProcessRegistry,
    def: &CompositeDef,
    evaluator: Arc<Evaluator>,
    registry_handle: Arc<OnceLock<Arc<ProcessRegistry>>>,
) {
    let def = def.clone();
    let label = def.name.clone();
    registry.register(label.clone(), move |config| {
        let resolved = resolve_params(&def.params, &config, &evaluator)
            .expect("composite param resolution failed");

        // Eval composite body to produce the inner state Value.
        let inner_state = match evaluator.eval_value(&def.body, &resolved) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("composite `{}` body eval failed: {e}", def.name);
                Value::map()
            }
        };

        // Build the bridge from the interface's wiring.
        let mut input_bridge: IndexMap<String, Vec<prism_schema::Key>> = IndexMap::new();
        for port in def.interface.inputs.keys() {
            input_bridge.insert(port.clone(), vec![prism_schema::Key::from(port.as_str())]);
        }
        let mut output_bridge: IndexMap<String, Vec<prism_schema::Key>> = IndexMap::new();
        for port in def.interface.outputs.keys() {
            output_bridge.insert(port.clone(), vec![prism_schema::Key::from(port.as_str())]);
        }

        let input_schemas: IndexMap<String, Schema> = def
            .interface
            .inputs
            .iter()
            .map(|(n, d)| {
                (
                    n.clone(),
                    crate::runtime::expr_process::lower_schema(&d.schema),
                )
            })
            .collect();
        let output_schemas: IndexMap<String, Schema> = def
            .interface
            .outputs
            .iter()
            .map(|(n, d)| {
                (
                    n.clone(),
                    crate::runtime::expr_process::lower_schema(&d.schema),
                )
            })
            .collect();

        let topology = Topology {
            state_schema: Schema::Any,
            initial_state: inner_state,
            processes: IndexMap::new(),
        };
        let mut engine = Engine::new(topology, HashMap::new());
        let registry = registry_handle
            .get()
            .cloned()
            .expect("registry handle not initialized");
        engine.set_registry(registry);
        engine.discover_all_processes();

        let composite = Composite::new(
            engine,
            Bridge {
                mappings: input_bridge,
            },
            Bridge {
                mappings: output_bridge,
            },
            input_schemas,
            output_schemas,
            1.0,
        );

        ProcessNode::Process(Box::new(composite))
    });
}

fn register_process_factory(
    registry: &mut ProcessRegistry,
    def: &ProcessDef,
    evaluator: Arc<Evaluator>,
) {
    let def = def.clone();
    let label = def.name.clone();
    registry.register(label.clone(), move |config| {
        let resolved = resolve_params(&def.params, &config, &evaluator)
            .expect("process param resolution failed");
        let interval = config
            .as_map()
            .and_then(|m| m.get("interval"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        ProcessNode::Process(Box::new(ExprProcess::from_def(
            &def,
            resolved,
            interval,
            Arc::clone(&evaluator),
        )))
    });
}

fn register_step_factory(
    registry: &mut ProcessRegistry,
    def: &StepDef,
    evaluator: Arc<Evaluator>,
) {
    let def = def.clone();
    let label = def.name.clone();
    registry.register(label.clone(), move |config| {
        let resolved = resolve_params(&def.params, &config, &evaluator)
            .expect("step param resolution failed");
        let priority = config
            .as_map()
            .and_then(|m| m.get("priority"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        ProcessNode::Step(Box::new(ExprStep::from_def(
            &def,
            resolved,
            priority,
            Arc::clone(&evaluator),
        )))
    });
}

fn register_chrysalis_brs_factory(
    registry: &mut ProcessRegistry,
    evaluator: Arc<Evaluator>,
) {
    registry.register("ChrysalisBrs", move |config| {
        let brs_config = brs_config_from_value(&config)
            .expect("ChrysalisBrs config decode failed");
        ProcessNode::Process(Box::new(ChrysalisBrs::new(
            brs_config,
            Arc::clone(&evaluator),
        )))
    });
}

/// Resolve a definer's `params` from the user-supplied `config`,
/// applying defaults where missing.
fn resolve_params(
    params: &[Param],
    config: &Value,
    evaluator: &Evaluator,
) -> Result<IndexMap<Name, Value>, EvalError> {
    let cfg_map = config.as_map();
    let mut out: IndexMap<Name, Value> = IndexMap::new();
    let empty_env: IndexMap<Name, Value> = IndexMap::new();
    for param in params {
        let supplied = cfg_map.and_then(|m| m.get(param.name.as_str()));
        let v = match (supplied, &param.default) {
            (Some(v), _) => v.clone(),
            (None, Some(default)) => evaluator.eval_value(default, &empty_env)?,
            (None, None) => Value::None,
        };
        out.insert(param.name.clone(), v);
    }
    Ok(out)
}
