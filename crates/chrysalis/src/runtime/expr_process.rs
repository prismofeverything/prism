//! `ExprProcess` — a [`prism_bigraph::Process`] whose `update` body
//! is a chrysalis [`Expr`].

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{PortSchema, Process, Update};
use prism_schema::Value;

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
        // Lower port types PROGRAM-AWARE (the one real lowering): a port
        // `cells :: map[Cell]` must become `Map{CompositeLink}`, not an opaque
        // `Map{Custom{"Cell"}}`. The latter is what the program-unaware lowering
        // produced — and the engine then PROMOTED it over the slot's declared
        // `Map{CompositeLink}`, degrading it so `divide_by_schema` saw an unknown
        // `Custom` and shared (mass never halved). Same schema source as the rest
        // of the system (`schema::lower_schema_in_program`).
        let input_schemas = lower_port_schema(def.interface.inputs.iter(), &evaluator.program);
        let output_schemas = lower_port_schema(def.interface.outputs.iter(), &evaluator.program);
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
                eprintln!("chrysalis ExprProcess `{}` eval error: {err}", self.label);
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

fn lower_port_schema<'a, I>(ports: I, program: &crate::ast::Program) -> PortSchema
where
    I: Iterator<Item = (&'a Name, &'a crate::ast::PortDecl)>,
{
    ports
        .map(|(name, decl)| {
            (
                name.clone(),
                crate::schema::lower_schema_in_program(&decl.schema, program),
            )
        })
        .collect()
}
