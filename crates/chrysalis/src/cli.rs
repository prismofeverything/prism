//! The `run` command, factored out of the `chrysalis` binary so that BOTH the
//! binary (over the std packages) and a **generated runner crate** (over a
//! non-std package's packages — see [`crate::codegen`]) call the *same* code.
//! This is the "single path" of the build tool: there is one `run`
//! implementation, parameterized by the host's process registry, value-methods,
//! and importable modules.

use std::collections::BTreeMap;

use indexmap::IndexMap;

use prism_bigraph::ProcessRegistry;
use prism_schema::MethodRegistry;

use crate::ast::{
    CompositeDef, Def, Expr, Interface, Param, PortDecl, Program, SchemaExpr,
};
use crate::compile::ModuleRegistry;
use crate::parse::parse_file;
use crate::runner::{invoke, invoke_driven, invoke_trace, run, serve_process, serve_stream};

/// Resolve a `stream` protocol's relative `.ys` `path` against the entry file's
/// dir (`ys_root`), exactly like a sibling `from cell import` — so a child cell
/// runs no matter the caller's cwd. A `protocol StreamingCell = stream<Cell,
/// path: 'cell.ys'>` declares the child by a path relative to the file it's
/// written in; absolutize it here (the one place that knows that file's dir)
/// rather than leaking cwd-dependence into the protocol runtime. A plain
/// relative string is rewritten; absolute / interpolated paths are left alone.
fn resolve_stream_paths(program: &mut crate::ast::Program, ys_root: &std::path::Path) {
    for def in &mut program.defs {
        let Def::Protocol(pd) = def else { continue };
        if pd.protocol != "stream" {
            continue;
        }
        for (field, value) in &mut pd.fields {
            if field != "path" {
                continue;
            }
            let Expr::Str(lit) = value else { continue };
            let Some(rel) = lit.as_plain() else { continue }; // interpolated → leave
            let path = std::path::Path::new(&rel);
            if path.is_absolute() {
                continue;
            }
            let abs = ys_root.join(path);
            if let Some(s) = abs.to_str() {
                *value = Expr::Str(crate::ast::StringLit::plain(s));
            }
        }
    }
}

/// If the file's entry is a bare `process`/`step` (no place-graph to live in),
/// wrap it in a synthesized **default harness** composite so it runs standalone —
/// "a heart beating on a bench." The harness gives every port a state slot, wires
/// the process to those slots (same-name self-loop, so a read-modify-write port
/// like `mass` ACCUMULATES its delta), seeds the input slots from config params
/// (`--mass 1.0` etc., defaulted to a type-zero), and exposes every slot as an
/// output so the run reports the evolved state. `interval` is engine-supplied, not
/// a slot. A composite entry already carries its own place-graph (its body), so it
/// is left alone. The harness is appended as the new entry (`prog.entry()` = last
/// interfaced def).
fn harness_process_entry(prog: &mut Program) {
    let (name, params, interface) = match prog.entry() {
        Some(Def::Process(p)) => (p.name.clone(), p.params.clone(), p.interface.clone()),
        Some(Def::Step(s)) => (s.name.clone(), s.params.clone(), s.interface.clone()),
        _ => return,
    };
    prog.push(Def::Composite(harness_composite(&name, &params, &interface)));
}

/// Build the default-harness composite wrapping a `process`/`step` named `entry`.
fn harness_composite(entry: &str, params: &[Param], interface: &Interface) -> CompositeDef {
    let is_slot = |n: &str| n != "interval";
    // Slots = union of in/out ports (minus `interval`), typed by their decl
    // (input type preferred). IndexMap preserves a stable order.
    let mut slots: IndexMap<crate::ast::Name, SchemaExpr> = IndexMap::new();
    for (n, d) in interface.inputs.iter().chain(interface.outputs.iter()) {
        if is_slot(n) {
            slots.entry(n.clone()).or_insert_with(|| d.schema.clone());
        }
    }

    // Config: the process's own params (passthrough, keep defaults) + each input
    // port as a seedable, defaulted config param (so `--port v` initializes it).
    let mut config: Vec<Param> = params.to_vec();
    for (n, d) in &interface.inputs {
        if is_slot(n) {
            let default = d.default.clone().unwrap_or_else(|| default_expr_for(&d.schema));
            config.push(Param::with_default(n.clone(), d.schema.clone(), default));
        }
    }

    // Body: each slot initialized (input slot ← its config param; output-only slot
    // ← a type-zero) + the process term, self-wired (same-name in/out, no interval),
    // its own config args threaded from the harness params.
    let mut body: Vec<Expr> = Vec::new();
    for (slot, ty) in &slots {
        let init = if interface.inputs.contains_key(slot) {
            Expr::var(slot.clone())
        } else {
            default_expr_for(ty)
        };
        body.push(Expr::entry(slot.clone(), init));
    }
    let mut term = Expr::term(entry);
    for p in params {
        term = term.arg_named(p.name.clone(), Expr::var(p.name.clone()));
    }
    for (n, _) in &interface.inputs {
        if is_slot(n) {
            term = term.input(n.clone(), Expr::var(n.clone()));
        }
    }
    for (n, _) in &interface.outputs {
        term = term.output(n.clone(), Expr::var(n.clone()));
    }
    body.push(Expr::entry(lower_first(entry), term.build()));

    // Every slot is an output port (bridged to itself), so the run reports it.
    let mut iface = Interface::new();
    for (slot, ty) in &slots {
        iface = iface.with_output(
            slot.clone(),
            PortDecl {
                schema: ty.clone(),
                default: None,
                contract: None,
                bridge: Some(vec![slot.clone()]),
            },
        );
    }

    CompositeDef {
        name: format!("{entry}__bench"),
        params: config,
        interface: iface,
        using: vec![],
        body: Expr::parallel(body),
    }
}

/// A type-appropriate zero literal for an unseeded harness slot.
fn default_expr_for(schema: &SchemaExpr) -> Expr {
    match schema {
        SchemaExpr::Bool => Expr::bool(false),
        SchemaExpr::Int => Expr::int(0),
        // Float / Quantity / Custom-numeric (e.g. a `Mass` alias) / fallback.
        _ => Expr::float(0.0),
    }
}

/// Lowercase the first character (a process `Grow` keys its harness node `grow`).
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
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
    // Resolve file-module imports by explicit origin: a dotted/relative path
    // (`.cell`, `lib.x`) is a file; a bare name (`diffusion`) is a native import,
    // resolved at compile against `modules`. No path search, no precedence.
    let mut prog = match parse_file(&path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parse {path}: {e}");
            return 1;
        }
    };
    // `.ys`-file imports are already resolved by `parse_file` (#50). The entry
    // file's dir is still needed to absolutize relative `stream:<.ys>` child
    // paths (so a `stream<Cell, path: 'cell.ys'>` cell spawns regardless of cwd).
    let ys_root = std::path::Path::new(&path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    resolve_stream_paths(&mut prog, ys_root);
    // A bare `process`/`step` entry gets a default harness so it runs standalone
    // (`chrysalis run grow.ys --mass 1 --glucose 5`). Skip when serving as a
    // stream child (`--serve-process`): there the entry is driven over the bridge,
    // not run on a bench.
    if !serve_node {
        harness_process_entry(&mut prog);
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
