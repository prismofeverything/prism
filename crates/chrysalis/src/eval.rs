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

use std::sync::Arc;

use indexmap::IndexMap;
use thiserror::Error;

use prism_schema::{value_type_name, Key, MethodError, MethodRegistry, Pattern, Value};

use crate::ast::{
    BinOp, Block, Def, Expr, Name, PathRoot, PlacePath, PortBindings, Program, ReactionDef,
    StringLit, StringSeg, TermArg, UnaryOp,
};
use crate::runtime::brs::{BrsConfig, BrsMode, FOREIGN_RULE};
use crate::runtime::rule::{BindingSource, Rule, RuleBindings};

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
}

impl Evaluator {
    pub fn new(program: Arc<Program>, methods: Arc<MethodRegistry>) -> Self {
        Self { program, methods }
    }

    // ===============================================================
    // Value-context evaluation
    // ===============================================================

    pub fn eval_value(
        &self,
        expr: &Expr,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        match expr {
            Expr::Unit => Ok(Value::None),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::Float(f) => Ok(Value::float(*f)),
            Expr::Str(lit) => self.eval_string(lit, env),

            Expr::Var(name) | Expr::Site { name, .. } => env
                .get(name)
                .cloned()
                .ok_or_else(|| EvalError::UnboundVar(name.clone())),

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

            Expr::Method { receiver, method, args } => {
                let recv = self.eval_value(receiver, env)?;
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval_value(a, env))
                    .collect::<Result<_, _>>()?;
                Ok(self.methods.dispatch(&recv, method, &arg_vals)?)
            }

            Expr::ReplaceWith { id, with } => {
                let id_val = self.eval_value(id, env)?;
                let with_val = self.eval_value(with, env)?;
                let mut out: IndexMap<Key, Value> = IndexMap::new();
                out.insert("_remove".into(), Value::List(vec![id_val]));
                out.insert("_add".into(), with_val);
                Ok(Value::Map(out))
            }

            Expr::Term { control, args, ports, body } => {
                self.eval_term_value(control, args, ports, body.as_deref(), env)
            }

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

    fn eval_block(
        &self,
        block: &Block,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
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

    fn eval_path(
        &self,
        path: &PlacePath,
        env: &IndexMap<Name, Value>,
    ) -> Result<Value, EvalError> {
        // Tier 1: only the local-relative root is supported; @ and ^
        // are valid syntactic forms but their lowering depends on the
        // composite context, which the engine resolves via wire
        // resolution at instantiation time. They appear in interface
        // wirings, not in expression bodies that eval_value evaluates.
        match &path.root {
            PathRoot::Local(name) => {
                let mut current = env
                    .get(name)
                    .cloned()
                    .ok_or_else(|| EvalError::UnboundVar(name.clone()))?;
                for seg in &path.segments {
                    current = current
                        .get_field(seg)
                        .cloned()
                        .unwrap_or(Value::None);
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
        let all_keyed = !elems.is_empty()
            && elems.iter().all(|e| matches!(e, Expr::KeyedEntry { .. }));
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
                self.build_pure_spec(
                    control,
                    args,
                    ports,
                    &def.params,
                    &def.interface,
                    env,
                )
            }
            Some(Def::Step(step_def)) => {
                let def = step_def.clone();
                self.build_pure_spec(
                    control,
                    args,
                    ports,
                    &def.params,
                    &def.interface,
                    env,
                )
            }
            Some(Def::Reaction(reaction_def)) => {
                let def = reaction_def.clone();
                self.build_reaction_value(&def, args, env)
            }
            Some(Def::Pattern(_))
            | Some(Def::Unit(_))
            | Some(Def::Context(_))
            | Some(Def::Binding { .. }) => Err(EvalError::InvalidForm {
                context: "value-term".into(),
                message: format!("control `{}` is not callable in value context", control),
            }),
            None => match control.as_str() {
                "BRS" => self.build_brs_value(args, ports, env),
                _ => self.build_plain_map_value(control, args, body, env),
            },
        }
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
        let resolved = self.resolve_args_against_params(&def.name, args, &def.params, env)?;
        let spec = self.build_spec_value(
            &def.name,
            &resolved,
            ports,
            &def.interface,
            env,
        )?;

        let mut outer: IndexMap<Key, Value> = IndexMap::new();
        outer.insert("_type".into(), Value::String(def.name.clone()));

        // Data slot for each output port: initial value comes from the
        // matching param if any, else None.
        for port in def.interface.outputs.keys() {
            let initial = resolved.get(port).cloned().unwrap_or(Value::None);
            outer.insert(Key::from(port.as_str()), initial);
        }
        // Data slot for each input port too (consumers of the composite
        // may write here).
        for port in def.interface.inputs.keys() {
            if !outer.contains_key(port.as_str()) {
                let initial = resolved.get(port).cloned().unwrap_or(Value::None);
                outer.insert(Key::from(port.as_str()), initial);
            }
        }

        outer.insert("_process".into(), spec);
        Ok(Value::Map(outer))
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

        let rule = Rule {
            label: def.name.clone(),
            redex,
            reactum: def.reactum.clone(),
            guard: def.guard.clone(),
            rate: def.rate.clone(),
            instantiation: IndexMap::new(),
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
        spec.insert(
            "address".into(),
            Value::String("local:ChrysalisBrs".into()),
        );
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
                        let pat = self.eval_pattern(sub, env, bindings)?;
                        bindings.insert(
                            name.clone(),
                            BindingSource::OuterKey(Key::from(name.as_str())),
                        );
                        // Return a one-entry Map where the redex key is the
                        // chrysalis variable name and the value is the
                        // sub-pattern. Auto-append `_rest` site.
                        let mut map: IndexMap<Key, Pattern> = IndexMap::new();
                        map.insert(Key::from(name.as_str()), pat);
                        map.insert(Key::from("_rest"), Pattern::Site);
                        Ok(Pattern::Map(map))
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

            Expr::Term { control, args, ports, body } => {
                self.eval_pattern_term(control, args, ports, body.as_deref(), env, bindings)
            }

            Expr::Parallel(elems) => {
                // All KeyedEntry → Pattern::Map; otherwise List.
                let all_keyed = !elems.is_empty()
                    && elems
                        .iter()
                        .all(|e| matches!(e, Expr::KeyedEntry { .. }));
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
                    if let Expr::Site { name: var_name, sort: None } = value {
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

        if !ports.outputs.is_empty() {
            // Build the link-graph `outputs` map: port_name → Pattern.
            // Targets must be link expressions (LinkVar / Unbound).
            let mut outputs_map: IndexMap<Key, Pattern> = IndexMap::new();
            for (port, target) in &ports.outputs {
                let pat = self.eval_pattern(target, env, bindings)?;
                outputs_map.insert(Key::from(port.as_str()), pat);
            }
            entries.insert("outputs".into(), Pattern::Map(outputs_map));
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
}

// ===============================================================
// Helpers
// ===============================================================

fn apply_binop(op: BinOp, lhs: &Value, rhs: &Value) -> Result<Value, EvalError> {
    use BinOp::*;
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
        Concat => match (lhs.as_str(), rhs.as_str()) {
            (Some(a), Some(b)) => Ok(Value::String(format!("{a}{b}"))),
            _ => Err(EvalError::TypeMismatch {
                expected: "String".into(),
                got: format!("({}, {})", value_type_name(lhs), value_type_name(rhs)),
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
            Value::List(
                segments
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
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

/// Compile a chrysalis [`BrsConfig`] from a config Value. Used by the
/// `ChrysalisBrs` factory.
pub fn brs_config_from_value(config: &Value) -> Result<BrsConfig, EvalError> {
    let map = config.as_map().ok_or_else(|| EvalError::InvalidForm {
        context: "BRS config".into(),
        message: "expected Map".into(),
    })?;
    let mut rules: Vec<Rule> = Vec::new();
    if let Some(rules_val) = map.get("rules") {
        if let Some(list) = rules_val.as_list() {
            for item in list {
                if let Value::Foreign(f) = item {
                    if f.type_name == FOREIGN_RULE {
                        if let Some(rule) = f.downcast_ref::<Rule>() {
                            rules.push(rule.clone());
                            continue;
                        }
                    }
                }
                return Err(EvalError::InvalidForm {
                    context: "BRS config".into(),
                    message: "rules list must contain ChrysalisRule values".into(),
                });
            }
        }
    }
    let mode = map
        .get("mode")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "stochastic" => BrsMode::Stochastic,
            _ => BrsMode::Deterministic,
        })
        .unwrap_or(BrsMode::Deterministic);
    let seed = map
        .get("seed")
        .and_then(|v| v.as_i64())
        .map(|i| i as u64)
        .unwrap_or(0);
    let interval = map
        .get("interval")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let max_per_tick = map
        .get("max_per_tick")
        .and_then(|v| v.as_i64())
        .map(|i| i as usize)
        .unwrap_or(1);
    Ok(BrsConfig {
        rules,
        mode,
        seed,
        interval,
        max_per_tick,
    })
}
