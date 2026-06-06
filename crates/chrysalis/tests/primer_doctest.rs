//! The chrysalis primer (`docs/chrysalis-primer.md`) is **executable**: every
//! ` ```ys ` block in it is parsed — and, when tagged `run`, executed — by this
//! test. So the canonical fluency reference can never silently drift from the
//! language (the same rot that let `docs/chrysalis-design.md` list `pattern` /
//! `expr {}` as live when they aren't). This is also the regression net under
//! the "subtract" cleanups: a simplification that changes surface behavior
//! breaks a primer block here first.
//!
//! Fence tags (the info string after the opening fence):
//!   ```ys          → must PARSE (`parse_program` succeeds)
//!   ```ys run      → must PARSE *and* RUN clean (no error) through the runner
//!   ```ys ignore   → skipped, but COUNTED — for PLANNED syntax not yet live,
//!                    so the primer can honestly show the roadmap without lying.
//!
//! Every fenced `ys` block must be a *complete, parseable program* (a sequence
//! of defs and/or a trailing term). Sub-expression fragments belong in inline
//! `code`, not in fenced blocks.

use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::run;

#[derive(Clone, Copy, PartialEq)]
enum Tag {
    Parse,
    Run,
    Ignore,
}

struct Block {
    line: usize,
    tag: Tag,
    code: String,
}

/// Pull every ` ```ys[ tag] ` fenced block out of a markdown string. Non-`ys`
/// fences (```sh, ```text, plain) are consumed and skipped.
fn extract_ys_blocks(md: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lines = md.lines().enumerate();
    while let Some((i, line)) = lines.next() {
        let trimmed = line.trim_start();
        let Some(info) = trimmed.strip_prefix("```") else {
            continue;
        };
        let mut parts = info.trim().split_whitespace();
        let lang = parts.next().unwrap_or("");
        if lang != "ys" {
            // Not a chrysalis block — consume to its closing fence and move on.
            for (_, l) in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    break;
                }
            }
            continue;
        }
        let tag = match parts.next() {
            Some("run") => Tag::Run,
            Some("ignore") => Tag::Ignore,
            _ => Tag::Parse,
        };
        let start = i + 2; // 1-based, first content line
        let mut code = String::new();
        for (_, l) in lines.by_ref() {
            if l.trim_start().starts_with("```") {
                break;
            }
            code.push_str(l);
            code.push('\n');
        }
        blocks.push(Block { line: start, tag, code });
    }
    blocks
}

#[test]
fn primer_blocks_parse_and_run() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/chrysalis-primer.md");
    let md = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read primer at {}: {e}", path.display()));

    let blocks = extract_ys_blocks(&md);
    assert!(
        !blocks.is_empty(),
        "no ```ys blocks found in {} — is the primer present?",
        path.display()
    );

    let (mut parsed, mut ran, mut ignored) = (0u32, 0u32, 0u32);
    let mut failures = Vec::new();

    for b in &blocks {
        if b.tag == Tag::Ignore {
            ignored += 1;
            continue;
        }
        let prog = match parse_program(&b.code) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!(
                    "L{} parse error: {e:?}\n--- block ---\n{}",
                    b.line, b.code
                ));
                continue;
            }
        };
        parsed += 1;
        if b.tag == Tag::Run {
            match run(&prog, std_registry(), std_methods(), std_modules(), 5.0) {
                Ok(_) => ran += 1,
                Err(e) => failures.push(format!(
                    "L{} run error: {e:?}\n--- block ---\n{}",
                    b.line, b.code
                )),
            }
        }
    }

    eprintln!(
        "primer: {} blocks ({parsed} parsed, {ran} ran, {ignored} ignored), {} failed",
        blocks.len(),
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} primer block(s) failed — the primer drifted from the language:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
