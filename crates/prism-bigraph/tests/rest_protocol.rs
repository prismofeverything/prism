//! End-to-end test for the `rest` protocol.
//!
//! Spins up a minimal in-process HTTP server that speaks the
//! rest-process wire protocol, registers a `Double` process, runs a
//! [`RestProcess`] through the protocol, and validates round-trip.
//!
//! The mock server is intentionally hand-rolled (TcpListener + raw
//! HTTP parsing) to avoid a heavy test dep — we control both sides of
//! the wire, so we only implement the endpoints + JSON shapes we
//! actually use.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::protocol::{ParsedAddress, ProtocolRegistry};
use prism_bigraph::protocols::RestProtocol;
use prism_bigraph::Core;
use prism_schema::Value;

// =============================================================================
// Mock HTTP server speaking the rest-process wire protocol.
// =============================================================================

struct MockServer {
    port: u16,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockServer {
    fn start() -> Self {
        // Bind to port 0 → OS picks a free one.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);

        listener.set_nonblocking(true).expect("set_nonblocking");

        let thread = thread::spawn(move || {
            while !shutdown_clone.load(std::sync::atomic::Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread::spawn(move || handle_request(stream));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            port,
            shutdown,
            thread: Some(thread),
        }
    }

    fn url_base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn handle_request(stream: TcpStream) {
    if let Err(e) = handle_request_impl(stream) {
        eprintln!("mock server: {e}");
    }
}

fn handle_request_impl(mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    // Request line: METHOD PATH HTTP/1.1
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let request_line = request_line.trim();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    // Headers — we just need Content-Length.
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 || header == "\r\n" {
            break;
        }
        let header = header.trim();
        if let Some(rest) = header.to_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body_str = std::str::from_utf8(&body).unwrap_or("").to_string();

    // Route.
    let (status, payload) = route(&method, &path, &body_str);

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    Ok(())
}

fn route(method: &str, path: &str, body: &str) -> (&'static str, String) {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    match (method, segments.as_slice()) {
        // POST /process/{class}/initialize
        ("POST", ["process", _class, "initialize"]) => {
            // Return a fixed process_id string.
            ("200 OK", "\"proc-001\"".to_string())
        }
        // GET /process/{class}/inputs/{id}
        ("GET", ["process", _class, "inputs", _id]) => ("200 OK", "{\"x\": \"float\"}".to_string()),
        // GET /process/{class}/outputs/{id}
        ("GET", ["process", _class, "outputs", _id]) => {
            ("200 OK", "{\"x\": \"float\"}".to_string())
        }
        // POST /process/{class}/update/{id}
        // Body: {state: {x: <n>}, interval: <f>}
        // We mock a "doubler": output.x = state.x * 2 * interval (interval=1 by default).
        ("POST", ["process", _class, "update", _id]) => {
            let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let state = parsed.get("state");
            let interval = parsed
                .get("interval")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            let x = state
                .and_then(|s| s.get("x"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let payload = serde_json::json!({"x": x * 2.0 * interval}).to_string();
            ("200 OK", payload)
        }
        // POST /process/{class}/end/{id}
        ("POST", ["process", _class, "end", _id]) => ("200 OK", "null".to_string()),

        _ => ("404 Not Found", "\"not found\"".to_string()),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[test]
fn rest_protocol_round_trip_against_mock_server() {
    let server = MockServer::start();
    // Give the listener a moment to be ready.
    thread::sleep(Duration::from_millis(50));

    let core = Core::from(Arc::new(ProcessRegistry::new()));
    let mut protocols = ProtocolRegistry::new();
    protocols.register(Arc::new(RestProtocol));

    // Address: rest protocol with map data
    let address = Value::Map(indexmap::IndexMap::from_iter([
        ("protocol".into(), Value::String("rest".into())),
        (
            "data".into(),
            Value::Map(indexmap::IndexMap::from_iter([
                ("process".into(), Value::String("Doubler".into())),
                ("host".into(), Value::String("127.0.0.1".into())),
                ("port".into(), Value::String(server.port.to_string())),
            ])),
        ),
    ]));
    let parsed = ParsedAddress::parse(&address).unwrap();
    let node = protocols
        .instantiate(&parsed, Value::None, &core)
        .expect("instantiate RestProcess");

    let proc = match node {
        prism_bigraph::process::ProcessNode::Process(p) => p,
        _ => panic!("expected Process"),
    };

    let state = Value::tree([("x", Value::float(3.0))]);
    let upd = proc.update(&state, 1.0).into_value().expect("Update");
    let x = upd.as_map().unwrap().get("x").unwrap().as_f64().unwrap();
    assert!((x - 6.0).abs() < 1e-9, "expected 6.0, got {x}");
}

#[test]
fn rest_protocol_inputs_outputs_cached_from_init() {
    let server = MockServer::start();
    thread::sleep(Duration::from_millis(50));

    let core = Core::from(Arc::new(ProcessRegistry::new()));
    let mut protocols = ProtocolRegistry::new();
    protocols.register(Arc::new(RestProtocol));

    let address = Value::Map(indexmap::IndexMap::from_iter([
        ("protocol".into(), Value::String("rest".into())),
        (
            "data".into(),
            Value::Map(indexmap::IndexMap::from_iter([
                ("process".into(), Value::String("Doubler".into())),
                ("host".into(), Value::String("127.0.0.1".into())),
                ("port".into(), Value::String(server.port.to_string())),
            ])),
        ),
    ]));
    let parsed = ParsedAddress::parse(&address).unwrap();
    let node = protocols
        .instantiate(&parsed, Value::None, &core)
        .unwrap();

    let proc = match node {
        prism_bigraph::process::ProcessNode::Process(p) => p,
        _ => panic!(),
    };

    // Server returned `{"x": "float"}` for both — both should have
    // one port named "x".
    let inputs = proc.inputs();
    let outputs = proc.outputs();
    assert_eq!(inputs.len(), 1);
    assert_eq!(outputs.len(), 1);
    assert!(inputs.contains_key("x"));
    assert!(outputs.contains_key("x"));
}
