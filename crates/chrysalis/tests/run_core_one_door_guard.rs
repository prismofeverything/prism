//! Run-Core one-door guard — `simplify` / the #59 Core-threading rule reaching its
//! LAST seam (the chrysalis `run()` / `compile` boundary; `docs/canonical-run-core.md`).
//!
//! ## What this pins
//! `prism_bigraph::Core` is the ONE runtime — procs + types + methods + protocols.
//! The canonical run boundary threads that ONE Core: `runner::run_state` /
//! `runner::run_document` take `core: Core`. The anti-pattern (#59 "registry-subset
//! drift") is a run/compile function that takes a LOOSE registry triple —
//! `registry: ProcessRegistry, methods: MethodRegistry, modules: ModuleRegistry` —
//! instead of the whole Core, splitting it back into 3 doors (and leaving protocols
//! with NO door, `compile.rs` hard-coding `stream_protocols()`). That split is what
//! blocks A8 `net:` and forced the spatio-flux prelude workaround.
//!
//! ## The ratchet (à la `prism-bigraph/tests/closure_guard.rs`)
//! `KNOWN_REMAINING` lists the files that STILL contain a subset door — `runner.rs`
//! + `compile.rs`, which `lang` is threading the canonical Core through (additive
//! sibling → migrate callers → retire the split). Two-sided:
//!   * a subset door in any file NOT listed fails (no NEW drift — in particular a
//!     domain `*_core()` integration, e.g. synth's `audio_core()`, must thread a
//!     Core, the very workaround synth refused to copy), and
//!   * a listed file with NO subset door left fails (forcing its deletion from the
//!     baseline as the split is retired, keeping the ratchet tight).
//!
//! Definition of done: `KNOWN_REMAINING` is `&[]` — every run/compile path threads
//! one Core. (Scope: `crates/chrysalis/src` — the run/compile boundary. Extending the
//! scan to the domain crates, as `sf_core()` / `audio_core()` dissolve their
//! workarounds, is the follow-on.)

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The registry-subset door: a function PARAMETER of type `ProcessRegistry` (a fn
/// that accepts a loose registry instead of the whole `Core`). Distinct from
/// `ProcessRegistry::new()` (constructing one to put INTO a Core), which is fine.
const SUBSET_DOOR: &str = "registry: ProcessRegistry";

/// Files that still hold a subset door, shrinking to `&[]` as `lang` threads the
/// canonical Core through `run`/`compile`. Each is repo-relative.
const KNOWN_REMAINING: &[&str] = &[
    "crates/chrysalis/src/runner.rs",  // run / to_document / invoke* — lang threading the Core
    "crates/chrysalis/src/compile.rs", // compile_with_registry / _methods / _modules — same
    "crates/chrysalis/src/cli.rs",     // run_command (the CLI entry that drives run) — same
];

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
fn the_run_boundary_threads_one_core_no_registry_subset_door() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join(SCAN_DIR), &mut files);
    assert!(!files.is_empty(), "no source files found under {SCAN_DIR}");

    // Files that DECLARE a subset door — scanning CODE only (strip whole-line
    // comments) so a doc comment naming the anti-pattern is not counted.
    let mut found: BTreeSet<String> = BTreeSet::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("read source");
        let declares = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .any(|l| l.contains(SUBSET_DOOR));
        if declares {
            let rel = file
                .strip_prefix(&root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            found.insert(rel);
        }
    }

    let known: BTreeSet<String> = KNOWN_REMAINING.iter().map(|s| s.to_string()).collect();

    let new_doors: Vec<&String> = found.difference(&known).collect();
    assert!(
        new_doors.is_empty(),
        "NEW registry-subset door(s) — a run/compile fn takes a loose `ProcessRegistry` \
         (+ methods + modules) instead of the whole `Core`:\n{}\n\nThread the ONE \
         `Core` instead (`run_state` / `run_document` are the canonical shape); see \
         docs/canonical-run-core.md.",
        new_doors.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n"),
    );

    let retired: Vec<&String> = known.difference(&found).collect();
    assert!(
        retired.is_empty(),
        "KNOWN_REMAINING is STALE — these files no longer declare a subset door (you \
         threaded the Core — nice). DELETE them from `KNOWN_REMAINING` so the ratchet \
         stays tight (definition of done: it is `&[]`):\n{}",
        retired.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n"),
    );
}
