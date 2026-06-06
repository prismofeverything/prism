//! Interpreter for chrysalis expression bodies.
//!
//! Two modes:
//!
//! - [`Evaluator::eval_value`] — reduces an [`Expr`] to a
//!   `prism_schema::Value`. The general-purpose evaluator used inside
//!   process / step / composite bodies and inside reaction
//!   reactum / guard / rate at fire time.
//! - [`Evaluator::eval_pattern`] — interprets an [`Expr`] as a
//!   `prism_schema::Pattern`, producing a [`RuleBindings`] table that
//!   records where each chrysalis pattern variable lives in the
//!   resulting Pattern.
//!
//! Top-level definitions ([`Def`]) are looked up in the
//! [`Program`]. Method calls dispatch through
//! `prism_schema::MethodRegistry`.

use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;
use thiserror::Error;

use prism_schema::registry::TypeRegistry;
use prism_schema::{Key, MethodError, MethodRegistry, Pattern, Value, value_type_name};

use crate::ast::{
    BinOp, Block, Def, Expr, Name, PathRoot, PlacePath, PortBindings, Program, ReactionDef,
    StringLit, StringSeg, TermArg, UnaryOp,
};
use crate::runtime::rule::{BindingSource, FOREIGN_RULE, Reactum, Rule, RuleBindings};

#[derive(Debug, Error)]
pub enum EvalError {
    #[error("unbound variable: `{0}`")]
    UnboundVar(String),

    #[error("type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: String, got: String },

    #[error("invalid form in {context}: {message}")]
    InvalidForm { context: String, message: String },

    #[error("unknown control: `{0}`")]
    UnknownControl(String),

    #[error("arity mismatch on `{control}`: expected {expected}, got {got}")]
    Arity {
        control: String,
        expected: String,
        got: String,
    },

    #[error("method dispatch failed: {0}")]
    Method(#[from] MethodError),

    #[error("{0}")]
    Other(String),
}

/// The chrysalis interpreter.
///
/// Holds shared references to the program and method registry. Cheap
/// to clone via the contained `Arc`s.
pub struct Evaluator {
    pub program: Arc<Program>,
    pub methods: Arc<MethodRegistry>,
    /// Imported native OBJECTS (`from integrators import rk4`) — bound as values
    /// resolvable by name in any body, so `rk4.integrate(...)` dispatches like
    /// any value method. The replacement for `extern` value handles.
    pub imports: IndexMap<Name, Value>,
    /// Imported native PROCESS names (`from core import RunProcess`) — used
    /// wholesale: a call site `Name[args] ~{…}->{…}` compiles to a
    /// `{address: local:Name, config, inputs, outputs}` spec with config + ports
    /// taken straight from the call site (no interface declaration needed).
    pub imported_processes: HashSet<Name>,
    /// Imported native FUNCTIONS (`from io import load`) — bare callables
    /// dispatched as `name(args)`. The host functions take `&[Value]` and
    /// return `Value`. Pairs with the `Expr::Call` resolution path.
    pub imported_functions: IndexMap<Name, crate::compile::HostFn>,
    /// The type registry (builtins + this program's `type`s + the std `Qubits`
    /// vocabulary). The Evaluator is schema-registry-aware so it can REALIZE a
    /// value at its declared type at construction — a `:: Qubits` param/def/slot
    /// turns a bare literal into a full tagged instance via `realize_with`. One
    /// program-aware registry, threaded; never a bare default.
    pub types: Arc<TypeRegistry>,
}

impl Evaluator {
    pub fn new(program: Arc<Program>, methods: Arc<MethodRegistry>) -> Self {
        let types = crate::compile::build_type_registry(&program);
        Self {
            program,
            methods,
            imports: IndexMap::new(),
            imported_processes: HashSet::new(),
            imported_functions: IndexMap::new(),
            types,
        }
    }

    /// As [`Evaluator::new`] but with a pre-built (shared) type registry — the
    /// compile path builds ONE and threads the same `Arc` into both the
    /// Evaluator and the [`Core`](prism_bigraph::Core), so there is a single
    /// registry per compile context.
    pub fn with_types(
        program: Arc<Program>,
        methods: Arc<MethodRegistry>,
        types: Arc<TypeRegistry>,
    ) -> Self {
        Self {
            program,
            methods,
            imports: IndexMap::new(),
            imported_processes: HashSet::new(),
            imported_functions: IndexMap::new(),
            types,
        }
    }

    /// Like [`Evaluator::new`], but seeded with native host imports resolved
    /// from a `ModuleRegistry` (objects bound by name, process names recognised
    /// as wholesale native controls, functions dispatchable as bare calls).
    pub fn with_native_imports(
        program: Arc<Program>,
        methods: Arc<MethodRegistry>,
        types: Arc<TypeRegistry>,
        imports: IndexMap<Name, Value>,
        imported_processes: HashSet<Name>,
        imported_functions: IndexMap<Name, crate::compile::HostFn>,
    ) -> Self {
        Self {
            program,
            methods,
            imports,
            imported_processes,
            imported_functions,
            types,
        }
    }

    // ===============================================================
    // Value-context evaluation
    // ===============================================================

    /// Evaluate a type method body with the receiver bound to `self` and the
    /// method's positional `args` bound to its `params` (defaults fill missing
    /// args). This is how chrysalis-defined type methods run — both as queries
    /// (`v.m(args)`) and as `apply` write-directives (see `Def::Type`).
    pub fn eval_method_body(
        &self,
        body: &Expr,
        self_val: &Value,
        params: &[crate::ast::Param],
        args: &[Value],
    ) -> Result<Value, EvalError> {
        let mut env: IndexMap<Name, Value> = IndexMap::new();
        env.insert("self".into(), self_val.clone());
        for (i, p) in params.iter().enumerate() {
            let v = match args.get(i) {
                Some(v) => v.clone(),
                None => match &p.default {
                    Some(d) => self.eval_value(d, &env)?,
                    None => Value::None,
                },
            };
            env.insert(p.name.clone(), v);
        }
        self.eval_value(body, &env)
    }

    pub fn eval_value(&self, expr: &Expr, env: &IndexMap<Name, Value>) -> Result<Value, EvalError> {
        match expr {
            Expr::Unit => Ok(Value::None),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::Float(f) => Ok(Value::float(*f)),
            Expr::Str(lit) => self.eval_string(lit, env),

            Expr::Var(name) | Expr::Site { name, .. } => {
                if let Some(v) = env.get(name).or_else(|| self.imports.get(name)) {
                    return Ok(v.clone());
                }
                // The unified entity lookup (#30): one name → one `EntityView`,
                // and a bare reference resolves to the entity's value-form when
                // it has one. Function slots become first-class function values;
                // composite/process/step/reaction slots become no-arg term
                // evaluations (the spec/Rule value). So `all` ≡ `all[]`,
                // `phosphorylate` ≡ `phosphorylate[]`. A definer that needs args
                // still reports the missing arg via `Term` evaluation.
                match self.program.entity(name) {
                    Some(entity) if entity.function.is_some() => Ok(function_value(name)),
                    Some(entity) if entity.has_value_form() => self.eval_value(
                        &Expr::Term {
                            control: name.clone(),
                            args: vec![],
                            ports: crate::ast::PortBindings::default(),
                            body: None,
                        },
                        env,
                    ),
                    _ => Err(EvalError::UnboundVar(name.clone())),
                }
            }

            Expr::Path(path) => self.eval_path(path, env),

            Expr::BinOp { op, lhs, rhs } => {
                let l = self.eval_value(lhs, env)?;
                let r = self.eval_value(rhs, env)?;
                apply_binop(*op, &l, &r)
            }
            Expr::UnaryOp { op, operand } => {
                let v = self.eval_value(operand, env)?;
                apply_unaryop(*op, &v)
            }

            Expr::If { cond, then_, else_ } => {
                let c = self.eval_value(cond, env)?;
                match c {
                    Value::Bool(true) => self.eval_value(then_, env),
                    Value::Bool(false) => match else_ {
                        Some(e) => self.eval_value(e, env),
                        None => Ok(Value::None),
                    },
                    other => Err(EvalError::TypeMismatch {
                        expected: "Bool".into(),
                        got: value_type_name(&other).into(),
                    }),
                }
            }

            Expr::Let { bindings, body } => {
                let mut new_env = env.clone();
                for (name, value_expr) in bindings {
                    let v = self.eval_value(value_expr, &new_env)?;
                    new_env.insert(name.clone(), v);
                }
                self.eval_value(body, &new_env)
            }

            Expr::Block(block) => self.eval_block(block, env),

            Expr::Method {
                receiver,
                method,
                args,
            } => {
                let recv = self.eval_value(receiver, env)?;
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval_value(a, env))
                    .collect::<Result<_, _>>()?;
                Ok(self.methods.dispatch(&recv, method, &arg_vals)?)
            }

            // Value field access: read `name` off the evaluated base (`None` if
            // absent), so any value composes under `.field` — e.g.
            // `all[].config.bridge`. (Place-path access stays `Expr::Path`.)
            Expr::Field { base, name } => {
                let v = self.eval_value(base, env)?;
                Ok(v.get_field(name).cloned().unwrap_or(Value::None))
            }

            Expr::Call { func, args } => self.eval_call(func, args, env),

            Expr::Comprehension {
                key_var,
                var,
                source,
                filter,
                body,
                key,
            } => {
                let source_val = self.eval_value(source, env)?;
                // Iterate a list (key = index) or a map (key = entry key). Both
                // bind the value to `var`; `key_var`, if present, binds the
                // key/index.
                let entries: Vec<(Value, Value)> = match source_val {
                    Value::List(items) => items
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (Value::Int(i as i64), v))
                        .collect(),
                    Value::Map(m) => m
                        .into_iter()
                        .map(|(k, v)| (Value::String(k.to_string()), v))
                        .collect(),
                    Value::Struct { values, .. } => values
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (Value::Int(i as i64), v))
                        .collect(),
                    other => {
                        return Err(EvalError::InvalidForm {
                            context: "comprehension".into(),
                            message: format!(
                                "`for {var} in …` expects a list or map, got {}",
                                value_type_name(&other)
                            ),
                        });
                    }
                };
                let mut list_out: Vec<Value> = Vec::new();
                let mut map_out: IndexMap<Key, Value> = IndexMap::new();
                for (k, v) in entries {
                    let mut scope = env.clone();
                    if let Some(kv) = key_var {
                        scope.insert(kv.clone(), k);
                    }
                    scope.insert(var.clone(), v);
                    let keep = match filter {
                        Some(pred) => {
                            matches!(self.eval_value(pred, &scope)?, Value::Bool(true))
                        }
                        None => true,
                    };
                    if !keep {
                        continue;
                    }
                    let body_val = self.eval_value(body, &scope)?;
                    match key {
                        // Map comprehension: insert key→body (later keys win, so a
                        // constant key collapses to the last match).
                        Some(key_expr) => {
                            let key_val = self.eval_value(key_expr, &scope)?;
                            let key_str = key_val.as_str().ok_or_else(|| EvalError::InvalidForm {
                                context: "comprehension".into(),
                                message: format!(
                                    "a map-comprehension key must be a string, got {}",
                                    value_type_name(&key_val)
                                ),
                            })?;
                            map_out.insert(Key::from(key_str), body_val);
                        }
                        None => list_out.push(body_val),
                    }
                }
                Ok(if key.is_some() {
                    Value::Map(map_out)
                } else {
                    Value::List(list_out)
                })
            }

            Expr::ReplaceWith { id, with } => {
                let id_val = self.eval_value(id, env)?;
                let with_val = self.eval_value(with, env)?;
                let mut out: IndexMap<Key, Value> = IndexMap::new();
                out.insert("_remove".into(), Value::List(vec![id_val]));
                out.insert("_add".into(), with_val);
                Ok(Value::Map(out))
            }

            Expr::Term {
                control,
                args,
                ports,
                body,
            } => self.eval_term_value(control, args, ports, body.as_deref(), env),

            Expr::Parallel(elems) => self.eval_parallel_value(elems, env),

            Expr::KeyedEntry { .. } => Err(EvalError::InvalidForm {
                context: "value".into(),
                message: "bare KeyedEntry outside a parallel/map context".into(),
            }),

            Expr::Map(entries) => self.eval_map_value(entries, env),
            Expr::Record(fields) => self.eval_record_value(fields, env),
            Expr::List(items) => {
                let vals: Vec<Value> = items
                    .iter()
                    .map(|e| self.eval_value(e, env))
                    .collect::<Result<_, _>>()?;
                Ok(Value::List(vals))
            }

            Expr::Unbound => Err(EvalError::InvalidForm {
                context: "value".into(),
                message: "`!` (unbound) is only valid inside a pattern".into(),
            }),
            Expr::LinkVar(_) => Err(EvalError::InvalidForm {
                context: "value".into(),
                message: "`~name` is only valid inside a pattern".into(),
            }),
            Expr::Rule { .. } => Err(EvalError::InvalidForm {
                context: "value".into(),
                message: "`=>` is only valid inside a reaction body".into(),
            }),
            // `where` in value context: ignore predicate, return inner.
            Expr::Where { inner, .. } => self.eval_value(inner, env),
        }
    }

    fn eval_block(&self, block: &Block, env: &IndexMap<Name, Value>) -> Result<Value, EvalError> {
        let mut new_env = env.clone();
        for (name, value_expr) in &block.bindings {
            let v = self.eval_value(value_expr, &new_env)?;
            new_env.insert(name.clone(), v);
        }
        self.eval_value(&block.value, &new_env)
    }

    fn eval_string(
        &self,
        lit: &StringLit,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut out = String::new();
        for seg in &lit.segments {
            match seg {
                StringSeg::Lit(s) => out.push_str(s),
                StringSeg::Expr(e) => {
                    let v = self.eval_value(e, env)?;
                    out.push_str(&value_to_string(&v));
                }
            }
        }
        Ok(Value::String(out))
    }

    /// Evaluate a function call `func(args)`. `func` is usually a `Var` naming a
    /// `def name(params) = body` ([`crate::ast::Def::Function`]); the body
    /// evaluates with `params` bound positionally to the evaluated args.
    fn eval_call(
        &self,
        func: &Expr,
        args: &[Expr],
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let arg_vals: Vec<Value> = args
            .iter()
            .map(|a| self.eval_value(a, env))
            .collect::<Result<_, _>>()?;
        // 1. Direct: `func` is a Var naming a top-level `def`ined function.
        if let Expr::Var(name) = func {
            if let Some(crate::ast::Def::Function(f)) = self.program.lookup(name) {
                let f = f.clone();
                return self.eval_function_body(&f.body, &f.params, &arg_vals);
            }
            // 1b. Or a native function imported via `from <module> import name`
            // (`load("file.ys")`, …). Dispatched against the host function map.
            if let Some(host) = self.imported_functions.get(name) {
                return host(&arg_vals).map_err(EvalError::Method);
            }
        }
        // 2. Indirect: `func` evaluates to a first-class function value — a
        // function passed as an argument, returned, or stored. Resolve it to its
        // definition and call. (Higher-order: `apply(double, 5)`, `f(x)`.)
        let fval = self.eval_value(func, env)?;
        if let Some(name) = function_value_name(&fval) {
            if let Some(crate::ast::Def::Function(f)) = self.program.lookup(&name) {
                let f = f.clone();
                return self.eval_function_body(&f.body, &f.params, &arg_vals);
            }
        }
        Err(EvalError::InvalidForm {
            context: "call".into(),
            message: match func {
                Expr::Var(n) => format!("`{n}` is not a function (no `def {n}(…)`)"),
                _ => "call target is not a function".into(),
            },
        })
    }

    /// Evaluate a function body with `params` bound positionally to `args`.
    /// Pure: the body sees only its parameters (no closure capture yet).
    fn eval_function_body(
        &self,
        body: &Expr,
        params: &[crate::ast::Param],
        args: &[Value],
    ) -> Result<Value, EvalError> {
        let mut env: IndexMap<Name, Value> = IndexMap::new();
        for (i, p) in params.iter().enumerate() {
            env.insert(p.name.clone(), args.get(i).cloned().unwrap_or(Value::None));
        }
        self.eval_value(body, &env)
    }

    fn eval_path(&self, path: &PlacePath, env: &IndexMap<Name, Value>) -> Result<Value, EvalError> {
        // Tier 1: only the local-relative root is supported; @ and ^
        // are valid syntactic forms but their lowering depends on the
        // composite context, which the engine resolves via wire
        // resolution at instantiation time. They appear in interface
        // wirings, not in expression bodies that eval_value evaluates.
        match &path.root {
            PathRoot::Local(name) => {
                // Resolve the root through the same logic as a bare `Var`: an env
                // binding, else a function/composite/process/step definer value
                // (so `all.config.bridge` ≡ `all[].config.bridge`), else
                // unbound. Then walk the field segments.
                let mut current = match env.get(name) {
                    Some(v) => v.clone(),
                    None => self.eval_value(&Expr::Var(name.clone()), env)?,
                };
                for seg in &path.segments {
                    current = current.get_field(seg).cloned().unwrap_or(Value::None);
                }
                Ok(current)
            }
            PathRoot::Here | PathRoot::Parent => Err(EvalError::InvalidForm {
                context: "value".into(),
                message: "`@` / `^` paths are wiring forms, not value expressions".into(),
            }),
        }
    }

    fn eval_parallel_value(
        &self,
        elems: &[Expr],
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        // Classify: all keyed? all anonymous? mixed?
        let all_keyed =
            !elems.is_empty() && elems.iter().all(|e| matches!(e, Expr::KeyedEntry { .. }));
        if all_keyed {
            let mut map: IndexMap<Key, Value> = IndexMap::new();
            for e in elems {
                if let Expr::KeyedEntry { key, value } = e {
                    let key_str = self.eval_string_to_str(key, env)?;
                    let v = self.eval_value(value, env)?;
                    map.insert(Key::from(key_str.as_str()), v);
                }
            }
            return Ok(Value::Map(map));
        }
        // Otherwise build a List, lifting each KeyedEntry into a
        // single-entry Map element. This is the "mixed" case from
        // the design's compilation map.
        let mut items: Vec<Value> = Vec::with_capacity(elems.len());
        for e in elems {
            let v = match e {
                Expr::KeyedEntry { key, value } => {
                    let key_str = self.eval_string_to_str(key, env)?;
                    let inner = self.eval_value(value, env)?;
                    Value::Map(IndexMap::from_iter([(Key::from(key_str.as_str()), inner)]))
                }
                _ => self.eval_value(e, env)?,
            };
            items.push(v);
        }
        Ok(Value::List(items))
    }

    fn eval_map_value(
        &self,
        entries: &[(StringLit, Expr)],
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut map: IndexMap<Key, Value> = IndexMap::new();
        for (key, value) in entries {
            let key_str = self.eval_string_to_str(key, env)?;
            let v = self.eval_value(value, env)?;
            map.insert(Key::from(key_str.as_str()), v);
        }
        Ok(Value::Map(map))
    }

    fn eval_record_value(
        &self,
        fields: &IndexMap<Name, Expr>,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut map: IndexMap<Key, Value> = IndexMap::new();
        for (name, value) in fields {
            let v = self.eval_value(value, env)?;
            map.insert(Key::from(name.as_str()), v);
        }
        Ok(Value::Map(map))
    }

    fn eval_string_to_str(
        &self,
        lit: &StringLit,
        env: &IndexMap<Name, Value>,
    ) -> Result<String, EvalError> {
        let v = self.eval_string(lit, env)?;
        match v {
            Value::String(s) => Ok(s),
            other => Err(EvalError::TypeMismatch {
                expected: "String".into(),
                got: value_type_name(&other).into(),
            }),
        }
    }

    // ---------------------------------------------------------------
    // Term construction in value context
    // ---------------------------------------------------------------

    fn eval_term_value(
        &self,
        control: &Name,
        args: &[TermArg],
        ports: &PortBindings,
        body: Option<&Expr>,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        // The unified entity dispatch (#30): one lookup returns every slot for
        // this name; priority order is composite > process > step > reaction >
        // protocol — matching the historical Def-variant precedence. Slice 2b
        // (when type/control declarations gain instantiation semantics) will
        // extend the priority list without changing this dispatch shape.
        if let Some(entity) = self.program.entity(control) {
            if let Some(def) = entity.composite {
                let def = def.clone();
                return self.build_composite_outer(&def, args, ports, env);
            }
            if let Some(def) = entity.process {
                let def = def.clone();
                return self.build_pure_spec(
                    control, "process", args, ports, &def.params, &def.interface, env,
                );
            }
            if let Some(def) = entity.step {
                let def = def.clone();
                return self.build_pure_spec(
                    control, "step", args, ports, &def.params, &def.interface, env,
                );
            }
            if let Some(def) = entity.reaction {
                let def = def.clone();
                return self.build_reaction_value(&def, args, env);
            }
            if let Some(def) = entity.protocol {
                let def = def.clone();
                return self.build_protocol_outer(&def, args, ports, env);
            }
            if entity.function.is_some() {
                return Err(EvalError::InvalidForm {
                    context: "value-term".into(),
                    message: format!(
                        "`{control}` is a function — call it as `{control}(args)`, not `{control}[args]`"
                    ),
                });
            }
            return Err(EvalError::InvalidForm {
                context: "value-term".into(),
                message: format!("control `{}` is not callable in value context", control),
            });
        }
        // No entity in the program: a free control. Native imports (`from core
        // import …`) bypass interface-redeclaration; built-ins like `BRS` have
        // hand-rolled value builders; everything else is a plain control whose
        // value is its bigraph atom (`{_type: <control>, …args}`).
        if self.imported_processes.contains(control) {
            return self.build_native_spec(control, args, ports, env);
        }
        match control.as_str() {
            "BRS" => self.build_brs_value(args, ports, env),
            _ => self.build_plain_map_value(control, args, body, env),
        }
    }

    /// Build a `{address, config, inputs, outputs}` spec for a WHOLESALE-imported
    /// native process. Unlike [`build_spec_value`], there is no declared
    /// interface, so config is every call-site named arg and the wired ports are
    /// exactly those the call site connects (`from core import RunProcess`).
    fn build_native_spec(
        &self,
        control: &Name,
        args: &[TermArg],
        ports: &PortBindings,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut config_map: IndexMap<Key, Value> = IndexMap::new();
        for a in args {
            if let TermArg::Named { name, value } = a {
                config_map.insert(Key::from(name.as_str()), self.eval_value(value, env)?);
            }
        }
        let lower =
            |binds: &IndexMap<Name, Expr>, ev: &Self| -> Result<IndexMap<Key, Value>, EvalError> {
                let mut m: IndexMap<Key, Value> = IndexMap::new();
                for (name, target) in binds {
                    let segs = lower_target_to_segments(target, ev, env)?;
                    m.insert(
                        Key::from(name.as_str()),
                        Value::List(segs.into_iter().map(Value::String).collect()),
                    );
                }
                Ok(m)
            };
        let inputs_map = lower(&ports.inputs, self)?;
        let outputs_map = lower(&ports.outputs, self)?;

        let mut spec: IndexMap<Key, Value> = IndexMap::new();
        // A native import's _type defaults to "link" (kind unknown from this
        // side — the registry decides Process vs Step at instantiation).
        // Discovery still finds it (any Link-kind in the schema, or this
        // `_type` hint when the slot is `Any`).
        spec.insert("_type".into(), Value::String("link".into()));
        spec.insert("address".into(), Value::String(format!("local:{control}")));
        spec.insert("config".into(), Value::Map(config_map));
        spec.insert("inputs".into(), Value::Map(inputs_map));
        spec.insert("outputs".into(), Value::Map(outputs_map));
        Ok(Value::Map(spec))
    }

    /// A protocol-bound control (`protocol StreamingCell = stream<Cell, …>` used as
    /// `StreamingCell[config] ~{} ->{}`): build the WRAPPED composite's node, then
    /// **override its `address`** with the typed protocol address `{_type: protocol,
    /// …fields}` so the engine instantiates it over that transport. The wrapped
    /// composite's config / bridge / wiring are unchanged — only *where it runs*
    /// changes (and daughters inherit the address on division). See
    /// docs/protocols-as-types.md.
    fn build_protocol_outer(
        &self,
        pd: &crate::ast::ProtocolDef,
        args: &[TermArg],
        ports: &PortBindings,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        // The wrapped control may be a COMPOSITE (a sub-bigraph) or a leaf
        // PROCESS/STEP — rest-addressing is UNIFORM across kinds. Build its
        // normal node, then override the `address` with the protocol's typed
        // one, so a remote leaf simulator (e.g. CopasiCvode) is a rest-addressed
        // PROCESS carrying its own params as config — drop-in to RunProcess.
        let mut node = match self.program.lookup(&pd.wrapped) {
            Some(Def::Composite(c)) => self.build_composite_outer(&c.clone(), args, ports, env)?,
            Some(Def::Process(p)) => {
                let p = p.clone();
                self.build_pure_spec(&pd.wrapped, "process", args, ports, &p.params, &p.interface, env)?
            }
            Some(Def::Step(s)) => {
                let s = s.clone();
                self.build_pure_spec(&pd.wrapped, "step", args, ports, &s.params, &s.interface, env)?
            }
            _ => {
                return Err(EvalError::InvalidForm {
                    context: "protocol".into(),
                    message: format!(
                        "`protocol {} = {}<{}, …>`: `{}` must be a composite, process, or step in scope",
                        pd.name, pd.protocol, pd.wrapped, pd.wrapped
                    ),
                });
            }
        };
        let address = self.build_protocol_address(pd, env)?;
        if let Value::Map(m) = &mut node {
            m.insert(Key::from("address"), address);
        }
        Ok(node)
    }

    /// The typed protocol address value `{_type: <protocol>, <field>: <value>, …}`
    /// — a first-class Custom value of the protocol's registered address type. Its
    /// `_type` tag selects both the schema type (for the algebra) and the transport
    /// (for `instantiate`).
    fn build_protocol_address(
        &self,
        pd: &crate::ast::ProtocolDef,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut m: IndexMap<Key, Value> = IndexMap::new();
        m.insert(Key::from("_type"), Value::String(pd.protocol.clone()));
        for (field, expr) in &pd.fields {
            m.insert(Key::from(field.as_str()), self.eval_value(expr, env)?);
        }
        Ok(Value::Map(m))
    }

    /// Build the spec for a composite call site.
    ///
    /// Shape — the SAME envelope as a leaf process/step spec (#47), with the
    /// leaf-vs-composite distinction living entirely inside `config`:
    /// ```text
    /// {
    ///   _type:    "composite",
    ///   address:  "local:Composite",
    ///   config:   { state, bridge, schema },   // inner body + bridge wiring
    ///   inputs:   { port: wire, … },
    ///   outputs:  { port: wire, … },
    ///   <slot>:   <inner state value>,         // seed_self_face — one per
    ///   …                                      //   output port wired to `%.field`
    /// }
    /// ```
    ///
    /// The composite's bridge connects its inner state to these outer
    /// data slots: each output port's internal value flows out to the
    /// sibling slot, where the surrounding pattern matcher / BRS can
    /// observe it. Instantiated through `Composite::from_config` (the
    /// upstream-aligned shape — `from_config_composite.rs`).
    fn build_composite_outer(
        &self,
        def: &crate::ast::CompositeDef,
        args: &[TermArg],
        ports: &PortBindings,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        // A composite compiles to a real prism subengine spec — the same
        // shape as any process — instantiated via `Composite::from_config`.
        // Inner state (the body) is encapsulated in `config.state`; the
        // `bridge` maps each interface port to its same-name internal path.
        // Outputs land on the PARENT via normal (parent-relative) wires.
        // See crates/prism-bigraph/tests/growth_division.rs for the proven
        // subengine grow/divide pattern.
        let resolved = self.composite_param_env(def, args, env)?;
        let inner_state = self.eval_value(&def.body, &resolved)?;
        // Each port's bridge wire: the explicit `@ internal.path` if declared,
        // else name-inference (the same-named top-level field).
        let wire_of = |name: &str, decl: &crate::ast::PortDecl| -> Value {
            let segs = decl
                .bridge
                .clone()
                .unwrap_or_else(|| vec![name.to_string()]);
            Value::List(segs.into_iter().map(Value::String).collect())
        };
        let mut bridge_in: IndexMap<Key, Value> = IndexMap::new();
        for (p, decl) in def.interface.inputs.iter() {
            bridge_in.insert(Key::from(p.as_str()), wire_of(p, decl));
        }
        let mut bridge_out: IndexMap<Key, Value> = IndexMap::new();
        for (p, decl) in def.interface.outputs.iter() {
            bridge_out.insert(Key::from(p.as_str()), wire_of(p, decl));
        }
        let bridge = Value::Map(IndexMap::from([
            (Key::from("inputs"), Value::Map(bridge_in)),
            (Key::from("outputs"), Value::Map(bridge_out)),
        ]));
        // Carry the DECLARED inner schema so the nested subengine is *informed*
        // by it (apply-critical `Array`/`Delta`/`Link` types inference can't
        // recover) instead of re-inferring — which would degrade an additive
        // `Array` field to a `List` and break the output bridge. The subengine
        // resolves it over its inferred floor (`resolve(infer(state), declared)`),
        // exactly as the top-level engine does.
        let inner_schema = crate::schema::composite_inner_schema(def, &self.program);
        let config = Value::Map(IndexMap::from([
            (Key::from("state"), inner_state),
            (Key::from("bridge"), bridge),
            (
                Key::from("schema"),
                prism_schema::schema_to_value(&inner_schema),
            ),
        ]));
        let composite_name: Name = "Composite".into();
        let mut spec = self.build_spec_value(
            &composite_name,
            "composite",
            &IndexMap::new(),
            ports,
            &def.interface,
            env,
        )?;
        if let Value::Map(m) = &mut spec {
            m.insert(Key::from("config"), config);
            // BRAND the node with its MOST-SPECIFIC type — the composite's own
            // name (`_type: "Cell"`), not the kind word `"composite"`. The matcher
            // (`?c :: Cell`) and method dispatch (`?c.divide()` via
            // `value_type_name`) recover "Cell" from here; the KIND (composite) is
            // recovered via `is_a("Cell", "composite")` against the registry's brand
            // lattice. This makes a composite node carry its name EXACTLY as a
            // molecule node already does (`_type: "ERK"`) — one uniform branding.
            // The `address` stays `local:Composite` (the generic composite factory);
            // the brand is for recognition, the address for instantiation.
            m.insert(Key::from("_type"), Value::String(def.name.clone()));
        }
        // Seed the EXPORTED FACE: each output port wired to `%.field` (the own
        // node) gets its initial value placed ON the node, so `cells.N.field`
        // exists from t=0 — the parent can match it (`?c.mass > …`) and the input
        // bridge can read it back each tick (the Form-3 self face;
        // cells-and-division §4.1). The value is the inner state at the port's
        // bridge path. Ports wired elsewhere (`^.glucose`, …) are untouched.
        seed_self_face(&mut spec);
        Ok(spec)
    }

    /// Evaluate a top-level `main` expression. If it's a composite call,
    /// **inline** it: the root state becomes the composite's body (its
    /// contents are then discoverable + inspectable), rather than a wrapped
    /// subengine spec that the root engine would never descend into.
    pub fn eval_top_level(
        &self,
        expr: &Expr,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        if let Expr::Term { control, args, .. } = expr {
            if let Some(crate::ast::Def::Composite(def)) = self.program.lookup(control) {
                // The composite body sees the outer (top-level) bindings, with its
                // own params + input defaults shadowing them — so a shared
                // `network :: CRN = …` resolves inside the body, and an input
                // (`amount: value`) isn't unbound. One binding rule for the body:
                // [`composite_param_env`].
                let resolved = self.composite_param_env(def, args, env)?;
                let mut body_env = env.clone();
                body_env.extend(resolved);
                return self.eval_value(&def.body, &body_env);
            }
        }
        self.eval_value(expr, env)
    }

    /// The parameter env for evaluating a composite's BODY: config params (from
    /// `args` or their defaults) PLUS each input port's declared default. This is
    /// the SINGLE eval-side rule for binding a composite's interface, shared by
    /// `eval_top_level` (the compile-time inline) and `build_composite_outer` (a
    /// composite used as a value/node) — so a body's input reference resolves
    /// the same way everywhere. An input's real value arrives per-tick via the
    /// bridge (driven) or via `--port` (invoke rebinds in `engine_for`); this is
    /// the t=0 seed. (Config defaults were always honored; input defaults were the
    /// gap — an input default made a composite "bare-runnable" → eagerly inlined →
    /// unbound input var.)
    fn composite_param_env(
        &self,
        def: &crate::ast::CompositeDef,
        args: &[TermArg],
        env: &IndexMap<Name, Value>,
    ) -> Result<IndexMap<Name, Value>, EvalError> {
        let mut resolved = self.resolve_args_against_params(&def.name, args, &def.params, env)?;
        for (name, decl) in &def.interface.inputs {
            if !resolved.contains_key(name) {
                if let Some(default) = &decl.default {
                    let v = self.eval_value(default, &resolved)?;
                    resolved.insert(name.clone(), v);
                }
            }
        }
        Ok(resolved)
    }

    /// Build a "pure" process / step spec — a `{address, config, inputs,
    /// outputs}` map with no outer-map wrap. Used for Process and Step
    /// definers and for built-ins like BRS, which have no observable
    /// data slots of their own.
    fn build_pure_spec(
        &self,
        control: &Name,
        kind: &'static str,
        args: &[TermArg],
        ports: &PortBindings,
        params: &[crate::ast::Param],
        interface: &crate::ast::Interface,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let resolved = self.resolve_args_against_params(control, args, params, env)?;
        self.build_spec_value(control, kind, &resolved, ports, interface, env)
    }

    /// Build just the `{_type, address, config, inputs, outputs}` spec map.
    ///
    /// `kind` is the type hint — `"process"`, `"step"`, or `"composite"` —
    /// embedded as `_type` on the value (upstream process-bigraph convention).
    /// Schema-first discovery uses this hint to recognise a process node when
    /// the surrounding schema is `Any` (e.g. a value stuffed into an untyped
    /// slot at runtime). `address` is for *instantiation*, NOT recognition —
    /// other values can also legitimately carry an `address` field.
    fn build_spec_value(
        &self,
        control: &Name,
        kind: &'static str,
        resolved_config: &IndexMap<Name, Value>,
        ports: &PortBindings,
        interface: &crate::ast::Interface,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let default_wire =
            |p: &str| Value::List(vec![Value::String(p.to_string())]);
        let mut inputs_map: IndexMap<Key, Value> = IndexMap::new();
        for port_name in interface.inputs.keys() {
            let wire = match ports.inputs.get(port_name) {
                Some(target) => lower_target_to_wire(target, self, env)?,
                None => default_wire(port_name),
            };
            inputs_map.insert(Key::from(port_name.as_str()), wire);
        }
        let mut outputs_map: IndexMap<Key, Value> = IndexMap::new();
        for port_name in interface.outputs.keys() {
            let wire = match ports.outputs.get(port_name) {
                Some(target) => lower_target_to_wire(target, self, env)?,
                None => default_wire(port_name),
            };
            outputs_map.insert(Key::from(port_name.as_str()), wire);
        }

        let config_map: IndexMap<Key, Value> = resolved_config
            .iter()
            .map(|(k, v)| (Key::from(k.as_str()), v.clone()))
            .collect();

        let mut spec: IndexMap<Key, Value> = IndexMap::new();
        spec.insert("_type".into(), Value::String(kind.to_string()));
        spec.insert(
            "address".into(),
            Value::String(format!("local:{}", control)),
        );
        spec.insert("config".into(), Value::Map(config_map));
        spec.insert("inputs".into(), Value::Map(inputs_map));
        spec.insert("outputs".into(), Value::Map(outputs_map));
        Ok(Value::Map(spec))
    }

    fn resolve_args_against_params(
        &self,
        control: &Name,
        args: &[TermArg],
        params: &[crate::ast::Param],
        env: &IndexMap<Name, Value>,
    ) -> Result<IndexMap<Name, Value>, EvalError> {
        let mut resolved: IndexMap<Name, Value> = IndexMap::new();
        for param in params {
            let supplied = args.iter().find_map(|a| match a {
                TermArg::Named { name, value } if name == &param.name => Some(value),
                _ => None,
            });
            let value = match (supplied, &param.default) {
                (Some(expr), _) => self.eval_value(expr, env)?,
                (None, Some(default)) => self.eval_value(default, env)?,
                (None, None) => {
                    return Err(EvalError::Arity {
                        control: control.clone(),
                        expected: format!("param `{}` (no default)", param.name),
                        got: "missing".into(),
                    });
                }
            };
            // Realize the bound value at its DECLARED type — a `:: Qubits` (or
            // any Custom) param promotes a bare literal to a full tagged
            // instance. The registry-threaded realize is the single typed-
            // construction path: the value is born carrying its type, so no
            // hand-written `_type:` is needed at the call site.
            let schema = crate::schema::lower_schema_in_program(&param.schema, &self.program);
            let value =
                prism_schema::algebra::realize_with(Some(self.types.as_ref()), &schema, &value);
            resolved.insert(param.name.clone(), value);
        }
        Ok(resolved)
    }

    /// Build a chrysalis [`Rule`] value (wrapped in `Value::Foreign`)
    /// from a `reaction` definer call site like
    /// `MassThresholdDivide[threshold: 2.0]`.
    fn build_reaction_value(
        &self,
        def: &ReactionDef,
        args: &[TermArg],
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        // Resolve params from defaults + supplied args.
        let mut resolved: IndexMap<Name, Value> = IndexMap::new();
        for param in &def.params {
            let supplied = args.iter().find_map(|a| match a {
                TermArg::Named { name, value } if name == &param.name => Some(value),
                _ => None,
            });
            let value = match (supplied, &param.default) {
                (Some(expr), _) => self.eval_value(expr, env)?,
                (None, Some(default)) => self.eval_value(default, env)?,
                (None, None) => {
                    return Err(EvalError::Arity {
                        control: def.name.clone(),
                        expected: format!("param `{}` (no default)", param.name),
                        got: "missing".into(),
                    });
                }
            };
            resolved.insert(param.name.clone(), value);
        }

        // Pattern lowering uses the resolved-params env so that
        // params can appear in the redex.
        let mut rule_env: IndexMap<Name, Value> = env.clone();
        for (k, v) in &resolved {
            rule_env.insert(k.clone(), v.clone());
        }

        let mut bindings = RuleBindings::new();
        let redex = self.eval_pattern_top(&def.redex, &rule_env, &mut bindings)?;

        // Classify the reactum. One that lowers to a prism Pattern is a
        // STRUCTURAL rewrite (MAPK-style link/rest rewrites, fired by
        // prism's native `instantiate`); one that doesn't (a method call or
        // computed expression like `?cell.divide(?cid)`) is COMPUTED,
        // evaluated against the match bindings at fire time.
        // Classify the reactum by STRUCTURE, not by catching a lowering error: a
        // pure template (controls/sites/links) is a STRUCTURAL rewrite (prism's
        // native `instantiate` — MAPK-style link/rest); one containing
        // computation (`?cell.divide(?cid)`, `?f.blueprint`, arithmetic) is
        // COMPUTED, evaluated against the match at fire time. A malformed
        // structural reactum now reports a real error (the `?`) instead of
        // silently mis-routing to a broken computed one.
        let reactum = if reactum_is_structural(&def.reactum) {
            let mut reactum_bindings = RuleBindings::new();
            let pat = self.eval_pattern(&def.reactum, &rule_env, &mut reactum_bindings)?;
            Reactum::Structural {
                reactum: pat,
                instantiation: IndexMap::new(),
            }
        } else {
            Reactum::Computed(def.reactum.clone())
        };

        let rule = Rule {
            label: def.name.clone(),
            redex,
            reactum,
            guard: def.guard.clone(),
            rate: def.rate.clone(),
            bindings,
            closure: Arc::new(resolved),
        };

        Ok(Value::Foreign(prism_schema::value::Foreign::new(
            FOREIGN_RULE,
            rule,
        )))
    }

    /// Build a BRS process spec from the surface form
    /// `BRS[rules: [...]] ~{state: ...} ->{state: ...}`.
    fn build_brs_value(
        &self,
        args: &[TermArg],
        ports: &PortBindings,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut config: IndexMap<Key, Value> = IndexMap::new();
        for arg in args {
            match arg {
                TermArg::Named { name, value } => {
                    let v = self.eval_value(value, env)?;
                    config.insert(Key::from(name.as_str()), v);
                }
                TermArg::Positional(_) => {
                    return Err(EvalError::Arity {
                        control: "BRS".into(),
                        expected: "named args".into(),
                        got: "positional".into(),
                    });
                }
            }
        }
        let inputs = lower_port_bindings(&ports.inputs, self, env)?;
        let outputs = lower_port_bindings(&ports.outputs, self, env)?;

        let mut spec: IndexMap<Key, Value> = IndexMap::new();
        spec.insert("address".into(), Value::String("local:Brs".into()));
        spec.insert("config".into(), Value::Map(config));
        spec.insert("inputs".into(), Value::Map(inputs));
        spec.insert("outputs".into(), Value::Map(outputs));
        Ok(Value::Map(spec))
    }

    /// Fallback: plain `{_type: K, …}` map.
    fn build_plain_map_value(
        &self,
        control: &Name,
        args: &[TermArg],
        body: Option<&Expr>,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut map: IndexMap<Key, Value> = IndexMap::new();
        map.insert("_type".into(), Value::String(control.clone()));
        for arg in args {
            match arg {
                TermArg::Named { name, value } => {
                    let v = self.eval_value(value, env)?;
                    map.insert(Key::from(name.as_str()), v);
                }
                TermArg::Positional(_) => {
                    return Err(EvalError::Arity {
                        control: control.clone(),
                        expected: "named args".into(),
                        got: "positional".into(),
                    });
                }
            }
        }
        if let Some(body_expr) = body {
            let body_val = self.eval_value(body_expr, env)?;
            if let Value::Map(inner) = body_val {
                for (k, v) in inner {
                    map.insert(k, v);
                }
            }
        }
        Ok(Value::Map(map))
    }

    // ===============================================================
    // Pattern-context evaluation
    // ===============================================================

    /// Evaluate the top-level redex Expr to a Pattern. Auto-wraps
    /// the result in an outer `Pattern::Map` with a `_rest` Site if
    /// not already a Map, so the rule matches against a containing
    /// map of siblings (the surface-design intent).
    pub fn eval_pattern_top(
        &self,
        expr: &Expr,
        env: &IndexMap<Name, Value>,
        bindings: &mut RuleBindings,
    ) -> Result<Pattern, EvalError> {
        // Strip a top-level `where` first — its predicate is already
        // pulled into ReactionDef.guard by the AST construction
        // convention.
        let inner = match expr {
            Expr::Where { inner, .. } => inner.as_ref(),
            other => other,
        };

        // Special case: an outer-level `?name : Sort` becomes an
        // outer-key binding `?name -> Pattern(Sort)`. We auto-append a
        // `_rest` Site so additional siblings are absorbed.
        match inner {
            Expr::Site {
                name,
                sort: Some(sort),
            } => {
                // A top-level typed site `?c :: Cell[…]` binds the matched NODE as
                // a VALUE (so the reactum's `?c.divide()` / `?c.mass` dispatch on
                // the cell, and the guard reads its fields) AND consumes the matched
                // entry on fire — the reaction REPLACES the node with the reactum's
                // products, the container owning the new keys. The value is captured
                // by an as-pattern `Bind`; the binding source `Node` carries both the
                // value (from `sites`) and the consumed key (from `key_map`). One
                // binding unifies "name the node" + "the container owns the key".
                let inner_pat = self.eval_pattern(sort, env, bindings)?;
                let pat = if matches!(sort.as_ref(), Expr::Site { .. }) {
                    // Legacy key/value SPLIT `?cid : ?cell::Cell` (sort is itself a
                    // site): `?cid` keeps the OuterKey (the matched KEY); the inner
                    // site already captured the value.
                    bindings.insert(
                        name.clone(),
                        BindingSource::OuterKey(Key::from(name.as_str())),
                    );
                    inner_pat
                } else {
                    bindings
                        .insert(name.clone(), BindingSource::Node(Key::from(name.as_str())));
                    Pattern::Bind {
                        name: Key::from(name.as_str()),
                        inner: Box::new(inner_pat),
                    }
                };
                let mut map: IndexMap<Key, Pattern> = IndexMap::new();
                map.insert(Key::from(name.as_str()), pat);
                map.insert(Key::from("_rest"), Pattern::Site);
                Ok(Pattern::Map(map))
            }
            _ => self.eval_pattern(inner, env, bindings),
        }
    }

    pub fn eval_pattern(
        &self,
        expr: &Expr,
        env: &IndexMap<Name, Value>,
        bindings: &mut RuleBindings,
    ) -> Result<Pattern, EvalError> {
        match expr {
            Expr::Bool(b) => Ok(Pattern::Atom(Value::Bool(*b))),
            Expr::Int(i) => Ok(Pattern::Atom(Value::Int(*i))),
            Expr::Float(f) => Ok(Pattern::Atom(Value::float(*f))),
            Expr::Str(lit) => {
                let v = self.eval_string(lit, env)?;
                Ok(Pattern::Atom(v))
            }

            Expr::Unbound => Ok(Pattern::Absent),
            Expr::LinkVar(name) => Ok(Pattern::link_var(name.clone())),

            Expr::Site { name, sort } => {
                match sort {
                    None => {
                        // Anonymous site. The caller (a surrounding
                        // Map pattern) names the prism key; we record
                        // the binding using whatever key the caller
                        // ended up using by inserting under a
                        // placeholder. In tier-1 the surrounding
                        // builder of the Map is responsible for
                        // recording the binding correctly — here we
                        // just emit `Pattern::Site`.
                        Ok(Pattern::Site)
                    }
                    Some(sub) => {
                        // A NESTED typed site `?name::Sort` is an AS-PATTERN:
                        // match `Sort` AND bind the whole matched node to
                        // `?name` (captured as a site), so the guard/reactum
                        // can reuse it (`?cid : ?cell::Cell` → use `?cell`).
                        // The TOP-LEVEL `?name : Sort` is the key binding,
                        // handled by `eval_pattern_top`.
                        let inner = self.eval_pattern(sub, env, bindings)?;
                        bindings
                            .insert(name.clone(), BindingSource::Site(Key::from(name.as_str())));
                        Ok(Pattern::Bind {
                            name: Key::from(name.as_str()),
                            inner: Box::new(inner),
                        })
                    }
                }
            }

            Expr::Var(name) => {
                // Variable references in pattern context resolve to the
                // closure-captured value as a literal atom.
                let v = env
                    .get(name)
                    .cloned()
                    .ok_or_else(|| EvalError::UnboundVar(name.clone()))?;
                Ok(Pattern::Atom(v))
            }

            Expr::Term {
                control,
                args,
                ports,
                body,
            } => self.eval_pattern_term(control, args, ports, body.as_deref(), env, bindings),

            Expr::Parallel(elems) => {
                // Associativity normalization: a nested Parallel (e.g. a spliced
                // `pattern` param) flattens into its parent —
                // `(a | (b | c)) ≡ (a | b | c)`. This is how unquote-SPLICING
                // falls out with no sigil. Then: all KeyedEntry → Map, else List.
                let mut flat: Vec<&Expr> = Vec::new();
                flatten_parallel(elems, &mut flat);
                let all_keyed =
                    !flat.is_empty() && flat.iter().all(|e| matches!(e, Expr::KeyedEntry { .. }));
                if all_keyed {
                    let mut map: IndexMap<Key, Pattern> = IndexMap::new();
                    for e in &flat {
                        if let Expr::KeyedEntry { key, value } = e {
                            let key_str = self.eval_string_to_str(key, env)?;
                            let p = self.eval_pattern(value, env, bindings)?;
                            map.insert(Key::from(key_str.as_str()), p);
                        }
                    }
                    Ok(Pattern::Map(map))
                } else {
                    let mut items: Vec<Pattern> = Vec::new();
                    for e in &flat {
                        items.push(self.eval_pattern(e, env, bindings)?);
                    }
                    Ok(Pattern::List(items))
                }
            }

            Expr::KeyedEntry { .. } => Err(EvalError::InvalidForm {
                context: "pattern".into(),
                message: "bare KeyedEntry outside a parallel/map context".into(),
            }),

            Expr::Map(entries) => {
                let mut map: IndexMap<Key, Pattern> = IndexMap::new();
                for (key, value) in entries {
                    let key_str = self.eval_string_to_str(key, env)?;
                    let p = self.eval_pattern(value, env, bindings)?;
                    map.insert(Key::from(key_str.as_str()), p);
                }
                Ok(Pattern::Map(map))
            }

            Expr::Record(fields) => {
                let mut map: IndexMap<Key, Pattern> = IndexMap::new();
                for (name, value) in fields {
                    let p = self.eval_pattern(value, env, bindings)?;
                    map.insert(Key::from(name.as_str()), p);
                }
                Ok(Pattern::Map(map))
            }

            Expr::List(items) => {
                let mut out: Vec<Pattern> = Vec::new();
                for e in items {
                    out.push(self.eval_pattern(e, env, bindings)?);
                }
                Ok(Pattern::List(out))
            }

            Expr::Where { inner, .. } => self.eval_pattern(inner, env, bindings),

            other => Err(EvalError::InvalidForm {
                context: "pattern".into(),
                message: format!("expression not lowerable to pattern: {:?}", other),
            }),
        }
    }

    fn eval_pattern_term(
        &self,
        control: &Name,
        args: &[TermArg],
        ports: &PortBindings,
        body: Option<&Expr>,
        env: &IndexMap<Name, Value>,
        bindings: &mut RuleBindings,
    ) -> Result<Pattern, EvalError> {
        // A `pattern Name[params] (body)` reference EXPANDS here: substitute the
        // call's args for the params in the pattern body, then lower the result.
        // Splicing a parallel arg into a parallel context falls out of the
        // associativity normalization in the `Parallel` arm — no unquote sigil.
        if let Some(pdef) = self.program.entity(control).and_then(|v| v.pattern) {
            let subs = pattern_substitution(pdef, args)?;
            let expanded = substitute_vars(&pdef.body, &subs);
            return self.eval_pattern(&expanded, env, bindings);
        }

        // `K[args](body)` in pattern context → `Pattern::sort(K, …)`.
        // Args become attributes (name → pattern). The optional body
        // (a Parallel/Map) becomes child entries; ports become an
        // `outputs` entry per the prism/MAPK convention.
        let mut entries: IndexMap<Key, Pattern> = IndexMap::new();
        for arg in args {
            match arg {
                TermArg::Named { name, value } => {
                    let p = self.eval_pattern(value, env, bindings)?;
                    // If the value is a Site, record the binding under
                    // the chrysalis variable name (Pattern::Site is
                    // anonymous, but we map it through the `name` field
                    // of the Site Expr that produced it).
                    if let Expr::Site {
                        name: var_name,
                        sort: None,
                    } = value
                    {
                        bindings.insert(
                            var_name.clone(),
                            BindingSource::Site(Key::from(name.as_str())),
                        );
                    }
                    entries.insert(Key::from(name.as_str()), p);
                }
                TermArg::Positional(_) => {
                    return Err(EvalError::Arity {
                        control: control.clone(),
                        expected: "named args".into(),
                        got: "positional".into(),
                    });
                }
            }
        }

        // Link-port bindings. In a pattern, `~{}` (inputs) and `->{}`
        // (outputs) are BOTH matchable — the in/out split is a *sort* on the
        // port, not a matching boundary (a Milner link is one undirected
        // edge; the matcher binds a `~name` link var by value-equality
        // wherever it appears). A side whose ports are all `!` collapses to
        // `Absent` (a free node — matches one with no such link field at
        // all); otherwise it's a Map of port → link pattern.
        if let Some(pat) = self.link_side(&ports.inputs, env, bindings)? {
            entries.insert("inputs".into(), pat);
        }
        if let Some(pat) = self.link_side(&ports.outputs, env, bindings)? {
            entries.insert("outputs".into(), pat);
        }

        if let Some(body_expr) = body {
            // The body's elements (typically a Parallel of KeyedEntries
            // or a Map literal) become the ion's children. Merge them
            // into `entries`.
            let body_pat = self.eval_pattern(body_expr, env, bindings)?;
            if let Pattern::Map(inner) = body_pat {
                for (k, p) in inner {
                    entries.insert(k, p);
                }
            }
        }

        Ok(Pattern::sort(control.clone(), entries))
    }

    /// Lower one side of an ion's link ports (`~{}` inputs or `->{}`
    /// outputs) to a matchable pattern. Empty → no constraint on that side.
    /// All ports `!` (unbound) → `Absent` (a free node: matches one with no
    /// such link field). Otherwise a `Map` of port → link pattern (`~name`
    /// → `LinkVar`, `!` → `Absent`), which requires the link field to exist.
    fn link_side(
        &self,
        ports: &IndexMap<Name, Expr>,
        env: &IndexMap<Name, Value>,
        bindings: &mut RuleBindings,
    ) -> Result<Option<Pattern>, EvalError> {
        if ports.is_empty() {
            return Ok(None);
        }
        let mut map: IndexMap<Key, Pattern> = IndexMap::new();
        for (port, target) in ports {
            let pat = self.eval_pattern(target, env, bindings)?;
            map.insert(Key::from(port.as_str()), pat);
        }
        if map.values().all(|p| matches!(p, Pattern::Absent)) {
            Ok(Some(Pattern::Absent))
        } else {
            Ok(Some(Pattern::Map(map)))
        }
    }
}

// ===============================================================
// Helpers
// ===============================================================

/// Build the param→arg substitution for a `pattern` call. Positional args fill
/// the params left-to-right; named args bind by param name (so both
/// `InCompartment[?k, …]` and `InCompartment[kind: ?k, …]` work).
fn pattern_substitution(
    pdef: &crate::ast::PatternDef,
    args: &[TermArg],
) -> Result<IndexMap<Name, Expr>, EvalError> {
    let mut subs: IndexMap<Name, Expr> = IndexMap::new();
    let mut pos = 0usize;
    for arg in args {
        match arg {
            TermArg::Positional(e) => {
                let param = pdef.params.get(pos).ok_or_else(|| EvalError::Arity {
                    control: pdef.name.clone(),
                    expected: format!("{} pattern arg(s)", pdef.params.len()),
                    got: format!("extra positional arg #{}", pos + 1),
                })?;
                subs.insert(param.name.clone(), e.clone());
                pos += 1;
            }
            TermArg::Named { name, value } => {
                subs.insert(name.clone(), value.clone());
            }
        }
    }
    Ok(subs)
}

/// Flatten nested `Parallel`s by the `|` associativity law `(a | (b | c)) ≡
/// (a | b | c)` — how unquote-SPLICING of a pattern param falls out with no
/// sigil.
fn flatten_parallel<'e>(elems: &'e [Expr], out: &mut Vec<&'e Expr>) {
    for e in elems {
        match e {
            Expr::Parallel(inner) => flatten_parallel(inner, out),
            other => out.push(other),
        }
    }
}

/// Capture-free substitution of `Var(name)` → its bound `Expr`, recursing
/// through every `Expr` variant. The engine of `pattern` expansion: substitute
/// the call's args for the params in the pattern body, then lower the result
/// with `eval_pattern`. (Patterns are structural fragments; string-interpolation
/// keys are not themselves rewritten beyond their sub-exprs.)
fn substitute_vars(expr: &Expr, subs: &IndexMap<Name, Expr>) -> Expr {
    use Expr::*;
    match expr {
        Var(n) => subs.get(n).cloned().unwrap_or_else(|| Var(n.clone())),
        Term {
            control,
            args,
            ports,
            body,
        } => Term {
            control: control.clone(),
            args: args.iter().map(|a| subst_term_arg(a, subs)).collect(),
            ports: subst_ports(ports, subs),
            body: body.as_ref().map(|b| Box::new(substitute_vars(b, subs))),
        },
        Parallel(es) => Parallel(es.iter().map(|e| substitute_vars(e, subs)).collect()),
        KeyedEntry { key, value } => KeyedEntry {
            key: key.clone(),
            value: Box::new(substitute_vars(value, subs)),
        },
        Map(entries) => Map(entries
            .iter()
            .map(|(k, v)| (k.clone(), substitute_vars(v, subs)))
            .collect()),
        Record(fields) => Record(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), substitute_vars(v, subs)))
                .collect(),
        ),
        List(items) => List(items.iter().map(|e| substitute_vars(e, subs)).collect()),
        Site { name, sort } => Site {
            name: name.clone(),
            sort: sort.as_ref().map(|s| Box::new(substitute_vars(s, subs))),
        },
        Rule { redex, reactum } => Rule {
            redex: Box::new(substitute_vars(redex, subs)),
            reactum: Box::new(substitute_vars(reactum, subs)),
        },
        Let { bindings, body } => Let {
            bindings: bindings
                .iter()
                .map(|(n, e)| (n.clone(), substitute_vars(e, subs)))
                .collect(),
            body: Box::new(substitute_vars(body, subs)),
        },
        Block(b) => Block(crate::ast::Block {
            bindings: b
                .bindings
                .iter()
                .map(|(n, e)| (n.clone(), substitute_vars(e, subs)))
                .collect(),
            value: Box::new(substitute_vars(&b.value, subs)),
        }),
        If { cond, then_, else_ } => If {
            cond: Box::new(substitute_vars(cond, subs)),
            then_: Box::new(substitute_vars(then_, subs)),
            else_: else_.as_ref().map(|e| Box::new(substitute_vars(e, subs))),
        },
        BinOp { op, lhs, rhs } => BinOp {
            op: op.clone(),
            lhs: Box::new(substitute_vars(lhs, subs)),
            rhs: Box::new(substitute_vars(rhs, subs)),
        },
        UnaryOp { op, operand } => UnaryOp {
            op: op.clone(),
            operand: Box::new(substitute_vars(operand, subs)),
        },
        Method {
            receiver,
            method,
            args,
        } => Method {
            receiver: Box::new(substitute_vars(receiver, subs)),
            method: method.clone(),
            args: args.iter().map(|e| substitute_vars(e, subs)).collect(),
        },
        Field { base, name } => Field {
            base: Box::new(substitute_vars(base, subs)),
            name: name.clone(),
        },
        Call { func, args } => Call {
            func: Box::new(substitute_vars(func, subs)),
            args: args.iter().map(|e| substitute_vars(e, subs)).collect(),
        },
        Comprehension {
            key_var,
            var,
            source,
            filter,
            body,
            key,
        } => Comprehension {
            key_var: key_var.clone(),
            var: var.clone(),
            source: Box::new(substitute_vars(source, subs)),
            filter: filter.as_ref().map(|f| Box::new(substitute_vars(f, subs))),
            body: Box::new(substitute_vars(body, subs)),
            key: key.as_ref().map(|k| Box::new(substitute_vars(k, subs))),
        },
        ReplaceWith { id, with } => ReplaceWith {
            id: Box::new(substitute_vars(id, subs)),
            with: Box::new(substitute_vars(with, subs)),
        },
        Where { inner, predicate } => Where {
            inner: Box::new(substitute_vars(inner, subs)),
            predicate: Box::new(substitute_vars(predicate, subs)),
        },
        Unit | Bool(_) | Int(_) | Float(_) | Str(_) | Path(_) | Unbound | LinkVar(_) => {
            expr.clone()
        }
    }
}

fn subst_term_arg(arg: &TermArg, subs: &IndexMap<Name, Expr>) -> TermArg {
    match arg {
        TermArg::Positional(e) => TermArg::Positional(substitute_vars(e, subs)),
        TermArg::Named { name, value } => TermArg::Named {
            name: name.clone(),
            value: substitute_vars(value, subs),
        },
    }
}

fn subst_ports(
    ports: &crate::ast::PortBindings,
    subs: &IndexMap<Name, Expr>,
) -> crate::ast::PortBindings {
    crate::ast::PortBindings {
        inputs: ports
            .inputs
            .iter()
            .map(|(k, v)| (k.clone(), substitute_vars(v, subs)))
            .collect(),
        outputs: ports
            .outputs
            .iter()
            .map(|(k, v)| (k.clone(), substitute_vars(v, subs)))
            .collect(),
    }
}

/// Is this reactum a pure STRUCTURAL template (controls / sites / links /
/// parallels / literals), fired by prism's native `instantiate`? Or does it
/// contain COMPUTATION (a method call like `?c.divide()`, a field read like
/// `?f.blueprint`, arithmetic, `if`, …) that must be EVALUATED against the match
/// at fire time? Classifying by STRUCTURE — rather than by catching a lowering
/// error — means a malformed structural reactum reports a real error instead of
/// silently becoming a (then-broken) computed one.
fn reactum_is_structural(expr: &Expr) -> bool {
    match expr {
        Expr::Term {
            args, ports, body, ..
        } => {
            args.iter().all(|a| match a {
                TermArg::Positional(e) => reactum_is_structural(e),
                TermArg::Named { value, .. } => reactum_is_structural(value),
            }) && ports.inputs.values().all(reactum_is_structural)
                && ports.outputs.values().all(reactum_is_structural)
                && body.as_deref().map_or(true, reactum_is_structural)
        }
        Expr::Parallel(es) => es.iter().all(reactum_is_structural),
        Expr::KeyedEntry { value, .. } => reactum_is_structural(value),
        Expr::Map(entries) => entries.iter().all(|(_, v)| reactum_is_structural(v)),
        Expr::Record(fields) => fields.values().all(reactum_is_structural),
        Expr::List(items) => items.iter().all(reactum_is_structural),
        Expr::Site { sort, .. } => sort.as_deref().map_or(true, reactum_is_structural),
        Expr::LinkVar(_)
        | Expr::Unbound
        | Expr::Var(_)
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Str(_) => true,
        // Method / Call / BinOp / UnaryOp / If / Field / Comprehension /
        // ReplaceWith / Let / Block / Where / Path / Unit / Rule → COMPUTED.
        _ => false,
    }
}

#[cfg(test)]
mod reactum_classify_tests {
    use super::reactum_is_structural;
    use crate::ast::Def;
    use crate::parse::parse_program;

    fn reactum_is_structural_for(src: &str) -> bool {
        let prog = parse_program(src).expect("parse");
        match prog.lookup("R") {
            Some(Def::Reaction(r)) => reactum_is_structural(&r.reactum),
            other => panic!("expected reaction R, got {other:?}"),
        }
    }

    #[test]
    fn structural_template_reactum() {
        assert!(reactum_is_structural_for(
            "reaction R ( (a: A) => (a: B (x: ?y | rest: ?r)) )"
        ));
    }

    #[test]
    fn method_call_reactum_is_computed() {
        assert!(!reactum_is_structural_for("reaction R ( ?c => ?c.divide() )"));
    }

    #[test]
    fn field_read_reactum_is_computed() {
        assert!(!reactum_is_structural_for(
            "reaction R ( (?f :: F) => F[blueprint: ?f.blueprint] )"
        ));
    }
}

/// A first-class function value: `{_type: "Function", _name: <name>}` — a
/// by-name reference to a `def`ined function, so it can be passed to / returned
/// from / stored by other functions.
fn function_value(name: &str) -> Value {
    Value::tree([
        ("_type", Value::from("Function")),
        ("_name", Value::from(name)),
    ])
}

/// If `v` is a function value, the name of the function it references.
fn function_value_name(v: &Value) -> Option<String> {
    if v.get_field("_type").and_then(|t| t.as_str()) == Some("Function") {
        v.get_field("_name")
            .and_then(|n| n.as_str())
            .map(str::to_string)
    } else {
        None
    }
}

fn apply_binop(op: BinOp, lhs: &Value, rhs: &Value) -> Result<Value, EvalError> {
    use BinOp::*;
    // Path join: `Path / segment` with string operands → "lhs/rhs" (so the
    // Output step can write `a.csv(path / a.name)`). `/` is otherwise division.
    if matches!(op, Div) {
        if let (Value::String(l), Value::String(r)) = (lhs, rhs) {
            let joined = if l.is_empty() || l.ends_with('/') {
                format!("{l}{r}")
            } else {
                format!("{l}/{r}")
            };
            return Ok(Value::String(joined));
        }
    }
    match op {
        Add | Sub | Mul | Div => {
            let (l, r) = (lhs.as_f64(), rhs.as_f64());
            match (l, r) {
                (Some(l), Some(r)) => {
                    // If both inputs are Int, keep Int result for Add/Sub/Mul.
                    let int_inputs = matches!(lhs, Value::Int(_)) && matches!(rhs, Value::Int(_));
                    let result = match op {
                        Add => l + r,
                        Sub => l - r,
                        Mul => l * r,
                        Div => l / r,
                        _ => unreachable!(),
                    };
                    if int_inputs && matches!(op, Add | Sub | Mul) {
                        Ok(Value::Int(result as i64))
                    } else {
                        Ok(Value::float(result))
                    }
                }
                _ => Err(EvalError::TypeMismatch {
                    expected: "numeric".into(),
                    got: format!("({}, {})", value_type_name(lhs), value_type_name(rhs)),
                }),
            }
        }
        Eq => Ok(Value::Bool(lhs == rhs)),
        Ne => Ok(Value::Bool(lhs != rhs)),
        Lt | Le | Gt | Ge => {
            let (l, r) = (lhs.as_f64(), rhs.as_f64());
            match (l, r) {
                (Some(l), Some(r)) => Ok(Value::Bool(match op {
                    Lt => l < r,
                    Le => l <= r,
                    Gt => l > r,
                    Ge => l >= r,
                    _ => unreachable!(),
                })),
                _ => Err(EvalError::TypeMismatch {
                    expected: "numeric".into(),
                    got: format!("({}, {})", value_type_name(lhs), value_type_name(rhs)),
                }),
            }
        }
        And => match (lhs.as_bool(), rhs.as_bool()) {
            (Some(a), Some(b)) => Ok(Value::Bool(a && b)),
            _ => Err(EvalError::TypeMismatch {
                expected: "Bool".into(),
                got: format!("({}, {})", value_type_name(lhs), value_type_name(rhs)),
            }),
        },
        Or => match (lhs.as_bool(), rhs.as_bool()) {
            (Some(a), Some(b)) => Ok(Value::Bool(a || b)),
            _ => Err(EvalError::TypeMismatch {
                expected: "Bool".into(),
                got: format!("({}, {})", value_type_name(lhs), value_type_name(rhs)),
            }),
        },
        Concat => match (lhs, rhs) {
            // String concatenation.
            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
            // List concatenation — `xs ++ [x]` etc. (builds up collections in
            // type-method bodies, e.g. a graph's `add_node`/`add_edge`).
            (Value::List(a), Value::List(b)) => {
                let mut out = a.clone();
                out.extend(b.iter().cloned());
                Ok(Value::List(out))
            }
            _ => Err(EvalError::TypeMismatch {
                expected: "String ++ String or List ++ List".into(),
                got: format!("({}, {})", value_type_name(lhs), value_type_name(rhs)),
            }),
        },
        // `x in xs` — membership: list contains the value (by equality), or
        // map contains the key. Enables set-difference in type methods
        // (e.g. a graph's `union_with`: add nodes `not (n in self.nodes)`).
        In => match rhs {
            Value::List(items) => Ok(Value::Bool(items.contains(lhs))),
            Value::Map(m) => Ok(Value::Bool(lhs.as_str().is_some_and(|k| m.contains_key(k)))),
            _ => Err(EvalError::TypeMismatch {
                expected: "`in` expects a list or map on the right".into(),
                got: value_type_name(rhs).to_string(),
            }),
        },
    }
}

fn apply_unaryop(op: UnaryOp, operand: &Value) -> Result<Value, EvalError> {
    match op {
        UnaryOp::Neg => match operand.as_f64() {
            Some(f) => {
                if matches!(operand, Value::Int(_)) {
                    Ok(Value::Int(-(f as i64)))
                } else {
                    Ok(Value::float(-f))
                }
            }
            None => Err(EvalError::TypeMismatch {
                expected: "numeric".into(),
                got: value_type_name(operand).into(),
            }),
        },
        UnaryOp::Not => match operand.as_bool() {
            Some(b) => Ok(Value::Bool(!b)),
            None => Err(EvalError::TypeMismatch {
                expected: "Bool".into(),
                got: value_type_name(operand).into(),
            }),
        },
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{}", f.0),
        Value::Bool(b) => b.to_string(),
        Value::None => "".into(),
        other => format!("{:?}", other),
    }
}

/// Lower `PortBindings` (call-site `~{port: target}` / `->{port: target}`)
/// to the wire-spec format `discover_processes` expects:
/// `{port: Value::List([segments…])}`.
fn lower_port_bindings(
    bindings: &IndexMap<Name, Expr>,
    evaluator: &Evaluator,
    env: &IndexMap<Name, Value>,
) -> Result<IndexMap<Key, Value>, EvalError> {
    let mut out: IndexMap<Key, Value> = IndexMap::new();
    for (port, target) in bindings {
        let segments = lower_target_to_segments(target, evaluator, env)?;
        out.insert(
            Key::from(port.as_str()),
            Value::List(segments.into_iter().map(Value::String).collect()),
        );
    }
    Ok(out)
}

/// Lower a port-wiring target to its WIRE value. A `~name` link var becomes a
/// link-graph attachment `{_link: name}` — the engine resolves it by name up the
/// place graph to the nearest `link name` declaration (the shared slot), so it is
/// depth-independent and every port wired `~name` shares the one slot (the
/// value-bearing hyperedge). Every other target is a place-graph PATH wire (a
/// `List` of segments). This is where the surface `~{port: ~name}` joins the
/// engine's link graph.
fn lower_target_to_wire(
    target: &Expr,
    evaluator: &Evaluator,
    env: &IndexMap<Name, Value>,
) -> Result<Value, EvalError> {
    if let Expr::LinkVar(name) = target {
        return Ok(Value::Map(IndexMap::from([(
            Key::from("_link"),
            Value::String(name.clone()),
        )])));
    }
    let segs = lower_target_to_segments(target, evaluator, env)?;
    Ok(Value::List(segs.into_iter().map(Value::String).collect()))
}

fn lower_target_to_segments(
    expr: &Expr,
    evaluator: &Evaluator,
    env: &IndexMap<Name, Value>,
) -> Result<Vec<String>, EvalError> {
    match expr {
        Expr::Path(path) => Ok(lower_place_path(path)),
        Expr::Var(name) => Ok(vec![name.clone()]),
        // Sub-process wiring targets sometimes appear as `cells` (a Var
        // resolving by string identity to its sibling key). Same handling.
        Expr::Site { name, sort: None } => Ok(vec![name.clone()]),
        // Treat a plain string literal as a single-segment path.
        Expr::Str(lit) => {
            let s = evaluator.eval_string_to_str(lit, env)?;
            Ok(vec![s])
        }
        other => Err(EvalError::InvalidForm {
            context: "port wiring".into(),
            message: format!("not a valid wire target: {:?}", other),
        }),
    }
}

/// Seed a composite node's exported **self face**: for each output port whose
/// outer wire is `["%", field]` (the own node), copy the initial value from the
/// inner state (at the port's bridge path) onto `node[field]`. So `cells.N.mass`
/// (and `divide`, …) exist at t=0 — matchable by the parent and readable by the
/// input bridge — the Form-3 face. Single-segment `%.field` faces only.
fn seed_self_face(spec: &mut Value) {
    let Some(m) = spec.as_map() else { return };
    let outputs = m
        .get("outputs")
        .and_then(|v| v.as_map())
        .cloned()
        .unwrap_or_default();
    let config = m.get("config");
    let state = config
        .and_then(|c| c.get_field("state"))
        .cloned()
        .unwrap_or(Value::None);
    let bridge_out = config
        .and_then(|c| c.get_field("bridge"))
        .and_then(|b| b.get_field("outputs"))
        .and_then(|o| o.as_map().cloned())
        .unwrap_or_default();

    let mut to_set: Vec<(Key, Value)> = Vec::new();
    for (port, wire) in &outputs {
        let segs: Vec<String> = wire
            .as_list()
            .map(|l| l.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        // Only a single-field self face: `["%", field]`.
        if segs.len() != 2 || segs[0] != "%" {
            continue;
        }
        let inner_bridge: Vec<Key> = bridge_out
            .get(port)
            .and_then(|v| v.as_list())
            .map(|l| l.iter().filter_map(|v| v.as_str().map(Key::from)).collect())
            .unwrap_or_default();
        if let Some(val) = state.get_path(&inner_bridge) {
            to_set.push((Key::from(segs[1].as_str()), val.clone()));
        }
    }
    if let Some(m) = spec.as_map_mut() {
        for (k, v) in to_set {
            m.insert(k, v);
        }
    }
}

fn lower_place_path(path: &PlacePath) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match &path.root {
        // `%` = SELF (one meaning): the process's OWN node. The engine bases a
        // wire starting with `"%"` on the node itself, so `%.mass` → `["%","mass"]`
        // = `cells.cX.mass` (the exported face), `%` → `["%"]` = the node. (Was
        // `[]` = the container; that "bare-%-as-container" use moved to `^`.)
        PathRoot::Here => out.push("%".into()),
        PathRoot::Parent => out.push("..".into()),
        PathRoot::Local(name) => out.push(name.clone()),
    }
    for seg in &path.segments {
        out.push(seg.clone());
    }
    out
}
