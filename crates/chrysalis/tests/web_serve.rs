//! `chrysalis serve` (Slice 0, `docs/web-bigraphs.md`): build a `.ys` engine, serve
//! it as a live web page, and prove the **server-authoritative loop** — a `step`/
//! `time` intent advances the engine and the rendered state reflects it. The web
//! boundary is `prism_bigraph::protocols::web`; this drives it end-to-end from a real
//! `.ys` through the run path (`parse_file` → `build_engine`). No HTTP-client dep — a
//! tiny `TcpStream` helper speaks the handful of routes.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;

use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_core, std_modules};
use chrysalis::runner::build_engine;
use prism_bigraph::protocols::web::serve_web_default;

fn ys(name: &str) -> String {
    format!("{}/ys/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Minimal HTTP/1.1 over `TcpStream` — returns the response body (the server sends
/// `Connection: close`, so read-to-EOF is the whole response).
fn http(base: &str, method: &str, path: &str, body: Option<&str>) -> String {
    let addr = base.trim_start_matches("http://");
    let mut stream = TcpStream::connect(addr).expect("connect to the web server");
    let req = match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{b}",
            b.len()
        ),
        None => format!("{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"),
    };
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    resp.splitn(2, "\r\n\r\n").nth(1).unwrap_or("").to_string()
}

#[test]
fn serve_renders_and_an_intent_advances_the_engine() {
    // bump.ys: a `Bump` process adds +1 to `v` each tick — a self-contained counter.
    let prog = parse_file(&ys("bump.ys")).expect("parse bump.ys");
    let engine =
        build_engine(&prog, std_core(), std_modules(), &BTreeMap::new()).expect("build engine");
    let server = serve_web_default(engine, "127.0.0.1:0").expect("serve");
    let base = server.base_url();

    // The shell page loads.
    assert!(http(&base, "GET", "/", None).contains("bigraph viewer"), "shell served");

    // Initial render: the place graph (the `v` slot) at t = 0.
    let s0 = http(&base, "GET", "/state", None);
    assert!(s0.contains("v"), "state renders the v slot: {s0}");
    assert!(s0.contains("t = 0"), "the initial clock is zero: {s0}");

    // Intents (eval pointed inward): a step, then apply time. The engine steps HERE
    // (server-authoritative); each response is the freshly-rendered state.
    let _ = http(&base, "POST", "/intent", Some(r#"{"kind":"step"}"#));
    let s1 = http(&base, "POST", "/intent", Some(r#"{"kind":"time","dt":3}"#));

    // The loop closed: the state changed and the clock advanced past 0.
    assert_ne!(s0, s1, "state changed after stepping (the server-authoritative loop)");
    assert!(!s1.contains("t = 0"), "the clock advanced after applying time: {s1}");

    // An unknown intent is a clean 4xx (not a panic) — the body still round-trips.
    // (We only check the server stays alive for a subsequent valid request.)
    let _ = http(&base, "POST", "/intent", Some(r#"{"kind":"bogus"}"#));
    assert!(http(&base, "GET", "/state", None).contains("v"), "server still serves after a bad intent");
}
