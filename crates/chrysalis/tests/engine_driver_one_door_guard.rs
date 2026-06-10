//! Engine-driver one-door guard — `simplify` / invariant #3 of
//! `docs/homoiconic-unification.md` ("no second eval").
//!
//! ## What this pins
//! The reflective tower has two rungs: the UPPER (surface `eval` / `meta::eval`:
//! `Expr → spec data`) and the LOWER (instantiate / discover: `spec data →
//! running`). The LOWER rung's engine itself — discover specs + the BSP tick — is
//! prism's (`prism_bigraph::Engine`); chrysalis DRIVES it, never clones it
//! (thin-layer). This guard pins that the *driver* — the
//! `Engine::from_state → discover_all_processes → run → state` orchestration — lives
//! in exactly ONE chrysalis module, `runner.rs` (the one-shot `runner::run_state` +
//! the streaming/sampling runner). Every surface path routes through it:
//!   - `runner::run` (a compiled program) and `runner::run_document` (a Document)
//!     call `run_state`;
//!   - the surface `instantiate` builtin (`eval.rs`, Stage 4a) calls `run_state`;
//!   - `meta::eval` / `load().run()` (prelude) route through `runner::run`.
//! So there is no second/third engine driver — invariant #3.
//!
//! ## The teeth (a source seam, à la `prism-bigraph/tests/closure_guard.rs`)
//! The discover-rung driver `discover_all_processes` must appear ONLY in
//! `crates/chrysalis/src/runner.rs`. Stage 4a first re-inlined the whole triple in
//! the EVALUATOR (`eval_instantiate`) — a third path bypassing `runner`; if any
//! file outside `runner.rs` drives the engine again, the file-set grows past
//! `{runner.rs}` and the build fails. `discover_all_processes` is the precise needle
//! (it names the discover rung), and the scan strips whole-line comments — so a doc
//! comment that merely *names* the driver (including this consolidation's own notes
//! in `eval.rs`/`runner.rs`) is not counted; only a real DRIVING call is.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The discover-rung driver call. Driving an engine means discovering its specs;
/// any "data → running" path bottoms out here, so this is the precise seam.
const DRIVER: &str = "discover_all_processes";

/// The ONE module allowed to drive the engine (it holds `run_state` + the sampler).
const DRIVER_HOME: &str = "crates/chrysalis/src/runner.rs";

const SCAN_DIR: &str = "crates/chrysalis/src";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_engine_driver_lives_in_one_module() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join(SCAN_DIR), &mut files);
    assert!(!files.is_empty(), "no source files found under {SCAN_DIR}");

    // Every file that DRIVES an engine (calls the discover rung).
    let mut driver_files: BTreeSet<String> = BTreeSet::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("read source");
        // Scan CODE only — strip whole-line `//` / `///` / `//!` comments so a doc
        // comment that *names* the driver (this guard's own subject, e.g. the
        // `eval_instantiate` / `run_state` notes) is not mistaken for DRIVING it.
        // (Block comments aren't used for this in the tree.)
        let drives = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .any(|l| l.contains(DRIVER));
        if drives {
            let rel = file
                .strip_prefix(&root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            driver_files.insert(rel);
        }
    }

    // Non-vacuous: the driver must exist SOMEWHERE (the runner) — else the needle
    // is stale and the guard would pass on an empty set.
    assert!(
        driver_files.contains(DRIVER_HOME),
        "the engine driver `{DRIVER}` was not found in {DRIVER_HOME} — the needle is \
         stale (was `run_state` renamed/removed?), so this guard is vacuous"
    );

    // The one door: nothing OUTSIDE the runner module drives the engine. A surface
    // path (the evaluator's `instantiate`, the prelude's `meta::eval`/`load`) must
    // route THROUGH `runner::run_state`, not inline a second driver.
    let extra: Vec<&String> = driver_files.iter().filter(|f| *f != DRIVER_HOME).collect();
    assert!(
        extra.is_empty(),
        "the engine driver (`{DRIVER}`) leaked OUT of {DRIVER_HOME} into:\n{}\n\nA surface \
         path that brings a spec to life must route through `runner::run_state` (the one \
         one-shot engine driver), not re-inline `Engine::from_state → {DRIVER} → run` — \
         that is a second/third eval (homoiconic-unification invariant #3).",
        extra.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n"),
    );
}
