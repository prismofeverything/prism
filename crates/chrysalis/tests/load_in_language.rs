//! Programs ARE data — demonstrated in two paths that share one runtime:
//!
//! 1. `load(path) -> Document value` reads a `.ys` file from disk into a
//!    plain `Value::Map` (`{_type: "Document", schema, state, _source}`).
//! 2. A `Value::Map` constructed BY HAND with the SAME shape runs through
//!    the SAME `Document.run(time)` method. No special path; map-construction
//!    is term-construction is data, and the runtime walks the data.
//!
//! Both reach the same final state. The principle (à la Lisp's `cons` +
//! `eval`): programs are values; the host functions that build them are no
//! more privileged than user code building them by hand.

use std::path::PathBuf;
use std::sync::Arc;

use chrysalis::prelude::{load, std_methods};
use indexmap::IndexMap;
use prism_schema::{Key, Value};

const TICK_YS: &str = "\
# A minimal counter — Tick adds 1 to count each step.
process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)
composite Main ->{count :: Float} (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}
)
";

/// Workspace tmpdir, deleted on Drop.
struct ScratchDir(PathBuf);
impl ScratchDir {
    fn new(name: &str) -> Self {
        let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("load-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn load_returns_program_as_value() {
    // Drop a tick.ys to disk so `load(path)` has something to read.
    let dir = ScratchDir::new("returns");
    let tick_path = dir.0.join("tick.ys");
    std::fs::write(&tick_path, TICK_YS).expect("write tick.ys");

    // `load(path)` is the `io::load` native function. From Rust we call the
    // public form directly; surface `.ys` callers reach the same code via
    // `from io import load` + `load("path")`.
    let doc = load(tick_path.to_str().unwrap()).expect("load returns a Document");

    // The returned value IS a plain `Value::Map`. The shape is the homoiconic
    // wire form: `{_type: "Document", schema, state, _source}`. Walkable like
    // any tree-of-maps.
    let map = doc.as_map().expect("Document is a Map");
    assert_eq!(
        map.get("_type").and_then(|v| v.as_str()),
        Some("Document"),
        "the Document carries its own `_type` tag"
    );
    assert!(
        map.contains_key("schema"),
        "the schema is part of the value"
    );
    assert!(
        map.contains_key("state"),
        "the state is part of the value"
    );
    assert_eq!(
        map.get("_source").and_then(|v| v.as_str()),
        Some(tick_path.to_str().unwrap()),
        "the Document remembers its source — so `.run` can recompile the program's Core"
    );
}

#[test]
fn document_run_dispatches_for_loaded_and_hand_built_alike() {
    let dir = ScratchDir::new("dispatch");
    let tick_path = dir.0.join("tick.ys");
    std::fs::write(&tick_path, TICK_YS).expect("write tick.ys");

    // Path A — LOADED: read the source via `load(path)`.
    let loaded = load(tick_path.to_str().unwrap()).expect("load(tick.ys)");

    // Path B — HAND-BUILT: a `Value::Map` constructed directly. The schema
    // is omitted (defaults to Any); `_source` is set so `.run` knows where
    // to fetch the program's user-defined process factories (the same
    // bookkeeping `load` writes). This is what a `.ys` author would type
    // when assembling a program from primitives: just map literals.
    let mut hand: IndexMap<Key, Value> = IndexMap::new();
    hand.insert(Key::from("_type"), Value::String("Document".into()));
    hand.insert(
        Key::from("_source"),
        Value::String(tick_path.to_string_lossy().into()),
    );
    let hand_built = Value::Map(hand);

    // Both flow through the SAME `Document.run(time)` method dispatcher.
    // (Same way Lisp's `eval` doesn't care whether the form came from `read`
    // or from `cons`/`list` — the interpreter sees a value either way.)
    let methods = Arc::new(std_methods());
    let arg = Value::float(5.0);
    let a = methods
        .dispatch(&loaded, "run", std::slice::from_ref(&arg))
        .expect("loaded.run(5)");
    let b = methods
        .dispatch(&hand_built, "run", std::slice::from_ref(&arg))
        .expect("hand_built.run(5)");

    // The two paths produce identical final states. Programs ARE data: the
    // representation a host function builds and the representation a user
    // assembles by hand are the SAME value, interpreted by the SAME runtime.
    let a_count = a.get_field("count").and_then(|v| v.as_f64());
    let b_count = b.get_field("count").and_then(|v| v.as_f64());
    assert_eq!(
        a_count, b_count,
        "loaded.run vs hand_built.run must agree — the homoiconic identity\n\
         loaded   count = {a_count:?}\n\
         hand     count = {b_count:?}"
    );
    assert_eq!(
        a_count,
        Some(5.0),
        "after 5 ticks of +1/step, count = 5"
    );
}
