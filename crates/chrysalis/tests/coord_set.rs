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
use chrysalis::coord::set_in_source;
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
