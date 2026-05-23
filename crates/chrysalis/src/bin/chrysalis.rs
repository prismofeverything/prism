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

use std::sync::Arc;

use chrysalis::compile::compile_with_modules;
use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_core, std_methods, std_modules, std_registry};
use prism_bigraph::protocols::RestProcessServer;

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
        "server" => cmd_server(rest),
        "repl" => chrysalis::repl::run().unwrap_or_else(|e| die("repl", e)),
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
    eprintln!(
        "usage:\n  chrysalis run|check <file.ys> [--time T]\n  \
         chrysalis bigraph <file.ys> | export <file.ys> <out.json> | import <doc.json>\n  \
         chrysalis server [--port P]\n  \
         chrysalis repl"
    );
}

/// `chrysalis server [--port P]` — serve the std [`Core`] over the rest-process
/// protocol so remote clients (a `.ys` with `rest:` nodes, or any RestProcess)
/// can run std processes / composites on this host. Port 0 (the default) lets the
/// OS pick; the chosen port is printed.
fn cmd_server(args: &[String]) {
    let mut port: u16 = 0;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--port" {
            i += 1;
            port = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(port);
        }
        i += 1;
    }

    let core = std_core();
    let server = RestProcessServer::start_on(Arc::clone(&core.processes), ("127.0.0.1", port))
        .unwrap_or_else(|e| die("server", e));

    let mut served: Vec<&str> = core.processes.type_names();
    served.sort_unstable();
    println!(
        "chrysalis server: serving {} process types on {} (Ctrl-C to stop)",
        served.len(),
        server.base_url()
    );
    for name in &served {
        println!("  - {name}");
    }

    // Park the main thread; the server runs on its background thread until the
    // process is killed. (`core`/`server` stay in scope so neither is dropped.)
    loop {
        std::thread::park();
    }
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

/// `chrysalis bigraph <file.ys>`           — print the process-bigraph document.
/// `chrysalis bigraph export <f.ys> <out>` — write the document to a file.
/// `chrysalis bigraph import <doc> [--time T]` — run a document directly.
/// Invariant: `bigraph import (bigraph export f)` ≡ `run f`.
fn cmd_bigraph(args: &[String]) {
    match args.split_first() {
        Some((sub, rest)) if sub == "export" => cmd_bigraph_export(rest),
        Some((sub, rest)) if sub == "import" => cmd_bigraph_import(rest),
        _ => cmd_bigraph_print(args),
    }
}

/// Compile a `.ys` to its [`Document`] (schema + state).
fn document_for(path: &str) -> prism_bigraph::Document {
    let prog = parse_file(path).unwrap_or_else(|e| die(&format!("parse {path}"), e));
    chrysalis::runner::to_document(&prog, std_registry(), std_methods(), std_modules())
        .unwrap_or_else(|e| die(&format!("compile {path}"), format!("{e:?}")))
}

fn cmd_bigraph_print(args: &[String]) {
    let (path, _) = path_and_time(args);
    let doc = document_for(&path);
    match serde_json::to_string_pretty(&doc) {
        Ok(json) => println!("{json}"),
        Err(e) => die("serialize", e),
    }
}

fn cmd_bigraph_export(args: &[String]) {
    let [src, out] = match args {
        [a, b] => [a.clone(), b.clone()],
        _ => {
            eprintln!("usage: chrysalis bigraph export <file.ys> <out.json>");
            std::process::exit(2);
        }
    };
    let doc = document_for(&src);
    let json = serde_json::to_string_pretty(&doc).unwrap_or_else(|e| die("serialize", e));
    std::fs::write(&out, json).unwrap_or_else(|e| die(&format!("write {out}"), e));
    println!("exported {src} → {out}");
}

fn cmd_bigraph_import(args: &[String]) {
    let (path, time) = path_and_time(args);
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| die(&format!("read {path}"), e));
    let doc: prism_bigraph::Document =
        serde_json::from_str(&json).unwrap_or_else(|e| die(&format!("parse {path}"), e));
    let state = chrysalis::runner::run_document(&doc, std_core(), time)
        .unwrap_or_else(|e| die(&format!("import {path}"), e));
    let keys: Vec<String> = state
        .as_map()
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();
    println!("ran {path} (t={time}); final state: {keys:?}");
}
