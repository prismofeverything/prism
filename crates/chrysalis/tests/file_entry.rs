//! Decision #24: a `.ys` file's value is its *last top-level term*. With no
//! explicit `main` (no trailing expression), the last `composite` definition is
//! the entry point — `run` inlines it, so its body becomes the root state.
//! Vocabulary defs above it are still registered + importable. The entry's
//! interface is what `chrysalis run file.ys` will bind config/inputs to and
//! render outputs from (the `invoke` path, layered on this).

use chrysalis::ast::def_name;
use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_methods, std_modules, std_registry};
use chrysalis::runner::run;

#[test]
fn last_composite_is_the_entry() {
    // No `main`, no trailing expr: the file ends in a composite definition.
    // A helper composite ABOVE it is not the entry — only the LAST one is.
    let src = "\
composite Other ( marker: 0.0 )

composite World[seed :: Float = 2.0] ->{count :: Float @ count} (
  count: seed
)
";
    let prog = parse_program(src).expect("parse");
    assert_eq!(
        def_name(prog.entry().expect("entry")),
        "World",
        "last interfaced def is the entry"
    );

    // Running with no `main` inlines `World`: its body (count: seed=2.0) is root.
    let state = run(&prog, std_registry(), std_methods(), std_modules(), 1.0).expect("run");
    let count = state
        .as_map()
        .and_then(|m| m.get("count"))
        .and_then(|v| v.as_f64());
    assert_eq!(
        count,
        Some(2.0),
        "World's body should become the root state; got {state:?}"
    );
}

#[test]
fn no_interfaced_term_is_a_pure_package() {
    // Only vocabulary, no composite/process/step/def ⇒ no entry: importable,
    // not runnable on its own.
    let src = "reaction R[k :: Float] (a => b)\ntype T = Float\n";
    let prog = parse_program(src).expect("parse");
    assert!(
        prog.entry().is_none(),
        "a defs-only file has no entry point"
    );
}

#[test]
fn function_def_is_an_entry_candidate() {
    // A `def` (pure function) is a valid entry per the rule (its params are the
    // inputs, its return the output) — exercised fully by the invoke path.
    let src = "process P ~{x :: Float} ->{x :: Float} ( {x: x} )\ndef transform(x :: Float) :: Float = x * 2.0\n";
    let prog = parse_program(src).expect("parse");
    assert_eq!(
        def_name(prog.entry().expect("entry")),
        "transform",
        "last term wins, even a def"
    );
}
