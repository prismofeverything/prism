//! The std prelude — prism-std's native capabilities assembled into the
//! registries the runner/CLI need. chrysalis bundles prism-std as its standard
//! library (prism-std → chrysalis), so a `.ys` using std imports
//! (`core`/`integrators`/`chem`/`io`) runs with no extra packages. Downstream
//! packages (e.g. spatio-flux) extend these with their own.

use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::{Core, ProcessNode, ProcessRegistry};
use prism_schema::{Key, MethodError, MethodRegistry, Value};

use crate::compile::ModuleRegistry;

/// Process factories from prism-std (currently `RunProcess`).
pub fn std_registry() -> ProcessRegistry {
    let mut r = ProcessRegistry::new();
    prism_std::register_processes(&mut r);
    r
}

/// Value-methods from prism-std (`TimeSeries`/`Integrator`/`Figure`/`Map`)
/// plus chrysalis's homoiconic `Document.run(time)` (the run-half of the
/// `load(path).run(time)` pair).
pub fn std_methods() -> MethodRegistry {
    let mut m = MethodRegistry::new();
    prism_std::register_methods(&mut m);
    register_document_methods(&mut m);
    m
}

/// Register `Document.run(time)` — the run-half of `load(path).run(time)`.
/// Takes a Document value (the program-as-data shape `load()` returns:
/// `{_type: "Document", schema, state}`), reconstructs the `prism_bigraph::Document`,
/// runs it against `std_core()`, and returns the final state value.
fn register_document_methods(m: &mut MethodRegistry) {
    m.register("Document", "run", |recv, args| {
        let time = args
            .first()
            .and_then(|v| v.as_f64())
            .ok_or_else(|| MethodError::BadArgs {
                type_name: "Document".into(),
                method: "run".into(),
                message: "expected a numeric `time` argument (Document.run(t))".into(),
            })?;
        // If the Document remembers its source (from `load(path)`), re-compile
        // it so user-defined process factories (`Tick`, `Cell`, …) are in the
        // Core. Hand-constructed Documents (no `_source`) fall through to
        // `std_core()` — works as long as they reference only std processes.
        if let Some(source_path) = recv.get_field("_source").and_then(|v| v.as_str()) {
            return run_from_source(source_path, time);
        }
        let doc = value_to_document(recv).ok_or_else(|| MethodError::BadArgs {
            type_name: "Document".into(),
            method: "run".into(),
            message: "receiver is not a Document value (expected {_type: 'Document', schema, state})".into(),
        })?;
        crate::runner::run_document(&doc, std_core(), time).map_err(|e| MethodError::Failed {
            type_name: "Document".into(),
            method: "run".into(),
            message: format!("run_document failed: {e}"),
        })
    });
}

/// Re-compile + run the source at `path` — used by `Document.run(time)` when
/// the Document remembers its origin file. Each call gets fresh registries
/// so a loaded program's own user-defined processes are in scope; without
/// this re-compile step, `std_core()` wouldn't know how to build them.
fn run_from_source(path: &str, time: f64) -> Result<Value, MethodError> {
    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "Document".into(),
        method: "run".into(),
        message: format!("{context}: {msg}"),
    };
    let src = std::fs::read_to_string(path).map_err(|e| mk_err(&format!("read {path}"), e.to_string()))?;
    let prog =
        crate::parse::parse_program(&src).map_err(|e| mk_err(&format!("parse {path}"), e.to_string()))?;
    // Use the source file's directory as ys_root so a nested `load('sibling.ys')`
    // inside this program resolves relative to where it lives.
    let ys_root = std::path::Path::new(path).parent().map(|p| p.to_path_buf());
    crate::runner::run(&prog, std_registry(), std_methods(), std_modules_at(ys_root), time)
        .map_err(|e| mk_err(&format!("run {path}"), format!("{e:?}")))
}

/// Materialize a `prism_bigraph::Document` from a chrysalis Value of the
/// shape `{_type: "Document", schema, state}`. The inverse of [`document_to_value`].
fn value_to_document(v: &Value) -> Option<prism_bigraph::Document> {
    let map = v.as_map()?;
    if map.get("_type")?.as_str()? != "Document" {
        return None;
    }
    Some(prism_bigraph::Document {
        schema: map.get("schema").cloned(),
        state: map.get("state")?.clone(),
        ..prism_bigraph::Document::new()
    })
}

/// Render a `prism_bigraph::Document` as a chrysalis Value — the
/// program-as-data shape `load(path)` returns. The Document already stores
/// its schema + state as values, so the conversion is just key wrapping.
fn document_to_value(doc: &prism_bigraph::Document) -> Value {
    let mut fields: IndexMap<Key, Value> = IndexMap::new();
    fields.insert(Key::from("_type"), Value::String("Document".into()));
    if let Some(schema) = &doc.schema {
        fields.insert(Key::from("schema"), schema.clone());
    }
    fields.insert(Key::from("state"), doc.state.clone());
    Value::Map(fields)
}

/// The std library assembled as a runnable [`Core`]: the std process factories +
/// the generic `Composite` factory (so a server can build composites from a doc)
/// + std value-methods. One object carrying the full std capability set — used by
/// `chrysalis server` and any in-process host that wants it whole.
pub fn std_core() -> Core {
    // The Composite factory needs the whole Core (to build subengines); the Core
    // contains the registry that contains this factory — a cycle resolved by a
    // OnceLock set once the Core is built.
    let handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());
    let mut registry = ProcessRegistry::new();
    prism_std::register_processes(&mut registry);
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle not initialized");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("Composite::from_config"),
            ))
        });
    }
    let mut methods = MethodRegistry::new();
    prism_std::register_methods(&mut methods);
    let core = Core::new()
        .with_processes(Arc::new(registry))
        .with_methods(Arc::new(methods))
        .with_protocols(Arc::new(crate::stream::stream_protocols()));
    let _ = handle.set(core.clone());
    core
}

/// The std importable modules: `core` (RunProcess/Simulate), `integrators`
/// (rk4/euler), `chem` (CRN), `io` (Path, `load`). Paths passed to `load`
/// resolve against the **CWD** — see [`std_modules_at`] for the ys-root
/// resolving variant the bin uses.
pub fn std_modules() -> ModuleRegistry {
    std_modules_at(None)
}

/// Std importable modules with `load(path)` resolving relative to `ys_root`
/// (the entry `.ys` file's directory) when the supplied path is relative —
/// the same convention `from … import` already uses for sibling files. With
/// `ys_root = None`, behaves like [`std_modules`] (CWD-relative). The bin's
/// `run` path and `Document.run`'s re-compile both call this with the right
/// root, so a `.ys` author can write `load('sibling.ys')` and have it Just Work.
pub fn std_modules_at(ys_root: Option<std::path::PathBuf>) -> ModuleRegistry {
    let load_fn: crate::compile::HostFn = Arc::new(move |args| {
        let path = args.first().and_then(|v| v.as_str()).ok_or_else(|| {
            MethodError::BadArgs {
                type_name: "io".into(),
                method: "load".into(),
                message: "expected a path string argument (load('file.ys'))".into(),
            }
        })?;
        // Relative path + a known ys_root → resolve relative to that root.
        // Absolute paths and the no-root case pass through unchanged.
        let resolved: std::path::PathBuf = match (ys_root.as_ref(), std::path::Path::new(path).is_absolute()) {
            (Some(root), false) => root.join(path),
            _ => std::path::PathBuf::from(path),
        };
        load_program_as_document(resolved.to_str().unwrap_or(path))
    });
    ModuleRegistry::new()
        .process("core", "RunProcess")
        .process("core", "Simulate")
        .object("integrators", "rk4", prism_std::integrator("rk4"))
        .object("integrators", "euler", prism_std::integrator("euler"))
        .type_(
            "chem",
            "CRN",
            "{species: list[string], reactions: list[{reactants: map[float], products: map[float], k: float}]}",
        )
        .type_("io", "Path", "string")
        .function("io", "load", load_fn)
}

/// Read + parse + compile a `.ys` file from disk and return it as a chrysalis
/// Value (`{_type: "Document", schema, state}`). The homoiconic move: a
/// program is the same kind of value the runtime operates on, reachable as
/// data from inside `.ys` itself via `load(path)`.
///
/// Each call constructs fresh std registries — so a loaded program's own
/// `load` imports get a clean module scope, exactly like a fresh
/// `chrysalis run`. The returned Document is runnable via `Document.run(time)`.
/// Public form of the `io::load` native function — the Rust-side door for
/// the surface `load(path)`. Returns the loaded program as a Document Value
/// (`{_type: "Document", schema, state, _source}`); the same value the
/// surface caller gets, callable from Rust without going through method
/// dispatch. Pairs with the `Document.run(time)` method.
pub fn load(path: &str) -> Result<Value, MethodError> {
    load_program_as_document(path)
}

fn load_program_as_document(path: &str) -> Result<Value, MethodError> {
    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "io".into(),
        method: "load".into(),
        message: format!("{context}: {msg}"),
    };
    let src = std::fs::read_to_string(path).map_err(|e| mk_err(&format!("read {path}"), e.to_string()))?;
    let prog =
        crate::parse::parse_program(&src).map_err(|e| mk_err(&format!("parse {path}"), e.to_string()))?;
    // Compile against fresh std registries — same path `chrysalis run` uses.
    let result = crate::compile::compile_with_modules(&prog, std_registry(), std_methods(), std_modules())
        .map_err(|e| mk_err(&format!("compile {path}"), format!("{e:?}")))?;
    let doc = crate::runner::document_of(&result);
    let mut value = document_to_value(&doc);
    // Remember the source path so `Document.run(time)` can re-compile the
    // program for its own Core (which knows the user-defined `process`/
    // `composite` factories the Document references). Without this stash, the
    // .run path would see "unregistered process(es)" for anything beyond std.
    if let Value::Map(m) = &mut value {
        m.insert(Key::from("_source"), Value::String(path.into()));
    }
    Ok(value)
}
