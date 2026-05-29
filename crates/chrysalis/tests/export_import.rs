//! `chrysalis bigraph export` / `import` round-trip: a compiled program rendered
//! to a process-bigraph Document (schema + state), serialized to JSON, read back,
//! and run — yields the same result as running the program directly. The
//! invariant: **`import(export(f)) ≡ run(f)`**.

use std::path::PathBuf;

use chrysalis::compile::compile_with_modules;
use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::{document_of, run, run_document};
use prism_bigraph::Document;
use prism_schema::Value;

const SRC: &str = include_str!("../ys/integrator-comparison.ys");

fn ys_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys").join(name)
}

/// integrator-comparison.ys with its library import rewritten to an absolute
/// path (so a temp copy outside ys/ resolves it) and `Output` pointed at a temp
/// dir. Returns the parsed (import-resolved) program + the out dir.
fn temp_program(tag: &str) -> (chrysalis::ast::Program, PathBuf) {
    let out = std::env::temp_dir().join(format!("prism-exportimport-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let abs_lib = ys_path("lib/simulators.ys");
    let src = SRC
        .replace("'lib/simulators.ys'", &format!("'{}'", abs_lib.display()))
        .replace(
            "IntegratorComparison[]",
            &format!("IntegratorComparison[out: '{}']", out.display()),
        );
    let tmp =
        std::env::temp_dir().join(format!("prism-exportimport-{tag}-{}.ys", std::process::id()));
    std::fs::write(&tmp, &src).expect("write temp ys");
    let prog = parse_file(&tmp).expect("parse_file temp ys");
    let _ = std::fs::remove_file(&tmp);
    (prog, out)
}

fn mse(state: &Value) -> Option<Value> {
    state.get_field("mse").cloned()
}

#[test]
fn import_export_equals_run() {
    let (prog, out) = temp_program("roundtrip");

    // Direct run.
    let direct = run(&prog, std_registry(), std_methods(), std_modules(), 2.0).expect("run");

    // export → serialize → deserialize → import, against the program's own core
    // (the document carries schema + state; the core supplies the factories its
    // addressed nodes reference).
    let result =
        compile_with_modules(&prog, std_registry(), std_methods(), std_modules()).expect("compile");
    let doc = document_of(&result);
    assert!(
        doc.schema.is_some(),
        "the document renders the schema alongside the state"
    );

    let json = serde_json::to_string(&doc).expect("serialize document");
    let doc2: Document = serde_json::from_str(&json).expect("deserialize document");
    let imported = run_document(&doc2, result.core.clone(), 2.0).expect("import + run");

    // The deterministic comparison output is identical across the round-trip.
    assert_eq!(
        mse(&direct),
        mse(&imported),
        "import(export(f)) ≡ run(f): the round-tripped document re-runs identically"
    );
    assert!(
        mse(&direct).is_some(),
        "sanity: the workflow produced an mse"
    );

    let _ = std::fs::remove_dir_all(&out);
}
