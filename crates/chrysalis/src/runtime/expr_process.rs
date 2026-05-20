//! `ExprProcess` — a [`prism_bigraph::Process`] whose `update` body
//! is a chrysalis [`Expr`].

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{PortSchema, Process, Update};
use prism_schema::{Schema, Value};

use crate::ast::{Expr, Name, ProcessDef};
use crate::eval::Evaluator;

/// A Process backed by a chrysalis expression body.
pub struct ExprProcess {
    pub label: String,
    pub body: Expr,
    pub input_schemas: PortSchema,
    pub output_schemas: PortSchema,
    pub interval: f64,
    /// Resolved configuration values (param name → Value), captured
    /// at factory invocation.
    pub config: Arc<IndexMap<Name, Value>>,
    pub evaluator: Arc<Evaluator>,
}

impl std::fmt::Debug for ExprProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExprProcess")
            .field("label", &self.label)
            .field("interval", &self.interval)
            .finish()
    }
}

impl ExprProcess {
    /// Build an `ExprProcess` from a chrysalis [`ProcessDef`] and a
    /// resolved config (param name → value).
    pub fn from_def(
        def: &ProcessDef,
        resolved_config: IndexMap<Name, Value>,
        interval: f64,
        evaluator: Arc<Evaluator>,
    ) -> Self {
        let input_schemas = lower_port_schema(def.interface.inputs.iter());
        let output_schemas = lower_port_schema(def.interface.outputs.iter());
        Self {
            label: def.name.clone(),
            body: def.body.clone(),
            input_schemas,
            output_schemas,
            interval,
            config: Arc::new(resolved_config),
            evaluator,
        }
    }
}

impl Process for ExprProcess {
    fn inputs(&self) -> PortSchema {
        self.input_schemas.clone()
    }

    fn outputs(&self) -> PortSchema {
        self.output_schemas.clone()
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        // The engine delivers state as a Value::Map keyed by input
        // port names. Bind each port to a variable of the same name
        // in the eval environment.
        //
        // Order: config first (param values), then state's port
        // entries, finally `interval` from the runtime — that last
        // assignment is canonical, since `interval` semantically IS
        // the tick width and shouldn't be overridden by a (possibly
        // missing) state slot.
        let mut env: IndexMap<Name, Value> = (*self.config).clone();
        if let Some(map) = state.as_map() {
            for (port_name, val) in map {
                env.insert(port_name.to_string(), val.clone());
            }
        }
        env.insert("interval".to_string(), Value::float(interval));

        match self.evaluator.eval_value(&self.body, &env) {
            Ok(value) => Update::value(value),
            Err(err) => {
                eprintln!(
                    "chrysalis ExprProcess `{}` eval error: {err}",
                    self.label
                );
                Update::Noop
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn lower_port_schema<'a, I>(ports: I) -> PortSchema
where
    I: Iterator<Item = (&'a Name, &'a crate::ast::PortDecl)>,
{
    ports
        .map(|(name, decl)| (name.clone(), lower_schema(&decl.schema)))
        .collect()
}

pub(crate) fn lower_schema(s: &crate::ast::SchemaExpr) -> Schema {
    match s {
        crate::ast::SchemaExpr::Any => Schema::Any,
        crate::ast::SchemaExpr::Bool => Schema::Bool { default: None },
        crate::ast::SchemaExpr::Int => Schema::Integer { default: None },
        crate::ast::SchemaExpr::Float => Schema::Float { default: None },
        crate::ast::SchemaExpr::String => Schema::String { default: None },
        crate::ast::SchemaExpr::Map(inner) => Schema::Map {
            value: Box::new(lower_schema(inner)),
        },
        crate::ast::SchemaExpr::List(inner) => Schema::List {
            element: Box::new(lower_schema(inner)),
        },
        // Tier-1 placeholder for nominal / self types; the engine
        // tolerates Any. Tier-2 will resolve these against the type
        // registry.
        crate::ast::SchemaExpr::Custom { .. } => Schema::Any,
        crate::ast::SchemaExpr::SelfType => Schema::Any,
        // Unit *scale* is erased: a Quantity's runtime value is a bare Float
        // magnitude (the dimensional check is the chrysalis check phase).
        // EXTENSIVITY survives, though — it's a divide-time property, so an
        // extensive quantity lowers to `Delta` (additive; halves on divide)
        // and an intensive one to `Float` (shares). See `divide_by_schema`.
        crate::ast::SchemaExpr::Quantity { extensive, .. } => {
            if *extensive {
                Schema::Delta { default: None }
            } else {
                Schema::Float { default: None }
            }
        }
        crate::ast::SchemaExpr::Array { shape, element } => Schema::Array {
            shape: shape.clone(),
            element: Box::new(lower_schema(element)),
        },
    }
}
