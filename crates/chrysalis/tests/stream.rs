//! The `stream:` protocol, child side: `serve_stream` runs a `.ys` composite as a
//! live `Trace[In] → Trace[Out]` Arrow filter (read frame → step → emit frame).
//! Proven both in-process (a `Cursor`) and as a *real subprocess* driven over OS
//! pipes — a `.ys` program proxied as a streaming process, the heart of #19.

use std::collections::BTreeMap;
use std::io::Cursor;

use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::{invoke_trace, serve_stream};
use prism_schema::Value;

// Producer: `n` grows by 1 each tick (interval 1.0).
const COUNTER: &str = "\
process Tick ~{n :: Float} ->{n :: Float} ( {n: 1.0} )

composite Counter[start :: Float = 0.0] ->{n :: Float @ n} (
  n: start |
  Tick ~{n: n} ->{n: n}
)
";

// Filter: mirrors each driven input `n` straight to output `out` (shared inner v).
const ECHO: &str = "\
composite Echo ~{n :: Float @ v} ->{out :: Float @ v} (
  v: n
)
";

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn outs(trace: &Value, key: &str) -> Vec<f64> {
    prism_trace::frames(trace)
        .iter()
        .map(|f| {
            f.get_field(key)
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN)
        })
        .collect()
}

/// A Counter output trace `n = [0..=top]`, to drive an Echo with.
fn drive_trace(top: f64) -> Value {
    let counter = chrysalis::parse::parse_program(COUNTER).expect("parse counter");
    invoke_trace(
        &counter,
        std_registry(),
        std_methods(),
        std_modules(),
        &args(&[("start", "0.0")]),
        top,
        1.0,
    )
    .expect("counter trace")
}

#[test]
fn serve_stream_is_a_live_trace_filter() {
    // Drive: n = [0,1,2,3,4,5]. Echo mirrors each frame, so out follows n.
    let in_bytes = prism_trace::serialize_trace(&drive_trace(5.0)).expect("serialize");

    let echo = chrysalis::parse::parse_program(ECHO).expect("parse echo");
    let mut out_bytes: Vec<u8> = Vec::new();
    serve_stream(
        &echo,
        std_registry(),
        std_methods(),
        std_modules(),
        &BTreeMap::new(),
        Cursor::new(in_bytes),
        &mut out_bytes,
    )
    .expect("serve_stream");

    let out = prism_trace::deserialize_trace(&out_bytes).expect("deserialize");
    assert_eq!(
        outs(&out, "out"),
        vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        "the filter emits one output frame per input frame"
    );
}

#[test]
fn ys_program_proxies_as_a_streaming_process_over_pipes() {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};

    // Write the child program to a temp file.
    let echo_path =
        std::env::temp_dir().join(format!("prism_stream_echo_{}.ys", std::process::id()));
    std::fs::write(&echo_path, ECHO).expect("write echo.ys");

    // Spawn it as a streaming filter over OS pipes — the real proxy.
    let mut child = Command::new(env!("CARGO_BIN_EXE_chrysalis"))
        .args(["run", echo_path.to_str().unwrap(), "--serve-stream"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn chrysalis --serve-stream");

    // Drive it with n = [0,1,2] (small — fits the pipe buffer without lock-step).
    let in_bytes = prism_trace::serialize_trace(&drive_trace(2.0)).expect("serialize");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&in_bytes)
        .expect("write child stdin"); // drop ⇒ EOF

    let mut out_bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut out_bytes)
        .expect("read child stdout");
    child.wait().expect("reap child");
    std::fs::remove_file(&echo_path).ok();

    let out = prism_trace::deserialize_trace(&out_bytes).expect("deserialize child output");
    assert_eq!(
        outs(&out, "out"),
        vec![0.0, 1.0, 2.0],
        "the child echoed each driven frame back over the pipe"
    );
}
