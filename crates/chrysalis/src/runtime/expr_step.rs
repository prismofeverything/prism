//! `ExprStep` — a [`prism_bigraph::Step`] whose `update` body is a
//! chrysalis [`Expr`].

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::{PortSchema, Step, Update};
use prism_schema::Value;

use crate::ast::{Expr, Name, StepDef};
use crate::eval::Evaluator;
use crate::schema::lower_schema;

pub struct ExprStep {
    pub label: String,
    pub body: Expr,
    pub input_schemas: PortSchema,
    pub output_schemas: PortSchema,
    pub priority: f64,
    pub config: Arc<IndexMap<Name, Value>>,
    pub evaluator: Arc<Evaluator>,
}

impl std::fmt::Debug for ExprStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExprStep")
            .field("label", &self.label)
            .finish()
    }
}

impl ExprStep {
    pub fn from_def(
        def: &StepDef,
        resolved_config: IndexMap<Name, Value>,
        priority: f64,
        evaluator: Arc<Evaluator>,
    ) -> Self {
        let input_schemas: PortSchema = def
            .interface
            .inputs
            .iter()
            .map(|(n, d)| (n.clone(), lower_schema(&d.schema)))
            .collect();
        let output_schemas: PortSchema = def
            .interface
            .outputs
            .iter()
            .map(|(n, d)| (n.clone(), lower_schema(&d.schema)))
            .collect();
        Self {
            label: def.name.clone(),
            body: def.body.clone(),
            input_schemas,
            output_schemas,
            priority,
            config: Arc::new(resolved_config),
            evaluator,
        }
    }
}

impl Step for ExprStep {
    fn inputs(&self) -> PortSchema {
        self.input_schemas.clone()
    }

    fn outputs(&self) -> PortSchema {
        self.output_schemas.clone()
    }

    fn priority(&self) -> f64 {
        self.priority
    }

    fn update(&self, state: &Value) -> Update {
        let mut env: IndexMap<Name, Value> = (*self.config).clone();
        if let Some(map) = state.as_map() {
            for (port_name, val) in map {
                env.insert(port_name.to_string(), val.clone());
            }
        }
        match self.evaluator.eval_value(&self.body, &env) {
            Ok(value) => Update::value(value),
            Err(err) => {
                eprintln!("chrysalis ExprStep `{}` eval error: {err}", self.label);
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
