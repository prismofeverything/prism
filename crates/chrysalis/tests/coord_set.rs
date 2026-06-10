//! `chrysalis coord set` — the serialize-from-data heartbeat writer (`simplify`,
//! after a hand-typed `{…}` in a status string broke the board render).
//!
//! The point of the command is to make board breakage STRUCTURALLY IMPOSSIBLE: the
//! author supplies DATA, the canonical unparser serializes it (escaping), and a
//! ROUND-TRIP GATE refuses to write anything that would not re-parse. This pins that
//! invariant — `set_in_source` never returns an `Ok` whose text fails to parse — plus
//! the field-set + auto-`tick` behaviour. Companion to `coord_heartbeats_parse`
//! (which checks the files) and `docs/coord-set-command.md`.

use chrysalis::ast::{Def, Expr, Program, StringSeg};
use chrysalis::coord::{join_board_in_source, set_in_source};
use chrysalis::parse::parse_program;
use indexmap::IndexMap;

fn record(p: &Program) -> &IndexMap<String, Expr> {
    p.defs
        .iter()
        .find_map(|d| match d {
            Def::Binding { value: Expr::Record(r), .. } => Some(r),
            _ => None,
        })
        .expect("a record binding")
}

/// A plain (non-interpolated) string field's text, or `None` if it isn't a plain string.
fn field_str(r: &IndexMap<String, Expr>, k: &str) -> Option<String> {
    match r.get(k)? {
        Expr::Str(lit) => {
            let mut s = String::new();
            for seg in &lit.segments {
                match seg {
                    StringSeg::Lit(t) => s.push_str(t),
                    StringSeg::Expr(_) => return None, // interpolation — not a plain value
                }
            }
            Some(s)
        }
        _ => None,
    }
}

fn field_int(r: &IndexMap<String, Expr>, k: &str) -> Option<i64> {
    match r.get(k)? {
        Expr::Int(n) => Some(*n),
        _ => None,
    }
}

#[test]
fn set_updates_a_field_and_auto_bumps_tick() {
    let src = "def p = {\n  status: 'old',\n  tick: 3\n}\n";
    let (out, bumped) =
        set_in_source(src, "p", &[("status".into(), "a new status".into())]).expect("set succeeds");
    assert!(bumped, "tick auto-bumps when not set explicitly");

    let prog = parse_program(&out).expect("output re-parses");
    let rec = record(&prog);
    assert_eq!(field_str(rec, "status").as_deref(), Some("a new status"));
    assert_eq!(field_int(rec, "tick"), Some(4), "3 -> 4");
}

#[test]
fn explicit_tick_is_not_double_bumped() {
    let src = "def p = { tick: 1 }\n";
    let (out, bumped) =
        set_in_source(src, "p", &[("tick".into(), "9".into())]).expect("set succeeds");
    assert!(!bumped, "an explicit tick is taken as-is");
    assert_eq!(field_int(record(&parse_program(&out).unwrap()), "tick"), Some(9));
}

#[test]
fn the_round_trip_gate_never_writes_unparseable_output() {
    let src = "def p = { note: 'ok', tick: 1 }\n";

    // EXACTLY the bug that broke the board: a curly-brace set in a string. The gate
    // must either safely encode it (Ok + re-parses) or REFUSE (Err) — never Ok-but-broken.
    let brace_bug = "a baseline {runner.rs, compile.rs} set";
    match set_in_source(src, "p", &[("note".into(), brace_bug.into())]) {
        Ok((out, _)) => assert!(
            parse_program(&out).is_ok(),
            "coord set returned Ok but the output does NOT parse — the gate FAILED:\n{out}"
        ),
        Err(_) => {} // refused — also safe (the board is never broken)
    }

    // The other board-breaker — an apostrophe — must round-trip safely (the unparser
    // escapes it), not be rejected.
    let (out, _) = set_in_source(src, "p", &[("note".into(), "it's fine".into())])
        .expect("an apostrophe value is encodable");
    let prog = parse_program(&out).expect("apostrophe output re-parses");
    let rec = record(&prog);
    assert_eq!(field_str(rec, "note").as_deref(), Some("it's fine"));
}

#[test]
fn a_missing_peer_binding_is_an_error_not_a_silent_noop() {
    let src = "def other = { tick: 1 }\n";
    assert!(
        set_in_source(src, "p", &[("status".into(), "x".into())]).is_err(),
        "setting a peer with no matching `def p = {{ … }}` must error"
    );
}

// ── TOTALITY (the enforcement prerequisite): `coord set` can express EVERY heartbeat
// field — nested (dotted path) + lists — so there is NEVER a reason to hand-edit a
// `.ys` heartbeat (the gap that made "always use the command" hollow).

#[test]
fn set_writes_a_nested_dotted_path() {
    // `build.state=green` descends into the nested `build` Record (the monotone build
    // watermark every agent writes each tick — previously hand-edited).
    let src = "def p = { build: { state: 'idle', green_tick: 0 }, tick: 1 }";
    let (out, _) = set_in_source(
        src,
        "p",
        &[
            ("build.state".into(), "green".into()),
            ("build.green_tick".into(), "7".into()),
        ],
    )
    .expect("nested set succeeds");
    let prog = parse_program(&out).expect("nested output re-parses");
    let build = match record(&prog).get("build") {
        Some(Expr::Record(b)) => b,
        _ => panic!("`build` is still a nested Record"),
    };
    assert_eq!(field_str(build, "state").as_deref(), Some("green"));
    assert_eq!(field_int(build, "green_tick"), Some(7));
}

#[test]
fn set_writes_a_json_list() {
    // `touching=["a","b"]` becomes a real List — the field the OLD command could not
    // set (it stringified everything), forcing the hand-edits this work removes.
    let src = "def p = { touching: [], tick: 1 }";
    let (out, _) = set_in_source(src, "p", &[("touching".into(), "[\"a.rs\",\"b.rs\"]".into())])
        .expect("list set succeeds");
    let prog = parse_program(&out).expect("list output re-parses");
    match record(&prog).get("touching") {
        Some(Expr::List(items)) => assert_eq!(items.len(), 2, "two elements"),
        other => panic!("`touching` is a List, got {other:?}"),
    }
}

#[test]
fn a_list_element_with_metacharacters_stays_a_plain_escaped_string() {
    // Structured data is escaped too: a list element carrying a `{` or `'` round-trips
    // as a PLAIN string (not interpolation), so the board stays safe — the whole point
    // of routing through the codec rather than hand-writing `.ys`.
    let src = "def p = { touching: [], tick: 1 }";
    let (out, _) = set_in_source(
        src,
        "p",
        &[("touching".into(), "[\"has {brace}\",\"it's fine\"]".into())],
    )
    .expect("escapable structured data succeeds (the unparser escaped it)");
    let prog = parse_program(&out).expect("re-parses — brace/apostrophe were escaped");
    let items = match record(&prog).get("touching") {
        Some(Expr::List(v)) => v.clone(),
        other => panic!("`touching` is a List, got {other:?}"),
    };
    // Each element must be a PLAIN (non-interpolated) string — a `{brace}` that became
    // interpolation would parse but break the board render (the class the gate misses).
    for (i, el) in items.iter().enumerate() {
        let plain = matches!(el, Expr::Str(lit) if lit.segments.iter().all(|s| matches!(s, StringSeg::Lit(_))));
        assert!(plain, "list element {i} stayed a plain string (no interpolation)");
    }
}

// ── BOOT INTO THE HEARTBEAT: `join_board_in_source` wires a peer into the aggregated
// board (import + mesh-link merge entry), idempotently + gate-guarded.

#[test]
fn join_board_adds_the_import_and_the_merge_entry() {
    let board = "from .mesh import mesh\n\ncomposite Board ->{ board :: map[any] @ board } (\n  \
                 link board :: map[any] mesh = { mesh: mesh }\n)\n\nBoard[]\n";
    let out = join_board_in_source(board, "pkg")
        .expect("join ok")
        .expect("the board changed (pkg was absent)");
    assert!(out.contains("from .pkg import pkg"), "import line added");
    assert!(out.contains("pkg: pkg"), "mesh-link merge entry added");
    parse_program(&out).expect("the joined board re-parses (the gate)");
}

#[test]
fn join_board_is_idempotent() {
    let board = "from .mesh import mesh\nfrom .pkg import pkg\n\ncomposite Board ->{ board :: map[any] @ board } (\n  \
                 link board :: map[any] mesh = { mesh: mesh, pkg: pkg }\n)\n\nBoard[]\n";
    assert!(
        join_board_in_source(board, "pkg").expect("ok").is_none(),
        "already joined → no change (self-healing, no duplicate)"
    );
}
