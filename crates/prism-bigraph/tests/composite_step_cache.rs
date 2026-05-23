//! Per-composite cache delegation (b): a composite caches its OWN step network.
//! The cache directive crosses the boundary as DATA in `config.cache`
//! (`{dir, forced}`), never a Rust handle — so building the composite a second
//! time skips its fresh inner steps, and forcing one re-runs it + its dependents.
//! This is the recursive form of the top-level incremental-step cache, reached
//! only through the composite interface (so it works for a remote composite too).

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{ProcessNode, Step};
use prism_bigraph::{Core, Schema, Update, Value};

type Counters = Arc<Mutex<HashMap<String, usize>>>;

/// A step that records each firing under its `tag`, reads its (optional) input
/// port, and emits `input + add`.
#[derive(Debug)]
struct Recording {
    tag: String,
    in_port: Option<String>,
    out_port: String,
    add: f64,
    counters: Counters,
}
impl Step for Recording {
    fn inputs(&self) -> IndexMap<String, Schema> {
        match &self.in_port {
            Some(p) => IndexMap::from([(p.clone(), Schema::float())]),
            None => IndexMap::new(),
        }
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(self.out_port.clone(), Schema::Overwrite { inner: Box::new(Schema::float()) })])
    }
    fn update(&self, state: &Value) -> Update {
        *self.counters.lock().unwrap().entry(self.tag.clone()).or_insert(0) += 1;
        let input = self
            .in_port
            .as_ref()
            .and_then(|p| state.get_field(p))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Update::value(Value::tree([(self.out_port.as_str(), Value::float(input + self.add))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A core whose `Recording` factory reads `{tag, in_port, out_port, add}` from
/// config and shares the firing `counters`.
fn recording_core(counters: &Counters) -> Core {
    let mut registry = ProcessRegistry::new();
    let counters = Arc::clone(counters);
    registry.register("Recording", move |config| {
        let s = |k: &str| config.get_field(k).and_then(|v| v.as_str()).map(String::from);
        ProcessNode::Step(Box::new(Recording {
            tag: s("tag").unwrap_or_default(),
            in_port: s("in_port"),
            out_port: s("out_port").unwrap_or_else(|| "out".to_string()),
            add: config.get_field("add").and_then(|v| v.as_f64()).unwrap_or(0.0),
            counters: Arc::clone(&counters),
        }))
    });
    Core::from(Arc::new(registry))
}

fn wire(seg: &str) -> Value {
    Value::List(vec![Value::String(seg.into())])
}

/// A `Recording` step node addressed `local:Recording`.
fn step(tag: &str, in_seg: Option<&str>, out_seg: &str, add: f64) -> Value {
    let mut config = vec![
        ("tag".to_string(), Value::String(tag.into())),
        ("out_port".to_string(), Value::String("out".into())),
        ("add".to_string(), Value::float(add)),
    ];
    let inputs = match in_seg {
        Some(seg) => {
            config.push(("in_port".to_string(), Value::String("in".into())));
            Value::tree([("in", wire(seg))])
        }
        None => Value::map(),
    };
    Value::tree([
        ("address", Value::String("local:Recording".into())),
        ("config", Value::Map(config.into_iter().map(|(k, v)| (k.into(), v)).collect())),
        ("inputs", inputs),
        ("outputs", Value::tree([("out", wire(out_seg))])),
    ])
}

/// A composite whose inner step DAG is a → b → c over slots va/vb/vc, with a
/// `cache` directive forcing the steps named in `forced`.
fn composite_config(cache_dir: &std::path::Path, forced: &[&str]) -> Value {
    let state = Value::tree([
        ("va", Value::float(0.0)),
        ("vb", Value::float(0.0)),
        ("vc", Value::float(0.0)),
        ("a", step("a", None, "va", 1.0)),
        ("b", step("b", Some("va"), "vb", 10.0)),
        ("c", step("c", Some("vb"), "vc", 100.0)),
    ]);
    let cache = Value::tree([
        ("dir", Value::String(cache_dir.display().to_string())),
        (
            "forced",
            Value::List(forced.iter().map(|s| Value::String((*s).into())).collect()),
        ),
    ]);
    Value::tree([
        ("state", state),
        ("bridge", Value::tree([("inputs", Value::map()), ("outputs", Value::map())])),
        ("cache", cache),
    ])
}

fn fires(c: &Counters) -> (usize, usize, usize) {
    let m = c.lock().unwrap();
    (
        m.get("a").copied().unwrap_or(0),
        m.get("b").copied().unwrap_or(0),
        m.get("c").copied().unwrap_or(0),
    )
}

#[test]
fn composite_caches_its_own_inner_steps_via_config() {
    let dir = std::env::temp_dir().join(format!("prism-composite-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let counters: Counters = Arc::new(Mutex::new(HashMap::new()));
    let core = recording_core(&counters);

    // Build 1: empty cache ⇒ the inner DAG runs (settles) once.
    let _c1 = Composite::from_config(&composite_config(&dir, &[]), &core).expect("from_config");
    assert_eq!(fires(&counters), (1, 1, 1), "build 1: inner steps fire");

    // Build 2: same cache dir, nothing forced ⇒ inner steps SKIP (reloaded).
    let _c2 = Composite::from_config(&composite_config(&dir, &[]), &core).expect("from_config");
    assert_eq!(fires(&counters), (1, 1, 1), "build 2: inner steps skip (cached)");

    // Build 3: force `b` ⇒ b re-runs, c cascades, a stays cached.
    let _c3 = Composite::from_config(&composite_config(&dir, &["b"]), &core).expect("from_config");
    assert_eq!(fires(&counters), (1, 2, 2), "build 3: forcing b re-runs b + dependent c");

    let _ = std::fs::remove_dir_all(&dir);
}
