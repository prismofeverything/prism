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
use crate::runner::{invoke, invoke_trace, run};

/// `run <file.ys> [--time T] [--<port> SOURCE ...] [--out FILE]` over the given
/// packages. Returns a process exit code (0 ok, non-zero on error); prints the
/// result to stdout and errors to stderr. A `composite` entry with no explicit
/// `main` is invoked compositionally (the command line binds its interface,
/// outputs render as JSON); otherwise the file is run as a script.
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
        eprintln!("usage: run <file.ys> [--time T] [--<port> SOURCE ...] [--out FILE] [--trace [--sample-dt DT]]");
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
        if trace {
            // output→trace: capture the per-tick output delta-log and emit it on
            // the Arrow wire (binary) to stdout or `--out FILE`.
            let captured =
                match invoke_trace(&prog, registry, methods, modules, &inputs, time, sample_dt) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("run {path}: {e}");
                        return 1;
                    }
                };
            let bytes = match prism_trace::serialize_trace(&captured) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("serialize trace: {e}");
                    return 1;
                }
            };
            let frames = prism_trace::len(&captured);
            match out {
                Some(file) => match std::fs::write(&file, &bytes) {
                    Ok(()) => eprintln!("ran {path} (t={time}, dt={sample_dt}); trace → {file} ({frames} frames)"),
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
            return 0;
        }
        let record = match invoke(&prog, registry, methods, modules, &inputs, time) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("run {path}: {e}");
                return 1;
            }
        };
        let json = match serde_json::to_string_pretty(&record) {
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
        return 0;
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
