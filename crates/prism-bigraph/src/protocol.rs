//! Protocol abstraction — where a process *runs* is decoupled from
//! how it's defined.
//!
//! Upstream `process-bigraph` parses an `address` field as
//! `"<protocol>:<data>"` and dispatches to a per-protocol resolver
//! that knows how to instantiate processes of that flavor. The
//! resolver is keyed by the protocol name (e.g. "local", "parallel",
//! "rest", "ray", "pool"); the rest of the engine is protocol-agnostic.
//! See `process_bigraph/protocols/__init__.py` upstream.
//!
//! Prism's [`Protocol`] trait captures the same shape in Rust. A
//! [`ProtocolRegistry`] holds the active protocols by name; the engine
//! consults it during process discovery. The default `local` protocol
//! delegates to the existing [`crate::factory::ProcessRegistry`].
//!
//! ## Address shapes
//!
//! Three forms are accepted:
//!
//! | Form | Example | Notes |
//! |---|---|---|
//! | `"<protocol>:<data>"` string | `"local:Cell"` | Sugar; `data` is the rest of the string. |
//! | `{"protocol": "...", "data": "..."}` map | `{"protocol": "local", "data": "Cell"}` | Canonical. `data` may be any `Value`. |
//! | bare `"Class"` string (no protocol) | `"Cell"` | Defaults to the `local` protocol. |

use std::sync::Arc;

use indexmap::IndexMap;
use prism_schema::{Key, Schema, Value};
use thiserror::Error;

use crate::factory::ProcessRegistry;
use crate::process::ProcessNode;

/// A schema record of `String`-typed fields — the common shape of a protocol's
/// address type (`{process}`, `{path}`, `{process, host, port}`). All fields are
/// intensive (`String`), so a divide *shares* an address (daughters inherit it).
pub fn string_record(fields: &[&str]) -> Schema {
    Schema::Tree {
        branches: fields
            .iter()
            .map(|f| (Key::from(*f), Schema::String { default: None }))
            .collect::<IndexMap<Key, Schema>>(),
    }
}

/// One protocol's resolver — given the address's `data` payload and a
/// process config, returns a runnable [`ProcessNode`].
///
/// Protocols are `Send + Sync` so the engine can hold an `Arc<dyn
/// Protocol>` without per-tick locking.
pub trait Protocol: Send + Sync + std::fmt::Debug {
    /// Protocol name as it appears in `address.protocol`.
    fn name(&self) -> &str;

    /// Instantiate a process for an address with this protocol.
    ///
    /// - `data` is the address's `data` payload — for most protocols
    ///   it's a `Value::String("ClassName")`. Richer protocols (REST,
    ///   Docker, …) accept a map.
    /// - `config` is the per-instance config from the spec.
    /// - `registry` is the local process registry. Most protocols
    ///   ignore it, but `local`, `parallel`, `pool`, `ray` all use it
    ///   to resolve the underlying class.
    fn instantiate(
        &self,
        data: &Value,
        config: Value,
        registry: &Arc<ProcessRegistry>,
    ) -> Result<ProcessNode, ProtocolError>;

    /// The protocol's **address type** — `(type name, representation schema)` —
    /// registered into the `Core`'s `TypeRegistry` so an address is a first-class
    /// typed value (`check`/`serialize`/`realize`/`divide` flow through the closed
    /// algebra), not an untyped blob. The type name == [`Protocol::name`], so an
    /// address value's `_type` tag selects both the schema type and this transport.
    /// `None` ⇒ no declared address type (validated structurally). See
    /// docs/protocols-as-types.md.
    fn address_type(&self) -> Option<(String, Schema)> {
        None
    }

    /// The protocol's per-tick batching runtime, if it has one. The engine
    /// registers this (via [`crate::Core`]) and calls
    /// [`crate::protocol_runtime::ProtocolRuntime::flush_pending`] between the
    /// invoke and collect passes — the explicit invoke→collect barrier a
    /// batching transport needs (e.g. the `parallel` pool, a future `ray:`).
    /// Default `None`: synchronous protocols (`local`, `rest`, `stream`) finish
    /// their work inside `Process::invoke` and need no flush.
    fn runtime(&self) -> Option<Arc<dyn crate::protocol_runtime::ProtocolRuntime>> {
        None
    }
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("unknown protocol `{0}`")]
    UnknownProtocol(String),

    #[error("class `{0}` not registered with protocol `{1}`")]
    UnknownClass(String, String),

    #[error("malformed address: {0}")]
    MalformedAddress(String),

    #[error("protocol `{protocol}`: {message}")]
    Other { protocol: String, message: String },
}

// =============================================================================
// Address parsing
// =============================================================================

/// Parsed address: `(protocol_name, data_payload)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedAddress {
    pub protocol: String,
    pub data: Value,
}

impl ParsedAddress {
    /// Parse the `address` field of a process spec. Returns an error if
    /// the value isn't a string or `{protocol, data}` map.
    pub fn parse(address: &Value) -> Result<Self, ProtocolError> {
        match address {
            Value::String(s) => Ok(Self::parse_string(s)),
            Value::Map(map) => {
                // Typed Custom form (the principled one): `{_type: <protocol>,
                // ...fields}` — the `_type` tag IS the protocol. A single-field
                // record unwraps to its value, so a single-field protocol
                // (`local`/`stream`/`parallel`) normalizes to the same `data`
                // (a String) the legacy forms produced; `rest`'s multi-field
                // record stays a map. So `instantiate` reads `data` unchanged.
                if let Some(ty) = map.get("_type").and_then(|v| v.as_str()) {
                    let mut fields: IndexMap<Key, Value> = map.clone();
                    fields.shift_remove("_type");
                    let data = if fields.len() == 1 {
                        fields.into_iter().next().map(|(_, v)| v).unwrap_or(Value::None)
                    } else {
                        Value::Map(fields)
                    };
                    return Ok(Self {
                        protocol: ty.to_string(),
                        data,
                    });
                }
                // Legacy canonical form: `{protocol, data}`.
                let protocol = map
                    .get("protocol")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ProtocolError::MalformedAddress(
                            "map address missing `protocol`/`_type`".into(),
                        )
                    })?
                    .to_string();
                let data = map.get("data").cloned().unwrap_or(Value::None);
                Ok(Self { protocol, data })
            }
            other => Err(ProtocolError::MalformedAddress(format!(
                "expected String or Map, got {other:?}"
            ))),
        }
    }

    /// Parse the legacy `"<protocol>:<data>"` string form. Falls back
    /// to the bare-class case (`"Cell"` → local protocol with data
    /// `"Cell"`).
    fn parse_string(s: &str) -> Self {
        match s.split_once(':') {
            Some((protocol, data)) => Self {
                protocol: protocol.to_string(),
                data: Value::String(data.to_string()),
            },
            None => Self {
                protocol: "local".to_string(),
                data: Value::String(s.to_string()),
            },
        }
    }
}

// =============================================================================
// Local protocol
// =============================================================================

/// The default protocol — instantiates processes by class name from
/// the [`ProcessRegistry`]. Mirrors upstream `local_lookup`.
#[derive(Debug)]
pub struct LocalProtocol;

impl Protocol for LocalProtocol {
    fn name(&self) -> &str {
        "local"
    }

    fn instantiate(
        &self,
        data: &Value,
        config: Value,
        registry: &Arc<ProcessRegistry>,
    ) -> Result<ProcessNode, ProtocolError> {
        let class_name = data.as_str().ok_or_else(|| {
            ProtocolError::MalformedAddress(format!(
                "local protocol expects data: String, got {data:?}"
            ))
        })?;
        registry
            .create(class_name, config)
            .ok_or_else(|| ProtocolError::UnknownClass(class_name.to_string(), "local".into()))
    }

    fn address_type(&self) -> Option<(String, Schema)> {
        Some(("local".into(), string_record(&["process"])))
    }
}

// =============================================================================
// Registry
// =============================================================================

/// Registry of active [`Protocol`] implementations, keyed by name.
///
/// Construction defaults always include the `local` protocol; other
/// protocols (parallel, rest, ray, …) are registered explicitly.
#[derive(Debug)]
pub struct ProtocolRegistry {
    protocols: std::collections::HashMap<String, Arc<dyn Protocol>>,
}

impl Default for ProtocolRegistry {
    fn default() -> Self {
        let mut protocols: std::collections::HashMap<String, Arc<dyn Protocol>> =
            std::collections::HashMap::new();
        protocols.insert("local".into(), Arc::new(LocalProtocol));
        Self { protocols }
    }
}

impl ProtocolRegistry {
    /// Fresh registry with only `local` registered.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, protocol: Arc<dyn Protocol>) {
        let name = protocol.name().to_string();
        self.protocols.insert(name, protocol);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Protocol>> {
        self.protocols.get(name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.protocols.keys().map(|s| s.as_str()).collect()
    }

    /// Every registered protocol's batching [`runtime`](Protocol::runtime), if any
    /// — the engine registers these so each is flushed between invoke and collect.
    pub fn runtimes(&self) -> Vec<Arc<dyn crate::protocol_runtime::ProtocolRuntime>> {
        self.protocols.values().filter_map(|p| p.runtime()).collect()
    }

    /// Every registered protocol's [`address_type`](Protocol::address_type) — the
    /// `Core` registers these into its `TypeRegistry` so addresses are typed values.
    pub fn address_types(&self) -> Vec<(String, Schema)> {
        self.protocols.values().filter_map(|p| p.address_type()).collect()
    }

    /// Look up the protocol for a parsed address and dispatch
    /// `instantiate`. Convenience wrapper.
    pub fn instantiate(
        &self,
        address: &ParsedAddress,
        config: Value,
        registry: &Arc<ProcessRegistry>,
    ) -> Result<ProcessNode, ProtocolError> {
        let protocol = self
            .get(&address.protocol)
            .ok_or_else(|| ProtocolError::UnknownProtocol(address.protocol.clone()))?;
        protocol.instantiate(&address.data, config, registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Step;
    use crate::update::Update;
    use indexmap::IndexMap;

    #[derive(Debug)]
    struct NoopStep;
    impl Step for NoopStep {
        fn inputs(&self) -> crate::ports::PortSchema {
            IndexMap::new()
        }
        fn outputs(&self) -> crate::ports::PortSchema {
            IndexMap::new()
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

    fn registry_with_noop() -> Arc<ProcessRegistry> {
        let mut r = ProcessRegistry::new();
        r.register("Noop", |_| ProcessNode::Step(Box::new(NoopStep)));
        Arc::new(r)
    }

    #[test]
    fn parse_legacy_string() {
        let parsed = ParsedAddress::parse(&Value::String("local:Cell".into())).unwrap();
        assert_eq!(parsed.protocol, "local");
        assert_eq!(parsed.data, Value::String("Cell".into()));
    }

    #[test]
    fn parse_map_form() {
        let addr = Value::Map(IndexMap::from_iter([
            ("protocol".into(), Value::String("rest".into())),
            ("data".into(), Value::String("Cell".into())),
        ]));
        let parsed = ParsedAddress::parse(&addr).unwrap();
        assert_eq!(parsed.protocol, "rest");
        assert_eq!(parsed.data, Value::String("Cell".into()));
    }

    #[test]
    fn parse_bare_class_defaults_to_local() {
        let parsed = ParsedAddress::parse(&Value::String("Cell".into())).unwrap();
        assert_eq!(parsed.protocol, "local");
        assert_eq!(parsed.data, Value::String("Cell".into()));
    }

    #[test]
    fn local_protocol_instantiates() {
        let registry = registry_with_noop();
        let protocols = ProtocolRegistry::new();
        let addr = ParsedAddress::parse(&Value::String("local:Noop".into())).unwrap();
        let node = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap();
        assert!(matches!(node, ProcessNode::Step(_)));
    }

    #[test]
    fn unknown_protocol_errors() {
        let registry = registry_with_noop();
        let protocols = ProtocolRegistry::new();
        let addr = ParsedAddress::parse(&Value::String("ray:Cell".into())).unwrap();
        let err = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownProtocol(p) if p == "ray"));
    }

    #[test]
    fn unknown_class_errors() {
        let registry = registry_with_noop();
        let protocols = ProtocolRegistry::new();
        let addr = ParsedAddress::parse(&Value::String("local:Nope".into())).unwrap();
        let err = protocols
            .instantiate(&addr, Value::None, &registry)
            .unwrap_err();
        assert!(matches!(err, ProtocolError::UnknownClass(_, _)));
    }

    #[test]
    fn default_registry_has_local() {
        let protocols = ProtocolRegistry::new();
        assert!(protocols.get("local").is_some());
        assert_eq!(protocols.names().len(), 1);
    }
}
