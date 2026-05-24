//! Dimensional check + unit lowering — the "check" half of
//! "check once, erase, run raw" (see docs/chrysalis-design.md, "Units
//! and quantities").
//!
//! [`UnitEnv::from_program`] resolves a program's `unit` and `context`
//! declarations — over the built-in SI base units — into the runtime
//! forms in [`prism_schema::units`]. [`UnitEnv::infer`] then walks an
//! expression assigning each subexpression a [`Unit`], checking that
//! `+`/`-`/comparisons agree dimensionally (consulting in-scope contexts
//! for cross-dimension coercions) and that `*`/`/` compose. Mismatches
//! are [`UnitError`]s — illegal programs rejected before they run.
//!
//! This is the phase that *erases*: once it succeeds, magnitudes are
//! bare `f64` and units never reach the engine.

use std::collections::HashMap;

use indexmap::IndexMap;

use prism_schema::units::{
    Bridge, Context, ContextRule, Conversion, Dimension, StateOp, Unit, resolve_conversion,
};

use crate::ast::{self, BinOp, Block, Def, Expr, Program, SchemaExpr, UnaryOp, UnitExpr};

#[derive(Clone, Debug, PartialEq)]
pub enum UnitError {
    UnknownUnit(String),
    UnknownVar(String),
    DimensionMismatch {
        op: String,
        left: Dimension,
        right: Dimension,
    },
    Unsupported(String),
}

fn dimensionless() -> Unit {
    Unit::multiplicative(Dimension::dimensionless(), 1.0)
}

/// SI base units, scale 1.0 in their own dimension.
fn builtin_units() -> HashMap<String, Unit> {
    let base = |d: &str| Unit::multiplicative(Dimension::base(d), 1.0);
    [
        ("kg", "mass"),
        ("m", "length"),
        ("s", "time"),
        ("mol", "substance"),
        ("K", "temperature"),
        ("A", "current"),
        ("cd", "luminous"),
    ]
    .into_iter()
    .map(|(u, d)| (u.to_string(), base(d)))
    .collect()
}

/// Lower an AST [`UnitExpr`] to a resolved [`Unit`] against known units.
fn lower_unit_expr(e: &UnitExpr, units: &HashMap<String, Unit>) -> Result<Unit, UnitError> {
    match e {
        UnitExpr::Named(n) => units
            .get(n)
            .cloned()
            .ok_or_else(|| UnitError::UnknownUnit(n.clone())),
        UnitExpr::Scalar(f) => Ok(Unit::multiplicative(Dimension::dimensionless(), *f)),
        UnitExpr::Mul(a, b) => {
            let a = lower_unit_expr(a, units)?;
            let b = lower_unit_expr(b, units)?;
            Ok(Unit::multiplicative(
                a.dimension.mul(&b.dimension),
                a.scale * b.scale,
            ))
        }
        UnitExpr::Div(a, b) => {
            let a = lower_unit_expr(a, units)?;
            let b = lower_unit_expr(b, units)?;
            Ok(Unit::multiplicative(
                a.dimension.div(&b.dimension),
                a.scale / b.scale,
            ))
        }
        UnitExpr::Pow(a, r) => {
            if r.den != 1 {
                return Err(UnitError::Unsupported(
                    "rational exponent in unit expression".into(),
                ));
            }
            let a = lower_unit_expr(a, units)?;
            Ok(Unit::multiplicative(
                a.dimension.pow(r.num as i64),
                a.scale.powi(r.num),
            ))
        }
    }
}

fn lower_dimension(d: &ast::Dimension) -> Result<Dimension, UnitError> {
    let mut out = Dimension::dimensionless();
    for (name, r) in &d.powers {
        if r.den != 1 {
            return Err(UnitError::Unsupported(
                "rational exponent in dimension".into(),
            ));
        }
        out = out.mul(&Dimension::base(name).pow(r.num as i64));
    }
    Ok(out)
}

/// Recognize the normalized shape of a context transform: `value / param`,
/// `value * param`, or `value * const` / `value / const`.
fn lower_bridge(transform: &Expr) -> Result<Bridge, UnitError> {
    let is_value = |e: &Expr| matches!(e, Expr::Var(n) if n == "value");
    let shape_err = || UnitError::Unsupported("unsupported context transform shape".into());
    match transform {
        Expr::BinOp {
            op: BinOp::Div,
            lhs,
            rhs,
        } if is_value(lhs) => match rhs.as_ref() {
            Expr::Var(p) => Ok(Bridge::DivByParam(p.clone())),
            Expr::Float(k) => Ok(Bridge::ScaleConst(1.0 / k)),
            _ => Err(shape_err()),
        },
        Expr::BinOp {
            op: BinOp::Mul,
            lhs,
            rhs,
        } => match (lhs.as_ref(), rhs.as_ref()) {
            (l, Expr::Var(p)) if is_value(l) => Ok(Bridge::MulByParam(p.clone())),
            (Expr::Var(p), r) if is_value(r) => Ok(Bridge::MulByParam(p.clone())),
            (l, Expr::Float(k)) if is_value(l) => Ok(Bridge::ScaleConst(*k)),
            _ => Err(shape_err()),
        },
        _ => Err(shape_err()),
    }
}

fn lower_context(cd: &ast::ContextDef) -> Result<Context, UnitError> {
    let mut rules = Vec::new();
    for r in &cd.rules {
        rules.push(ContextRule {
            from: lower_dimension(&r.from)?,
            to: lower_dimension(&r.to)?,
            bidirectional: r.bidirectional,
            bridge: lower_bridge(&r.transform)?,
        });
    }
    Ok(Context {
        name: cd.name.clone(),
        rules,
    })
}

/// Resolved unit + context environment for a program.
pub struct UnitEnv {
    pub units: HashMap<String, Unit>,
    pub contexts: HashMap<String, Context>,
}

impl UnitEnv {
    /// Resolve all `unit`/`context` declarations over the SI base units.
    /// Declarations are processed in order, so a unit may be defined in
    /// terms of earlier ones (`fL = um^3`).
    pub fn from_program(p: &Program) -> Result<UnitEnv, UnitError> {
        let mut units = builtin_units();
        let mut contexts = HashMap::new();
        for def in &p.defs {
            match def {
                Def::Unit(ud) => {
                    let resolved = lower_unit_expr(&ud.definition, &units)?;
                    let unit = match ud.affine_offset {
                        Some(offset) => Unit::affine(resolved.dimension, resolved.scale, offset),
                        None => resolved,
                    };
                    units.insert(ud.name.clone(), unit);
                }
                Def::Context(cd) => {
                    contexts.insert(cd.name.clone(), lower_context(cd)?);
                }
                _ => {}
            }
        }
        Ok(UnitEnv { units, contexts })
    }

    /// The unit of a schema slot: `Quantity` carries one; everything else
    /// is dimensionless for the purpose of the dimensional check.
    pub fn unit_of_schema(&self, s: &SchemaExpr) -> Result<Unit, UnitError> {
        match s {
            SchemaExpr::Quantity { unit, .. } => lower_unit_expr(unit, &self.units),
            // An array carries its element's dimension element-wise — a
            // field of concentrations *is* a concentration dimensionally.
            SchemaExpr::Array { element, .. } => self.unit_of_schema(element),
            _ => Ok(dimensionless()),
        }
    }

    /// Variable→unit map for a process/step body: config params + input
    /// ports.
    pub fn vars_for(
        &self,
        params: &[ast::Param],
        interface: &ast::Interface,
    ) -> Result<HashMap<String, Unit>, UnitError> {
        let mut vars = HashMap::new();
        for p in params {
            vars.insert(p.name.clone(), self.unit_of_schema(&p.schema)?);
        }
        for (name, decl) in &interface.inputs {
            vars.insert(name.clone(), self.unit_of_schema(&decl.schema)?);
        }
        Ok(vars)
    }

    /// Infer the unit of an expression, checking dimensional consistency.
    /// `ctxs` are the contexts in scope (from enclosing `using` clauses).
    pub fn infer(
        &self,
        e: &Expr,
        vars: &HashMap<String, Unit>,
        ctxs: &[&Context],
    ) -> Result<Unit, UnitError> {
        self.lower_expr(e, vars, ctxs).map(|(_, u)| u)
    }

    /// Erase units: rewrite a checked body into a unit-free expression
    /// with conversions baked in as ordinary arithmetic — a scale
    /// constant for a same-dimension unit mismatch, or `*`/`/` by a
    /// context factor (e.g. a compartment volume) for a cross-dimension
    /// coercion. The result runs on bare `f64` in [`crate::eval`]; units
    /// never reach the engine.
    pub fn lower_body(
        &self,
        e: &Expr,
        vars: &HashMap<String, Unit>,
        ctxs: &[&Context],
    ) -> Result<Expr, UnitError> {
        self.lower_expr(e, vars, ctxs).map(|(e, _)| e)
    }

    /// Shared engine for [`infer`](Self::infer) and
    /// [`lower_body`](Self::lower_body): returns the erased expression
    /// together with its inferred unit.
    fn lower_expr(
        &self,
        e: &Expr,
        vars: &HashMap<String, Unit>,
        ctxs: &[&Context],
    ) -> Result<(Expr, Unit), UnitError> {
        match e {
            Expr::Float(_) | Expr::Int(_) | Expr::Bool(_) | Expr::Str(_) => {
                Ok((e.clone(), dimensionless()))
            }
            Expr::Var(n) => {
                let u = vars
                    .get(n)
                    .cloned()
                    .ok_or_else(|| UnitError::UnknownVar(n.clone()))?;
                Ok((e.clone(), u))
            }
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => {
                let (le, u) = self.lower_expr(operand, vars, ctxs)?;
                Ok((Expr::neg(le), u))
            }
            Expr::UnaryOp {
                op: UnaryOp::Not,
                operand,
            } => {
                let (le, _) = self.lower_expr(operand, vars, ctxs)?;
                Ok((
                    Expr::UnaryOp {
                        op: UnaryOp::Not,
                        operand: Box::new(le),
                    },
                    dimensionless(),
                ))
            }
            Expr::BinOp { op, lhs, rhs } => {
                let (la, ua) = self.lower_expr(lhs, vars, ctxs)?;
                let (lb, ub) = self.lower_expr(rhs, vars, ctxs)?;
                match op {
                    BinOp::Mul => Ok((
                        Expr::mul(la, lb),
                        Unit::multiplicative(ua.dimension.mul(&ub.dimension), ua.scale * ub.scale),
                    )),
                    BinOp::Div => Ok((
                        Expr::div(la, lb),
                        Unit::multiplicative(ua.dimension.div(&ub.dimension), ua.scale / ub.scale),
                    )),
                    BinOp::Add | BinOp::Sub => {
                        let lb = self.coerce(lb, &ub, &ua, op, ctxs)?;
                        Ok((binop(*op, la, lb), ua))
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        let lb = self.coerce(lb, &ub, &ua, op, ctxs)?;
                        Ok((binop(*op, la, lb), dimensionless()))
                    }
                    BinOp::And | BinOp::Or | BinOp::Concat | BinOp::In => {
                        Ok((binop(*op, la, lb), dimensionless()))
                    }
                }
            }
            Expr::Block(block) => {
                let mut scoped = vars.clone();
                let mut bindings = Vec::with_capacity(block.bindings.len());
                for (name, value) in &block.bindings {
                    let (lv, u) = self.lower_expr(value, &scoped, ctxs)?;
                    scoped.insert(name.clone(), u);
                    bindings.push((name.clone(), lv));
                }
                let (lvalue, u) = self.lower_expr(&block.value, &scoped, ctxs)?;
                Ok((Expr::Block(Block::from_parts(bindings, lvalue)), u))
            }
            Expr::Record(fields) => {
                let mut out = IndexMap::with_capacity(fields.len());
                for (name, value) in fields {
                    let (lv, _) = self.lower_expr(value, vars, ctxs)?;
                    out.insert(name.clone(), lv);
                }
                Ok((Expr::Record(out), dimensionless()))
            }
            Expr::KeyedEntry { key, value } => {
                let (lv, u) = self.lower_expr(value, vars, ctxs)?;
                Ok((
                    Expr::KeyedEntry {
                        key: key.clone(),
                        value: Box::new(lv),
                    },
                    u,
                ))
            }
            _ => Err(UnitError::Unsupported(
                "expression form not handled by the unit checker".into(),
            )),
        }
    }

    /// Convert the already-lowered `rhs` from unit `from` into `to`'s
    /// unit, baking the conversion in as arithmetic. Errors if the
    /// dimensions are unbridgeable (even via `ctxs`).
    fn coerce(
        &self,
        rhs: Expr,
        from: &Unit,
        to: &Unit,
        op: &BinOp,
        ctxs: &[&Context],
    ) -> Result<Expr, UnitError> {
        let conv =
            resolve_conversion(from, to, ctxs).ok_or_else(|| UnitError::DimensionMismatch {
                op: format!("{op:?}"),
                left: to.dimension.clone(),
                right: from.dimension.clone(),
            })?;
        Ok(apply_rewrite(rhs, conv))
    }
}

fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

/// Bake a resolved [`Conversion`] into the (unit-erased) value expression:
/// a same-dimension scale multiplies, an affine unit adds its offset, and a
/// context bridge becomes `value {/, *} <param>` (the factor supplied from
/// the named state field). Resolution itself lives in
/// `prism_schema::units::resolve_conversion` — chrysalis no longer
/// re-implements it (one resolver, one `Conversion` type).
fn apply_rewrite(e: Expr, conv: Conversion) -> Expr {
    match conv {
        Conversion::Identity => e,
        Conversion::Scale(k) => Expr::mul(e, Expr::float(k)),
        Conversion::Affine { scale, offset } => {
            Expr::add(Expr::mul(e, Expr::float(scale)), Expr::float(offset))
        }
        Conversion::ByState { op, param } => match op {
            StateOp::DivBy => Expr::div(e, Expr::var(param)),
            StateOp::MulBy => Expr::mul(e, Expr::var(param)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::fixtures::nuclear_shuttle::program;
    use prism_schema::{MethodRegistry, Value};
    use std::sync::Arc;

    fn proc<'a>(p: &'a Program, name: &str) -> &'a ast::ProcessDef {
        match p.lookup(name) {
            Some(Def::Process(d)) => d,
            _ => panic!("{name} is not a process"),
        }
    }

    #[test]
    fn resolves_units_and_contexts() {
        let env = UnitEnv::from_program(&program()).unwrap();
        assert_eq!(
            env.units["molecule"].dimension,
            Dimension::base("substance")
        );
        assert_eq!(env.units["fL"].dimension, Dimension::base("length").pow(3));
        assert!(env.contexts.contains_key("concentration"));
    }

    #[test]
    fn transport_body_is_dimensionally_consistent() {
        let prog = program();
        let env = UnitEnv::from_program(&prog).unwrap();
        let t = proc(&prog, "Transport");
        let vars = env.vars_for(&t.params, &t.interface).unwrap();
        // Whole body checks with no context (transport converts explicitly).
        assert!(env.infer(&t.body, &vars, &[]).is_ok());

        // The `flux` binding works out to [substance]:
        //   (fL/s) · (substance/length^3) · s = substance
        let Expr::Block(b) = &t.body else {
            panic!("transport body is not a block");
        };
        let mut scoped = vars.clone();
        scoped.insert(
            "cyt_conc".into(),
            env.infer(&b.bindings[0].1, &scoped, &[]).unwrap(),
        );
        scoped.insert(
            "nuc_conc".into(),
            env.infer(&b.bindings[1].1, &scoped, &[]).unwrap(),
        );
        let (name, flux) = &b.bindings[2];
        assert_eq!(name, "flux");
        let flux_unit = env.infer(flux, &scoped, &[]).unwrap();
        assert_eq!(flux_unit.dimension, Dimension::base("substance"));
    }

    #[test]
    fn sense_needs_the_enclosing_concentration_context() {
        let prog = program();
        let env = UnitEnv::from_program(&prog).unwrap();
        let s = proc(&prog, "Sense");
        let vars = env.vars_for(&s.params, &s.interface).unwrap();

        // `tf > k_on` compares a Count against a Conc: a dimension
        // mismatch on its own.
        assert!(matches!(
            env.infer(&s.body, &vars, &[]),
            Err(UnitError::DimensionMismatch { .. })
        ));

        // In the nucleus's `concentration` context, the Count coerces via
        // the volume — the comparison type-checks. Same `Sense`, correct
        // in whichever compartment scopes the context.
        let conc = env.contexts.get("concentration").unwrap();
        assert!(env.infer(&s.body, &vars, &[conc]).is_ok());
    }

    #[test]
    fn unbridgeable_dimensions_are_rejected() {
        let env = UnitEnv::from_program(&program()).unwrap();
        let mut vars = HashMap::new();
        vars.insert(
            "m".into(),
            Unit::multiplicative(Dimension::base("mass"), 1.0),
        );
        vars.insert(
            "t".into(),
            Unit::multiplicative(Dimension::base("time"), 1.0),
        );
        let e = Expr::add(Expr::var("m"), Expr::var("t"));
        assert!(matches!(
            env.infer(&e, &vars, &[]),
            Err(UnitError::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn lowered_sense_runs_unit_correct_through_eval() {
        let prog = program();
        let env = UnitEnv::from_program(&prog).unwrap();
        let s = proc(&prog, "Sense");
        let vars = env.vars_for(&s.params, &s.interface).unwrap();
        let conc = env.contexts.get("concentration").unwrap();

        // Erase: `tf > k_on` becomes `tf > k_on * volume` — k_on coerced
        // from a concentration into a count via the nucleus volume.
        let lowered = env.lower_body(&s.body, &vars, &[conc]).unwrap();

        let evalr = Evaluator::new(Arc::new(Program::new()), Arc::new(MethodRegistry::new()));
        let active = |body: &Expr, tf: f64| -> bool {
            let mut e: IndexMap<String, Value> = IndexMap::new();
            e.insert("tf".into(), Value::float(tf));
            e.insert("k_on".into(), Value::float(0.5));
            e.insert("volume".into(), Value::float(100.0));
            evalr.eval_value(body, &e).unwrap().get_field("active") == Some(&Value::Bool(true))
        };

        // 100 fL nucleus, threshold 0.5/fL ⇒ the gene fires at 50 molecules.
        assert!(active(&lowered, 60.0)); // 60/100 = 0.6 > 0.5
        assert!(!active(&lowered, 40.0)); // 40/100 = 0.4 < 0.5

        // The un-erased body compares a raw count to a concentration and
        // is wrong: 40 molecules already "exceeds" 0.5.
        assert!(active(&s.body, 40.0));
    }

    #[test]
    fn arrays_carry_their_element_units() {
        let prog = program();
        let env = UnitEnv::from_program(&prog).unwrap();

        // A concentration FIELD: Array[[2,2], Quantity[molecule/fL]] — the
        // spatio-flux shape (a grid of concentrations).
        let conc_field = SchemaExpr::array(
            vec![2, 2],
            SchemaExpr::quantity(
                UnitExpr::named("molecule").div(UnitExpr::named("fL")),
                false,
                false,
            ),
        );
        let conc_dim = Dimension::base("substance").div(&Dimension::base("length").pow(3));

        // The array's dimension is its element's, applied element-wise.
        let field_u = env.unit_of_schema(&conc_field).unwrap();
        assert_eq!(field_u.dimension, conc_dim);

        let mut vars = HashMap::new();
        vars.insert("field".to_string(), field_u);
        vars.insert(
            "rate".to_string(),
            env.unit_of_schema(&SchemaExpr::quantity(UnitExpr::per("s"), false, false))
                .unwrap(),
        );
        vars.insert(
            "interval".to_string(),
            env.unit_of_schema(&SchemaExpr::quantity(UnitExpr::named("s"), false, false))
                .unwrap(),
        );

        // field * rate * interval  →  still [substance]/[length]^3.
        let scaled = Expr::mul(
            Expr::mul(Expr::var("field"), Expr::var("rate")),
            Expr::var("interval"),
        );
        assert_eq!(env.infer(&scaled, &vars, &[]).unwrap().dimension, conc_dim);

        // field + rate  →  concentration vs 1/time: a dimension mismatch.
        let bad = Expr::add(Expr::var("field"), Expr::var("rate"));
        assert!(matches!(
            env.infer(&bad, &vars, &[]),
            Err(UnitError::DimensionMismatch { .. })
        ));
    }
}
