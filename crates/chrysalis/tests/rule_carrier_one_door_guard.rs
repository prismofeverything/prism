//! Rule-carrier one-door guard — `simplify` / Stage 3 of
//! `docs/homoiconic-unification.md` (the #46 homoiconic-surface audit; invariant
//! #1 generalized to the Stage-4c CARRIER policy).
//!
//! ## What this pins
//! Stage 4c made every reaction producer emit a freshly-built chrysalis `Rule` as
//! its *at-rest carrier*: transparent `_pat:"Rule"` DATA when STRUCTURAL (the BRS
//! evals it via `ReactionRule::from_data_value`, uniform with a process spec), else
//! the in-process `Foreign(FOREIGN_RULE)` carrier when COMPUTED (its closures need
//! *this* evaluator at fire time). That structural-vs-computed split is ONE policy
//! — it must live in ONE place (`runtime::rule::carry_rule`), not be re-inlined per
//! producer. The 4c flip first TRIPLICATED it across `build_reaction_value` /
//! `eval_rule_expr` / `build_reaction_constructor`; this guard pins the
//! consolidation so a second door cannot silently reappear (`generative-core.md`
//! §3 — the regression is made LOUD, not silent).
//!
//! ## The teeth (a source seam, à la `prism-bigraph/tests/closure_guard.rs`)
//! The carrier CONSTRUCTION `Foreign::new(FOREIGN_RULE …` must appear EXACTLY ONCE
//! across `crates/chrysalis/src`, and that once must be in `runtime/rule.rs`
//! (`carry_rule`). A producer that re-inlines `to_data_value(rule)
//! .unwrap_or_else(|| Foreign::new(FOREIGN_RULE, rule))` raises the count to 2 → the
//! build fails. A `Foreign(FOREIGN_RULE)` *read* (`f.type_name == FOREIGN_RULE`) is
//! a match, not a `::new(` construction, so it is correctly NOT counted — the `::new(`
//! in the needle is what distinguishes building a carrier from inspecting one.
//!
//! Scope note: this is a property/architecture guard (robust to refactoring),
//! complementary to `instantiate_one_door_guard` (which pins the LOWERING core,
//! `build_rule`) — together they pin both rungs of reaction production: one
//! lowering, one carrier.

use std::fs;
use std::path::{Path, PathBuf};

/// The carrier CONSTRUCTION. The `::new(` distinguishes *building* a carrier from
/// the many `f.type_name == FOREIGN_RULE` *reads* (matches, not constructions).
const CARRIER_CTOR: &str = "Foreign::new(FOREIGN_RULE";

/// The ONE file allowed to construct the carrier (it holds `carry_rule`).
const CARRIER_HOME: &str = "crates/chrysalis/src/runtime/rule.rs";

/// The source tree whose reaction producers must funnel through `carry_rule`.
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
fn the_rule_carrier_lives_in_exactly_one_place() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join(SCAN_DIR), &mut files);
    assert!(!files.is_empty(), "no source files found under {SCAN_DIR}");

    // (repo-relative file, occurrence count) for every file that constructs a carrier.
    let mut sites: Vec<(String, usize)> = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("read source");
        let n = text.matches(CARRIER_CTOR).count();
        if n > 0 {
            let rel = file
                .strip_prefix(&root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            sites.push((rel, n));
        }
    }

    let total: usize = sites.iter().map(|(_, n)| n).sum();
    assert_eq!(
        total, 1,
        "the Stage-4c rule-carrier decision (`to_data_value(rule)` else \
         `Foreign(FOREIGN_RULE)`) must live in EXACTLY ONE place (`carry_rule`), so a \
         producer cannot re-fork the structural-vs-computed policy per-site. Found \
         {total} construction site(s):\n{}\n\nRoute the producer through \
         `crate::runtime::rule::carry_rule(rule)` instead of re-inlining the idiom.",
        sites
            .iter()
            .map(|(f, n)| format!("  - {f}: {n}×"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    assert_eq!(
        sites.first().map(|(f, _)| f.as_str()),
        Some(CARRIER_HOME),
        "the single rule-carrier construction must live in {CARRIER_HOME} (`carry_rule`), \
         but it is in {:?} — move it back so the carrier policy stays where producers expect it.",
        sites.first().map(|(f, _)| f.as_str()),
    );
}
