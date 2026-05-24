//! The `stream:` protocol — a `.ys` program proxied as a LOCAL process over OS
//! pipes + the Arrow trace wire (the mirror of [`prism_bigraph`]'s `rest:` over
//! HTTP). The realization (co-designed): "run a whole simulation" and "be one
//! component" are the SAME stepper `(state, dt) → update`, differing only in who
//! supplies the clock. So the child here is the *same* `--serve-stream` mode the
//! pipe `A | B` uses; [`StreamProcess`] just drives it **lock-step** — one frame
//! per engine tick, the clock riding in the frame `time` (the dynamic-dt).
//!
//! Each `update(state, interval)`: stamp cumulative time, push the input as a
//! delta-frame (first absolute, then diffs), read one output delta-frame back,
//! and return it as the engine update. The parent's state at the child's output
//! ports thus tracks the child's output. So a child `.ys` is stepped by the
//! parent engine indistinguishably from an in-thread process.

use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use prism_bigraph::{
    PortSchema, Process, ProcessNode, ProcessRegistry, Protocol, ProtocolError, ProtocolRegistry,
    Update,
};
use prism_schema::{algebra, schema_to_value, Key, Schema, Value};
use prism_trace::{TraceReader, TraceWriter};

/// The chrysalis binary that runs the child (`<bin> run <child> --serve-stream`):
/// `CHRYSALIS_BIN` if set (tests point it at the cargo-built binary), else the
/// current executable.
fn chrysalis_binary() -> String {
    std::env::var("CHRYSALIS_BIN")
        .ok()
        .or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "chrysalis".to_string())
}

/// A [`Process`] that proxies `update()` to a child `.ys` running `--serve-stream`
/// over OS pipes, lock-step (one frame each way per tick).
pub struct StreamProcess {
    program: String,
    binary: String,
    inner: Mutex<Inner>,
}

struct Inner {
    child: Option<Child>,
    stdout: Option<ChildStdout>, // held until the reader is lazily created
    writer: Option<TraceWriter<ChildStdin>>,
    reader: Option<TraceReader<ChildStdout>>,
    prev_input: Option<Value>,
    element: Schema, // input element (the record of the child's input ports)
    time: f64,
}

impl Inner {
    fn empty() -> Self {
        Self {
            child: None,
            stdout: None,
            writer: None,
            reader: None,
            prev_input: None,
            element: Schema::Any,
            time: 0.0,
        }
    }
}

impl std::fmt::Debug for StreamProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamProcess").field("program", &self.program).finish_non_exhaustive()
    }
}

impl StreamProcess {
    /// Proxy the child `.ys` at `program`, spawned with the default chrysalis
    /// binary ([`chrysalis_binary`]).
    pub fn new(program: impl Into<String>) -> Self {
        Self::with_binary(program, chrysalis_binary())
    }

    /// As [`new`](Self::new) but with an explicit binary (tests pass the
    /// cargo-built `chrysalis`).
    pub fn with_binary(program: impl Into<String>, binary: impl Into<String>) -> Self {
        Self { program: program.into(), binary: binary.into(), inner: Mutex::new(Inner::empty()) }
    }

    /// Spawn `<binary> run <program> --serve-stream`, returning the child, its
    /// stdout, an input `TraceWriter` (header written), and the input element.
    #[allow(clippy::type_complexity)]
    fn spawn(
        &self,
        first_state: &Value,
    ) -> std::io::Result<(Child, ChildStdout, TraceWriter<ChildStdin>, Schema)> {
        let mut child = Command::new(&self.binary)
            .args(["run", self.program.as_str(), "--serve-stream"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("no child stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no child stdout"))?;
        // Header element: the record of the child's input ports (a Tree of the
        // state's top-level fields), so the child's connect-time `refines` check
        // sees a matching shape.
        let element = record_schema(first_state);
        let writer = TraceWriter::new(stdin, "stream", &schema_to_value(&element))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok((child, stdout, writer, element))
    }
}

/// The element schema of a state record: a `Tree` of its (non-sentinel) top-level
/// fields, each inferred. Matches a composite's `~{inputs}` interface shape.
fn record_schema(state: &Value) -> Schema {
    match state {
        Value::Map(m) => Schema::Tree {
            branches: m
                .iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(k, v)| (k.clone(), Schema::infer(v)))
                .collect(),
        },
        other => Schema::infer(other),
    }
}

impl Process for StreamProcess {
    // The engine routes via the spec's `{inputs, outputs}` wiring (like `rest:`);
    // the proxied child just shuttles `Value`s, so the port schemas are empty.
    fn inputs(&self) -> PortSchema {
        IndexMap::new()
    }
    fn outputs(&self) -> PortSchema {
        IndexMap::new()
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return Update::Noop,
        };

        // Lazily spawn the child + write the input header on the first tick.
        if inner.child.is_none() {
            match self.spawn(state) {
                Ok((child, stdout, writer, element)) => {
                    inner.child = Some(child);
                    inner.stdout = Some(stdout);
                    inner.writer = Some(writer);
                    inner.element = element;
                }
                Err(e) => {
                    eprintln!("StreamProcess spawn {}: {e}", self.program);
                    return Update::Noop;
                }
            }
        }

        // Push the input as a delta-frame (first absolute, then diffs), stamped
        // with cumulative time = the parent's clock for the child.
        let payload = match &inner.prev_input {
            None => state.clone(),
            Some(prev) => algebra::diff(&inner.element, prev, state).unwrap_or(Value::None),
        };
        let t = inner.time;
        if let Some(w) = inner.writer.as_mut() {
            if w.push(t, &payload).is_err() || w.flush().is_err() {
                eprintln!("StreamProcess write {}", self.program);
                return Update::Noop;
            }
        }
        inner.prev_input = Some(state.clone());
        inner.time += interval;

        // Lazily open the reader (the child emits its output header only AFTER it
        // has read the first input frame).
        if inner.reader.is_none() {
            match inner.stdout.take() {
                Some(stdout) => match TraceReader::new(stdout) {
                    Ok(r) => inner.reader = Some(r),
                    Err(e) => {
                        eprintln!("StreamProcess reader {}: {e}", self.program);
                        return Update::Noop;
                    }
                },
                None => return Update::Noop,
            }
        }

        // Read one output delta-frame → the engine update. A `None` payload is a
        // no-change tick — apply nothing.
        match inner.reader.as_mut().and_then(Iterator::next) {
            Some(Ok((_t, output))) if !matches!(output, Value::None) => Update::value(output),
            _ => Update::Noop,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Drop for StreamProcess {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            // Close stdin (drop the writer) → the child sees EOF and exits.
            if let Some(w) = inner.writer.take() {
                let _ = w.finish();
            }
            if let Some(mut child) = inner.child.take() {
                let _ = child.wait();
            }
        }
    }
}

/// Build a `Tree` schema with one `Float` branch named `key` (test helper export
/// kept private; used by the integration test via the public API).
#[doc(hidden)]
pub fn float_record(key: &str) -> Schema {
    Schema::Tree { branches: [(Key::from(key), Schema::float())].into_iter().collect() }
}

// ── StreamProtocol ───────────────────────────────────────────────────

/// The `stream` protocol: an address `stream:<child.ys>` instantiates a
/// [`StreamProcess`] that proxies that child `.ys` over pipes — the pipe-transport
/// mirror of `RestProtocol`. `binary` overrides the chrysalis binary (tests point
/// it at the cargo-built one; `None` ⇒ `CHRYSALIS_BIN`/current exe).
#[derive(Debug, Default)]
pub struct StreamProtocol {
    pub binary: Option<String>,
}

impl Protocol for StreamProtocol {
    fn name(&self) -> &str {
        "stream"
    }
    fn instantiate(
        &self,
        data: &Value,
        _config: Value,
        _registry: &Arc<ProcessRegistry>,
    ) -> Result<ProcessNode, ProtocolError> {
        let path = data.as_str().ok_or_else(|| {
            ProtocolError::MalformedAddress(format!(
                "stream protocol expects data: String (a `.ys` path), got {data:?}"
            ))
        })?;
        let proc = match &self.binary {
            Some(bin) => StreamProcess::with_binary(path, bin.clone()),
            None => StreamProcess::new(path),
        };
        Ok(ProcessNode::Process(Box::new(proc)))
    }
}

/// A [`ProtocolRegistry`] with `local` + `stream`, folded into the chrysalis
/// `Core` so a `stream:` node in state is discovered and stepped like any local
/// process.
pub fn stream_protocols() -> ProtocolRegistry {
    let mut registry = ProtocolRegistry::new();
    registry.register(Arc::new(StreamProtocol::default()));
    registry
}
