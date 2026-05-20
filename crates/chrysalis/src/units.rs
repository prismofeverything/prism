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

use prism_schema::units::{resolve_conversion, Bridge, Context, ContextRule, Dimension, Unit};

use crate::ast::{self, BinOp, Def, Expr, Program, SchemaExpr, UnaryOp, UnitExpr};

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
        match e {
            Expr::Float(_) | Expr::Int(_) | Expr::Bool(_) | Expr::Str(_) => Ok(dimensionless()),
            Expr::Var(n) => vars
                .get(n)
                .cloned()
                .ok_or_else(|| UnitError::UnknownVar(n.clone())),
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => self.infer(operand, vars, ctxs),
            Expr::UnaryOp {
                op: UnaryOp::Not, ..
            } => Ok(dimensionless()),
            Expr::BinOp { op, lhs, rhs } => {
                let a = self.infer(lhs, vars, ctxs)?;
                let b = self.infer(rhs, vars, ctxs)?;
                match op {
                    BinOp::Mul => Ok(Unit::multiplicative(
                        a.dimension.mul(&b.dimension),
                        a.scale * b.scale,
                    )),
                    BinOp::Div => Ok(Unit::multiplicative(
                        a.dimension.div(&b.dimension),
                        a.scale / b.scale,
                    )),
                    BinOp::Add | BinOp::Sub => self.require_same(op, a, b, ctxs),
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.require_same(op, a, b, ctxs)?;
                        Ok(dimensionless())
                    }
                    BinOp::And | BinOp::Or | BinOp::Concat => Ok(dimensionless()),
                }
            }
            Expr::Block(block) => {
                let mut scoped = vars.clone();
                for (name, value) in &block.bindings {
                    let u = self.infer(value, &scoped, ctxs)?;
                    scoped.insert(name.clone(), u);
                }
                self.infer(&block.value, &scoped, ctxs)
            }
            Expr::Record(fields) => {
                for (_, value) in fields {
                    self.infer(value, vars, ctxs)?;
                }
                Ok(dimensionless())
            }
            Expr::KeyedEntry { value, .. } => self.infer(value, vars, ctxs),
            _ => Err(UnitError::Unsupported(
                "expression form not handled by the unit checker".into(),
            )),
        }
    }

    /// `+`/`-`/comparison: operands must share a dimension, or a context
    /// must bridge them. Returns the left operand's unit.
    fn require_same(
        &self,
        op: &BinOp,
        a: Unit,
        b: Unit,
        ctxs: &[&Context],
    ) -> Result<Unit, UnitError> {
        if resolve_conversion(&b, &a, ctxs).is_some() {
            Ok(a)
        } else {
            Err(UnitError::DimensionMismatch {
                op: format!("{op:?}"),
                left: a.dimension,
                right: b.dimension,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::nuclear_shuttle::program;

    fn proc<'a>(p: &'a Program, name: &str) -> &'a ast::ProcessDef {
        match p.lookup(name) {
            Some(Def::Process(d)) => d,
            _ => panic!("{name} is not a process"),
        }
    }

    #[test]
    fn resolves_units_and_contexts() {
        let env = UnitEnv::from_program(&program()).unwrap();
        assert_eq!(env.units["molecule"].dimension, Dimension::base("substance"));
        assert_eq!(
            env.units["fL"].dimension,
            Dimension::base("length").pow(3)
        );
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
        vars.insert("m".into(), Unit::multiplicative(Dimension::base("mass"), 1.0));
        vars.insert("t".into(), Unit::multiplicative(Dimension::base("time"), 1.0));
        let e = Expr::add(Expr::var("m"), Expr::var("t"));
        assert!(matches!(
            env.infer(&e, &vars, &[]),
            Err(UnitError::DimensionMismatch { .. })
        ));
    }
}
