//! The rest bridge speaks REAL schemas, not `Schema::Any`. A process with typed
//! ports (`map[float]`, `delta`, `overwrite[float]`) is served over REST; the
//! `RestProcess` client reconstructs those exact schemas from the server's
//! rendered type expressions — so cross-boundary apply/reconcile (a delta sums,
//! an overwrite overwrites) flows through the closed schema algebra rather than
//! the additive `Any` default. The bridge is the point of connection between
//! separable simulation components; it must agree on the actual schema.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::{RestProcess, RestProcessServer};
use prism_bigraph::{Core, Schema, Update, Value};

/// Typed ports across the three reconcile-relevant kinds.
#[derive(Debug)]
struct TypedProcess;
impl Process for TypedProcess {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("state".to_string(), Schema::map(Schema::float()))])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("state".to_string(), Schema::map(Schema::float())),
            ("delta".to_string(), Schema::Delta { default: None }),
            ("gate".to_string(), Schema::overwrite(Schema::float())),
        ])
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        Update::Noop
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn rest_bridge_reconstructs_real_port_schemas() {
    let mut registry = ProcessRegistry::new();
    registry.register("Typed", |_config| ProcessNode::Process(Box::new(TypedProcess)));
    let server = RestProcessServer::start(Core::from(Arc::new(registry))).expect("start rest server");
    std::thread::sleep(Duration::from_millis(50));

    let rp = RestProcess::initialize(
        format!("http://127.0.0.1:{}", server.port()),
        "Typed",
        Value::map(),
        Core::new(),
    )
    .expect("initialize rest process");

    // The client reconstructs the REAL schemas from the server's rendered types.
    assert_eq!(
        rp.inputs().get("state"),
        Some(&Schema::map(Schema::float())),
        "input `state` round-trips as map[float], not Any"
    );

    let outputs = rp.outputs();
    assert_eq!(outputs.get("state"), Some(&Schema::map(Schema::float())));
    assert_eq!(
        outputs.get("delta"),
        Some(&Schema::Delta { default: None }),
        "a Delta port round-trips as Delta (reconciles ADDITIVELY across the boundary)"
    );
    assert_eq!(
        outputs.get("gate"),
        Some(&Schema::overwrite(Schema::float())),
        "an overwrite[float] port round-trips as Overwrite (OVERWRITES across the boundary)"
    );
    // Nothing collapsed to Any — the bridge speaks actual schemas.
    assert!(
        !outputs.values().any(|s| matches!(s, Schema::Any)),
        "no port should degrade to Schema::Any: {outputs:?}"
    );
}
