//! Arrow-IPC wire codec for the delta-log [`Trace`](crate::trace).
//!
//! A trace serializes to an **Arrow IPC stream**. The Arrow schema carries the
//! prism element schema + name as **metadata** — the *mandatory header* a
//! consumer reads (and `refines`-checks) before any data flows. The frames then
//! travel as two columns: `time: Float64` and `payload: Utf8` (each cell the
//! JSON of one `Value` — row 0 the `initial`, rows 1.. the `deltas`). The
//! delta-log shape is preserved; replay is `apply` over the decoded rows.
//!
//! This is the real, streamable, Flight-ready wire. The payload is a JSON cell
//! today; densifying it to typed `Float64` columns for the fixed-shape case is a
//! contained follow-on — it changes only this batch encode/decode, not the
//! framing, the header, the streaming model, or any consumer.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::Arc;

use arrow::array::{Array, Float64Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema as ArrowSchema, SchemaRef};
use arrow::ipc::reader::StreamReader;
use arrow::ipc::writer::StreamWriter;
use prism_schema::Value;

const META_NAME: &str = "prism.trace.name";
const META_ELEMENT: &str = "prism.trace.element";
const COL_TIME: &str = "time";
const COL_PAYLOAD: &str = "payload";

/// Trace codec / IO error.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("arrow: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("malformed trace stream: {0}")]
    Malformed(String),
}

/// The Arrow header schema for a trace: two columns, plus the prism `element`
/// (a schema-as-value) and `name` carried as metadata. Carrying the element in
/// the header is what lets a consumer `refines`-check before consuming data.
fn header_schema(name: &str, element: &Value) -> Result<ArrowSchema, CodecError> {
    let mut md = HashMap::new();
    md.insert(META_NAME.to_string(), name.to_string());
    md.insert(META_ELEMENT.to_string(), serde_json::to_string(element)?);
    Ok(ArrowSchema::new(vec![
        Field::new(COL_TIME, DataType::Float64, false),
        Field::new(COL_PAYLOAD, DataType::Utf8, false),
    ])
    .with_metadata(md))
}

/// A one-batch `RecordBatch` of `(time, payload)` rows.
fn make_batch(schema: SchemaRef, times: &[f64], payloads: &[String]) -> Result<RecordBatch, CodecError> {
    let t = Float64Array::from(times.to_vec());
    let p = StringArray::from(payloads.iter().map(String::as_str).collect::<Vec<_>>());
    Ok(RecordBatch::try_new(schema, vec![Arc::new(t), Arc::new(p)])?)
}

/// The `(times, payload-JSON)` rows of a trace: row 0 = `initial`, rows 1.. =
/// `deltas` (aligned with `times`, one row per frame).
fn trace_rows(trace: &Value) -> Result<(Vec<f64>, Vec<String>), CodecError> {
    let times = crate::trace::times(trace);
    let mut payloads = Vec::with_capacity(times.len());
    if !times.is_empty() {
        let initial = trace.get_field("initial").cloned().unwrap_or(Value::None);
        payloads.push(serde_json::to_string(&initial)?);
        if let Some(deltas) = trace.get_field("deltas").and_then(|v| v.as_list()) {
            for d in deltas {
                payloads.push(serde_json::to_string(d)?);
            }
        }
    }
    Ok((times, payloads))
}

/// Reconstruct a [`Trace`](crate::trace) Value from header fields + decoded rows
/// (row 0 = `initial`, rows 1.. = `deltas`).
fn assemble(name: &str, element: Value, times: Vec<Value>, payloads: Vec<Value>) -> Value {
    let (initial, deltas) = match payloads.split_first() {
        Some((first, rest)) => (first.clone(), rest.to_vec()),
        None => (Value::None, Vec::new()),
    };
    Value::tree([
        ("_type", Value::from("Trace")),
        ("name", Value::from(name)),
        ("element", element),
        ("initial", initial),
        ("deltas", Value::List(deltas)),
        ("times", Value::List(times)),
    ])
}

/// Pull the `(time, payload-Value)` rows out of one `RecordBatch`.
fn batch_rows(batch: &RecordBatch) -> Result<Vec<(f64, Value)>, CodecError> {
    let t = batch
        .column_by_name(COL_TIME)
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>())
        .ok_or_else(|| CodecError::Malformed(format!("missing `{COL_TIME}` Float64 column")))?;
    let p = batch
        .column_by_name(COL_PAYLOAD)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| CodecError::Malformed(format!("missing `{COL_PAYLOAD}` Utf8 column")))?;
    let mut out = Vec::with_capacity(batch.num_rows());
    for i in 0..batch.num_rows() {
        out.push((t.value(i), serde_json::from_str(p.value(i))?));
    }
    Ok(out)
}

/// Serialize a [`Trace`](crate::trace) to an Arrow-IPC stream (header + one batch).
pub fn serialize_trace(trace: &Value) -> Result<Vec<u8>, CodecError> {
    let name = trace.get_field("name").and_then(|v| v.as_str()).unwrap_or("trace");
    let element = trace.get_field("element").cloned().unwrap_or(Value::None);
    let schema: SchemaRef = Arc::new(header_schema(name, &element)?);
    let (times, payloads) = trace_rows(trace)?;

    let mut buf = Vec::new();
    {
        let mut w = StreamWriter::try_new(&mut buf, schema.as_ref())?;
        if !times.is_empty() {
            w.write(&make_batch(schema.clone(), &times, &payloads)?)?;
        }
        w.finish()?;
    }
    Ok(buf)
}

/// Deserialize an Arrow-IPC trace stream back to a [`Trace`](crate::trace) Value.
/// Inverse of [`serialize_trace`]: `deserialize_trace(serialize_trace(t)) ≡ t`.
pub fn deserialize_trace(bytes: &[u8]) -> Result<Value, CodecError> {
    let reader = StreamReader::try_new(bytes, None)?;
    let (name, element) = header_of(reader.schema().metadata())?;

    let mut times = Vec::new();
    let mut payloads = Vec::new();
    for batch in reader {
        for (t, v) in batch_rows(&batch?)? {
            times.push(Value::float(t));
            payloads.push(v);
        }
    }
    Ok(assemble(&name, element, times, payloads))
}

/// Read the `(name, element)` header out of an Arrow schema's metadata.
fn header_of(md: &HashMap<String, String>) -> Result<(String, Value), CodecError> {
    let name = md.get(META_NAME).cloned().unwrap_or_else(|| "trace".to_string());
    let element = md.get(META_ELEMENT).map(|s| serde_json::from_str(s)).transpose()?.unwrap_or(Value::None);
    Ok((name, element))
}

/// Streaming writer: writes the header on construction, then one frame per
/// [`push`](TraceWriter::push) (the first is the `initial`, the rest `deltas`).
/// The pipe/persistence form of [`serialize_trace`].
pub struct TraceWriter<W: Write> {
    inner: StreamWriter<W>,
    schema: SchemaRef,
}

impl<W: Write> TraceWriter<W> {
    /// Create a writer over `w` for a trace whose element is `element` (a
    /// schema-as-value) — writing the mandatory header immediately.
    pub fn new(w: W, name: &str, element: &Value) -> Result<Self, CodecError> {
        let schema: SchemaRef = Arc::new(header_schema(name, element)?);
        let inner = StreamWriter::try_new(w, schema.as_ref())?;
        Ok(Self { inner, schema })
    }

    /// Append one frame (`time`, `value`): the first call is the `initial`
    /// absolute state, each subsequent call a delta.
    pub fn push(&mut self, time: f64, value: &Value) -> Result<(), CodecError> {
        let batch = make_batch(self.schema.clone(), &[time], &[serde_json::to_string(value)?])?;
        self.inner.write(&batch)?;
        Ok(())
    }

    /// Flush the IPC end-of-stream marker.
    pub fn finish(mut self) -> Result<(), CodecError> {
        self.inner.finish()?;
        Ok(())
    }
}

/// Streaming reader: the header exposes the carried `element` (for a `refines`
/// check before consuming); iteration yields `(time, value)` per frame — the
/// first the `initial`, the rest `deltas`. The dual of [`TraceWriter`].
pub struct TraceReader<R: Read> {
    inner: StreamReader<R>,
    element: Value,
    name: String,
    buf: VecDeque<(f64, Value)>,
}

impl<R: Read> TraceReader<R> {
    /// Open a reader over `r`, reading the header (so [`element`](Self::element)
    /// is available before any frame).
    pub fn new(r: R) -> Result<Self, CodecError> {
        let inner = StreamReader::try_new(r, None)?;
        let (name, element) = header_of(inner.schema().metadata())?;
        Ok(Self { inner, element, name, buf: VecDeque::new() })
    }

    /// The carried element schema as a Value (`prism_schema::value_to_schema`
    /// for a `Schema`); the basis for the connect-time `refines` check.
    pub fn element(&self) -> &Value {
        &self.element
    }

    /// The trace name from the header.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl<R: Read> Iterator for TraceReader<R> {
    type Item = Result<(f64, Value), CodecError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(row) = self.buf.pop_front() {
                return Some(Ok(row));
            }
            match self.inner.next()? {
                Ok(batch) => match batch_rows(&batch) {
                    Ok(rows) => self.buf.extend(rows),
                    Err(e) => return Some(Err(e)),
                },
                Err(e) => return Some(Err(e.into())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{frames, trace_of};
    use prism_schema::{algebra, value_to_schema, Schema};

    fn map_float() -> Schema {
        Schema::Map { value: Box::new(Schema::float()) }
    }
    fn mf(pairs: &[(&str, f64)]) -> Value {
        Value::tree(pairs.iter().map(|(k, v)| (*k, Value::float(*v))))
    }
    fn sample() -> Value {
        trace_of(
            "kinetics",
            &map_float(),
            vec![
                (0.0, mf(&[("a", 1.0), ("b", 0.0)])),
                (1.0, mf(&[("a", 0.5), ("b", 0.5)])),
                (2.0, mf(&[("a", 0.25), ("b", 0.75)])),
            ],
        )
    }

    #[test]
    fn round_trips_through_arrow_ipc() {
        let trace = sample();
        let bytes = serialize_trace(&trace).expect("serialize");
        let back = deserialize_trace(&bytes).expect("deserialize");
        assert_eq!(back, trace, "deserialize(serialize(trace)) ≡ trace");
    }

    #[test]
    fn header_carries_the_element_schema() {
        let bytes = serialize_trace(&sample()).unwrap();
        let reader = TraceReader::new(std::io::Cursor::new(bytes)).unwrap();
        // The mandatory header: the element schema is available BEFORE any frame.
        assert_eq!(value_to_schema(reader.element()).expect("element"), map_float());
        assert_eq!(reader.name(), "kinetics");
    }

    #[test]
    fn empty_trace_round_trips() {
        let trace = trace_of("empty", &map_float(), Vec::<(f64, Value)>::new());
        let back = deserialize_trace(&serialize_trace(&trace).unwrap()).unwrap();
        assert_eq!(back, trace);
    }

    #[test]
    fn streaming_writer_reader_replays_to_frames() {
        let trace = sample();
        let want = frames(&trace);
        let times = crate::times(&trace);
        let element = trace.get_field("element").unwrap();

        let mut buf = Vec::new();
        {
            let mut w = TraceWriter::new(&mut buf, "kinetics", element).unwrap();
            w.push(times[0], trace.get_field("initial").unwrap()).unwrap(); // initial
            for (i, d) in trace.get_field("deltas").and_then(|v| v.as_list()).unwrap().iter().enumerate() {
                w.push(times[i + 1], d).unwrap(); // deltas
            }
            w.finish().unwrap();
        }

        let reader = TraceReader::new(std::io::Cursor::new(buf)).unwrap();
        let rows: Vec<(f64, Value)> = reader.map(Result::unwrap).collect();
        assert_eq!(rows.len(), 3);

        // Fold the streamed (initial, deltas…) back to the frames.
        let elem = map_float();
        let mut cur = rows[0].1.clone();
        let mut got = vec![cur.clone()];
        for (_, d) in &rows[1..] {
            cur = algebra::apply(&elem, &cur, d);
            got.push(cur.clone());
        }
        assert_eq!(got, want, "streamed (time, delta) rows replay to the original frames");
    }
}
