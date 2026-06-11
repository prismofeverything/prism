//! Phase 2 consumer (#67): the LOCKFILE — the colimit's **chosen section**. A
//! 3-package diamond with a version constraint (the compatibility functor) resolves
//! to a deterministic, reproducible `project.lock` that pins exactly one version per
//! package name.

use std::path::Path;

use chrysalis::lockfile;
use chrysalis::manifest::Manifest;
use chrysalis::prelude::std_modules;
use chrysalis::resolver::resolve;

fn write_pkg(root: &Path, name: &str, project_ys: &str, entry_name: &str, entry_ys: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.ys"), project_ys).unwrap();
    std::fs::write(dir.join(entry_name), entry_ys).unwrap();
}

fn tick(name: &str) -> String {
    format!("process {name} ~{{n :: Float}} ->{{n :: Float}} (\n  {{n: 1.0}}\n)\n")
}

#[test]
fn a_dependency_graph_resolves_to_a_deterministic_lockfile() {
    let root = std::env::temp_dir().join(format!("pkg-lock-{}", std::process::id()));
    // The shared apex `d`, and `a`/`b` (at distinct versions) each depending on it.
    write_pkg(
        &root,
        "d",
        "def package = { name: 'd', version: '1.0.0', exports: ['Pulse'] }\n",
        "lib.ys",
        &tick("Pulse"),
    );
    write_pkg(
        &root,
        "a",
        "def package = { name: 'a', version: '1.2.0', dependencies: { d: { path: '../d' } }, exports: ['ATick'] }\n",
        "lib.ys",
        &tick("ATick"),
    );
    write_pkg(
        &root,
        "b",
        "def package = { name: 'b', version: '1.5.0', dependencies: { d: { path: '../d' } }, exports: ['BTick'] }\n",
        "lib.ys",
        &tick("BTick"),
    );
    // prog constrains a + b by a version requirement (`^1.0`, satisfied by 1.2/1.5).
    write_pkg(
        &root,
        "prog",
        "def package = { name: 'prog', dependencies: { a: { path: '../a', version: '^1.0' }, b: { path: '../b', version: '^1.0' } } }\n",
        "main.ys",
        "composite Main ->{ n :: Float } (\n  n: 0.0\n)\nMain[]\n",
    );

    let prog_dir = root.join("prog");
    let manifest = Manifest::load(&prog_dir).expect("prog manifest");
    let resolution = resolve(&manifest, std_modules()).expect("the constrained graph resolves");

    // Write project.lock + read it back.
    lockfile::write(&resolution.graph, &prog_dir).expect("writes project.lock");
    let lock_path = prog_dir.join(lockfile::LOCKFILE);
    assert!(lock_path.is_file(), "project.lock was written");
    let text = std::fs::read_to_string(&lock_path).unwrap();

    // Reproducible: re-resolve + re-write → byte-identical (the pin is stable).
    let resolution2 = resolve(&manifest, std_modules()).unwrap();
    lockfile::write(&resolution2.graph, &prog_dir).unwrap();
    assert_eq!(
        text,
        std::fs::read_to_string(&lock_path).unwrap(),
        "the lockfile is reproducible"
    );

    // The lock is the colimit's chosen section: the shared apex `d` pinned ONCE, the
    // graph edges + the chosen versions recorded.
    let locked = lockfile::parse(&text).expect("lock parses");
    let names: Vec<&str> = locked.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "d"], "one entry per name, sorted");
    let d = locked.iter().find(|p| p.name == "d").unwrap();
    assert_eq!(d.version.to_string(), "1.0.0");
    assert!(d.dependencies.is_empty());
    for dependent in ["a", "b"] {
        let p = locked.iter().find(|p| p.name == dependent).unwrap();
        assert!(
            p.dependencies.contains(&"d".to_string()),
            "{dependent} → d edge is pinned"
        );
    }

    std::fs::remove_dir_all(&root).ok();
}
