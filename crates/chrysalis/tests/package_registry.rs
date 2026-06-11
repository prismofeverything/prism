//! Phase 3 consumer (#67): the local **registry**. A project declares a `registry:` +
//! a version-only dependency (`foo: { version: '^1.0' }`); the resolver resolves it
//! against the registry — the **version solver** picks the highest satisfying version
//! — and links it, exactly like a path dep whose path the registry chose.

use std::path::Path;

use chrysalis::manifest::Manifest;
use chrysalis::parse::parse_program_in;
use chrysalis::prelude::std_modules;
use chrysalis::resolver::resolve;
use chrysalis::runner::run;

fn write_file(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn tick(name: &str) -> String {
    format!("process {name} ~{{n :: Float}} ->{{n :: Float}} (\n  {{n: 1.0}}\n)\n")
}

#[test]
fn a_registry_dependency_resolves_to_the_highest_satisfying_version() {
    let root = std::env::temp_dir().join(format!("pkg-registry-{}", std::process::id()));
    let reg = root.join("registry");
    // `foo` in the registry at 1.0.0 AND 1.2.0 (both export `Tick`).
    for v in ["1.0.0", "1.2.0"] {
        let dir = reg.join("foo").join(v);
        write_file(
            &dir.join("project.ys"),
            &format!("def package = {{ name: 'foo', version: '{v}', exports: ['Tick'] }}\n"),
        );
        write_file(&dir.join("lib.ys"), &tick("Tick"));
    }
    // The app: a REGISTRY dependency on `foo ^1.0` (no path/native).
    let app = root.join("app");
    write_file(
        &app.join("project.ys"),
        &format!(
            "def package = {{ name: 'app', registry: '{}', dependencies: {{ foo: {{ version: '^1.0' }} }} }}\n",
            reg.display()
        ),
    );
    write_file(
        &app.join("main.ys"),
        "from foo import Tick\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  t: Tick ~{n: n} ->{n: n}\n)\nMain[]\n",
    );

    let manifest = Manifest::load(&app).expect("app manifest");
    let resolution = resolve(&manifest, std_modules()).expect("the registry dependency resolves");

    // The version solver picked `foo@1.2.0` — the highest satisfying `^1.0`, not 2.x.
    let foo = resolution
        .graph
        .iter()
        .find(|p| p.name == "foo")
        .expect("foo resolved into the graph");
    assert_eq!(foo.version.to_string(), "1.2.0", "highest satisfying version");
    assert!(resolution.core.processes.contains("Tick"));

    // END-TO-END: the program runs `foo`'s `Tick` (fetched from the registry) to n=5.0.
    let src = std::fs::read_to_string(app.join("main.ys")).unwrap();
    let prog = parse_program_in(&src, &app).expect("parse app/main.ys");
    let state = run(&prog, resolution.core, resolution.modules, 5.0).expect("run the linked program");
    assert_eq!(
        state.get_field("n").and_then(|v| v.as_f64()),
        Some(5.0),
        "the registry dependency's Tick ran through the linked Core: {state:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_registry_dependency_without_a_registry_errors_clearly() {
    let root = std::env::temp_dir().join(format!("pkg-registry-noreg-{}", std::process::id()));
    let app = root.join("app");
    write_file(
        &app.join("project.ys"),
        "def package = { name: 'app', dependencies: { foo: { version: '^1.0' } } }\n",
    );
    let manifest = Manifest::load(&app).expect("app manifest");
    let err = resolve(&manifest, std_modules())
        .err()
        .expect("a registry dep with no `registry:` declared must error");
    assert!(
        err.contains("registry") && err.contains("foo"),
        "the error names the dep + the missing registry: {err}"
    );
    std::fs::remove_dir_all(&root).ok();
}
