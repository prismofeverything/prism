//! `web` — the world-boundary's **browser backend** (`categorical-core.md` §7b):
//! serve a running [`Engine`] as a live web page. **`render` = `quote` pointed at a
//! browser** (state → DOM); **intents = `eval` from the browser** (`step`/`time`/
//! `fire` → the engine). **Server-authoritative**: the engine + BRS live here; the
//! browser renders + sends intents; the server steps and the next state reflects it
//! (ORGANISM's loop). Drop ANY `.ys` in → explore / step / apply-time in a browser.
//!
//! Built on [`super::http`] — the SAME serve door as [`super::rest_server`] (peer-RPC)
//! and [`super::registry`] (package store). web / audio / mesh / file are ONE
//! world-boundary; this is its browser backend.
//!
//! ## The boundary vs the functor — render is INJECTED
//! The *boundary* (serve loop, intents, the HTML shell) lives here. The *render
//! functor* (bigraph → DOM/SVG) is `prism-viz`'s — but `prism-viz` deps
//! `prism-bigraph`, so it cannot be called from here (a cycle). So render is
//! **injected** as a `Fn(&Value, f64) -> String`. We ship a schema-only
//! [`default_render`] (the place graph as nested HTML) so the boundary works
//! standalone; the rich viz render (the link graph, SVG, #11/#65 — ONE `Render`
//! functor over different target props) is injected by chrysalis. *Slice 0.*
//!
//! ## Wire
//! ```text
//! GET  /         → the HTML shell (renders state, posts intents)
//! GET  /state    → the rendered state (server-side render = quote→DOM)
//! POST /intent   → {"kind":"step"} | {"kind":"time","dt":N} → step; returns new state
//! ```

use std::sync::{Arc, Mutex};

use prism_schema::Value;

use crate::engine::Engine;
use crate::protocols::http::{self, HttpServer};

/// A render functor: a running engine's state (+ its time) → an HTML body. The
/// generic seam `prism-viz` injects through (so the boundary never deps viz).
pub type Render = Arc<dyn Fn(&Value, f64) -> String + Send + Sync>;

type SharedEngine = Arc<Mutex<Engine>>;

/// A live web view of a running [`Engine`] — a [`route`] handler over the shared
/// [`HttpServer`] plumbing. Drop shuts it down and joins.
pub struct WebServer {
    server: HttpServer,
    engine: SharedEngine,
}

impl WebServer {
    /// The bound port.
    pub fn port(&self) -> u16 {
        self.server.port()
    }

    /// `http://127.0.0.1:{port}` — open this in a browser.
    pub fn base_url(&self) -> String {
        self.server.base_url()
    }

    /// The engine, for tests / introspection (locked).
    pub fn engine(&self) -> &SharedEngine {
        &self.engine
    }
}

/// Serve `engine` as a live web page on `addr` (e.g. `"127.0.0.1:0"` for an
/// OS-chosen port, or `"0.0.0.0:8780"` to expose it). `render` turns the engine
/// state into the HTML body — inject `prism-viz`'s functor, or use
/// [`serve_web_default`] for the built-in schema-only render.
pub fn serve_web<R>(engine: Engine, render: R, addr: impl std::net::ToSocketAddrs) -> std::io::Result<WebServer>
where
    R: Fn(&Value, f64) -> String + Send + Sync + 'static,
{
    serve_web_arc(engine, Arc::new(render), addr)
}

/// Serve with the built-in [`default_render`] (the place graph as nested HTML). The
/// zero-dependency form — works without `prism-viz`.
pub fn serve_web_default(engine: Engine, addr: impl std::net::ToSocketAddrs) -> std::io::Result<WebServer> {
    serve_web_arc(engine, Arc::new(default_render), addr)
}

fn serve_web_arc(engine: Engine, render: Render, addr: impl std::net::ToSocketAddrs) -> std::io::Result<WebServer> {
    let engine: SharedEngine = Arc::new(Mutex::new(engine));
    let handler = {
        let engine = Arc::clone(&engine);
        move |req: &http::Request| route(req, &engine, &render)
    };
    let server = HttpServer::start(addr, handler)?;
    Ok(WebServer { server, engine })
}

/// The web handler: serve the shell, render the state, apply an intent. The HTTP
/// plumbing lives in [`super::http`].
fn route(req: &http::Request, engine: &SharedEngine, render: &Render) -> (&'static str, String) {
    match (req.method.as_str(), req.path.as_str()) {
        // The shell page — renders state, wires the intent buttons.
        ("GET", "/") | ("GET", "/index.html") => ("200 OK", shell_html()),

        // The rendered state (server-side render = quote→DOM).
        ("GET", "/state") => ("200 OK", render_now(engine, render)),

        // An intent: step / apply-time. eval pointed inward — it mutates the engine,
        // then we return the freshly rendered state so the client updates at once.
        ("POST", "/intent") => {
            let parsed: serde_json::Value =
                serde_json::from_str(&req.body).unwrap_or(serde_json::Value::Null);
            let kind = parsed.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            {
                let mut e = engine.lock().unwrap();
                match kind {
                    "step" => {
                        e.tick();
                    }
                    "time" => {
                        let dt = parsed.get("dt").and_then(|d| d.as_f64()).unwrap_or(1.0);
                        e.run(dt);
                    }
                    _ => return ("400 Bad Request", json_msg("unknown intent kind")),
                }
            }
            ("200 OK", render_now(engine, render))
        }

        _ => ("404 Not Found", json_msg("not found")),
    }
}

fn render_now(engine: &SharedEngine, render: &Render) -> String {
    let e = engine.lock().unwrap();
    render(e.state(), e.time())
}

fn json_msg(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

// ── the built-in render: the place graph as nested HTML ──────────────

/// The built-in schema-only render: the engine state's **place graph** as nested
/// HTML (a node's children nested under it; scalars as leaves), plus the clock. The
/// minimal `Value→DOM` functor — `prism-viz` grows the rich one (the link graph,
/// SVG). Generic: works for any `.ys`.
pub fn default_render(state: &Value, time: f64) -> String {
    let mut s = String::new();
    s.push_str(&format!("<div class=\"clock\">t = {time}</div>"));
    s.push_str("<div class=\"bigraph\">");
    render_node(&mut s, "root", state);
    s.push_str("</div>");
    s
}

fn render_node(out: &mut String, name: &str, value: &Value) {
    match value {
        Value::Map(m) => {
            out.push_str(&format!(
                "<div class=\"node\"><span class=\"name\">{}</span><div class=\"children\">",
                esc(name)
            ));
            for (k, v) in m {
                render_node(out, k.as_str(), v);
            }
            out.push_str("</div></div>");
        }
        Value::List(items) => {
            out.push_str(&format!(
                "<div class=\"node\"><span class=\"name\">{}</span><div class=\"children\">",
                esc(name)
            ));
            for (i, v) in items.iter().enumerate() {
                render_node(out, &i.to_string(), v);
            }
            out.push_str("</div></div>");
        }
        scalar => {
            out.push_str(&format!(
                "<div class=\"leaf\"><span class=\"name\">{}</span><span class=\"val\">{}</span></div>",
                esc(name),
                esc(&scalar_text(scalar))
            ));
        }
    }
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::None => "∅".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format!("{}", f.0),
        Value::String(s) => s.clone(),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
        other => format!("{other:?}"),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The shell page — minimal HTML + JS: load `/state`, render it, and wire the
/// intent buttons (`step` / `play` / `+time`). Intentionally dependency-free
/// (no framework): the server renders; the browser displays + posts intents.
fn shell_html() -> String {
    r##"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>prism · bigraph viewer</title>
<style>
  body { font: 14px/1.5 ui-monospace, Menlo, monospace; margin: 0; background:#0f1115; color:#d7dae0; }
  header { padding: 10px 16px; background:#171a21; border-bottom:1px solid #262b36; display:flex; gap:8px; align-items:center; }
  header h1 { font-size:14px; margin:0 12px 0 0; color:#9aa4b2; font-weight:600; }
  button { font:inherit; background:#222834; color:#d7dae0; border:1px solid #323a49; border-radius:6px; padding:5px 11px; cursor:pointer; }
  button:hover { background:#2b3342; }
  button.on { background:#2d6; color:#06210f; border-color:#2d6; }
  #view { padding: 16px; }
  .clock { color:#7f8a9a; margin-bottom:10px; }
  .node { margin-left: 14px; border-left:1px solid #262b36; padding-left:10px; }
  .node > .name { color:#7fb0ff; }
  .leaf { margin-left: 14px; padding-left:10px; }
  .leaf .name { color:#9aa4b2; }
  .leaf .name::after { content:" = "; color:#4b5566; }
  .val { color:#e6c07b; }
</style>
</head>
<body>
<header>
  <h1>prism · bigraph viewer</h1>
  <button onclick="intent({kind:'step'})">step</button>
  <button id="play" onclick="togglePlay()">▶ play</button>
  <button onclick="intent({kind:'time',dt:1})">+1 time</button>
</header>
<div id="view">loading…</div>
<script>
async function intent(body) {
  const r = await fetch('/intent', {method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body)});
  if (r.ok) document.getElementById('view').innerHTML = await r.text();
}
async function refresh() {
  const r = await fetch('/state');
  if (r.ok) document.getElementById('view').innerHTML = await r.text();
}
let timer = null;
function togglePlay() {
  const b = document.getElementById('play');
  if (timer) { clearInterval(timer); timer = null; b.classList.remove('on'); b.textContent = '▶ play'; }
  else { timer = setInterval(() => intent({kind:'step'}), 400); b.classList.add('on'); b.textContent = '⏸ pause'; }
}
refresh();
</script>
</body>
</html>
"##
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Core;
    use prism_schema::{Key, Schema, StateMap};

    fn counter_state() -> (Schema, Value) {
        let mut m = StateMap::new();
        m.insert(Key::from("count"), Value::Float(0.0.into()));
        (Schema::map(Schema::float()), Value::Map(m))
    }

    #[test]
    fn default_render_shows_the_place_graph() {
        let (_s, v) = counter_state();
        let html = default_render(&v, 3.0);
        assert!(html.contains("t = 3"));
        assert!(html.contains("count"));
    }

    #[test]
    fn shell_is_served_and_state_renders() {
        let (schema, state) = counter_state();
        let engine = Engine::from_state(schema, state, Core::new()).unwrap();
        let server = serve_web_default(engine, "127.0.0.1:0").unwrap();
        let base = server.base_url();

        let shell = ureq::get(&base).call().unwrap().into_string().unwrap();
        assert!(shell.contains("bigraph viewer"));
        let state = ureq::get(&format!("{base}/state")).call().unwrap().into_string().unwrap();
        assert!(state.contains("count"));
    }
}
