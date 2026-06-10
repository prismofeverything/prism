//! Coord-board "remain parsable" guard — `simplify`.
//!
//! The agent mesh board (`coord/board.ys`) MERGES every peer's heartbeat
//! (`coord/<peer>.ys`) through one `map[any] mesh` link, so a SINGLE unparsable
//! heartbeat takes the whole board render down for EVERY peer — and the heartbeats
//! are hand-edited `.ys`, whose string fields carry metacharacters (`'` is the
//! string delimiter, `{…}` is interpolation). That is a recurring, high-blast-radius
//! break (the board has wedged on a stray apostrophe / brace more than once).
//!
//! This pins the **remain-parsable invariant**: every `coord/*.ys` parses, and the
//! board renders end-to-end. It is the cargo-test enforcement of what the proposed
//! `chrysalis coord set` command (`docs/coord-set-command.md`) makes correct-by-
//! construction — a safety net even before that command lands, and a backstop for
//! any hand-edit that bypasses it.
//!
//! The durable fix is serialize-from-data (the command); this guard is the CHECK
//! that the invariant holds. Two teeth:
//!   * every `coord/*.ys` parses in-process (precise per-file diagnostics), and
//!   * `chrysalis run coord/board.ys --time 1` exits clean (the actual render path —
//!     transitively requires every imported peer to parse + the merge to compose).
//! A self-test (`the_guard_detects_the_failure_modes_it_pins`) proves the parse
//! check actually rejects the break it claims to (an unescaped `'`), so the guard
//! is not vacuous.

use std::path::PathBuf;
use std::process::Command;

use chrysalis::ast::{Def, Expr, StringLit, StringSeg};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

/// True if a string literal interpolates (`'…{x}…'` — has an `Expr` segment).
fn lit_interpolates(lit: &StringLit) -> bool {
    lit.segments.iter().any(|s| matches!(s, StringSeg::Expr(_)))
}

/// True if any string reachable from `e` interpolates. A coord heartbeat is pure
/// DATA, so this must be false — an interpolated `{x}` is exactly the board-breaker
/// the render check below MISSES (it parses, then evals to an unbound var, but the
/// render exits 0 with a message that has no substring `error`). Walked over the data
/// shapes a heartbeat uses (records, maps, lists, keyed entries, strings).
fn expr_interpolates(e: &Expr) -> bool {
    match e {
        Expr::Str(lit) => lit_interpolates(lit),
        Expr::List(items) | Expr::Parallel(items) => items.iter().any(expr_interpolates),
        Expr::Record(fields) => fields.values().any(expr_interpolates),
        Expr::Map(entries) => entries
            .iter()
            .any(|(k, v)| lit_interpolates(k) || expr_interpolates(v)),
        Expr::KeyedEntry { key, value } => lit_interpolates(key) || expr_interpolates(value),
        _ => false, // scalars + any non-data Expr a heartbeat must not contain
    }
}

/// The escaping discipline these `.ys` string fields require — surfaced on failure
/// so the fix is obvious (and points at the real fix: don't hand-serialize).
const HINT: &str = "a `.ys` string is delimited by `'` (double it — `''` — for a \
literal apostrophe) and `{...}` is interpolation (avoid braces in prose). The durable \
fix is to update heartbeats via `chrysalis coord set` (serialize from data through the \
unparser) rather than hand-editing — see docs/coord-set-command.md.";

#[test]
fn every_coord_heartbeat_parses_and_the_board_renders() {
    let root = repo_root();
    let coord = root.join("coord");

    // (1) Every coord/*.ys parses in-process — precise per-file diagnostics. Globs
    // the directory, so a NEW agent's heartbeat is covered automatically.
    let mut files: Vec<PathBuf> = std::fs::read_dir(&coord)
        .expect("coord dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ys"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no coord/*.ys files found under {}",
        coord.display()
    );

    let mut parse_failures = Vec::new();
    let mut interp_violations = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(path).expect("read heartbeat");
        match chrysalis::parse::parse_program(&text) {
            Err(e) => parse_failures.push(format!("  - coord/{name}: {e}")),
            // A `{…}` in a string is interpolation — it PARSES (so the check above
            // passes), then evals to an unbound var, which the render check below
            // MISSES (exit 0, no `error` substring). Catch it here at the AST level.
            Ok(program) => {
                for def in &program.defs {
                    if let Def::Binding { name: peer, value, .. } = def {
                        if expr_interpolates(value) {
                            interp_violations.push(format!(
                                "  - coord/{name}: `def {peer} = …` has a string with `{{…}}` interpolation"
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(
        parse_failures.is_empty(),
        "coord heartbeat(s) do not parse — the board merge breaks for EVERY peer:\n{}\n\n{}",
        parse_failures.join("\n"),
        HINT,
    );
    assert!(
        interp_violations.is_empty(),
        "coord heartbeat(s) contain STRING INTERPOLATION (a `{{` in a string) — it parses but the \
         board render evals it to an unbound var (and exits 0, so the render check below misses \
         it). A heartbeat is pure DATA:\n{}\n\n{}",
        interp_violations.join("\n"),
        HINT,
    );

    // (2) The board renders end-to-end (imports resolve + the merge composes + the
    // tick runs) — exactly how an agent views it. Shell out to the real bin from the
    // repo root so the `.peer` imports resolve relative to coord/board.ys (the
    // documented `chrysalis run coord/board.ys` invocation). Mirrors ys_files_run.rs.
    let bin = env!("CARGO_BIN_EXE_chrysalis");
    let out = Command::new(bin)
        .current_dir(&root)
        .args(["run", "coord/board.ys", "--time", "1"])
        .output()
        .expect("spawn chrysalis");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let bad = !out.status.success()
        || stderr.to_lowercase().contains("error")
        || stderr.to_lowercase().contains("panic");
    assert!(
        !bad,
        "`chrysalis run coord/board.ys --time 1` did not render clean — a peer heartbeat \
         likely broke the merge:\n  exit={:?}\n  stderr: {}\n  stdout: {}\n\n{}",
        out.status.code(),
        stderr.trim(),
        stdout.trim(),
        HINT,
    );
}

#[test]
fn the_guard_detects_the_failure_modes_it_pins() {
    // Not vacuous: the parse check must REJECT the real break (an unescaped `'`
    // inside a string field) and ACCEPT its proper escape (`''`). If this ever
    // flips, the lexer's escaping rule changed and the HINT above (+ the proposed
    // `coord set` serializer) need updating in lockstep.
    let broken = "def p = { note: 'it's broken' }";
    assert!(
        chrysalis::parse::parse_program(broken).is_err(),
        "the parse guard is VACUOUS — an unescaped apostrophe parsed when it should fail"
    );
    let escaped = "def p = { note: 'it''s fine' }";
    assert!(
        chrysalis::parse::parse_program(escaped).is_ok(),
        "the doubled-apostrophe escape (`''`) no longer parses — the lexer rule changed; \
         update the HINT and the `coord set` serializer accordingly"
    );

    // The interpolation gap the AST check closes: a `{x}` in a string PARSES (so the
    // parse check passes it), yet it is the board-breaker. `expr_interpolates` must
    // catch it — else the strengthening is vacuous.
    let braced = "def p = { note: 'a baseline {x} here', tick: 1 }";
    let prog = chrysalis::parse::parse_program(braced)
        .expect("a `{x}` string PARSES (interpolation) — exactly why the parse check misses it");
    let binding = prog
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Binding { value, .. } => Some(value),
            _ => None,
        })
        .expect("the binding value");
    assert!(
        expr_interpolates(binding),
        "expr_interpolates FAILED to catch a `{{x}}` interpolation — the AST check is vacuous"
    );

    // …and a brace-free heartbeat must NOT be flagged (no false positive).
    let clean = chrysalis::parse::parse_program("def p = { note: 'no braces at all', tick: 1 }")
        .expect("clean parses");
    let clean_val = clean
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Binding { value, .. } => Some(value),
            _ => None,
        })
        .expect("the binding value");
    assert!(
        !expr_interpolates(clean_val),
        "expr_interpolates flagged a brace-free heartbeat — false positive"
    );
}
