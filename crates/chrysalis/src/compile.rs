//! Lower a chrysalis [`Program`] into prism runtime artifacts.
//!
//! Output:
//!
//! ```text
//! CompileResult {
//!     topology:      prism_bigraph::Topology,
//!     initial_state: prism_schema::Value,
//!     evaluator:     Arc<Evaluator>,
//!     core:          prism_bigraph::Core,   // the ONE shared Core
//! }
//! ```
//!
//! Caller wires the result into an [`Engine`] with the WHOLE `core` (never a
//! registry subset — see the threading rule on `prism_bigraph::core`):
//!
//! ```ignore
//! let result = chrysalis::compile::compile(&program)?;
//! let engine = prism_bigraph::Engine::from_state(
//!     result.topology.state_schema.clone(),
//!     result.initial_state.clone(),
//!     result.core.clone(),
//! )?;
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use prism_bigraph::composite::Composite;
use prism_bigraph::{BigraphicalReactiveSystem, Core, ProcessNode, ProcessRegistry, Topology};
use prism_schema::algebra;
use prism_schema::registry::TypeMethods;
use prism_schema::units::Context;
use prism_schema::{
    DivideContext, Key, MethodError, MethodRegistry, Schema, TypeRegistry, Value,
    divide_by_schema,
};

use crate::ast::{
    ContextUse, Def, Expr, Name, Param, PortDecl, ProcessDef, Program, SchemaExpr, StepDef, TermArg,
};
use crate::eval::{EvalError, Evaluator};
use crate::runtime::expr_process::ExprProcess;
use crate::runtime::expr_step::ExprStep;
use crate::runtime::rule::{extract_rules, to_prism_rule};
use crate::units::UnitEnv;

/// Output of compiling a chrysalis [`Program`].
///
/// Carries the one shared [`Core`] — never its individual registries. Earlier
/// `registry` / `methods` / `type_registry` fields were SUBSETS of `core`
/// (`core.processes` / `core.methods` / `core.types`); exposing them invited the
/// registry-subset drift the threading rule forbids (a caller passing
/// `result.registry` to `Engine::from_state` got a process-only Core, silently
/// dropping types/methods/protocols). Read what you need off `core`.
pub struct CompileResult {
    pub topology: Topology,
    pub initial_state: Value,
    pub evaluator: Arc<Evaluator>,
    /// The unified runtime [`Core`] (types + processes + methods + protocols).
    /// Pass to [`prism_bigraph::Engine::from_state`] so the engine and every
    /// subengine it builds share it.
    pub core: Core,
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("eval error: {0}")]
    Eval(#[from] EvalError),

    #[error("compile error: {0}")]
    Other(String),
}

/// A native function importable into a `.ys` body as a bare call (`overlay(...)`).
/// (Bare-function imports are wired in a later milestone; the type + builder
/// exist now so the host-facing API is stable.)
pub type HostFn = Arc<dyn Fn(&[Value]) -> Result<Value, MethodError> + Send + Sync>;

/// What a native module exports under a name.
// `Function`'s payload is consumed in the bare-function milestone (`Expr::Call`);
// for now an imported function resolves but errs, so the field isn't read yet.
#[allow(dead_code)]
enum Export {
    /// A whole process — its factory lives in the `ProcessRegistry`.
    Process,
    /// A value/object bound by name; methods on it dispatch via `MethodRegistry`.
    Object(Value),
    /// A bare callable invoked as `name(args)`.
    Function(HostFn),
    /// A native type whose representation is the schema source string; imported
    /// types become first-class via a synthetic `type` def (`network: CRN`).
    Type(String),
}

/// The native modules a host (e.g. spatio-flux) makes importable from `.ys` via
/// `from <module> import <names>` — the replacement for `extern`. The host owns
/// the implementations (process factories in the [`ProcessRegistry`], value
/// methods in the [`MethodRegistry`]); this declares only *which* names each
/// module exports and of what kind, so chrysalis can resolve a `Def::Use`.
#[derive(Clone, Default)]
pub struct ModuleRegistry {
    modules: HashMap<String, ModuleExports>,
}

#[derive(Clone, Default)]
struct ModuleExports {
    processes: HashSet<String>,
    objects: IndexMap<String, Value>,
    functions: IndexMap<String, HostFn>,
    /// name → representation schema source (parsed into a synthetic `type` def).
    types: IndexMap<String, String>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The set of NATIVE host module names this registry knows (`core`,
    /// `integrators`, `diffusion`, …). File-module import resolution consults it
    /// so a native module wins over a same-named sibling `.ys` (std-module-first:
    /// `from diffusion import …` binds the native even when a `diffusion.ys` demo
    /// sits beside the importer). (#50)
    pub fn module_names(&self) -> HashSet<String> {
        self.modules.keys().cloned().collect()
    }

    /// Declare that `module` exports a whole native process named `name`
    /// (its factory must be registered in the `ProcessRegistry`).
    pub fn process(mut self, module: &str, name: &str) -> Self {
        self.modules
            .entry(module.to_string())
            .or_default()
            .processes
            .insert(name.to_string());
        self
    }

    /// Declare that `module` exports an object `name` bound to `value`; its
    /// methods (`name.method(args)`) dispatch via the `MethodRegistry`.
    pub fn object(mut self, module: &str, name: &str, value: Value) -> Self {
        self.modules
            .entry(module.to_string())
            .or_default()
            .objects
            .insert(name.to_string(), value);
        self
    }

    /// Declare that `module` exports a bare function `name` (`name(args)`).
    pub fn function(mut self, module: &str, name: &str, f: HostFn) -> Self {
        self.modules
            .entry(module.to_string())
            .or_default()
            .functions
            .insert(name.to_string(), f);
        self
    }

    /// Declare that `module` exports a native type `name` with the given
    /// representation schema (surface source, e.g. `"{species: list[string], …}"`).
    /// The import becomes a first-class `type` (`network: CRN` resolves, methods
    /// like `from_sbml` attach to it).
    pub fn type_(mut self, module: &str, name: &str, representation: &str) -> Self {
        self.modules
            .entry(module.to_string())
            .or_default()
            .types
            .insert(name.to_string(), representation.to_string());
        self
    }

    fn resolve(&self, module: &str, name: &str) -> Option<Export> {
        let m = self.modules.get(module)?;
        if m.processes.contains(name) {
            Some(Export::Process)
        } else if let Some(v) = m.objects.get(name) {
            Some(Export::Object(v.clone()))
        } else if let Some(src) = m.types.get(name) {
            Some(Export::Type(src.clone()))
        } else {
            m.functions
                .get(name)
                .map(|f| Export::Function(Arc::clone(f)))
        }
    }
}

/// Resolve every `from <module> import …` (`Def::Use`) against the host
/// `modules`: object exports bind by name (resolvable in any body), process
/// exports are recognised as wholesale native controls, and type exports become
/// synthetic `type` defs (so the import is a first-class type).
/// Resolution of every `from <module> import …` declaration in a program:
/// each imported name lands in exactly one of these buckets. Threaded into
/// the [`Evaluator`](crate::eval::Evaluator) so bodies can reference the
/// imports as plain identifiers.
pub(crate) struct ResolvedImports {
    /// Object exports — bound by name in the eval env (`rk4`, `Path`, …).
    pub imports: IndexMap<Name, Value>,
    /// Wholesale native processes — recognised in term position (`RunProcess[…]`).
    pub processes: HashSet<Name>,
    /// Bare native functions — callable as `name(args)` (`load("file.ys")`).
    pub functions: IndexMap<Name, HostFn>,
    /// Native types lowered to synthetic `Def::Type` entries so they integrate
    /// with the regular type-resolution path.
    pub type_defs: Vec<Def>,
}

fn resolve_imports(
    program: &Program,
    modules: &ModuleRegistry,
) -> Result<ResolvedImports, CompileError> {
    let mut imports: IndexMap<Name, Value> = IndexMap::new();
    let mut processes: HashSet<Name> = HashSet::new();
    let mut functions: IndexMap<Name, HostFn> = IndexMap::new();
    let mut type_defs: Vec<Def> = Vec::new();
    for def in &program.defs {
        if let Def::Use { module, names } = def {
            for name in names {
                match modules.resolve(module, name) {
                    Some(Export::Process) => {
                        processes.insert(name.clone());
                    }
                    Some(Export::Object(v)) => {
                        imports.insert(name.clone(), v);
                    }
                    Some(Export::Type(repr_src)) => {
                        let representation =
                            crate::parse::parse_schema_expr(&repr_src).map_err(|e| {
                                CompileError::Other(format!(
                                    "native type `{module}::{name}` has an invalid representation: {e}"
                                ))
                            })?;
                        type_defs.push(Def::Type(crate::ast::TypeDef {
                            name: name.clone(),
                            params: vec![],
                            representation,
                            methods: vec![],
                        }));
                    }
                    Some(Export::Function(f)) => {
                        functions.insert(name.clone(), f);
                    }
                    None => {
                        return Err(CompileError::Other(format!(
                            "unknown import `{name}` from native module `{module}`"
                        )));
                    }
                }
            }
        }
    }
    Ok(ResolvedImports {
        imports,
        processes,
        functions,
        type_defs,
    })
}

/// Compile a chrysalis program into prism runtime artifacts.
///
/// Entry conventions: the program must contain a top-level
/// `main = <expr>` binding. `<expr>` evaluates to a [`Value::Map`]
/// (typically a single process spec) that becomes the engine's
/// initial state.
pub fn compile(program: &Program) -> Result<CompileResult, CompileError> {
    compile_with_registry(program, ProcessRegistry::new())
}

/// Like [`compile`], but starts from a caller-provided `registry` — e.g. one
/// pre-populated with NATIVE process factories that the program's native
/// imports (`from <module> import Name`) reference (spatio-flux's numerical
/// processes: diffusion, FBA, kinetics, particles). chrysalis registers its
/// own factories on top, so a native `Name` resolves `local:Name` to the
/// factory the caller supplied under `Name`.
pub fn compile_with_registry(
    program: &Program,
    registry: ProcessRegistry,
) -> Result<CompileResult, CompileError> {
    compile_with_methods(program, registry, MethodRegistry::new())
}

/// Like [`compile_with_registry`], but also takes a pre-populated
/// [`MethodRegistry`] of NATIVE value-methods — e.g. spatio-flux's `TimeSeries`
/// `species_mse` / `overlay` — that the program's ys-native bodies dispatch to
/// (`a.species_mse(b)`). The divide + user-`type` methods are registered on top.
/// This is where the two process kinds meet: native processes via `registry`,
/// native methods via `methods`, ys-native logic via the compiled bodies.
pub fn compile_with_methods(
    program: &Program,
    registry: ProcessRegistry,
    methods: MethodRegistry,
) -> Result<CompileResult, CompileError> {
    compile_with_modules(program, registry, methods, ModuleRegistry::new())
}

/// Like [`compile_with_methods`], but also takes a [`ModuleRegistry`] declaring
/// the native modules a `.ys` may `from <module> import …` — the replacement for
/// `extern`. A *process* import (`from core import RunProcess`) becomes a
/// wholesale native control wired straight from its call site; an *object*
/// import (`from integrators import rk4`) binds a value resolvable in bodies, so
/// `rk4.integrate(network, state, interval)` dispatches via the `MethodRegistry`.
pub fn compile_with_modules(
    program: &Program,
    mut registry: ProcessRegistry,
    mut methods: MethodRegistry,
    modules: ModuleRegistry,
) -> Result<CompileResult, CompileError> {
    // Resolve native host imports (`Def::Use`): object/process bindings + bare
    // native functions + synthetic `type` defs for imported native types.
    let ResolvedImports {
        imports,
        processes: imported_processes,
        functions: imported_functions,
        type_defs,
    } = resolve_imports(program, &modules)?;
    // Inject the imported types so they're first-class (resolve in annotations,
    // register in the TypeRegistry, carry methods) — indistinguishable from a
    // `type` declared in the `.ys`.
    let program_owned;
    let program: &Program = if type_defs.is_empty() {
        program
    } else {
        let mut p = program.clone();
        p.defs.extend(type_defs);
        program_owned = p;
        &program_owned
    };

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
    let program_arc = Arc::new(program.clone());
    // ONE program-aware type registry (builtins + user `type`s + std `Qubits`),
    // threaded into the divide methods (a cell divides through the REAL registry,
    // so a `Custom`-typed face dispatches), the Evaluator (realize-at-binding),
    // AND the Core below — a single registry per compile context, never an
    // ad-hoc default.
    let type_registry = build_type_registry(&program_arc);
    register_divide_methods(&mut methods, &program);
    register_user_type_methods(&mut methods, &program_arc);
    let methods = Arc::new(methods);

    // The ONE late-bound Core handle. The Core can't exist yet — its process
    // registry holds factories that capture the evaluator (the factory↔evaluator
    // cycle), and the Composite factory needs the WHOLE Core to build subengines.
    // So construct the evaluator + factories against this empty handle, build the
    // Core, then `set` it once (line below). The Core-threading rule: the engine,
    // every node (`set_core`), every subengine (`from_config`), AND the evaluator
    // all share THIS handle's Core — no component holds a registry subset.
    let core_handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());

    let evaluator = Arc::new(Evaluator::with_native_imports(
        Arc::clone(&program_arc),
        imports,
        imported_processes,
        imported_functions,
        Arc::clone(&core_handle),
    ));

    // `registry` arrives with any native factories the caller supplied;
    // chrysalis registers its own on top.

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
            // Types register in the TypeRegistry + MethodRegistry, not as
            // process factories — see `register_user_types` below.
            Def::Type(_) => {}
            // Functions are resolved by the evaluator from the program defs
            // when called (`Expr::Call`); they register no process factory.
            Def::Function(_) => {}
            Def::Reaction(_)
            | Def::Pattern(_)
            | Def::Unit(_)
            | Def::Context(_)
            // Contracts are type-level metadata (consumed by `check` +
            // schema lowering); they register no process factory.
            | Def::Contract(_)
            // A `protocol` alias registers no factory: its uses build the WRAPPED
            // composite via the generic `Composite` factory, with the address
            // overridden to the typed protocol address (see eval::build_protocol_outer).
            | Def::Protocol(_)
            // `Use`: file-module imports are resolved away by `parse::parse_file`;
            // native host imports are resolved separately (binds imported names).
            // Either way it registers no factory here.
            | Def::Use { .. }
            | Def::Binding { .. } => {}
        }
    }

    // The BRS is prism's `BigraphicalReactiveSystem`, registered under
    // `Brs`; chrysalis only adapts its reaction values into it.
    register_brs_factory(&mut registry, Arc::clone(&evaluator));

    // Generic composite: a composite compiles to a plain spec
    // `{address: "local:Composite", config: {state, bridge}}` and is
    // instantiated through the engine's ported `Composite::from_config`
    // (the upstream model), not chrysalis's old `{_type, _process}` wrapper.
    {
        let handle = Arc::clone(&core_handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle not initialized");
            let composite = Composite::from_config(&config, core)
                .unwrap_or_else(|| panic!("Composite::from_config failed: {config:?}"));
            ProcessNode::Process(Box::new(composite))
        });
    }

    let registry = Arc::new(registry);
    // The unified Core threaded into the engine + every subengine.
    let core = Core::new()
        .with_types(Arc::clone(&type_registry))
        .with_processes(Arc::clone(&registry))
        .with_methods(Arc::clone(&methods))
        .with_protocols(Arc::new(crate::stream::stream_protocols()));
    let _ = core_handle.set(core.clone());

    // Evaluate `main`. If main is a call to a user-defined composite,
    // *inline* it: the top-level engine becomes that composite, so its
    // inner state slots (cells, sub-processes) live at engine root
    // and the outer state is directly inspectable. Otherwise treat
    // main's value as the initial state directly.
    // A file IS a composite. A top-level `main = …` binding is its body (the
    // root composite to inline); with no `main`, the body is empty — the
    // file's `type`/`process`/`composite` defs are still registered and
    // exported (importable, callable, dispatchable). No mandatory entry point.
    let main_expr: Option<Expr> = match program.lookup("main") {
        Some(Def::Binding { value, .. }) => Some(value.clone()),
        // No explicit `main` ⇒ the file's value is its last top-level term
        // (decision #24). A *bare-runnable* `composite` entry (all config
        // params + inputs defaulted) runs by inlining a bare call to it (its
        // body becomes the root state). An entry that needs config/inputs is
        // run via the `invoke` path (CLI-bound), not as a bare root — so it is
        // left out here (empty root) rather than failing on missing args.
        _ => match program.entry() {
            Some(Def::Composite(d)) if bare_runnable(d) => Some(Expr::term(d.name.clone()).build()),
            _ => None,
        },
    };
    let env: IndexMap<Name, Value> = collect_top_level_bindings(&program, &evaluator)?;

    // Evaluate the body. If `main` is a composite call, `eval_top_level`
    // inlines it: the root state becomes the composite's body, so its child
    // processes/composites are discoverable and its contents inspectable.
    // Nested composites compile to real subengine specs
    // `{address: "local:Composite", config: {state, bridge}}` instantiated
    // via `Composite::from_config` — see crate::eval::build_composite_outer.
    let initial_state = match &main_expr {
        Some(expr) => evaluator.eval_top_level(expr, &env)?,
        None => Value::map(),
    };

    // Inference (from the value) recovers structure (`Tree`/`List`/`Float`)
    // but loses APPLY-critical semantics: a field is a `List` (replace) when
    // it should be an `Array` (element-wise additive); an extensive scalar a
    // `Float` when it should be a `Delta`. So resolve the AST's declared
    // composite schema onto the inferred one (the declaration refines).
    let inferred = Schema::infer(&initial_state);
    let state_schema = match &main_expr {
        Some(Expr::Term { control, .. }) => match program.lookup(control) {
            Some(Def::Composite(def)) => {
                // Resolve the inferred structure with the AST-declared schema:
                // the declaration (the refining side) contributes the apply-
                // critical types inference can't recover — `Array` (additive)
                // over an inferred `List` (replace), `Delta` over `Float` — and
                // pushes a uniform `Map`/`List` element type onto inferred
                // per-key `Tree` branches. (Replaces the hand-rolled
                // `overlay_apply_types` with the algebra's `resolve`.)
                let derived = crate::schema::composite_inner_schema(def, &program);
                algebra::resolve(&inferred, &derived)
            }
            _ => inferred,
        },
        _ => inferred,
    };

    let topology = Topology {
        state_schema,
        initial_state: initial_state.clone(),
        processes: IndexMap::new(),
    };

    Ok(CompileResult {
        topology,
        initial_state,
        evaluator,
        core,
    })
}

/// Evaluate top-level `name = expr` bindings into a starter env so
/// that compile-time references resolve. (Used by both `compile` and the
/// `invoke` path, which seeds it before binding CLI config/inputs.)
pub(crate) fn collect_top_level_bindings(
    program: &Program,
    evaluator: &Evaluator,
) -> Result<IndexMap<Name, Value>, CompileError> {
    let mut env: IndexMap<Name, Value> = IndexMap::new();
    for def in &program.defs {
        if let Def::Binding { name, schema, value } = def {
            // `main` is handled separately at the top of compile().
            if name == "main" {
                continue;
            }
            let v = evaluator.eval_value(value, &env)?;
            // `def x :: T = …` realizes the value at its DECLARED type
            // (registry-threaded) — `def plus :: Qubits = {…}` becomes a full
            // tagged instance, the same single typed-construction path as
            // params. Untyped `def`s pass through unchanged.
            let v = match schema {
                Some(s) => {
                    let sch = crate::schema::lower_schema_in_program(s, &evaluator.program);
                    prism_schema::algebra::realize_with(Some(evaluator.types()), &sch, &v)
                }
                None => v,
            };
            env.insert(name.clone(), v);
        }
    }
    Ok(env)
}

/// A composite is *bare-runnable* (inlinable as the root with no CLI args) when
/// every config param and input port has a default. Otherwise it must be run via
/// the `invoke` path, which binds config/inputs from the command line.
fn bare_runnable(d: &crate::ast::CompositeDef) -> bool {
    d.params.iter().all(|p| p.default.is_some())
        && d.interface.inputs.values().all(|p| p.default.is_some())
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

fn register_step_factory(registry: &mut ProcessRegistry, def: &StepDef, evaluator: Arc<Evaluator>) {
    let def = def.clone();
    let label = def.name.clone();
    registry.register(label.clone(), move |config| {
        let resolved =
            resolve_params(&def.params, &config, &evaluator).expect("step param resolution failed");
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

/// Register the BRS factory. A chrysalis `BRS[rules: […]]` compiles to a
/// spec `{address: "local:Brs", config: {rules, mode, …}}`; this factory
/// decodes the chrysalis [`Rule`](crate::runtime::rule::Rule) carriers,
/// adapts each to a `prism_schema::ReactionRule` (via `to_prism_rule`,
/// which closes the reactum/guard/rate expressions over the evaluator),
/// and runs them on prism's `BigraphicalReactiveSystem`. There is no
/// chrysalis-side BRS — prism owns matching, firing, and diffing.
fn register_brs_factory(registry: &mut ProcessRegistry, evaluator: Arc<Evaluator>) {
    registry.register("Brs", move |config| {
        let rules = extract_rules(&config)
            .iter()
            .map(|r| to_prism_rule(r, Arc::clone(&evaluator)))
            .collect();
        ProcessNode::Process(Box::new(BigraphicalReactiveSystem::from_config(
            rules, &config,
        )))
    });
}

/// Register a **type-relative** `divide` value method per composite.
/// Dispatched on the value's type (`?c.divide()` keys on the cell's
/// `_type`), it derives the instance schema (`composite_instance_schema` —
/// the program's real schema, single source) and runs the schema-driven
/// `divide_by_schema`: extensive fields (`mass`) halve, everything else is
/// shared. Each daughter's `id` is reissued. No literal `mass / 2`, no
/// ad-hoc registry — divide is relative to the value's type, as it should be.
/// Register every `type Name = … with { method(args) = body }` method against
/// its type name, so a value tagged `_type: Name` can dispatch them. Each
/// method runs as `eval_method_body` with `self` bound to the receiver. A Map
/// result is re-tagged with the receiver's `_type` so write-methods can chain
/// (`g.add_node("a").add_node("b")`) and stay dispatchable.
///
/// These methods are reachable two ways (the algebraic-effects split):
///   * **query** — `value.method(args)` in any expression / process body;
///   * **write action** — an `apply` directive `{_call: {method, args}}` on a
///     slot of this type, interpreted by the type's `apply` handler
///     ([`ChrysalisType`]), generalizing `_add`/`_remove`/`_divide`.
fn register_user_type_methods(methods: &mut MethodRegistry, program: &Arc<Program>) {
    for def in &program.defs {
        let Def::Type(td) = def else { continue };
        let type_name = td.name.clone();
        for m in &td.methods {
            let body = m.body.clone();
            let params = m.params.clone();
            let prog = Arc::clone(program);
            let tn = type_name.clone();
            let mn = m.name.clone();
            methods.register(type_name.clone(), m.name.clone(), move |recv, args| {
                // Method bodies are self-contained (self / params / builtins /
                // comprehension — no cross-method dispatch yet), so a
                // method-free evaluator suffices.
                let ev = Evaluator::new(Arc::clone(&prog), Arc::new(MethodRegistry::new()));
                // Methods are PURE: they return the *data describing the
                // change* (a delta like `{nodes: {_add: [x]}}`) or a query
                // value — never a mutated state. The framework applies the
                // delta (via the representation), so nothing is tagged/mutated
                // here.
                ev.eval_method_body(&body, recv, &params, args)
                    .map_err(|e| MethodError::Failed {
                        type_name: tn.clone(),
                        method: mn.clone(),
                        message: e.to_string(),
                    })
            });
        }
    }
}

/// Register every `type Name = <repr> …` in the `TypeRegistry` with its
/// representation schema and a [`RepresentationType`] handler, so a
/// `Custom(Name)` slot is first-class: the algebra runs on its representation.
/// The program-aware type registry: builtins + the program's `type`s + the std
/// `Qubits` quantum vocabulary. Built ONCE per compile and threaded into the
/// Evaluator and the Core (one registry, never an ad-hoc default). Also what
/// every standalone `Evaluator::new` builds from its own program.
pub(crate) fn build_type_registry(program: &Arc<Program>) -> Arc<TypeRegistry> {
    let mut types = TypeRegistry::new();
    register_user_types(&mut types, program);
    crate::quantum::register_quantum_type(&mut types);
    // The `reaction` type — a reaction at rest as transmittable DATA (AlChemy).
    // A `:: reaction` / `:: map[reaction]` slot AUTO-reifies a chrysalis Rule to
    // the closure-free `Foreign(FOREIGN_REACTION, …)` form the BRS reads as a
    // rule (rules-as-state) and that crosses `:: bigraph` bridges (#42) — one
    // value for store / link-share / send. See `runtime::rule::ReactionType`.
    types.register_full(
        "reaction",
        Schema::Any,
        None,
        Some(Arc::new(crate::runtime::rule::ReactionType)),
        Vec::new(),
    );
    Arc::new(types)
}

fn register_user_types(types: &mut TypeRegistry, program: &Arc<Program>) {
    for def in &program.defs {
        match def {
            Def::Type(td) => {
                let repr = crate::schema::lower_schema_in_program(&td.representation, program);
                types.register_full(
                    td.name.clone(),
                    repr,
                    None,
                    Some(Arc::new(RepresentationType)),
                    Vec::new(),
                );
            }
            // A composite IS a type whose REPRESENTATION is its `CompositeLink`
            // (nominal `Custom{Cell}` "is-a" structural `CompositeLink`). So a
            // `Custom{Cell}` slot — however it was lowered — polymorphically
            // delegates the whole algebra (divide/apply/serialize) to the
            // composite's real structural schema: `divide_by_schema(Custom{Cell})`
            // → `type_divide("Cell")` → `divide_by_schema(CompositeLink)`, so the
            // cell's extensive `mass` face halves instead of an unknown-type share.
            // (Resolves the old "composite as Custom = category error" by giving the
            // name a definition, rather than forbidding the name.)
            Def::Composite(c) => {
                let repr = crate::schema::def_schema(def, program);
                types.register_full(
                    c.name.clone(),
                    repr,
                    None,
                    Some(Arc::new(RepresentationType)),
                    // A composite IS-A `composite` (Cardelli record width-subtyping:
                    // `Cell` is the composite-node shape PLUS its declared face, a
                    // longer record). So `is_a("Cell", "composite")` and, via the
                    // brand lattice, `is_a("Cell", "link")` — the node-kind discovery,
                    // matcher subsumption, and method-inheritance all consult this one
                    // edge instead of a `_type == "composite"` string compare.
                    vec!["composite".into()],
                );
            }
            _ => {}
        }
    }
}

/// A user `type`'s algebra handler: every structural op **delegates to the
/// type's representation schema** (the entry's `schema`). So `Custom(Name)`
/// behaves exactly as its representation — additive numbers, structural
/// `_add`/`_remove` collections — and a delta a method produced
/// (`{nodes: {_add: [x]}}`) applies and composes through it. There is *no*
/// per-method `apply` logic: write-methods are pure delta-constructors run at
/// the producer; this just runs the resulting delta on the representation.
/// This is what makes a user type "indistinguishable from a built-in".
struct RepresentationType;

impl TypeMethods for RepresentationType {
    fn default(&self, reg: &TypeRegistry, schema: &Schema) -> Value {
        algebra::default_with(Some(reg), schema)
    }
    fn apply(&self, reg: &TypeRegistry, schema: &Schema, state: &Value, update: &Value) -> Value {
        algebra::apply_with(Some(reg), schema, state, update)
    }
    fn divide(
        &self,
        reg: &TypeRegistry,
        schema: &Schema,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value> {
        divide_by_schema(schema, state, ctx, reg)
    }
    fn serialize(&self, reg: &TypeRegistry, schema: &Schema, state: &Value) -> Value {
        algebra::serialize_with(Some(reg), schema, state)
    }
    fn realize(&self, reg: &TypeRegistry, schema: &Schema, encoded: &Value) -> Value {
        algebra::realize_with(Some(reg), schema, encoded)
    }
    fn check(&self, reg: &TypeRegistry, schema: &Schema, state: &Value) -> bool {
        algebra::check_with(Some(reg), schema, state)
    }
}

fn register_divide_methods(methods: &mut MethodRegistry, program: &Program) {
    for def in &program.defs {
        let Def::Composite(c) = def else { continue };
        // Only composites with a divisible self-exported face get `divide`
        // (a faceless wiring composite has nothing to split).
        let schema = crate::schema::def_schema(def, program);
        if schema.node_data_branches().is_empty() {
            continue;
        }
        methods.register(c.name.clone(), "divide", move |_recv, _args| {
            // `divide()` emits a binary `_divide` DIRECTIVE (split into two, no
            // per-daughter overrides). The schema-driven split happens LATE, at
            // APPLY time, via the `_divide` sentinel + `divide_by_schema(
            // CompositeLink)` — the SAME path the Form-3 `Divider` uses. So there
            // is ONE split mechanism, and the apply splits the LIVE node
            // (including this tick's growth), conserving mass across a divide.
            // The container (the firing) keys the daughters `<mother>_0/_1`.
            Ok(Value::Map(IndexMap::from([(
                Key::from("_divide"),
                Value::List(vec![Value::map(), Value::map()]),
            )])))
        });
    }
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

/// Rebase a `using` arg into a wire on a CHILD process. A `using ctx(factor:
/// @.slot)` arg `@.slot` names the *composite's* `slot`; from a child process
/// inside that composite the composite's slot is the child's CONTAINER (a
/// sibling), so the `@`-rooted (own-scope) path must become a bare `Local` one,
/// which lowers container-relative (`["slot"]`). Without this the factor wire
/// resolves to `["%","slot"]` = the *process node's* own slot (empty) and the
/// coercion silently reads `None`. Non-`@.x` args pass through unchanged.
fn sibling_wire(value: &Expr) -> Expr {
    if let Expr::Path(pp) = value {
        if matches!(pp.root, crate::ast::PathRoot::Here) && !pp.segments.is_empty() {
            let mut segments = pp.segments.clone();
            let head = segments.remove(0);
            return Expr::Path(crate::ast::PlacePath {
                root: crate::ast::PathRoot::Local(head),
                segments,
            });
        }
    }
    value.clone()
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
            control,
            ports,
            body,
            ..
        } => {
            if let Some(inputs) = proc_inputs.get(control) {
                for cu in using {
                    for arg in &cu.args {
                        if let TermArg::Named { name, value } = arg {
                            if inputs.contains(name) && !ports.inputs.contains_key(name) {
                                ports.inputs.insert(name.clone(), sibling_wire(value));
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
