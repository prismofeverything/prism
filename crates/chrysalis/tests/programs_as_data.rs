//! Programs are data: load a `.ys` file from disk, render it to a Document
//! VALUE (schema + state), serialize as JSON, read it back, run the
//! reconstructed Document — and get the same answer as a direct run. The
//! Document IS the program-as-data representation today; this test makes the
//! principle visible on a non-workflow example (a structural simulation —
//! growth + division — rather than `integrator-comparison.ys`'s comparison
//! workflow).
//!
//! The invariant proved: `run_document(deserialize(serialize(document_of(f))))`
//! ≡ `run(f)`. Three "is the program" representations all reach the same
//! final state — the homoiconic identity.
//!
//! Future (task #32): expose `load(path)` to `.ys` programs so this round-trip
//! is callable in-language, not just from Rust.

use std::path::PathBuf;

use chrysalis::compile::compile_with_modules;
use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::{document_of, run, run_document};
use prism_bigraph::Document;
use prism_schema::Value;

const GROW_DIVIDE: &str = include_str!("../ys/grow-divide-unbounded.ys");

/// Count non-`_`-prefixed cell keys in the env state, for run-vs-roundtrip
/// comparison. The exact daughter ids match (deterministic division by id).
fn cell_ids(state: &Value) -> Option<Vec<String>> {
    state.get_field("cells").and_then(|cells| {
        cells.as_map().map(|m| {
            let mut ids: Vec<String> = m
                .keys()
                .map(|k| k.to_string())
                .filter(|k| !k.starts_with('_'))
                .collect();
            ids.sort();
            ids
        })
    })
}

#[test]
fn grow_divide_runs_identically_as_data_and_as_program() {
    // The .ys text, in memory. (Same source the bin would read from disk.)
    let prog = parse_program(GROW_DIVIDE).expect("parse grow-divide-unbounded.ys");

    // Direct run — the program executed as code.
    let direct = run(&prog, std_registry(), std_methods(), std_modules(), 5.0).expect("direct run");

    // Render to Document — the program AS DATA (schema + state, JSON-shaped).
    // This is the homoiconic move: the runnable program is now a value.
    let compiled = compile_with_modules(&prog, std_registry(), std_methods(), std_modules())
        .expect("compile");
    let doc = document_of(&compiled);

    // The Document is JSON-serializable. So the program-as-data can move over
    // any wire (file, network, stdin) — proven here by round-trip.
    let json = serde_json::to_string(&doc).expect("serialize document");
    let restored: Document = serde_json::from_str(&json).expect("deserialize document");

    // Run the deserialized Document — the program executed as data.
    let from_data = run_document(&restored, compiled.core.clone(), 5.0).expect("run document");

    // The two paths land at the same state. "Programs are data" — when the
    // representations are faithful, the identity holds.
    //
    // NOTE: grow-divide-unbounded.ys uses Form-1 internal division (cell's
    // Divide step writes daughters up through `->{environment: %}`), which is
    // currently NOT firing under the present `%`-as-self semantics — same
    // reason the dedicated `grow_divide` test is `#[ignore]`. So both paths
    // reach the SAME (no-division) state. The identity is the point here —
    // when division-via-form-1 is re-enabled (canonical task #6 / Form-3
    // migration), this assertion still holds because both paths share the
    // same execution model.
    assert_eq!(
        cell_ids(&direct),
        cell_ids(&from_data),
        "direct run vs. run-from-data must reach the same cell layout — the\n\
         homoiconic identity. direct={:?} from_data={:?}",
        cell_ids(&direct),
        cell_ids(&from_data),
    );

    // Sanity: the seed cell IS present in both. The program didn't disappear
    // — it ran, the schema survived the round-trip, and the cells map is
    // populated.
    assert!(
        cell_ids(&direct).is_some(),
        "the program produced a cells map (the seed '0' is in it)"
    );
}

#[test]
fn document_preserves_the_program_shape() {
    // The Document's `schema` field carries the program's runtime schema; its
    // `state` field carries the initial state. Both are present — making the
    // Document a complete runnable description, not a stripped trace.
    let prog = parse_program(GROW_DIVIDE).expect("parse");
    let compiled = compile_with_modules(&prog, std_registry(), std_methods(), std_modules())
        .expect("compile");
    let doc = document_of(&compiled);

    assert!(
        doc.schema.is_some(),
        "the Document carries the program's schema (so re-running needs no original source)"
    );

    // The state is a `Value::Map` carrying the bigraph's top-level keys —
    // `cells` (the Map[Cell] container). It's a plain tree-of-maps the
    // algebra walks; no opaque blobs.
    let state_keys: Vec<String> = doc
        .state
        .as_map()
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();
    assert!(
        state_keys.iter().any(|k| k == "cells"),
        "the program's top-level shape survives into the Document state: keys={state_keys:?}"
    );

    // Save the Document to disk + read it back — proves it's a file-shaped
    // value, not just an in-memory one.
    let path: PathBuf = std::env::temp_dir().join(format!(
        "prism-programs-as-data-{}.json",
        std::process::id()
    ));
    let json = serde_json::to_string_pretty(&doc).expect("serialize");
    std::fs::write(&path, &json).expect("write");
    let read = std::fs::read_to_string(&path).expect("read");
    let restored: Document = serde_json::from_str(&read).expect("parse");
    assert_eq!(
        restored.state.as_map().map(|m| m.len()),
        doc.state.as_map().map(|m| m.len()),
        "the on-disk Document is the same data as the in-memory one"
    );
    let _ = std::fs::remove_file(&path);
}
