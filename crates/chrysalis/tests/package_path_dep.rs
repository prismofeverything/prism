//! Phase 1 consumer (#67, the `pkg` agent): a 2-package local project — a `.ys`
//! library `foo` exporting `process Tick`, and a program package `app` that DEPENDS
//! on `foo` by path and uses `Tick` inside its own composite. Proves the package
//! linker end-to-end: `Manifest` (project.ys as `.ys`-data) → `resolve_core` (the
//! dependency colimit) → `Core::merge` (the join-semilattice) → `run` reaches the
//! dependency's def.
//!
//! This is `ys/bump.ys` SPLIT across a package boundary: the reusable `Tick` process
//! lives in the dependency; the program composes it. That the split runs identically
//! to the monolith is the linking thesis, demonstrated on a real consumer.

use std::path::PathBuf;

use chrysalis::manifest::Manifest;
use chrysalis::parse::parse_program_in;
use chrysalis::prelude::std_modules;
use chrysalis::resolver::resolve;
use chrysalis::runner::run;

/// Build the 2-package fixture under a unique temp dir; returns its root. Layout:
/// `<root>/foo` (the library) + `<root>/app` (the program depending on it).
fn write_two_package_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pkg-phase1-{tag}-{}", std::process::id()));
    let foo = root.join("foo");
    let app = root.join("app");
    std::fs::create_dir_all(&foo).unwrap();
    std::fs::create_dir_all(&app).unwrap();

    // The dependency `foo`: a library exporting one process. Defs-only (no root) —
    // compile registers + exports its defs with no mandatory entry point.
    std::fs::write(
        foo.join("project.ys"),
        "def package = { name: 'foo', version: '0.1.0', exports: ['Tick'] }\n",
    )
    .unwrap();
    std::fs::write(
        foo.join("lib.ys"),
        "process Tick ~{n :: Float} ->{n :: Float} (\n  {n: 1.0}\n)\n",
    )
    .unwrap();

    // The program `app`: depends on `foo` by path, and uses `Tick` in its composite.
    std::fs::write(
        app.join("project.ys"),
        "def package = { name: 'app', dependencies: { foo: { path: '../foo' } } }\n",
    )
    .unwrap();
    std::fs::write(
        app.join("main.ys"),
        "from foo import Tick\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  tick: Tick ~{n: n} ->{n: n}\n)\nMain[]\n",
    )
    .unwrap();

    root
}

#[test]
fn a_program_links_and_runs_a_path_dependencys_process() {
    let root = write_two_package_project("link-run");
    let app_main = root.join("app").join("main.ys");

    // 1. The program's manifest is found by walking up to `app/project.ys`.
    let manifest = Manifest::find(&app_main).expect("app manifest found");
    assert_eq!(manifest.name, "app");
    assert_eq!(manifest.dependencies.len(), 1);

    // 2. Resolution links the dependency's Core in via `Core::merge` + declares its
    //    process exports as importable — the STRUCTURAL proof: `foo`'s exported
    //    `Tick` is now in the linked Core's process registry.
    let (linked, modules) = resolve(&manifest, std_modules()).expect("dependencies resolve + link");
    assert!(
        linked.processes.contains("Tick"),
        "the path dependency `foo`'s `Tick` reached the linked Core through Core::merge. \
         Linked processes = {:?}",
        linked.processes.type_names(),
    );

    // 3. END-TO-END proof: the program — whose composite imports + uses `Tick` from
    //    the dependency — runs against the linked Core + import surface. `Tick` adds
    //    1.0 per tick, so `n` accumulates to 5.0 over 5 ticks. The dependency's
    //    behaviour reached the program purely through the package link.
    let src = std::fs::read_to_string(&app_main).unwrap();
    let prog = parse_program_in(&src, root.join("app")).expect("parse app/main.ys");
    let state = run(&prog, linked, modules, 5.0).expect("run the linked program");
    assert_eq!(
        state.get_field("n").and_then(|v| v.as_f64()),
        Some(5.0),
        "app's composite ran the dependency's Tick to 5.0 — linking works end to end: {state:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_missing_dependency_path_is_a_clear_error() {
    // Robustness: a dependency pointing at a non-existent package fails with a
    // located, actionable error rather than a panic.
    let root = std::env::temp_dir().join(format!("pkg-phase1-missing-{}", std::process::id()));
    let app = root.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("project.ys"),
        "def package = { name: 'app', dependencies: { ghost: { path: '../ghost' } } }\n",
    )
    .unwrap();

    let manifest = Manifest::load(&app).expect("app manifest loads");
    // `.err()` (not `expect_err`) — the Ok type carries a `ModuleRegistry`, which is
    // intentionally not `Debug` (it holds host-function closures).
    let err = resolve(&manifest, std_modules())
        .err()
        .expect("a missing dependency must error");
    assert!(
        err.contains("ghost"),
        "the error names the missing dependency: {err}"
    );

    std::fs::remove_dir_all(&root).ok();
}
