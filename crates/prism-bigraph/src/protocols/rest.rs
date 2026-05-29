//! `rest` protocol — process runs on a remote HTTP server.
//!
//! The wire protocol matches upstream
//! `process_bigraph/protocols/rest.py` + `rest_process/server.py`. A
//! `RestProcess` talks to a remote server exposing:
//!
//! ```text
//! POST /process/{class}/initialize    body=config         → process_id
//! GET  /process/{class}/inputs/{id}                       → ports map
//! GET  /process/{class}/outputs/{id}                      → ports map
//! POST /process/{class}/update/{id}   body={state,interval} → update
//! POST /process/{class}/end/{id}                          → ack
//! ```
//!
//! Same `Process` trait surface as local — the simulation can't tell
//! whether a process runs in-thread or over HTTP. That's the bridge
//! abstraction in action.
//!
//! ## Address shape
//!
//! ```text
//! {
//!   "protocol": "rest",
//!   "data": {
//!     "process": "Cell",
//!     "host":    "localhost",
//!     "port":    "22222"
//!   }
//! }
//! ```
//!
//! ## Today
//!
//! Port schemas come back from the server as JSON; we cache them and
//! expose [`Schema::Any`] regardless (the wiring layer enforces
//! shape, the REST process just shuttles `Value`s). State and updates
//! round-trip through `value_to_json` / `json_to_value`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use prism_schema::{
    Schema, Value,
    schema::{json_to_value, value_to_json},
};

use crate::defer::Defer;
use crate::factory::ProcessRegistry;
use crate::ports::PortSchema;
use crate::process::{Process, ProcessNode};
use crate::protocol::{Protocol, ProtocolError};
use crate::update::Update;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

// ── RestProcess ──────────────────────────────────────────────────────

/// A [`Process`] that proxies `update()` calls over HTTP to a remote
/// server speaking the rest-process wire protocol.
pub struct RestProcess {
    process_class: String,
    process_id: String,
    base_url: String,
    cached_inputs: PortSchema,
    cached_outputs: PortSchema,
    agent: ureq::Agent,
    ended: Mutex<bool>,
}

impl std::fmt::Debug for RestProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestProcess")
            .field("class", &self.process_class)
            .field("id", &self.process_id)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl RestProcess {
    /// Initialize a remote process. POSTs the config to
    /// `/process/{class}/initialize`, caches the returned process_id
    /// and port schemas, returns the wrapped instance.
    pub fn initialize(
        base_url: impl Into<String>,
        process_class: impl Into<String>,
        config: Value,
    ) -> Result<Self, ProtocolError> {
        let base_url = base_url.into();
        let process_class = process_class.into();

        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build();

        // 1. POST /initialize → process_id
        let init_url = format!("{base_url}/process/{process_class}/initialize");
        let init_body =
            serde_json::to_value(value_to_json(&config)).map_err(|e| ProtocolError::Other {
                protocol: "rest".into(),
                message: format!("encode config: {e}"),
            })?;
        let resp =
            agent
                .post(&init_url)
                .send_json(init_body)
                .map_err(|e| ProtocolError::Other {
                    protocol: "rest".into(),
                    message: format!("POST initialize: {e}"),
                })?;
        let raw_id = resp.into_string().map_err(|e| ProtocolError::Other {
            protocol: "rest".into(),
            message: format!("read initialize body: {e}"),
        })?;
        // Server returns a JSON string with quotes — strip them.
        let process_id = raw_id.trim().trim_matches('"').to_string();

        // 2. GET /inputs and /outputs.
        let cached_inputs = fetch_port_schema(
            &agent,
            &format!("{base_url}/process/{process_class}/inputs/{process_id}"),
        )?;
        let cached_outputs = fetch_port_schema(
            &agent,
            &format!("{base_url}/process/{process_class}/outputs/{process_id}"),
        )?;

        Ok(Self {
            process_class,
            process_id,
            base_url,
            cached_inputs,
            cached_outputs,
            agent,
            ended: Mutex::new(false),
        })
    }
}

impl Drop for RestProcess {
    fn drop(&mut self) {
        let mut ended = self.ended.lock().unwrap();
        if *ended {
            return;
        }
        let end_url = format!(
            "{}/process/{}/end/{}",
            self.base_url, self.process_class, self.process_id
        );
        // Best-effort — ignore errors at teardown.
        let _ = self.agent.post(&end_url).send_string("");
        *ended = true;
    }
}

impl Process for RestProcess {
    fn inputs(&self) -> PortSchema {
        self.cached_inputs.clone()
    }
    fn outputs(&self) -> PortSchema {
        self.cached_outputs.clone()
    }
    /// Non-blocking: **fire the HTTP request on its own thread now, join it in
    /// `.get()`.** This is the seam that parallelizes `rest:` nodes (mirror of the
    /// `stream:` concurrent dispatch). The engine's invoke pass calls `invoke` on
    /// every due process *before* collecting any result, so all `rest:` round-trips
    /// are in flight concurrently; the collect pass joins them. Wall-clock per tick
    /// ≈ the slowest call, not the sum. `ureq` is synchronous, so concurrency comes
    /// from the spawned thread (not an async future) — one blocking POST per thread.
    fn invoke(&self, state: &Value, interval: f64) -> Defer<Update> {
        // Build the payload EAGERLY (state is borrowed) so the thread owns it.
        let url = format!(
            "{}/process/{}/update/{}",
            self.base_url, self.process_class, self.process_id
        );
        let body = serde_json::json!({
            "state": value_to_json(state),
            "interval": interval,
        });
        let agent = self.agent.clone(); // cheap: Arc inside
        let label = format!("{}/{}", self.process_class, self.process_id);
        let handle = std::thread::spawn(move || match agent.post(&url).send_json(body) {
            Ok(resp) => match resp.into_json::<serde_json::Value>() {
                Ok(raw) if !raw.is_null() => Update::value(json_to_value(&raw)),
                _ => Update::Noop,
            },
            Err(e) => {
                eprintln!("RestProcess update {label}: {e}");
                Update::Noop
            }
        });
        Defer::lazy(move || handle.join().unwrap_or(Update::Noop))
    }

    /// Blocking convenience — a direct `update()` caller still gets the result.
    /// The engine uses `invoke` for concurrency.
    fn update(&self, state: &Value, interval: f64) -> Update {
        self.invoke(state, interval).get()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Fetch `/inputs/{id}` or `/outputs/{id}` and convert into a [`PortSchema`].
/// Each reported type expression is parsed back into a REAL [`Schema`] (the
/// inverse of `Schema`'s Display / upstream's `render`), so the bridge speaks
/// actual schemas — cross-boundary apply/reconcile (delta sums, overwrite
/// overwrites, units) flows through the closed algebra, not an additive `Any`.
fn fetch_port_schema(agent: &ureq::Agent, url: &str) -> Result<PortSchema, ProtocolError> {
    let resp = agent.get(url).call().map_err(|e| ProtocolError::Other {
        protocol: "rest".into(),
        message: format!("GET port schema {url}: {e}"),
    })?;
    let json: serde_json::Value = resp.into_json().map_err(|e| ProtocolError::Other {
        protocol: "rest".into(),
        message: format!("read port body {url}: {e}"),
    })?;
    let map = match json.as_object() {
        Some(obj) => obj,
        None => return Ok(IndexMap::new()),
    };
    let mut ports = IndexMap::new();
    for (k, v) in map {
        // Parse the server's reported type expression back into a real Schema.
        // A non-string (or unknown) type degrades to `Any` rather than failing.
        let schema = v
            .as_str()
            .map(prism_schema::parse_type_expression)
            .unwrap_or(Schema::Any);
        ports.insert(k.clone(), schema);
    }
    Ok(ports)
}

// ── RestProtocol ────────────────────────────────────────────────────

/// The `rest` protocol. Reads `host`/`port`/`process` out of the
/// address's `data` map and constructs a [`RestProcess`] that calls
/// the corresponding rest-process server.
#[derive(Debug, Default)]
pub struct RestProtocol;

impl Protocol for RestProtocol {
    fn name(&self) -> &str {
        "rest"
    }

    fn instantiate(
        &self,
        data: &Value,
        config: Value,
        _registry: &Arc<ProcessRegistry>,
    ) -> Result<ProcessNode, ProtocolError> {
        let map = data.as_map().ok_or_else(|| {
            ProtocolError::MalformedAddress(format!(
                "rest protocol expects data: Map<process,host,port>, got {data:?}"
            ))
        })?;
        let process = map
            .get("process")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::MalformedAddress("rest: missing `process`".into()))?;
        let host = map
            .get("host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::MalformedAddress("rest: missing `host`".into()))?;
        let port_field = map
            .get("port")
            .ok_or_else(|| ProtocolError::MalformedAddress("rest: missing `port`".into()))?;
        let port_str = match port_field {
            Value::String(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            other => {
                return Err(ProtocolError::MalformedAddress(format!(
                    "rest: `port` must be String or Int, got {other:?}"
                )));
            }
        };
        let base_url = format!("http://{host}:{port_str}");
        let rest_process = RestProcess::initialize(base_url, process, config)?;
        Ok(ProcessNode::Process(Box::new(rest_process)))
    }

    /// Address type — the typed REST endpoint (matches upstream
    /// `RestProtocol.data: RestData{process, host, port}`).
    fn address_type(&self) -> Option<(String, prism_schema::Schema)> {
        Some((
            "rest".into(),
            crate::protocol::string_record(&["process", "host", "port"]),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_protocol_name() {
        let p = RestProtocol;
        assert_eq!(p.name(), "rest");
    }

    #[test]
    fn rest_address_parsing_rejects_missing_fields() {
        let p = RestProtocol;
        let registry = Arc::new(ProcessRegistry::new());

        let bad = Value::Map(IndexMap::from_iter([(
            "process".into(),
            Value::String("Cell".into()),
        )]));
        let err = p.instantiate(&bad, Value::None, &registry).unwrap_err();
        assert!(matches!(err, ProtocolError::MalformedAddress(_)));
    }

    #[test]
    fn rest_address_parsing_rejects_non_map() {
        let p = RestProtocol;
        let registry = Arc::new(ProcessRegistry::new());
        let bad = Value::String("local:Cell".into());
        let err = p.instantiate(&bad, Value::None, &registry).unwrap_err();
        assert!(matches!(err, ProtocolError::MalformedAddress(_)));
    }
}
