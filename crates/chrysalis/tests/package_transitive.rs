//! Phase 2 consumer (#67): transitive resolution as a **colimit over the dependency
//! DAG**. The keystone is the **diamond** — `prog → A, B`, `A → D`, `B → D` — where
//! the shared apex `D` must be resolved + linked exactly ONCE (not double-unioned
//! into a self-conflict). Plus semver: a violated requirement and an incompatible
//! transitive version are clear conflicts.

use std::path::{Path, PathBuf};

use chrysalis::manifest::Manifest;
use chrysalis::parse::parse_program_in;
use chrysalis::prelude::std_modules;
use chrysalis::resolver::resolve;
use chrysalis::runner::run;

/// Write a package directory: `<root>/<name>/{project.ys, lib.ys (or main.ys)}`.
fn write_pkg(root: &Path, name: &str, project_ys: &str, entry_name: &str, entry_ys: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.ys"), project_ys).unwrap();
    std::fs::write(dir.join(entry_name), entry_ys).unwrap();
}

fn unique_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pkg-phase2-{tag}-{}", std::process::id()))
}

/// A one-output `+1.0/tick` process named `name` (the shared building block).
fn tick_process(name: &str) -> String {
    format!("process {name} ~{{n :: Float}} ->{{n :: Float}} (\n  {{n: 1.0}}\n)\n")
}

#[test]
fn a_diamond_dependency_is_resolved_and_linked_once() {
    let root = unique_root("diamond");
    // D — the shared apex. Exports `Pulse`.
    write_pkg(
        &root,
        "d",
        "def package = { name: 'd', version: '1.0.0', exports: ['Pulse'] }\n",
        "lib.ys",
        &tick_process("Pulse"),
    );
    // A and B — each depends on D, exports its own process.
    write_pkg(
        &root,
        "a",
        "def package = { name: 'a', version: '1.0.0', dependencies: { d: { path: '../d' } }, exports: ['ATick'] }\n",
        "lib.ys",
        &tick_process("ATick"),
    );
    write_pkg(
        &root,
        "b",
        "def package = { name: 'b', version: '1.0.0', dependencies: { d: { path: '../d' } }, exports: ['BTick'] }\n",
        "lib.ys",
        &tick_process("BTick"),
    );
    // prog — depends on A and B (so D is reached transitively via BOTH).
    write_pkg(
        &root,
        "prog",
        "def package = { name: 'prog', dependencies: { a: { path: '../a' }, b: { path: '../b' } } }\n",
        "main.ys",
        "from a import ATick\nfrom b import BTick\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  at: ATick ~{n: n} ->{n: n} |\n  bt: BTick ~{n: n} ->{n: n}\n)\nMain[]\n",
    );

    let prog_main = root.join("prog").join("main.ys");
    let manifest = Manifest::find(&prog_main).expect("prog manifest");
    let resolution = resolve(&manifest, std_modules()).expect("the diamond resolves");

    // (1) D is in the resolved graph EXACTLY ONCE despite two paths to it — the
    //     diamond deduped at the shared apex (the colimit).
    let d_count = resolution.graph.iter().filter(|p| p.name == "d").count();
    assert_eq!(d_count, 1, "the shared apex D must be resolved once, not per-path");
    // a and b each record their edge to d.
    let names: Vec<&str> = resolution.graph.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"a") && names.contains(&"b") && names.contains(&"d"));
    for p in &resolution.graph {
        if p.name == "a" || p.name == "b" {
            assert!(p.dependencies.contains(&"d".to_string()), "{} → d edge recorded", p.name);
        }
    }

    // (2) Every package's theory is in the linked Core — D's Pulse contributed once.
    assert!(resolution.core.processes.contains("Pulse"));
    assert!(resolution.core.processes.contains("ATick"));
    assert!(resolution.core.processes.contains("BTick"));

    // (3) END-TO-END: prog composes ATick + BTick (both +1/tick) → n = 2.0/tick = 10.0
    //     over 5 ticks. The whole DAG linked into one runnable program.
    let src = std::fs::read_to_string(&prog_main).unwrap();
    let prog = parse_program_in(&src, root.join("prog")).expect("parse prog/main.ys");
    let state = run(&prog, resolution.core, resolution.modules, 5.0).expect("run the linked DAG");
    assert_eq!(
        state.get_field("n").and_then(|v| v.as_f64()),
        Some(10.0),
        "ATick + BTick each ran 5 ticks through the linked DAG: {state:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_version_requirement_violation_is_a_clear_error() {
    let root = unique_root("reqviol");
    write_pkg(
        &root,
        "d",
        "def package = { name: 'd', version: '1.0.0', exports: ['Pulse'] }\n",
        "lib.ys",
        &tick_process("Pulse"),
    );
    // prog requires d ^2.0, but d is 1.0.0 → unsatisfiable.
    write_pkg(
        &root,
        "prog",
        "def package = { name: 'prog', dependencies: { d: { path: '../d', version: '^2.0' } } }\n",
        "main.ys",
        "from d import Pulse\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  p: Pulse ~{n: n} ->{n: n}\n)\nMain[]\n",
    );

    let manifest = Manifest::load(root.join("prog")).expect("prog manifest");
    let err = resolve(&manifest, std_modules())
        .err()
        .expect("an unsatisfiable version requirement must error");
    assert!(
        err.contains("1.0.0") && err.contains("^2.0"),
        "the error names the version + the requirement: {err}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn incompatible_transitive_versions_conflict() {
    let root = unique_root("verconflict");
    // Two DIFFERENT packages both named `d`, at incompatible versions.
    write_pkg(
        &root,
        "d1",
        "def package = { name: 'd', version: '1.0.0', exports: ['Pulse'] }\n",
        "lib.ys",
        &tick_process("Pulse"),
    );
    write_pkg(
        &root,
        "d2",
        "def package = { name: 'd', version: '2.0.0', exports: ['Pulse'] }\n",
        "lib.ys",
        &tick_process("Pulse"),
    );
    write_pkg(
        &root,
        "a",
        "def package = { name: 'a', version: '1.0.0', dependencies: { d: { path: '../d1' } }, exports: ['ATick'] }\n",
        "lib.ys",
        &tick_process("ATick"),
    );
    write_pkg(
        &root,
        "b",
        "def package = { name: 'b', version: '1.0.0', dependencies: { d: { path: '../d2' } }, exports: ['BTick'] }\n",
        "lib.ys",
        &tick_process("BTick"),
    );
    // prog → A (→ d@1.0) and B (→ d@2.0): `d` is required at two incompatible versions.
    write_pkg(
        &root,
        "prog",
        "def package = { name: 'prog', dependencies: { a: { path: '../a' }, b: { path: '../b' } } }\n",
        "main.ys",
        "composite Main ->{ n :: Float } (\n  n: 0.0\n)\nMain[]\n",
    );

    let manifest = Manifest::load(root.join("prog")).expect("prog manifest");
    let err = resolve(&manifest, std_modules())
        .err()
        .expect("two incompatible versions of `d` must conflict");
    assert!(
        err.contains("incompatible versions") && err.contains("d"),
        "the error reports the version conflict on `d`: {err}"
    );

    std::fs::remove_dir_all(&root).ok();
}
