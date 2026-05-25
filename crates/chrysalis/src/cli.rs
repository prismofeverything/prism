//! The `run` command, factored out of the `chrysalis` binary so that BOTH the
//! binary (over the std packages) and a **generated runner crate** (over a
//! non-std package's packages — see [`crate::codegen`]) call the *same* code.
//! This is the "single path" of the build tool: there is one `run`
//! implementation, parameterized by the host's process registry, value-methods,
//! and importable modules.

use std::collections::BTreeMap;

use prism_bigraph::ProcessRegistry;
use prism_schema::MethodRegistry;

use crate::ast::{Def, Expr, TermArg};
use crate::compile::ModuleRegistry;
use crate::parse::parse_file;
use crate::runner::{invoke, invoke_driven, invoke_trace, run, serve_process, serve_stream};

/// Resolve `.ys`-file imports before compiling. A `from <module> import <names>`
/// is a **file module** when a `.ys` file backs it — it brings in that file's
/// top-level defs (recursively for its own `.ys` imports), PREPENDED so the
/// importer's own entry (its LAST interfaced def) is preserved:
///   - single-segment `from cell import Cell` ⇒ `<ys_root>/cell.ys` **iff it
///     exists** (a sibling file); otherwise it's a native/host import, left for
///     compile's `resolve_imports`;
///   - dotted `from <pkg>.<sub>.<file> import …` ⇒ `<ys_root>/<sub>/<file>.ys`
///     (package-rooted), always a file module.
/// The `ys_root` (the entry file's dir) is threaded constant so a nested import
/// resolves from the same package root, not the importer's subdir. (#25)
fn resolve_file_modules(
    program: &mut crate::ast::Program,
    ys_root: &std::path::Path,
) -> Result<(), String> {
    let mut prefix: Vec<Def> = Vec::new();
    let mut kept: Vec<Def> = Vec::new();
    for def in std::mem::take(&mut program.defs) {
        let (module, names) = match &def {
            Def::Use { module, names } => (module.clone(), names.clone()),
            _ => {
                kept.push(def);
                continue;
            }
        };
        // The `.ys` file backing this module, if any: dotted skips the package
        // segment, single-segment is a sibling of the entry file.
        let file = if module.contains('.') {
            let rel: std::path::PathBuf = module.split('.').skip(1).collect();
            ys_root.join(&rel).with_extension("ys")
        } else {
            ys_root.join(&module).with_extension("ys")
        };
        // A single-segment module is a file module only if its `.ys` exists; else
        // it's a native/host import (kept for compile's `resolve_imports`).
        if !module.contains('.') && !file.exists() {
            kept.push(def);
            continue;
        }
        {
            let mut imported = parse_file(&file)
                .map_err(|e| format!("import from `{module}` ({}): {e}", file.display()))?;
            resolve_file_modules(&mut imported, ys_root)?;
                // The module's host imports + TYPE vocabulary always come along; its
                // VALUE defs come by transitive reachability from the imported names
                // (so `import CometSection` pulls `Comet`, `Plot`, `Output`, … it
                // references — #25 transitive value-deps).
                let mut value_defs: std::collections::HashMap<String, Def> =
                    std::collections::HashMap::new();
                for d in imported.defs {
                    if matches!(d, Def::Use { .. } | Def::Type(_)) {
                        prefix.push(d);
                    } else {
                        value_defs.insert(crate::ast::def_name(&d).to_string(), d);
                    }
                }
                let mut want: Vec<String> = names.iter().map(|n| n.to_string()).collect();
                let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
                while let Some(name) = want.pop() {
                    if !taken.insert(name.clone()) {
                        continue;
                    }
                    if let Some(d) = value_defs.get(&name) {
                        let mut refs = std::collections::HashSet::new();
                        collect_def_refs(d, &mut refs);
                        for r in refs {
                            if !taken.contains(&r) && value_defs.contains_key(&r) {
                                want.push(r);
                            }
                        }
                        prefix.push(d.clone());
                    }
                }
        }
    }
    // Merge imported defs (prefix) ahead of the program's own (kept), so the
    // importer's last-def entry is preserved; de-duplicate by content so several
    // sections re-importing the shared report.section module don't duplicate
    // Trace/Figure or `from io import Path` (a duplicate type import would clash).
    let mut seen = std::collections::HashSet::new();
    program.defs = prefix
        .into_iter()
        .chain(kept)
        .filter(|d| {
            let key = match d {
                Def::Use { module, names } => format!("use:{module}:{}", names.join(",")),
                other => format!("def:{}", crate::ast::def_name(other)),
            };
            seen.insert(key)
        })
        .collect();
    Ok(())
}

/// The other-def names a def REFERENCES in its body (controls + bare vars) — to
/// pull a module's transitive value-deps when importing one of its defs.
fn collect_def_refs(def: &Def, out: &mut std::collections::HashSet<String>) {
    match def {
        Def::Composite(d) => collect_refs(&d.body, out),
        Def::Process(d) => collect_refs(&d.body, out),
        Def::Step(d) => collect_refs(&d.body, out),
        Def::Function(d) => collect_refs(&d.body, out),
        _ => {}
    }
}

fn collect_refs(e: &Expr, out: &mut std::collections::HashSet<String>) {
    match e {
        Expr::Var(n) => {
            out.insert(n.to_string());
        }
        Expr::Term {
            control,
            args,
            body,
            ..
        } => {
            out.insert(control.to_string());
            for a in args {
                collect_refs(arg_expr(a), out);
            }
            if let Some(b) = body {
                collect_refs(b, out);
            }
        }
        Expr::Parallel(v) | Expr::List(v) => {
            for x in v {
                collect_refs(x, out);
            }
        }
        Expr::KeyedEntry { value, .. } => collect_refs(value, out),
        Expr::Map(pairs) => {
            for (_, x) in pairs {
                collect_refs(x, out);
            }
        }
        Expr::Record(m) => {
            for x in m.values() {
                collect_refs(x, out);
            }
        }
        Expr::Block(b) => {
            for (_, x) in &b.bindings {
                collect_refs(x, out);
            }
            collect_refs(&b.value, out);
        }
        Expr::Let { bindings, body } => {
            for (_, x) in bindings {
                collect_refs(x, out);
            }
            collect_refs(body, out);
        }
        Expr::If { cond, then_, else_ } => {
            collect_refs(cond, out);
            collect_refs(then_, out);
            if let Some(e) = else_ {
                collect_refs(e, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_refs(lhs, out);
            collect_refs(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_refs(operand, out),
        Expr::Method { receiver, args, .. } => {
            collect_refs(receiver, out);
            for x in args {
                collect_refs(x, out);
            }
        }
        Expr::Field { base, .. } => collect_refs(base, out),
        Expr::Call { func, args } => {
            collect_refs(func, out);
            for x in args {
                collect_refs(x, out);
            }
        }
        Expr::Comprehension {
            source,
            filter,
            body,
            ..
        } => {
            collect_refs(source, out);
            if let Some(f) = filter {
                collect_refs(f, out);
            }
            collect_refs(body, out);
        }
        Expr::Rule { redex, reactum } => {
            collect_refs(redex, out);
            collect_refs(reactum, out);
        }
        Expr::Site { sort, .. } => {
            if let Some(s) = sort {
                collect_refs(s, out);
            }
        }
        Expr::ReplaceWith { id, with } => {
            collect_refs(id, out);
            collect_refs(with, out);
        }
        Expr::Where { inner, predicate } => {
            collect_refs(inner, out);
            collect_refs(predicate, out);
        }
        // Literals + place/link forms reference no other def.
        Expr::Unit
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Str(_)
        | Expr::Path(_)
        | Expr::Unbound
        | Expr::LinkVar(_) => {}
    }
}

fn arg_expr(a: &TermArg) -> &Expr {
    match a {
        TermArg::Positional(e) => e,
        TermArg::Named { value, .. } => value,
    }
}

/// `run <file.ys> [--time T] [--<port> SOURCE ...] [--in TRACE] [--out FILE]
/// [--trace [--sample-dt DT]]` over the given packages. Returns a process exit
/// code; prints results to stdout, errors to stderr. A `composite` entry with no
/// explicit `main` is invoked compositionally (decision #24): batch (a JSON
/// record), `--trace` (capture the outputs as an Arrow delta-log trace), or
/// `--in TRACE` (drive the inputs from an Arrow trace — the pipe
/// `A.ys --trace | B.ys --in -`). Otherwise the file runs as a script.
pub fn run_command(
    args: &[String],
    registry: ProcessRegistry,
    methods: MethodRegistry,
    modules: ModuleRegistry,
) -> i32 {
    // Reserved flags (`--time`, `--out`) vs arbitrary `--<port> SOURCE`
    // interface args (decision #24).
    let mut path: Option<String> = None;
    let mut time = 2.0_f64;
    let mut out: Option<String> = None;
    let mut input: Option<String> = None;
    let mut trace = false;
    let mut serve = false;
    let mut serve_node = false;
    let mut sample_dt = 1.0_f64;
    let mut inputs: BTreeMap<String, String> = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--time" => {
                i += 1;
                time = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(time);
            }
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "--in" => {
                i += 1;
                input = args.get(i).cloned();
            }
            "--trace" => trace = true,
            "--serve-stream" => serve = true,
            "--serve-process" => serve_node = true,
            "--sample-dt" => {
                i += 1;
                sample_dt = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(sample_dt);
            }
            flag if flag.starts_with("--") => {
                i += 1;
                inputs.insert(
                    flag[2..].to_string(),
                    args.get(i).cloned().unwrap_or_default(),
                );
            }
            p => path = Some(p.to_string()),
        }
        i += 1;
    }
    let Some(path) = path else {
        eprintln!(
            "usage: run <file.ys> [--time T] [--<port> SOURCE ...] [--in TRACE | --serve-stream] [--out FILE] [--trace [--sample-dt DT]]"
        );
        return 2;
    };
    let mut prog = match parse_file(&path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parse {path}: {e}");
            return 1;
        }
    };
    // Resolve `.ys`-file imports (a dotted `from pkg.sub import file`): merge the
    // imported files' defs into this program before compiling (#25).
    let ys_root = std::path::Path::new(&path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    if let Err(e) = resolve_file_modules(&mut prog, ys_root) {
        eprintln!("{e}");
        return 1;
    }

    // A `composite` entry with no explicit `main` ⇒ compositional invocation.
    let invokes = matches!(prog.entry(), Some(Def::Composite(_))) && prog.lookup("main").is_none();
    if invokes {
        // Process proxy (`--serve-process`): the `stream:` protocol's child side —
        // run the entry as a faithful subengine, FORWARDING its update delta each
        // tick (the pipe mirror of the `rest:` server). Distinct from the
        // `--serve-stream` trace filter below.
        if serve_node {
            return match serve_process(
                &prog,
                registry,
                methods,
                modules,
                &inputs,
                std::io::stdin().lock(),
                std::io::stdout().lock(),
            ) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("serve-process {path}: {e}");
                    1
                }
            };
        }

        // Live streaming filter (`--serve-stream`): a `Trace[In] → Trace[Out]`
        // transformer — read input frames from stdin, emit absolute output frames
        // (a replayable delta-log) to stdout. The data-pipeline (`A | B`) mode.
        if serve {
            return match serve_stream(
                &prog,
                registry,
                methods,
                modules,
                &inputs,
                std::io::stdin().lock(),
                std::io::stdout().lock(),
            ) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("serve-stream {path}: {e}");
                    1
                }
            };
        }

        // Driven (input→trace): `--in` feeds an Arrow trace into the inputs.
        if let Some(src) = input {
            let bytes = match read_input_bytes(&src) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("read input {src}: {e}");
                    return 1;
                }
            };
            let in_trace = match prism_trace::deserialize_trace(&bytes) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("decode input trace: {e}");
                    return 1;
                }
            };
            let out_trace =
                match invoke_driven(&prog, registry, methods, modules, &inputs, &in_trace) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("run {path}: {e}");
                        return 1;
                    }
                };
            return if trace {
                emit_trace(&out_trace, out, &path, time, sample_dt)
            } else {
                emit_json(&last_frame(&out_trace), out, &path, time)
            };
        }

        // output→trace: capture the per-tick output delta-log onto the Arrow wire.
        if trace {
            return match invoke_trace(&prog, registry, methods, modules, &inputs, time, sample_dt) {
                Ok(captured) => emit_trace(&captured, out, &path, time, sample_dt),
                Err(e) => {
                    eprintln!("run {path}: {e}");
                    1
                }
            };
        }

        // Batch: the final-frame output record as JSON.
        return match invoke(&prog, registry, methods, modules, &inputs, time) {
            Ok(record) => emit_json(&record, out, &path, time),
            Err(e) => {
                eprintln!("run {path}: {e}");
                1
            }
        };
    }

    // Otherwise a `main`/script file: run for `time` and report.
    match run(&prog, registry, methods, modules, time) {
        Ok(state) => {
            let keys: Vec<String> = state
                .as_map()
                .map(|m| m.keys().map(|k| k.to_string()).collect())
                .unwrap_or_default();
            println!("ran {path} (t={time}); final state: {keys:?}");
            0
        }
        Err(e) => {
            eprintln!("run {path}: {e}");
            1
        }
    }
}

/// Read an input-trace SOURCE as bytes: `-` is stdin, otherwise a file path.
fn read_input_bytes(src: &str) -> std::io::Result<Vec<u8>> {
    if src == "-" {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        std::fs::read(src)
    }
}

/// Emit a `Trace` value on the Arrow wire to `out` (a file) or stdout (binary).
fn emit_trace(
    trace: &prism_schema::Value,
    out: Option<String>,
    path: &str,
    time: f64,
    dt: f64,
) -> i32 {
    let bytes = match prism_trace::serialize_trace(trace) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("serialize trace: {e}");
            return 1;
        }
    };
    let frames = prism_trace::len(trace);
    match out {
        Some(file) => match std::fs::write(&file, &bytes) {
            Ok(()) => eprintln!("ran {path} (t={time}, dt={dt}); trace → {file} ({frames} frames)"),
            Err(e) => {
                eprintln!("write {file}: {e}");
                return 1;
            }
        },
        None => {
            use std::io::Write;
            if let Err(e) = std::io::stdout().write_all(&bytes) {
                eprintln!("write stdout: {e}");
                return 1;
            }
        }
    }
    0
}

/// Emit a record `Value` as pretty JSON to `out` (a file) or stdout.
fn emit_json(record: &prism_schema::Value, out: Option<String>, path: &str, time: f64) -> i32 {
    let json = match serde_json::to_string_pretty(record) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("serialize outputs: {e}");
            return 1;
        }
    };
    match out {
        Some(file) => match std::fs::write(&file, &json) {
            Ok(()) => eprintln!("ran {path} (t={time}); outputs → {file}"),
            Err(e) => {
                eprintln!("write {file}: {e}");
                return 1;
            }
        },
        None => println!("{json}"),
    }
    0
}

/// The final frame of an output trace (the batch result of a driven run).
fn last_frame(trace: &prism_schema::Value) -> prism_schema::Value {
    prism_trace::frames(trace)
        .pop()
        .unwrap_or(prism_schema::Value::None)
}
