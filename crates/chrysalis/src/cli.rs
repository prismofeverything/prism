//! The `run` command, factored out of the `chrysalis` binary so that BOTH the
//! binary (over the std packages) and a **generated runner crate** (over a
//! non-std package's packages — see [`crate::codegen`]) call the *same* code.
//! This is the "single path" of the build tool: there is one `run`
//! implementation, parameterized by the host's process registry, value-methods,
//! and importable modules.

use std::collections::BTreeMap;

use prism_bigraph::ProcessRegistry;
use prism_schema::MethodRegistry;

use crate::ast::Def;
use crate::compile::ModuleRegistry;
use crate::parse::parse_file;
use crate::runner::{invoke, invoke_driven, invoke_trace, run};

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
            "--sample-dt" => {
                i += 1;
                sample_dt = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(sample_dt);
            }
            flag if flag.starts_with("--") => {
                i += 1;
                inputs.insert(flag[2..].to_string(), args.get(i).cloned().unwrap_or_default());
            }
            p => path = Some(p.to_string()),
        }
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("usage: run <file.ys> [--time T] [--<port> SOURCE ...] [--in TRACE] [--out FILE] [--trace [--sample-dt DT]]");
        return 2;
    };
    let prog = match parse_file(&path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parse {path}: {e}");
            return 1;
        }
    };

    // A `composite` entry with no explicit `main` ⇒ compositional invocation.
    let invokes =
        matches!(prog.entry(), Some(Def::Composite(_))) && prog.lookup("main").is_none();
    if invokes {
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
            let out_trace = match invoke_driven(
                &prog, registry, methods, modules, &inputs, &in_trace, sample_dt,
            ) {
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
    prism_trace::frames(trace).pop().unwrap_or(prism_schema::Value::None)
}
