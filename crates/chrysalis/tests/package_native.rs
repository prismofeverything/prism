//! Phase 5 consumer (#67): the **native convergence** — one resolver, two Core
//! *sources*. A program depends on BOTH a native dependency (a Rust crate's
//! `domain_core()`, here a hand-built stand-in for `prism-audio`'s `audio_core()`)
//! and a `.ys` path dependency, and `resolve_with_natives` colimits them uniformly —
//! *native vs `.ys` is just where a part's Core comes from*.

use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrysalis::manifest::Manifest;
use chrysalis::parse::parse_program_in;
use chrysalis::prelude::std_modules;
use chrysalis::resolver::resolve_with_natives;
use chrysalis::runner::run;
use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::update::Update;
use prism_bigraph::Core;
use prism_schema::{Schema, Value};

/// A native module: emits +1.0 to its `n` output each tick (≈ `prism-audio`'s
/// `Oscillator` — a process that lives in a Rust crate, not in `.ys`).
#[derive(Debug)]
struct Pulse;
impl Process for Pulse {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("n".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("n".to_string(), Schema::float())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        Update::value(Value::tree([("n", Value::float(1.0))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A native crate's `domain_core()` — the Core a generated codegen runner would hand
/// to the resolver. Std floor + the crate's own `Pulse` process.
fn audio_core() -> Core {
    let mut registry = ProcessRegistry::new();
    registry.register("Pulse", |_| ProcessNode::Process(Box::new(Pulse)));
    Core::new().with_processes(Arc::new(registry))
}

fn write_pkg(root: &Path, name: &str, project_ys: &str, entry_name: &str, entry_ys: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.ys"), project_ys).unwrap();
    std::fs::write(dir.join(entry_name), entry_ys).unwrap();
}

#[test]
fn a_program_links_a_native_core_and_a_ys_dependency_together() {
    let root = std::env::temp_dir().join(format!("pkg-native-{}", std::process::id()));
    // The `.ys` dependency `foo`, exporting a process.
    write_pkg(
        &root,
        "foo",
        "def package = { name: 'foo', version: '1.0.0', exports: ['Tick'] }\n",
        "lib.ys",
        "process Tick ~{n :: Float} ->{n :: Float} (\n  {n: 1.0}\n)\n",
    );
    // The program: a NATIVE dep `audio` (its Core supplied below) + the `.ys` dep `foo`.
    write_pkg(
        &root,
        "app",
        "def package = { name: 'app', dependencies: { audio: { native: '../crates/prism-audio' }, foo: { path: '../foo' } } }\n",
        "main.ys",
        "from audio import Pulse\nfrom foo import Tick\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  p: Pulse ~{n: n} ->{n: n} |\n  t: Tick ~{n: n} ->{n: n}\n)\nMain[]\n",
    );

    let app_main = root.join("app").join("main.ys");
    let manifest = Manifest::find(&app_main).expect("app manifest");

    // Supply the native dependency's Core (keyed by the edge name) — what a codegen
    // runner does after linking the crate + calling its `prelude::core()`.
    let native_cores: HashMap<String, Core> = HashMap::from([("audio".to_string(), audio_core())]);
    let resolution = resolve_with_natives(&manifest, std_modules(), native_cores)
        .expect("native + `.ys` dependencies resolve + link");

    // Both Core sources are in the linked theory — the native `Pulse` AND the `.ys` `Tick`.
    assert!(resolution.core.processes.contains("Pulse"), "the native crate's Pulse linked");
    assert!(resolution.core.processes.contains("Tick"), "the `.ys` dependency's Tick linked");
    // The native dependency is in the resolved graph (its crate dir recorded).
    assert!(resolution.graph.iter().any(|p| p.name == "audio"));

    // END-TO-END: the program composes BOTH (`Pulse` + `Tick`, each +1/tick) → n = 2/tick
    // = 10.0 over 5 ticks. A mixed native + `.ys` program runs through one linked Core.
    let src = std::fs::read_to_string(&app_main).unwrap();
    let prog = parse_program_in(&src, root.join("app")).expect("parse app/main.ys");
    let state = run(&prog, resolution.core, resolution.modules, 5.0).expect("run the mixed program");
    assert_eq!(
        state.get_field("n").and_then(|v| v.as_f64()),
        Some(10.0),
        "native Pulse + `.ys` Tick each ran 5 ticks through the colimited Core: {state:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn an_unsupplied_native_dependency_errors_with_the_codegen_hint() {
    // In-process `resolve` supplies NO native Cores, so a native dep must route through
    // codegen — the error says so rather than trying to compile a crate as `.ys`.
    let root = std::env::temp_dir().join(format!("pkg-native-nohint-{}", std::process::id()));
    write_pkg(
        &root,
        "app",
        "def package = { name: 'app', dependencies: { audio: { native: '../crates/prism-audio' } } }\n",
        "main.ys",
        "composite Main ->{ n :: Float } (\n  n: 0.0\n)\nMain[]\n",
    );
    let manifest = Manifest::load(root.join("app")).expect("app manifest");
    let err = chrysalis::resolver::resolve(&manifest, std_modules())
        .err()
        .expect("an unsupplied native dep must error");
    assert!(
        err.contains("native dependency `audio`") && err.contains("codegen"),
        "the error names the native dep + points at codegen: {err}"
    );

    std::fs::remove_dir_all(&root).ok();
}
