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
}

impl Evaluator {
    pub fn new(program: Arc<Program>, methods: Arc<MethodRegistry>) -> Self {
        Self {
            program,
            methods,
            imports: IndexMap::new(),
            imported_processes: HashSet::new(),
        }
    }

    /// Like [`Evaluator::new`], but seeded with native host imports resolved
    /// from a `ModuleRegistry` (objects bound by name; process names recognised
    /// as wholesale native controls).
    pub fn with_native_imports(
        program: Arc<Program>,
        methods: Arc<MethodRegistry>,
        imports: IndexMap<Name, Value>,
        imported_processes: HashSet<Name>,
    ) -> Self {
        Self {
            program,
            methods,
            imports,
            imported_processes,
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
                    Ok(v.clone())
                } else if matches!(
                    self.program.lookup(name),
                    Some(crate::ast::Def::Function(_))
                ) {
                    // A bare reference to a `def`ined function → a first-class
                    // function value (passable to / returnable from functions).
                    Ok(function_value(name))
                } else if matches!(
                    self.program.lookup(name),
                    Some(
                        crate::ast::Def::Composite(_)
                            | crate::ast::Def::Process(_)
                            | crate::ast::Def::Step(_)
                    )
                ) {
                    // A bare reference to a composite/process/step definer → its
                    // no-arg instantiation (the composite-as-data spec), so
                    // `all` ≡ `all[]`. A definer needing args reports the missing
                    // arg — still informative, and signals it's a definer rather
                    // than "unbound".
                    self.eval_value(
                        &Expr::Term {
                            control: name.clone(),
                            args: vec![],
                            ports: crate::ast::PortBindings::default(),
                            body: None,
                        },
                        env,
                    )
                } else {
                    Err(EvalError::UnboundVar(name.clone()))
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
                var,
                source,
                filter,
                body,
            } => {
                let source_val = self.eval_value(source, env)?;
                let Value::List(items) = source_val else {
                    return Err(EvalError::InvalidForm {
                        context: "comprehension".into(),
                        message: format!(
                            "`for {var} in …` expects a list, got {}",
                            value_type_name(&source_val)
                        ),
                    });
                };
                let mut out: Vec<Value> = Vec::new();
                for item in items {
                    let mut scope = env.clone();
                    scope.insert(var.clone(), item);
                    let keep = match filter {
                        Some(pred) => {
                            matches!(self.eval_value(pred, &scope)?, Value::Bool(true))
                        }
                        None => true,
                    };
                    if keep {
                        out.push(self.eval_value(body, &scope)?);
                    }
                }
                Ok(Value::List(out))
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
        match self.program.lookup(control) {
            Some(Def::Composite(composite_def)) => {
                let def = composite_def.clone();
                self.build_composite_outer(&def, args, ports, env)
            }
            Some(Def::Process(process_def)) => {
                let def = process_def.clone();
                self.build_pure_spec(control, args, ports, &def.params, &def.interface, env)
            }
            Some(Def::Step(step_def)) => {
                let def = step_def.clone();
                self.build_pure_spec(control, args, ports, &def.params, &def.interface, env)
            }
            Some(Def::Reaction(reaction_def)) => {
                let def = reaction_def.clone();
                self.build_reaction_value(&def, args, env)
            }
            Some(Def::Protocol(protocol_def)) => {
                let pd = protocol_def.clone();
                self.build_protocol_outer(&pd, args, ports, env)
            }
            Some(Def::Function(_)) => Err(EvalError::InvalidForm {
                context: "value-term".into(),
                message: format!(
                    "`{control}` is a function — call it as `{control}(args)`, not `{control}[args]`"
                ),
            }),
            Some(Def::Pattern(_))
            | Some(Def::Unit(_))
            | Some(Def::Context(_))
            | Some(Def::Type(_))
            | Some(Def::Contract(_))
            | Some(Def::Import { .. })
            | Some(Def::Use { .. })
            | Some(Def::Binding { .. }) => Err(EvalError::InvalidForm {
                context: "value-term".into(),
                message: format!("control `{}` is not callable in value context", control),
            }),
            None if self.imported_processes.contains(control) => {
                // A wholesale native process import (`from core import …`):
                // no Def, no declared interface — wire straight from the call.
                self.build_native_spec(control, args, ports, env)
            }
            None => match control.as_str() {
                "BRS" => self.build_brs_value(args, ports, env),
                _ => self.build_plain_map_value(control, args, body, env),
            },
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
        let wrapped = match self.program.lookup(&pd.wrapped) {
            Some(Def::Composite(c)) => c.clone(),
            _ => {
                return Err(EvalError::InvalidForm {
                    context: "protocol".into(),
                    message: format!(
                        "`protocol {} = {}<{}, …>`: `{}` must be a composite in scope",
                        pd.name, pd.protocol, pd.wrapped, pd.wrapped
                    ),
                });
            }
        };
        let mut node = self.build_composite_outer(&wrapped, args, ports, env)?;
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

    /// Build an outer-map for a composite call site.
    ///
    /// Shape:
    /// ```text
    /// {
    ///   _type:    "<Control>",
    ///   <slot>:    <data slot value>,    // one per output port
    ///   <slot>:    <data slot value>,    // one per input port
    ///   _process: { address, config, inputs, outputs }
    /// }
    /// ```
    ///
    /// The composite's bridge connects its inner state to these outer
    /// data slots: each output port's internal value flows out to the
    /// sibling slot, where the surrounding pattern matcher / BRS can
    /// observe it.
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
        let resolved = self.resolve_args_against_params(&def.name, args, &def.params, env)?;
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
            &IndexMap::new(),
            ports,
            &def.interface,
            env,
        )?;
        if let Value::Map(m) = &mut spec {
            m.insert(Key::from("config"), config);
        }
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
                let resolved =
                    self.resolve_args_against_params(&def.name, args, &def.params, env)?;
                // The composite body sees the outer (top-level) bindings, with
                // its own params shadowing them — so a shared `network :: CRN =
                // …` resolves inside the body.
                let mut body_env = env.clone();
                body_env.extend(resolved);
                return self.eval_value(&def.body, &body_env);
            }
        }
        self.eval_value(expr, env)
    }

    /// Build a "pure" process / step spec — a `{address, config, inputs,
    /// outputs}` map with no outer-map wrap. Used for Process and Step
    /// definers and for built-ins like BRS, which have no observable
    /// data slots of their own.
    fn build_pure_spec(
        &self,
        control: &Name,
        args: &[TermArg],
        ports: &PortBindings,
        params: &[crate::ast::Param],
        interface: &crate::ast::Interface,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let resolved = self.resolve_args_against_params(control, args, params, env)?;
        self.build_spec_value(control, &resolved, ports, interface, env)
    }

    /// Build just the `{address, config, inputs, outputs}` spec map.
    fn build_spec_value(
        &self,
        control: &Name,
        resolved_config: &IndexMap<Name, Value>,
        ports: &PortBindings,
        interface: &crate::ast::Interface,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        let mut inputs_map: IndexMap<Key, Value> = IndexMap::new();
        for port_name in interface.inputs.keys() {
            let wire_segments = match ports.inputs.get(port_name) {
                Some(target) => lower_target_to_segments(target, self, env)?,
                None => vec![port_name.clone()],
            };
            inputs_map.insert(
                Key::from(port_name.as_str()),
                Value::List(wire_segments.into_iter().map(Value::String).collect()),
            );
        }
        let mut outputs_map: IndexMap<Key, Value> = IndexMap::new();
        for port_name in interface.outputs.keys() {
            let wire_segments = match ports.outputs.get(port_name) {
                Some(target) => lower_target_to_segments(target, self, env)?,
                None => vec![port_name.clone()],
            };
            outputs_map.insert(
                Key::from(port_name.as_str()),
                Value::List(wire_segments.into_iter().map(Value::String).collect()),
            );
        }

        let config_map: IndexMap<Key, Value> = resolved_config
            .iter()
            .map(|(k, v)| (Key::from(k.as_str()), v.clone()))
            .collect();

        let mut spec: IndexMap<Key, Value> = IndexMap::new();
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
        let reactum = {
            let mut reactum_bindings = RuleBindings::new();
            match self.eval_pattern(&def.reactum, &rule_env, &mut reactum_bindings) {
                Ok(pat) => Reactum::Structural {
                    reactum: pat,
                    instantiation: IndexMap::new(),
                },
                Err(_) => Reactum::Computed(def.reactum.clone()),
            }
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
                let pat = self.eval_pattern(sort, env, bindings)?;
                bindings.insert(
                    name.clone(),
                    BindingSource::OuterKey(Key::from(name.as_str())),
                );
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
                // All KeyedEntry → Pattern::Map; otherwise List.
                let all_keyed =
                    !elems.is_empty() && elems.iter().all(|e| matches!(e, Expr::KeyedEntry { .. }));
                if all_keyed {
                    let mut map: IndexMap<Key, Pattern> = IndexMap::new();
                    for e in elems {
                        if let Expr::KeyedEntry { key, value } = e {
                            let key_str = self.eval_string_to_str(key, env)?;
                            let p = self.eval_pattern(value, env, bindings)?;
                            map.insert(Key::from(key_str.as_str()), p);
                        }
                    }
                    Ok(Pattern::Map(map))
                } else {
                    let mut items: Vec<Pattern> = Vec::new();
                    for e in elems {
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

fn lower_place_path(path: &PlacePath) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match &path.root {
        PathRoot::Here => {} // empty — relative to self
        PathRoot::Parent => out.push("..".into()),
        PathRoot::Local(name) => out.push(name.clone()),
    }
    for seg in &path.segments {
        out.push(seg.clone());
    }
    out
}
