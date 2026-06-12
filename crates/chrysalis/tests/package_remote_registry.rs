//! Phase 4b consumer (#67): the **remote** registry. A project declares `registry:` as an
//! `http://…` URL (mesh's transport, `prism_bigraph::protocols::registry`); the resolver
//! fetches the package over the wire, caches it under `target/registry-cache/`, and links
//! it EXACTLY like a local registry dep — the `Registry` trait is the only seam, blind to
//! whether the bytes came from disk or the network. The M4-enabling proof: a package
//! published to a server (on another host, in production) resolves + runs identically to a
//! local one.

use std::path::Path;

use chrysalis::manifest::Manifest;
use chrysalis::parse::parse_program_in;
use chrysalis::prelude::std_modules;
use chrysalis::registry::publish_remote;
use chrysalis::resolver::resolve;
use chrysalis::runner::run;
use prism_bigraph::protocols::registry::RegistryServer;

fn write_file(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn tick(name: &str) -> String {
    format!("process {name} ~{{n :: Float}} ->{{n :: Float}} (\n  {{n: 1.0}}\n)\n")
}

#[test]
fn a_remote_registry_dependency_publishes_resolves_and_runs() {
    let root = std::env::temp_dir().join(format!("pkg-remote-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();

    // A registry SERVER over mesh's transport. In production this is another host
    // (`RegistryServer::start_on("0.0.0.0:PORT")`); here it's in-process on a free port.
    let server = RegistryServer::start().expect("start the registry server");
    let base_url = server.base_url();

    // Publish `foo@1.0.0` AND `foo@1.2.0` to the server over the wire (both export `Tick`).
    for v in ["1.0.0", "1.2.0"] {
        let dir = root.join("src").join(v);
        write_file(
            &dir.join("project.ys"),
            &format!("def package = {{ name: 'foo', version: '{v}', exports: ['Tick'] }}\n"),
        );
        write_file(&dir.join("lib.ys"), &tick("Tick"));
        let published = publish_remote(&dir, &base_url).expect("publish foo over the wire");
        assert_eq!(published.to_string(), v);
    }
    assert_eq!(server.entry_count(), 2, "two versions published to the server");

    // The app: a REGISTRY dependency on `foo ^1.0`, with the registry = the server URL.
    let app = root.join("app");
    write_file(
        &app.join("project.ys"),
        &format!(
            "def package = {{ name: 'app', registry: '{base_url}', dependencies: {{ foo: {{ version: '^1.0' }} }} }}\n"
        ),
    );
    write_file(
        &app.join("main.ys"),
        "from foo import Tick\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  t: Tick ~{n: n} ->{n: n}\n)\nMain[]\n",
    );

    let manifest = Manifest::load(&app).expect("app manifest");
    let resolution =
        resolve(&manifest, std_modules()).expect("the remote registry dependency resolves");

    // The version solver picked `foo@1.2.0` — highest satisfying `^1.0` — fetched over HTTP.
    let foo = resolution
        .graph
        .iter()
        .find(|p| p.name == "foo")
        .expect("foo resolved into the graph");
    assert_eq!(foo.version.to_string(), "1.2.0", "highest satisfying version, over the wire");
    assert!(resolution.core.processes.contains("Tick"));

    // The fetched package landed in the project's cache, loaded like a path dep.
    let cached = app.join("target").join("registry-cache").join("foo").join("1.2.0");
    assert!(cached.join("project.ys").is_file(), "the remote package cached locally");

    // END-TO-END: the program runs the remotely-fetched `Tick` to n=5.0 — a package from
    // the network links + runs identically to a local one.
    let src = std::fs::read_to_string(app.join("main.ys")).unwrap();
    let prog = parse_program_in(&src, &app).expect("parse app/main.ys");
    let state =
        run(&prog, resolution.core, resolution.modules, 5.0).expect("run the linked program");
    assert_eq!(
        state.get_field("n").and_then(|v| v.as_f64()),
        Some(5.0),
        "the remote dependency's Tick ran through the linked Core: {state:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}
