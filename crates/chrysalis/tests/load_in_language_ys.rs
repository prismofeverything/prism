//! The homoiconic demo in surface `.ys` — both `load(path).run(t)` AND a
//! hand-built `{_type: 'Document', _source: path}` value run through the
//! same `Document.run(t)` dispatch and reach the same final state.
//!
//! Pairs with `load_in_language.rs` (which exercises the same paths via the
//! Rust API). Here the demo runs through the CLI bin, so the principle is
//! visible to anyone running `chrysalis run` — programs are data, callable
//! and constructible inside `.ys` itself.

use std::path::PathBuf;
use std::process::Command;

fn chrysalis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chrysalis")
}

const TICK_YS: &str = "\
process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)
composite Main ->{count :: Float} (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}
)
";

/// The demo composite: BOTH paths produce the loaded program's count
/// after 5 ticks. The `via_loaded` arm calls `load(path).run(5).count`.
/// The `via_manual` arm builds a `{_type: 'Document', _source: path}` value
/// with map literals (no `load`!) and runs it the same way.
const DEMO_YS_TEMPLATE: &str = "\
# Programs are data — two paths, one runtime.
#
# `via_loaded` reads the program from disk via the io::load native function.
# `via_manual` constructs an EQUIVALENT Document value by hand using only
# map literals — the same syntax any chrysalis value uses. Both flow through
# the same `Document.run(time)` method dispatcher.
#
# Lisp parallel: load ~ read, Document.run ~ eval, map literals are cons/list.
from io import load

composite Demo ~{} ->{
  via_loaded :: Float,
  via_manual :: Float
} (
  via_loaded: load('TICK_PATH').run(5.0).count |
  via_manual: {
    '_type': 'Document',
    '_source': 'TICK_PATH'
  }.run(5.0).count
)
";

#[test]
fn programs_are_data_in_surface_ys() {
    // Workspace tmpdir, deleted on Drop.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("load-in-language-ys");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    let tick_path = dir.join("tick.ys");
    let demo_path = dir.join("demo.ys");
    std::fs::write(&tick_path, TICK_YS).expect("write tick.ys");
    let demo_src = DEMO_YS_TEMPLATE
        .replace("TICK_PATH", tick_path.to_str().unwrap());
    std::fs::write(&demo_path, demo_src).expect("write demo.ys");

    let out = Command::new(chrysalis_bin())
        .args(["run", demo_path.to_str().unwrap(), "--time", "0"])
        .output()
        .expect("spawn chrysalis run");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "chrysalis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );

    // The output is the composite's output record. The TWO arms agree (the
    // homoiconic identity) AND match the loaded program's actual answer (5).
    assert!(
        stdout.contains("\"via_loaded\": 5.0"),
        "loaded arm should reach 5.0:\n{stdout}"
    );
    assert!(
        stdout.contains("\"via_manual\": 5.0"),
        "hand-built arm should reach 5.0 (the same as loaded):\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
