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

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use prism_bigraph::composite::Composite;
use prism_bigraph::{ProcessNode, ProcessRegistry, Topology};
use prism_schema::units::Context;
use prism_schema::{MethodRegistry, Schema, Value};

use crate::ast::{
    CompositeDef, ContextUse, Def, Expr, Name, Param, PortDecl, ProcessDef, Program, SchemaExpr,
    StepDef, TermArg,
};
use crate::units::UnitEnv;
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
    // Reject ill-typed connections up front — illegal connections are
    // unrepresentable. Validated on the surface program (full unit info).
    let connection_errors = crate::check::validate_connections(program);
    if !connection_errors.is_empty() {
        return Err(CompileError::Other(format!(
            "{} invalid connection(s): {}",
            connection_errors.len(),
            connection_errors
                .iter()
                .map(|e| format!("{}::{}.{} — {}", e.composite, e.child, e.port, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    // Resolve units/contexts, then erase: lower each process body to bare
    // f64 and surface any context factor (e.g. `volume`) as an input port.
    let unit_env = UnitEnv::from_program(program).ok();
    let program = lower_program(program, unit_env.as_ref());
    let methods = Arc::new(MethodRegistry::new());
    let program_arc = Arc::new(program.clone());
    let evaluator = Arc::new(Evaluator::new(Arc::clone(&program_arc), Arc::clone(&methods)));

    let registry_handle: Arc<OnceLock<Arc<ProcessRegistry>>> = Arc::new(OnceLock::new());

    let mut registry = ProcessRegistry::new();

    // Register a factory per user-defined composite / process / step.
    for def in &program.defs {
        match def {
            // Composites need no per-name factory: they compile to plain
            // specs `{address: "local:Composite", config: {state, bridge}}`
            // and instantiate through the generic `Composite` factory
            // (registered below) via `Composite::from_config`.
            Def::Composite(_) => {}
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

    // Generic composite: a composite compiles to a plain spec
    // `{address: "local:Composite", config: {state, bridge}}` and is
    // instantiated through the engine's ported `Composite::from_config`
    // (the upstream model), not chrysalis's old `{_type, _process}` wrapper.
    {
        let handle = Arc::clone(&registry_handle);
        registry.register("Composite", move |config| {
            let registry = handle
                .get()
                .cloned()
                .expect("registry handle not initialized");
            let composite = Composite::from_config(&config, registry)
                .unwrap_or_else(|| panic!("Composite::from_config failed: {config:?}"));
            ProcessNode::Process(Box::new(composite))
        });
    }

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
    let env: IndexMap<Name, Value> = collect_top_level_bindings(&program, &evaluator)?;

    // Evaluate `main`. If it's a composite call, `eval_top_level` inlines
    // it: the root state becomes the composite's body, so its child
    // processes/composites are discoverable and its contents inspectable.
    // Nested composites compile to real subengine specs
    // `{address: "local:Composite", config: {state, bridge}}` instantiated
    // via `Composite::from_config` — see crate::eval::build_composite_outer.
    let initial_state = evaluator.eval_top_level(&main_expr, &env)?;

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

// NOTE: the old per-name `register_composite_factory` (which built a
// `Composite::new` sub-engine and the bespoke `{_type, _process}` outer
// map) was removed. Composites now compile to plain specs
// `{address: "local:Composite", config: {state, bridge}}` and instantiate
// through the single generic `Composite` factory (via `from_config`).

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

// ===============================================================
// Units: erase process bodies + surface context factors as inputs
// ===============================================================

/// Lower every process body: erase units to bare-`f64` arithmetic and
/// surface context factors (e.g. `volume`) as input ports. Unit/context
/// declarations, composites, and non-unit programs are untouched.
fn lower_program(program: &Program, env: Option<&UnitEnv>) -> Program {
    let mut out = program.clone();
    // 1. Erase process bodies, surfacing context factors as input ports.
    for def in &mut out.defs {
        if let Def::Process(p) = def {
            *p = lower_process_def(p, env);
        }
    }
    // 2. Map each process to its (post-erasure) input port names.
    let proc_inputs: HashMap<String, HashSet<String>> = out
        .defs
        .iter()
        .filter_map(|d| match d {
            Def::Process(p) => Some((p.name.clone(), p.interface.inputs.keys().cloned().collect())),
            _ => None,
        })
        .collect();
    // 2b. Thread context factors across composite boundaries. A factor
    //     (e.g. `volume`) may be activated by an ANCESTOR composite but
    //     consumed by a process nested inside a CHILD composite. Each
    //     intermediate composite that doesn't itself provide the factor
    //     must accept it as an input so the ancestor can route it inward;
    //     the auto-built input bridge + same-name default wiring then carry
    //     it down every link of the chain.
    let factor_names: HashSet<String> = out
        .defs
        .iter()
        .filter_map(|d| match d {
            Def::Context(c) => Some(c.params.iter().map(|p| p.name.clone())),
            _ => None,
        })
        .flatten()
        .collect();
    if !factor_names.is_empty() {
        thread_factor_inputs(&mut out, &factor_names, &proc_inputs);
    }
    // 3. For composites with `using ctx(factor: path)`, wire each factor
    //    into the child processes that declare it — honoring the path even
    //    when the factor name differs from the compartment slot name.
    for def in &mut out.defs {
        if let Def::Composite(c) = def {
            if !c.using.is_empty() {
                let using = c.using.clone();
                inject_using_factors(&mut c.body, &using, &proc_inputs);
            }
        }
    }
    out
}

/// Inject `using`-bound context factors as input wirings on the child
/// process terms that declare them. An explicit call-site binding wins
/// (explicit beats implicit).
fn inject_using_factors(
    e: &mut Expr,
    using: &[ContextUse],
    proc_inputs: &HashMap<String, HashSet<String>>,
) {
    match e {
        Expr::Term {
            control, ports, body, ..
        } => {
            if let Some(inputs) = proc_inputs.get(control) {
                for cu in using {
                    for arg in &cu.args {
                        if let TermArg::Named { name, value } = arg {
                            if inputs.contains(name) && !ports.inputs.contains_key(name) {
                                ports.inputs.insert(name.clone(), value.clone());
                            }
                        }
                    }
                }
            }
            if let Some(b) = body {
                inject_using_factors(b, using, proc_inputs);
            }
        }
        Expr::Parallel(items) => {
            for i in items {
                inject_using_factors(i, using, proc_inputs);
            }
        }
        Expr::KeyedEntry { value, .. } => inject_using_factors(value, using, proc_inputs),
        Expr::Block(b) => {
            for (_, v) in &mut b.bindings {
                inject_using_factors(v, using, proc_inputs);
            }
            inject_using_factors(&mut b.value, using, proc_inputs);
        }
        _ => {}
    }
}

/// Thread context factors through nested composites. A composite that
/// (transitively) contains a process declaring factor `F` as an input —
/// but does NOT itself provide `F` via a `using` clause — gains `F` as an
/// input port, so its parent can route the factor across the sub-engine
/// boundary. The auto-built input bridge (`F → [F]`) plus same-name default
/// wiring then carry `F` down every link from the activating ancestor to
/// the consuming process.
fn thread_factor_inputs(
    out: &mut Program,
    factor_names: &HashSet<String>,
    proc_inputs: &HashMap<String, HashSet<String>>,
) {
    // Factors each composite already provides via `using` (so it serves its
    // descendants from its own slot rather than threading further up).
    let provides: HashMap<String, HashSet<String>> = out
        .defs
        .iter()
        .filter_map(|d| match d {
            Def::Composite(c) => Some((
                c.name.clone(),
                c.using
                    .iter()
                    .flat_map(|cu| cu.args.iter())
                    .filter_map(|a| match a {
                        TermArg::Named { name, .. } => Some(name.clone()),
                        _ => None,
                    })
                    .collect::<HashSet<String>>(),
            )),
            _ => None,
        })
        .collect();

    // Composite bodies, cloned for analysis.
    let bodies: HashMap<String, Expr> = out
        .defs
        .iter()
        .filter_map(|d| match d {
            Def::Composite(c) => Some((c.name.clone(), c.body.clone())),
            _ => None,
        })
        .collect();

    // Fixpoint over `needs[C]` = the factors C must accept as inputs.
    let mut needs: HashMap<String, HashSet<String>> =
        bodies.keys().map(|n| (n.clone(), HashSet::new())).collect();
    loop {
        let mut changed = false;
        let snapshot = needs.clone();
        for (cname, body) in &bodies {
            let mut child: HashSet<String> = HashSet::new();
            collect_child_factor_needs(body, proc_inputs, factor_names, &snapshot, &mut child);
            let prov = &provides[cname];
            let entry = needs.get_mut(cname).unwrap();
            for f in child {
                if !prov.contains(&f) && entry.insert(f) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Surface the threaded factors as input ports.
    for def in &mut out.defs {
        if let Def::Composite(c) = def {
            if let Some(fs) = needs.get(&c.name) {
                for f in fs {
                    c.interface
                        .inputs
                        .entry(f.clone())
                        .or_insert_with(|| PortDecl::required(SchemaExpr::Float));
                }
            }
        }
    }
}

/// Collect the factors that the DIRECT child terms of a composite body
/// require: a child process contributes the factors among its inputs; a
/// child composite contributes its already-computed `needs`.
fn collect_child_factor_needs(
    e: &Expr,
    proc_inputs: &HashMap<String, HashSet<String>>,
    factor_names: &HashSet<String>,
    needs: &HashMap<String, HashSet<String>>,
    out: &mut HashSet<String>,
) {
    match e {
        Expr::Term { control, body, .. } => {
            if let Some(inputs) = proc_inputs.get(control) {
                for f in inputs.intersection(factor_names) {
                    out.insert(f.clone());
                }
            }
            if let Some(n) = needs.get(control) {
                for f in n {
                    out.insert(f.clone());
                }
            }
            if let Some(b) = body {
                collect_child_factor_needs(b, proc_inputs, factor_names, needs, out);
            }
        }
        Expr::Parallel(items) | Expr::List(items) => {
            for i in items {
                collect_child_factor_needs(i, proc_inputs, factor_names, needs, out);
            }
        }
        Expr::KeyedEntry { value, .. } => {
            collect_child_factor_needs(value, proc_inputs, factor_names, needs, out)
        }
        Expr::Map(entries) => {
            for (_, v) in entries {
                collect_child_factor_needs(v, proc_inputs, factor_names, needs, out);
            }
        }
        Expr::Block(b) => {
            for (_, v) in &b.bindings {
                collect_child_factor_needs(v, proc_inputs, factor_names, needs, out);
            }
            collect_child_factor_needs(&b.value, proc_inputs, factor_names, needs, out);
        }
        _ => {}
    }
}

/// Erase units in one process body. On any analysis failure (no unit
/// env, un-typed body, unhandled form) the def is returned unchanged, so
/// non-unit programs are unaffected.
fn lower_process_def(def: &ProcessDef, env: Option<&UnitEnv>) -> ProcessDef {
    let Some(env) = env else {
        return def.clone();
    };
    let Ok(vars) = env.vars_for(&def.params, &def.interface) else {
        return def.clone();
    };
    let ctxs: Vec<&Context> = env.contexts.values().collect();
    let Ok(lowered) = env.lower_body(&def.body, &vars, &ctxs) else {
        return def.clone();
    };

    let mut before = HashSet::new();
    collect_free_vars(&def.body, &mut before);
    let mut after = HashSet::new();
    collect_free_vars(&lowered, &mut after);

    let mut out = def.clone();
    out.body = lowered;
    // Context factors introduced by erasure (e.g. `volume`) become input
    // ports. Default same-name wiring (build_spec_value) connects each to
    // the enclosing compartment's slot — the slot a
    // `using ctx(factor: @.factor)` clause names.
    for factor in after.difference(&before) {
        out.interface
            .inputs
            .entry(factor.clone())
            .or_insert_with(|| PortDecl::required(SchemaExpr::Float));
    }
    out
}

/// Collect every `Var` name referenced in an expression. Used to find
/// the factor vars erasure introduced (`after − before`).
fn collect_free_vars(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Var(n) => {
            out.insert(n.clone());
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_free_vars(lhs, out);
            collect_free_vars(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_free_vars(operand, out),
        Expr::Block(b) => {
            for (_, v) in &b.bindings {
                collect_free_vars(v, out);
            }
            collect_free_vars(&b.value, out);
        }
        Expr::Record(fields) => {
            for (_, v) in fields {
                collect_free_vars(v, out);
            }
        }
        Expr::KeyedEntry { value, .. } => collect_free_vars(value, out),
        Expr::Method { receiver, args, .. } => {
            collect_free_vars(receiver, out);
            for a in args {
                collect_free_vars(a, out);
            }
        }
        Expr::If { cond, then_, else_ } => {
            collect_free_vars(cond, out);
            collect_free_vars(then_, out);
            if let Some(e) = else_ {
                collect_free_vars(e, out);
            }
        }
        Expr::List(items) | Expr::Parallel(items) => {
            for i in items {
                collect_free_vars(i, out);
            }
        }
        _ => {}
    }
}
