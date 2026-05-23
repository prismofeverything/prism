//! The `chrysalis` build tool. Operates on `.ys` source over chrysalis's bundled
//! std library (prism-std):
//!
//! ```text
//! chrysalis run     <file.ys> [--time T]   parse → compile → run (workflow self-outputs)
//! chrysalis bigraph <file.ys>              emit the process-bigraph document (JSON)
//! chrysalis check   <file.ys>              parse + compile (contract/connection check), no run
//! ```
//!
//! std-only programs (importing `core`/`integrators`/`chem`/`io`) run in-process
//! here. Programs importing extra native packages need the codegen+compile path
//! (`chrysalis compile`, planned — see the task plan).

use chrysalis::compile::compile_with_modules;
use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_methods, std_modules, std_registry};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        usage();
        std::process::exit(2);
    };
    match cmd.as_str() {
        "run" => cmd_run(rest),
        "bigraph" => cmd_bigraph(rest),
        "check" => cmd_check(rest),
        "compile" => {
            eprintln!(
                "`chrysalis compile` (codegen for non-std packages) is not implemented yet; \
                 `chrysalis run` executes std programs in-process."
            );
            std::process::exit(1);
        }
        other => {
            eprintln!("chrysalis: unknown subcommand `{other}`");
            usage();
            std::process::exit(2);
        }
    }
}

fn usage() {
    eprintln!("usage: chrysalis <run|bigraph|check> <file.ys> [--time T]");
}

/// Parse `<file.ys> [--time T]` from the subcommand args.
fn path_and_time(args: &[String]) -> (String, f64) {
    let mut path = None;
    let mut time = 2.0;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--time" => {
                i += 1;
                time = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(time);
            }
            p => path = Some(p.to_string()),
        }
        i += 1;
    }
    match path {
        Some(p) => (p, time),
        None => {
            usage();
            std::process::exit(2);
        }
    }
}

fn die(context: &str, msg: impl std::fmt::Display) -> ! {
    eprintln!("chrysalis: {context}: {msg}");
    std::process::exit(1);
}

fn cmd_run(args: &[String]) {
    let (path, time) = path_and_time(args);
    let prog = parse_file(&path).unwrap_or_else(|e| die(&format!("parse {path}"), e));
    let state = chrysalis::runner::run(&prog, std_registry(), std_methods(), std_modules(), time)
        .unwrap_or_else(|e| die(&format!("run {path}"), e));
    let keys: Vec<String> = state
        .as_map()
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();
    println!("ran {path} (t={time}); final state: {keys:?}");
}

fn cmd_check(args: &[String]) {
    let (path, _) = path_and_time(args);
    let prog = parse_file(&path).unwrap_or_else(|e| die(&format!("parse {path}"), e));
    match compile_with_modules(&prog, std_registry(), std_methods(), std_modules()) {
        Ok(_) => println!("{path}: ok"),
        Err(e) => die(&path, format!("{e:?}")),
    }
}

fn cmd_bigraph(args: &[String]) {
    let (path, _) = path_and_time(args);
    let prog = parse_file(&path).unwrap_or_else(|e| die(&format!("parse {path}"), e));
    let result = compile_with_modules(&prog, std_registry(), std_methods(), std_modules())
        .unwrap_or_else(|e| die(&format!("compile {path}"), format!("{e:?}")));
    // The compiled main IS the process-bigraph document the engine runs.
    match serde_json::to_string_pretty(&result.initial_state) {
        Ok(json) => println!("{json}"),
        Err(e) => die("serialize", e),
    }
}
