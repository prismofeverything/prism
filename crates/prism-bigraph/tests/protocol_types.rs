//! Protocols as types (docs/protocols-as-types.md): each protocol's address is a
//! first-class Custom type registered in the Core, and the `_type`-tagged address
//! value parses + drives discovery identically to the legacy forms.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocol::{ParsedAddress, ProtocolRegistry};
use prism_bigraph::protocols::{ParallelProtocol, RestProtocol};
use prism_bigraph::{Core, Engine, Schema, Update, Value};

fn wire(segs: &[&str]) -> Value {
    Value::List(segs.iter().map(|s| Value::String(s.to_string())).collect())
}

#[test]
fn typed_address_parses_to_legacy_data() {
    // Single-field protocol: `{_type:"local", process:"Cell"}` unwraps to the same
    // `data` (a String) the legacy `"local:Cell"` produced — so `instantiate` is
    // unchanged.
    let local = Value::tree([
        ("_type", Value::String("local".into())),
        ("process", Value::String("Cell".into())),
    ]);
    let p = ParsedAddress::parse(&local).unwrap();
    assert_eq!(p.protocol, "local");
    assert_eq!(p.data.as_str(), Some("Cell"));
    // Identical to the legacy string form.
    assert_eq!(ParsedAddress::parse(&Value::String("local:Cell".into())).unwrap(), p);

    // Multi-field protocol: `rest` keeps its record as `data` (a map).
    let rest = Value::tree([
        ("_type", Value::String("rest".into())),
        ("process", Value::String("Composite".into())),
        ("host", Value::String("127.0.0.1".into())),
        ("port", Value::String("8080".into())),
    ]);
    let p = ParsedAddress::parse(&rest).unwrap();
    assert_eq!(p.protocol, "rest");
    let m = p.data.as_map().expect("rest data is a record");
    assert_eq!(m.get("host").and_then(|v| v.as_str()), Some("127.0.0.1"));
    assert_eq!(m.get("port").and_then(|v| v.as_str()), Some("8080"));
}

#[test]
fn core_registers_protocol_address_types() {
    // A Core with rest + parallel protocols registers their address types, so an
    // address is a first-class typed value the algebra can `check`.
    let mut protocols = ProtocolRegistry::new(); // includes `local`
    protocols.register(Arc::new(RestProtocol));
    protocols.register(Arc::new(ParallelProtocol::default()));
    let core = Core::new().with_protocols(Arc::new(protocols));

    for name in ["local", "rest", "parallel"] {
        assert!(core.types.get(name).is_some(), "{name} address type registered");
    }

    let rest_addr = Value::tree([
        ("process", Value::String("Composite".into())),
        ("host", Value::String("127.0.0.1".into())),
        ("port", Value::String("8080".into())),
    ]);
    assert!(
        core.types.type_check("rest", &rest_addr),
        "a well-formed rest address checks against its registered {{process,host,port}} type"
    );
}

// A process that adds a fixed +5 to `x` each tick — to observe a typed-address
// node actually running.
#[derive(Debug)]
struct Add5;
impl Process for Add5 {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("x".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("x".to_string(), Schema::float())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        Update::value(Value::tree([("x", Value::float(5.0))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn engine_discovers_and_runs_a_typed_address_node() {
    // A process node whose `address` is the TYPED Custom value
    // `{_type:"local", process:"Add5"}` is discovered + driven exactly like a
    // legacy `"local:Add5"` node.
    let mut registry = ProcessRegistry::new();
    registry.register("Add5", |_| ProcessNode::Process(Box::new(Add5)));

    let node = Value::tree([
        ("_type", Value::String("process".into())),
        (
            "address",
            Value::tree([
                // address._type "local" is the PROTOCOL (transport); the
                // outer `_type: "process"` above is the NODE-kind hint for
                // schema-first discovery.
                ("_type", Value::String("local".into())),
                ("process", Value::String("Add5".into())),
            ]),
        ),
        ("config", Value::map()),
        ("inputs", Value::tree([("x", wire(&["x"]))])),
        ("outputs", Value::tree([("x", wire(&["x"]))])),
    ]);
    let state = Value::tree([("x", Value::float(1.0)), ("adder", node)]);

    let mut engine =
        Engine::from_state(Schema::Any, state, Core::from(Arc::new(registry))).expect("engine");
    engine.discover_all_processes();
    engine.run(1.0);

    let x = engine.state().get_field("x").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    assert_eq!(x, 6.0, "the typed-address Add5 node ran: 1 + 5 = 6 (got {x})");
}
