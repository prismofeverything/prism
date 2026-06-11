//! Phase 3 consumer (#67): the lockfile as a **PIN**. `project.lock` pins a registry
//! dependency to a recorded version (reproducible builds — `install`/`run` honor it),
//! and `chrysalis update` ignores the pin to re-resolve to the newest satisfying
//! version. This is the lockfile-as-INPUT the Phase-2 lock deferred.

use std::path::Path;

use chrysalis::lockfile;
use chrysalis::manifest::Manifest;
use chrysalis::prelude::std_modules;
use chrysalis::resolver::{resolve, resolve_update, ResolvedPackage};
use chrysalis::version::Version;

fn write_file(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn tick(name: &str) -> String {
    format!("process {name} ~{{n :: Float}} ->{{n :: Float}} (\n  {{n: 1.0}}\n)\n")
}

fn foo_version(graph: &[ResolvedPackage]) -> Version {
    graph.iter().find(|p| p.name == "foo").expect("foo resolved").version
}

#[test]
fn the_lockfile_pins_a_registry_dependency_and_update_re_resolves() {
    let root = std::env::temp_dir().join(format!("pkg-lockpin-{}", std::process::id()));
    let reg = root.join("registry");
    for v in ["1.0.0", "1.2.0"] {
        let dir = reg.join("foo").join(v);
        write_file(
            &dir.join("project.ys"),
            &format!("def package = {{ name: 'foo', version: '{v}', exports: ['Tick'] }}\n"),
        );
        write_file(&dir.join("lib.ys"), &tick("Tick"));
    }
    let app = root.join("app");
    write_file(
        &app.join("project.ys"),
        &format!(
            "def package = {{ name: 'app', registry: '{}', dependencies: {{ foo: {{ version: '^1.0' }} }} }}\n",
            reg.display()
        ),
    );
    let manifest = Manifest::load(&app).expect("app manifest");

    // (1) A FRESH resolve (no lock yet) picks the newest satisfying — foo@1.2.0.
    let r = resolve(&manifest, std_modules()).expect("fresh resolve");
    assert_eq!(foo_version(&r.graph), Version::new(1, 2, 0));

    // (2) PIN foo@1.0.0 in project.lock. A subsequent resolve HONORS the pin (even
    //     though the registry has a newer 1.2.0) — reproducible builds.
    let pin = vec![ResolvedPackage {
        name: "foo".to_string(),
        version: Version::new(1, 0, 0),
        source: reg.join("foo").join("1.0.0"),
        dependencies: vec![],
    }];
    lockfile::write(&pin, &app).expect("write the pin lock");
    let r = resolve(&manifest, std_modules()).expect("resolve honoring the lock");
    assert_eq!(
        foo_version(&r.graph),
        Version::new(1, 0, 0),
        "the lockfile pinned foo@1.0.0"
    );

    // (3) `update` IGNORES the pin → re-resolves to the newest, foo@1.2.0.
    let r = resolve_update(&manifest, std_modules()).expect("update re-resolves");
    assert_eq!(
        foo_version(&r.graph),
        Version::new(1, 2, 0),
        "update re-resolved to foo@1.2.0"
    );

    std::fs::remove_dir_all(&root).ok();
}
