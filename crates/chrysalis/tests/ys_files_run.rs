//! Every shipped `.ys` example actually **runs** via `chrysalis run` — not just
//! parses (that's `ys_files_roundtrip.rs`). A smoke test: run each for one tick
//! and assert it exits cleanly with no `error`/`panic` on stderr. This is the
//! guard that catches runtime rot (an example that stops working when the
//! execution model / wiring semantics change) — the gap that let `.ys` files
//! drift out of sync with the engine.
//!
//! Files not yet runnable are listed in `SKIP` with a reason, so the gap is
//! DOCUMENTED, not hidden. As the "modernize all .ys" migration proceeds, files
//! move off this list (task: modernize all .ys files).

use std::process::Command;

/// `(file, why)` — `.ys` that don't yet run via `chrysalis run`. Each is
/// tracked against an open task. As blockers land, files move off this list.
const SKIP: &[(&str, &str)] = &[
    (
        "grow-divide-glucose.ys",
        "tier-1 stretch (#30 slice 3): the reaction's `?c.divide()` reactum is a \
         homoiconic method call on a sited cell — needs `?c :: Cell` + \
         `divide()` as a first-class entity method. Runs once that lands.",
    ),
    (
        "agreement-engines.ys",
        "integration: drives COPASI/Tellurium over rest — needs a running \
         process-server (process-server/serve.sh 8765). Run via the \
         agreement_engines integration test (--ignored).",
    ),
    (
        "schlogl-engines.ys",
        "integration: drives COPASI + RK4 on the bistable Schlögl over rest — \
         needs process-server/serve.sh 8765. Run via the schlogl integration test.",
    ),
];

fn skip_reason(name: &str) -> Option<&'static str> {
    SKIP.iter().find(|(f, _)| *f == name).map(|(_, why)| *why)
}

#[test]
fn all_ys_examples_run_clean() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys");
    let bin = env!("CARGO_BIN_EXE_chrysalis");

    let mut ran = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("ys dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ys"))
        .collect();
    entries.sort();

    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if let Some(why) = skip_reason(&name) {
            skipped.push(format!("{name} ({why})"));
            continue;
        }
        let out = Command::new(bin)
            .args(["run", path.to_str().unwrap(), "--time", "1"])
            .output()
            .expect("spawn chrysalis");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let bad = !out.status.success()
            || stderr.to_lowercase().contains("error")
            || stderr.to_lowercase().contains("panic")
            // ExprProcess runtime errors print to stdout too.
            || stdout.to_lowercase().contains(" error");
        if bad {
            failures.push(format!(
                "{name}: exit={:?}\n  stderr: {}\n  stdout: {}",
                out.status.code(),
                stderr.trim(),
                stdout.trim()
            ));
        } else {
            ran.push(name);
        }
    }

    eprintln!("ran clean ({}): {ran:?}", ran.len());
    eprintln!("skipped ({}): {skipped:#?}", skipped.len());
    assert!(
        failures.is_empty(),
        "{} .ys file(s) failed to run clean:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
