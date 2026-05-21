//! Closure guard — the structural residue check for the schema algebra.
//!
//! Encapsulation (the algebra owning the transformation surface) and the
//! executable laws cover most of the closure invariant; this test catches what
//! they can't: a hand-rolled shortcut sneaking back into the engine, the
//! Composite, or chrysalis. It greps those sources for the forbidden patterns
//! from `docs/schema-algebra.md` ("Schema::Any.apply shortcut, hand-rolled
//! `_add`/`_remove`, bespoke merges/diffs/overlays") and ratchets a baseline
//! of *known remaining* violations toward **empty**.
//!
//! Two-sided ratchet:
//!   * a violation **not** in `KNOWN_REMAINING` fails the test (no new
//!     shortcuts), and
//!   * a `KNOWN_REMAINING` entry that no longer matches fails the test (forcing
//!     you to delete it as the rebuild removes each stand-in).
//!
//! Definition of done for the rebuild: `KNOWN_REMAINING` is `&[]`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// `(label, forbidden substring)` — each is an "extra operation outside the
/// algebra" that could reappear *without a compile error*. (The deleted
/// `Schema::infer_and_merge` / `Schema::resolve` stub / chrysalis
/// `overlay_apply_types` are compile-enforced gone — calling them no longer
/// builds — so they aren't scanned here; that also avoids tripping on the
/// historical mentions in `// replaces the old …` comments.)
const PATTERNS: &[(&str, &str)] = &[
    // Using the bottom sort to dodge a real schema in apply — the engine
    // accumulation / passthrough dodge.
    ("any-apply-shortcut", "Schema::Any.apply"),
    // Inline `_add`/`_remove` munging at a call site (belongs inside apply).
    ("apply-add-remove", "apply_add_remove"),
    // Hand-rolled `diff` in the Composite (should be `algebra::diff`).
    ("hand-rolled-diff", "fn compute_delta"),
    // Hand-rolled `merge` in the Composite (should be `algebra::merge`).
    ("hand-rolled-merge", "fn merge_value_maps"),
];

/// Source trees that must stay inside the algebra. (prism-schema is the algebra
/// itself, so it is intentionally *not* scanned — its `apply` may recurse with
/// the bottom sort, exactly as upstream `apply(Node(), ...)` does.)
const SCAN_DIRS: &[&str] = &["crates/prism-bigraph/src", "crates/chrysalis/src"];

/// Violations still permitted, shrinking to `&[]` as the rebuild lands. Each is
/// `(repo-relative file, pattern label)`.
///
/// Ratchet log (all cleared — the closure invariant is achieved):
///   * #18 removed `infer-and-merge` (engine) and `overlay-apply-types` (chrysalis). ✓
///   * #19 removed `apply-add-remove` (engine, via promote+apply in
///     `apply_projections_to`) and `any-apply-shortcut` (engine: dead
///     `run_collecting` deleted; passthrough accumulation uses `reconcile`). ✓
///   * #21 removed `hand-rolled-diff` / `hand-rolled-merge` (composite: the
///     output bridge is `algebra::diff` on the inner schema; dead
///     `merge_value_maps` deleted). ✓
///
/// Empty ⇒ every schema/state transformation in engine/composite/chrysalis
/// goes through `prism_schema::algebra`. Keep it empty.
const KNOWN_REMAINING: &[(&str, &str)] = &[];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
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
fn algebra_closure_is_maintained() {
    let root = repo_root();

    let mut files = Vec::new();
    for dir in SCAN_DIRS {
        collect_rs_files(&root.join(dir), &mut files);
    }
    assert!(!files.is_empty(), "no source files found to scan under {SCAN_DIRS:?}");

    let mut found: BTreeSet<(String, String)> = BTreeSet::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("read source");
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        for (label, needle) in PATTERNS {
            if text.contains(needle) {
                found.insert((rel.clone(), label.to_string()));
            }
        }
    }

    let known: BTreeSet<(String, String)> = KNOWN_REMAINING
        .iter()
        .map(|(f, l)| (f.to_string(), l.to_string()))
        .collect();

    let new_violations: Vec<_> = found.difference(&known).collect();
    let stale_baseline: Vec<_> = known.difference(&found).collect();

    assert!(
        new_violations.is_empty(),
        "NEW algebra-closure violations (a shortcut outside the algebra reappeared):\n{}\n\n\
         Route the transformation through prism_schema::algebra::* instead.",
        new_violations
            .iter()
            .map(|(f, l)| format!("  - {f}: {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    assert!(
        stale_baseline.is_empty(),
        "KNOWN_REMAINING has stale entries (these stand-ins are gone — delete them \
         from the baseline so the ratchet stays tight):\n{}",
        stale_baseline
            .iter()
            .map(|(f, l)| format!("  - {f}: {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
