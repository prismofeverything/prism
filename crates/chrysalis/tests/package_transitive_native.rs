//! The transitive-native gap fix (#67, the M4 / packages-outside-this-dir enabler):
//! `collect_native_crates` finds a native crate declared in a TRANSITIVE `.ys`
//! dependency (the `synth`-as-a-dependency case — `synth`'s manifest has `native:
//! audio`). Without this, a project depending on a mixed package errors on the
//! unsupplied transitive native; with it, codegen links every transitive native crate.

use std::path::Path;

use chrysalis::manifest::Manifest;
use chrysalis::prelude::std_modules;
use chrysalis::resolver::{collect_native_crates, resolve};

fn write_pkg(root: &Path, name: &str, project_ys: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.ys"), project_ys).unwrap();
    std::fs::write(
        dir.join("lib.ys"),
        "process Noop ~{n :: Float} ->{n :: Float} (\n  {n: 0.0}\n)\n",
    )
    .unwrap();
}

#[test]
fn collects_a_native_crate_from_a_transitive_dependency() {
    let root = std::env::temp_dir().join(format!("pkg-transnative-{}", std::process::id()));
    // `mid` is a MIXED `.ys` package: it has a native dependency on a crate (≈ synth).
    write_pkg(
        &root,
        "mid",
        "def package = { name: 'mid', version: '1.0.0', dependencies: { audio: { native: '../crates/prism-audio' } } }\n",
    );
    // `top` depends on `mid` by path — a PURE `.ys` edge, NO direct native dep.
    write_pkg(
        &root,
        "top",
        "def package = { name: 'top', dependencies: { mid: { path: '../mid' } } }\n",
    );

    let manifest = Manifest::load(root.join("top")).expect("top manifest");
    // The top manifest has NO direct native dependency...
    assert!(
        !manifest.dependencies.iter().any(|d| d.source.is_native()),
        "top has no DIRECT native dep"
    );
    // ...but the TRANSITIVE walk finds `mid`'s `audio` native edge — the gap, fixed.
    let natives = collect_native_crates(&manifest).expect("collect native crates");
    assert_eq!(natives.len(), 1, "the transitive native crate is found");
    assert_eq!(natives[0].edge_name, "audio");
    assert!(
        natives[0].crate_dir.to_string_lossy().contains("prism-audio"),
        "the crate dir resolves relative to mid: {:?}",
        natives[0].crate_dir
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_pure_ys_dag_has_no_native_crates() {
    let root = std::env::temp_dir().join(format!("pkg-transnative-pure-{}", std::process::id()));
    write_pkg(&root, "leaf", "def package = { name: 'leaf', version: '1.0.0' }\n");
    write_pkg(
        &root,
        "top",
        "def package = { name: 'top', dependencies: { leaf: { path: '../leaf' } } }\n",
    );
    let manifest = Manifest::load(root.join("top")).expect("top manifest");
    assert!(
        collect_native_crates(&manifest).expect("collect").is_empty(),
        "a pure-.ys DAG has no native crates"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_package_with_no_lib_ys_resolves_as_a_deps_only_wrapper() {
    // A package with NO `lib.ys` (a native-dep wrapper / deps-only umbrella — the shape
    // `packages/synth` has) contributes no own theory but passes its DEPS through. (The
    // resolver dogfooding surfaced this when a project depended on `synth`.)
    let root = std::env::temp_dir().join(format!("pkg-nolib-{}", std::process::id()));
    // `leaf` is a real `.ys` library exporting `Tick`.
    let leaf = root.join("leaf");
    std::fs::create_dir_all(&leaf).unwrap();
    std::fs::write(
        leaf.join("project.ys"),
        "def package = { name: 'leaf', version: '1.0.0', exports: ['Tick'] }\n",
    )
    .unwrap();
    std::fs::write(
        leaf.join("lib.ys"),
        "process Tick ~{n :: Float} ->{n :: Float} (\n  {n: 1.0}\n)\n",
    )
    .unwrap();
    // `wrapper` has NO `lib.ys` — just a manifest depending on `leaf`.
    let wrapper = root.join("wrapper");
    std::fs::create_dir_all(&wrapper).unwrap();
    std::fs::write(
        wrapper.join("project.ys"),
        "def package = { name: 'wrapper', version: '1.0.0', dependencies: { leaf: { path: '../leaf' } } }\n",
    )
    .unwrap();
    // `top` depends on the lib-less `wrapper`.
    let top = root.join("top");
    std::fs::create_dir_all(&top).unwrap();
    std::fs::write(
        top.join("project.ys"),
        "def package = { name: 'top', dependencies: { wrapper: { path: '../wrapper' } } }\n",
    )
    .unwrap();

    let manifest = Manifest::load(&top).expect("top manifest");
    let resolution =
        resolve(&manifest, std_modules()).expect("resolves despite wrapper having no lib.ys");
    // The wrapper passed `leaf` THROUGH (its own theory empty); `leaf`'s `Tick` is linked.
    assert!(
        resolution.core.processes.contains("Tick"),
        "leaf's Tick reached through the lib-less wrapper"
    );

    std::fs::remove_dir_all(&root).ok();
}
