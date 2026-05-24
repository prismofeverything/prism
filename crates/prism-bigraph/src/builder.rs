//! Fluent builder for [`crate::Engine`].
//!
//! Bundles the setup of registries and engine state behind a single
//! chainable API. The engine itself remains the runtime context
//! (mirrors the role of upstream's `Core`); the builder is the
//! ergonomic front end.
//!
//! ## Why a builder, not a god-object
//!
//! Upstream `process-bigraph` consolidates registries onto a single
//! mutable `Core` because Python's mutable-by-default style makes that
//! ergonomic. In Rust, a build-then-freeze god-object fights with the
//! ownership model. We get the same setup ergonomics — one chainable
//! handle — without the runtime-time god object: the engine itself
//! aggregates whatever registries are needed, and they enter through
//! the builder.
//!
//! ## Example
//!
//! ```ignore
//! use prism_bigraph::Engine;
//! use prism_schema::{Schema, Value};
//!
//! let engine = Engine::builder()
//!     .schema(Schema::Any)
//!     .state(Value::None)
//!     .register_process("Grow", |_config| {
//!         /* … */ unimplemented!()
//!     })
//!     .register_method("Float", "double", |v, _args| {
//!         Ok(Value::float(v.as_f64().unwrap_or(0.0) * 2.0))
//!     })
//!     .build()
//!     .unwrap();
//! ```

use std::sync::Arc;

use prism_schema::{MethodRegistry, Schema, TypeRegistry, Value};

use crate::engine::Engine;
use crate::factory::ProcessRegistry;
use crate::process::ProcessNode;
use crate::protocol::{Protocol, ProtocolRegistry};

/// Fluent builder for [`Engine`]. Construct via [`Engine::builder`].
#[derive(Default)]
pub struct EngineBuilder {
    schema: Option<Schema>,
    state: Option<Value>,
    process_registry: Option<ProcessRegistry>,
    type_registry: Option<TypeRegistry>,
    method_registry: Option<MethodRegistry>,
    protocol_registry: Option<ProtocolRegistry>,
    /// If true, automatically discover processes from state after init.
    /// Default true. Set false for manual control.
    auto_discover: bool,
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self {
            auto_discover: true,
            ..Self::default()
        }
    }

    // ── Required state ──

    /// Set the state-tree schema. Required.
    pub fn schema(mut self, schema: Schema) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Set the initial state value. Required.
    pub fn state(mut self, state: Value) -> Self {
        self.state = Some(state);
        self
    }

    // ── Registry handles ──

    /// Replace the process registry with a pre-built one.
    pub fn process_registry(mut self, registry: ProcessRegistry) -> Self {
        self.process_registry = Some(registry);
        self
    }

    /// Replace the type registry with a pre-built one.
    pub fn type_registry(mut self, registry: TypeRegistry) -> Self {
        self.type_registry = Some(registry);
        self
    }

    /// Replace the method registry with a pre-built one.
    pub fn method_registry(mut self, registry: MethodRegistry) -> Self {
        self.method_registry = Some(registry);
        self
    }

    /// Replace the protocol registry with a pre-built one. By default
    /// only the `local` protocol is registered.
    pub fn protocol_registry(mut self, registry: ProtocolRegistry) -> Self {
        self.protocol_registry = Some(registry);
        self
    }

    /// Register an additional protocol inline. First call creates the
    /// protocol registry (defaulting to one with the `local` protocol);
    /// subsequent calls add to it.
    pub fn register_protocol(mut self, protocol: Arc<dyn Protocol>) -> Self {
        self.protocol_registry
            .get_or_insert_with(ProtocolRegistry::new)
            .register(protocol);
        self
    }

    // ── Inline registration ──

    /// Register a process factory inline. The first call creates the
    /// process registry if one wasn't supplied; subsequent calls add
    /// to it.
    pub fn register_process<F>(mut self, name: impl Into<String>, factory: F) -> Self
    where
        F: Fn(Value) -> ProcessNode + Send + Sync + 'static,
    {
        self.process_registry
            .get_or_insert_with(ProcessRegistry::new)
            .register(name, factory);
        self
    }

    /// Register a method on a value-receiver type inline.
    pub fn register_method<F>(
        mut self,
        type_name: impl Into<String>,
        method: impl Into<String>,
        f: F,
    ) -> Self
    where
        F: Fn(&Value, &[Value]) -> prism_schema::MethodResult + Send + Sync + 'static,
    {
        self.method_registry
            .get_or_insert_with(MethodRegistry::new)
            .register(type_name, method, f);
        self
    }

    // ── Discovery control ──

    /// Disable automatic process discovery after construction. Manual
    /// control via `engine.discover_all_processes()` afterwards.
    pub fn no_auto_discover(mut self) -> Self {
        self.auto_discover = false;
        self
    }

    // ── Build ──

    /// Construct the engine. Returns an error if schema or state was
    /// not supplied.
    pub fn build(self) -> Result<Engine, String> {
        let schema = self.schema.ok_or("EngineBuilder: schema is required")?;
        let state = self.state.ok_or("EngineBuilder: state is required")?;
        let process_registry = Arc::new(self.process_registry.unwrap_or_default());
        let protocol_registry = Arc::new(self.protocol_registry.unwrap_or_default());

        let mut engine = Engine::from_state_with_protocols(
            schema,
            state,
            Arc::clone(&process_registry),
            Arc::clone(&protocol_registry),
        )?;

        if let Some(reg) = self.type_registry {
            engine.set_type_registry(Arc::new(reg));
        }
        if let Some(reg) = self.method_registry {
            engine.set_method_registry(Arc::new(reg));
        }

        if self.auto_discover {
            engine.discover_all_processes();
        }

        Ok(engine)
    }
}

impl Engine {
    /// Start a fluent builder for setting up an engine with its
    /// registries. See [`EngineBuilder`].
    pub fn builder() -> EngineBuilder {
        EngineBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Step;
    use crate::update::Update;

    #[test]
    fn builder_requires_schema_and_state() {
        let result = Engine::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_constructs_minimal_engine() {
        let engine = Engine::builder()
            .schema(Schema::Any)
            .state(Value::map())
            .build()
            .unwrap();
        assert!(engine.node_names().is_empty());
    }

    #[test]
    fn builder_registers_method_inline() {
        let engine = Engine::builder()
            .schema(Schema::Any)
            .state(Value::map())
            .register_method("Float", "double", |v, _| {
                Ok(Value::float(v.as_f64().unwrap_or(0.0) * 2.0))
            })
            .build()
            .unwrap();
        let methods = engine.method_registry().unwrap();
        let result = methods.dispatch(&Value::float(3.0), "double", &[]).unwrap();
        assert_eq!(result.as_f64(), Some(6.0));
    }

    #[test]
    fn builder_registers_process_inline() {
        #[derive(Debug)]
        struct NoopStep;
        impl Step for NoopStep {
            fn inputs(&self) -> crate::ports::PortSchema {
                indexmap::IndexMap::new()
            }
            fn outputs(&self) -> crate::ports::PortSchema {
                indexmap::IndexMap::new()
            }
            fn update(&self, _state: &Value) -> Update {
                Update::Noop
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }

        let engine = Engine::builder()
            .schema(Schema::Any)
            .state(Value::map())
            .register_process("Noop", |_config| ProcessNode::Step(Box::new(NoopStep)))
            .build()
            .unwrap();
        // Registry was set via builder; check the type was registered.
        // (We don't have a public Engine accessor for the registry,
        // but the test confirms compilation/build round-trip.)
        let _ = engine;
    }
}
