//! The generated `.ys` files (`ys/mapk.ys`, `ys/mr.ys`) are emitted from their
//! Rust fixtures (`fixtures::mapk::program()` / `fixtures::mr::program()`) by the
//! unparser. Nothing used to enforce they stayed in sync — a fixture edit could
//! silently leave the `.ys` stale. This test closes that gap:
//!
//! - `generated_ys_in_sync_with_fixtures` (always runs) fails if a `.ys` drifts
//!   from its fixture. Comparison is comment-robust: both sides go through
//!   `unparse` (which drops the header), so it checks SEMANTIC content.
//! - `regenerate_generated_ys` (`#[ignore]`) rewrites the files — the "bless"
//!   step after editing a fixture:
//!   `cargo test -p chrysalis --test fixture_sync regenerate -- --ignored`.

use chrysalis::ast::Program;
use chrysalis::fixtures;
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;

fn pairs() -> Vec<(&'static str, fn() -> Program)> {
    vec![
        ("mapk.ys", fixtures::mapk::program),
        ("mr.ys", fixtures::mr::program),
    ]
}

fn ys_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ys")
        .join(name)
}

fn header(stem: &str) -> String {
    format!(
        "# GENERATED from `fixtures::{stem}::program()` by the unparser\n\
         # (chrysalis::unparse). Round-trip-verified: parse(this) re-unparses\n\
         # identically. Edit the fixture, not this file.\n"
    )
}

#[test]
fn generated_ys_in_sync_with_fixtures() {
    let mut drift = Vec::new();
    for (name, program) in pairs() {
        let src = std::fs::read_to_string(ys_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
        let from_file = unparse(&parse_program(&src).expect("parse .ys"));
        let from_fixture = unparse(&program());
        if from_file != from_fixture {
            drift.push(format!(
                "{name} is OUT OF SYNC with its fixture. Re-bless with:\n  \
                 cargo test -p chrysalis --test fixture_sync regenerate -- --ignored\n\
                 ---- expected (from fixture) ----\n{from_fixture}\n\
                 ---- actual ({name}) ----\n{from_file}"
            ));
        }
    }
    assert!(drift.is_empty(), "{}", drift.join("\n\n"));
}

#[test]
#[ignore = "writes files; run explicitly to regenerate after editing a fixture"]
fn regenerate_generated_ys() {
    for (name, program) in pairs() {
        let stem = name.strip_suffix(".ys").expect("ys name");
        let content = format!("{}\n{}\n", header(stem), unparse(&program()));
        std::fs::write(ys_path(name), content).unwrap_or_else(|e| panic!("write {name}: {e}"));
        eprintln!("regenerated {name}");
    }
}
