//! The `chrysalis` build tool. Operates on `.ys` source over chrysalis's bundled
//! std library (prism-std):
//!
//! ```text
//! chrysalis new     <dir>                  scaffold a new project (project.ys + main.ys)
//! chrysalis run     <file.ys> [--time T]   parse → compile → run (workflow self-outputs)
//! chrysalis bigraph <file.ys>              emit the process-bigraph document (JSON)
//! chrysalis check   <file.ys>              parse + compile (contract/connection check), no run
//! chrysalis format  <file.ys> [-w]         parse → unparse (canonical layout)
//! ```
//!
//! std-only programs (importing `core`/`integrators`/`chem`/`io`) run in-process
//! here. Programs importing extra native packages need the codegen+compile path
//! (`chrysalis compile`, planned — see the task plan).

use std::sync::Arc;

use chrysalis::compile::compile_with_core;
use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_core, std_modules, std_modules_at};
use prism_bigraph::protocols::RestProcessServer;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        usage();
        std::process::exit(2);
    };
    match cmd.as_str() {
        "new" => cmd_new(rest),
        "run" => cmd_run(rest),
        "add" => cmd_add(rest),
        "remove" => cmd_remove(rest),
        "install" => cmd_install(rest),
        "update" => cmd_update(rest),
        "bigraph" => cmd_bigraph(rest),
        "check" => cmd_check(rest),
        "format" => cmd_format(rest),
        "server" => cmd_server(rest),
        "coord" => cmd_coord(rest),
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
        "usage:\n  chrysalis new <dir> [--force]\n  \
         chrysalis run <file.ys> [--time T] [--<port> SOURCE ...] [--out FILE]\n  \
         chrysalis check <file.ys> [--time T]\n  \
         chrysalis format <file.ys> [-w | --write]\n  \
         chrysalis bigraph <file.ys> | export <file.ys> <out.json> | import <doc.json>\n  \
         chrysalis server [--port P]\n  \
         chrysalis repl\n\
         \n  SOURCE: literal | file:PATH | - (stdin) | stream:… (reserved)"
    );
}

/// `chrysalis new <dir> [--force]` — scaffold a new chrysalis project.
///
/// Creates `<dir>/project.ys` (a `package <name>` directive) and `<dir>/main.ys`
/// (a minimal runnable composite). Flat layout — the convention is
/// `ys_root = entry_file_dir`, so siblings of `main.ys` are auto-importable
/// without any path config. Larger projects can move sources into `ys/` later.
///
/// The package name is derived from the directory's basename, normalized to a
/// valid identifier (non-alphanumeric → `-`). `chrysalis new .` populates the
/// current directory.
///
/// Refuses to overwrite an existing `project.ys` or `main.ys` without
/// `--force`, so re-running on a populated dir doesn't blow away work.
fn cmd_new(args: &[String]) {
    let mut dir: Option<String> = None;
    let mut force = false;
    for a in args {
        match a.as_str() {
            "--force" => force = true,
            "-h" | "--help" => {
                eprintln!("usage: chrysalis new <dir> [--force]");
                std::process::exit(0);
            }
            p if !p.starts_with('-') => dir = Some(p.to_string()),
            other => die("new", format!("unknown flag `{other}`")),
        }
    }
    let dir = dir.unwrap_or_else(|| {
        eprintln!("usage: chrysalis new <dir> [--force]");
        std::process::exit(2);
    });

    let dir_path = std::path::PathBuf::from(&dir);
    std::fs::create_dir_all(&dir_path)
        .unwrap_or_else(|e| die(&format!("create {dir}"), e));

    // Derive the package name from the directory's basename; for `.` fall back
    // to the current dir's name. Normalize non-identifier chars to `-`.
    let basename = dir_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "untitled".into());
    let package = sanitize_package_name(&basename);

    let project_ys = dir_path.join("project.ys");
    let main_ys = dir_path.join("main.ys");
    if !force {
        for p in [&project_ys, &main_ys] {
            if p.exists() {
                die(
                    "new",
                    format!("`{}` already exists (use --force to overwrite)", p.display()),
                );
            }
        }
    }

    let project_contents = format!(
        "# chrysalis project manifest — {package}.\n\
         #\n\
         # `chrysalis run <file.ys>` resolves imports relative to the entry file's\n\
         # directory — sibling `.ys` files are auto-importable. Move sources into a\n\
         # `ys/` subdir if the project grows.\n\
         #\n\
         # This project is STD-ONLY (runs in-process against the bundled std library).\n\
         # To link a non-std native package (HiGHS solver, rapier2d physics, …),\n\
         # uncomment the `package` directive below and ensure that crate's\n\
         # `prelude::{{core, modules}}` is exported (a package = a Core) — `chrysalis run`\n\
         # then generates + builds + caches a runner that links it. See\n\
         # docs/chrysalis-design.md (the codegen path).\n\
         #\n\
         # package {package}\n"
    );
    let main_contents = "\
# A minimal chrysalis program — a counter that ticks up by one each step.
# Run:   chrysalis run main.ys --time 5

process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)

composite Main ->{count :: Float} (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}
)
";

    std::fs::write(&project_ys, project_contents)
        .unwrap_or_else(|e| die(&format!("write {}", project_ys.display()), e));
    std::fs::write(&main_ys, main_contents)
        .unwrap_or_else(|e| die(&format!("write {}", main_ys.display()), e));

    eprintln!(
        "created {dir}/\n  - project.ys   (package {package})\n  - main.ys\n\
         \nnext:\n  cd {dir} && chrysalis run main.ys --time 5"
    );
}

/// Normalize an arbitrary directory basename to a valid chrysalis package
/// identifier. Lowercase ASCII alphanumerics + `-` survive; everything else
/// (spaces, dots, `_` → `-`) maps to a single dash. Leading digits get an
/// `_` prefix so the result is a valid identifier. Empty → `untitled`.
fn sanitize_package_name(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return "untitled".into();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// `chrysalis format <file.ys> [-w]` — canonicalize a `.ys` file's layout by
/// running it through `parse → unparse`. Without `-w` writes to stdout (so
/// `git diff <(chrysalis format f.ys) f.ys` previews the change); with `-w`
/// or `--write` rewrites in place.
///
/// CAVEAT: comments (#-to-EOL) are not yet preserved through the round-trip —
/// the lexer skips them, so the parser never sees them. `format` emits a
/// stderr warning when the source has comments and refuses to write in-place
/// without `--force`. Comment preservation lands with task #10.
fn cmd_format(args: &[String]) {
    let mut path: Option<String> = None;
    let mut write = false;
    let mut force = false;
    for a in args {
        match a.as_str() {
            "-w" | "--write" => write = true,
            "--force" => force = true,
            "-h" | "--help" => {
                eprintln!("usage: chrysalis format <file.ys> [-w | --write] [--force]");
                std::process::exit(0);
            }
            p if !p.starts_with('-') => path = Some(p.to_string()),
            other => die("format", format!("unknown flag `{other}`")),
        }
    }
    let path = path.unwrap_or_else(|| {
        eprintln!("usage: chrysalis format <file.ys> [-w | --write] [--force]");
        std::process::exit(2);
    });

    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| die(&format!("read {path}"), e));
    let has_comments = source.lines().any(|l| l.trim_start().starts_with('#'));

    let prog = chrysalis::parse::parse_program(&source)
        .unwrap_or_else(|e| die(&format!("parse {path}"), e));
    let formatted = chrysalis::unparse::unparse(&prog);

    if has_comments {
        eprintln!(
            "chrysalis format: warning — `{path}` contains comments; format \
             does not yet preserve them (task #10). Output will lose them."
        );
        if write && !force {
            die(
                "format",
                "refusing to overwrite a commented file without --force",
            );
        }
    }

    if write {
        std::fs::write(&path, &formatted)
            .unwrap_or_else(|e| die(&format!("write {path}"), e));
        eprintln!("formatted {path}");
    } else {
        print!("{formatted}");
    }
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
    let server = RestProcessServer::start_on(core.clone(), ("127.0.0.1", port))
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

/// `chrysalis coord <serve|push|pull|set>` — the agent coordination board (#62
/// dogfood). `serve` hosts the board as one `map[any]` mesh link; `push` merges a
/// peer's key over the socket (per-source, no collision); `pull` prints the
/// converged board; `set <peer> field=value …` updates a peer's durable heartbeat
/// FILE from data (serialize + round-trip gate — no hand-typed braces/quotes). `set`
/// is the ONE board door: it CREATES the heartbeat on first use (boot into the board
/// in one command) + joins coord/board.ys; a field may be a dotted path
/// (`build.state=green`) and a value may be a JSON list (`touching=["a","b"]`).
fn cmd_coord(args: &[String]) {
    // Pull out `--port P`; the rest are positional (sub [peer] [json]).
    let mut port = chrysalis::coord::DEFAULT_PORT;
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--port" {
            i += 1;
            if let Some(p) = args.get(i).and_then(|s| s.parse().ok()) {
                port = p;
            }
        } else {
            pos.push(&args[i]);
        }
        i += 1;
    }
    let Some((sub, rest)) = pos.split_first() else {
        die("coord", "usage: chrysalis coord <serve|push <peer> '<json>'|pull|set <peer> field=value …> [--port P]");
    };
    match *sub {
        "serve" => chrysalis::coord::serve(port).unwrap_or_else(|e| die("coord serve", e)),
        "push" => {
            let peer = rest.first().unwrap_or_else(|| die("coord push", "need <peer>"));
            let json = rest.get(1).unwrap_or_else(|| die("coord push", "need '<json>'"));
            chrysalis::coord::push(port, peer, json).unwrap_or_else(|e| die("coord push", e));
        }
        "pull" => chrysalis::coord::pull(port).unwrap_or_else(|e| die("coord pull", e)),
        // `set <peer> field=value …` — update a heartbeat FROM DATA (serialize
        // through the unparser + round-trip gate), so a brace / apostrophe can never
        // be hand-typed into the board. `tick` auto-bumps unless set explicitly.
        "set" => {
            let peer = rest.first().unwrap_or_else(|| die("coord set", "need <peer>"));
            if rest.len() < 2 {
                die("coord set", "need at least one field=value");
            }
            let assignments: Vec<(String, String)> = rest[1..]
                .iter()
                .map(|a| match a.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => die("coord set", format!("expected field=value, got `{a}`")),
                })
                .collect();
            chrysalis::coord::set(peer, &assignments).unwrap_or_else(|e| die("coord set", e));
        }
        other => die("coord", format!("unknown subcommand `{other}` (serve|push|pull|set)")),
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
    // A `.ys` under a non-std `project.ys` is run via the codegen path (generate
    // + build + cache a runner crate linking that package); otherwise run
    // in-process over the std packages. Both call the SAME `cli::run_command`.
    let entry_path = args.iter().find(|a| !a.starts_with("--")).cloned();
    if let Some(path) = entry_path.as_deref() {
        // A structured `.ys`-package manifest (`def package = { … }`). Native parts (a
        // `native:` dependency — a Rust crate chrysalis can't link in-process) route
        // through the codegen runner (#67 Phase 5c); pure-`.ys` dependencies resolve
        // in-process (#67 Phase 1). A dep-less / non-package `.ys` has no such manifest,
        // so `Manifest::find` returns `None` and we fall through to the std run below.
        if let Some(manifest) = chrysalis::manifest::Manifest::find(path) {
            if chrysalis::codegen::has_native_parts(&manifest) {
                std::process::exit(chrysalis::codegen::run_structured(&manifest, "run", args));
            }
            if !manifest.dependencies.is_empty() {
                let prog_dir = std::path::Path::new(path).parent().map(|d| d.to_path_buf());
                match chrysalis::resolver::resolve(&manifest, std_modules_at(prog_dir)) {
                    Ok(r) => {
                        // Pin the resolved graph (reproducible builds). Best-effort —
                        // a read-only project must not fail the run.
                        if let Err(e) = chrysalis::lockfile::write(&r.graph, &manifest.dir) {
                            eprintln!(
                                "chrysalis: warning: could not write {}: {e}",
                                chrysalis::lockfile::LOCKFILE
                            );
                        }
                        std::process::exit(chrysalis::cli::run_command(args, r.core, r.modules))
                    }
                    Err(e) => die("resolving dependencies", e),
                }
            }
        }
    }
    // `load(path)` resolves relative to the entry file's directory — so a
    // `.ys` demo can reference siblings without absolute paths. Mirrors the
    // `from … import` convention already in place for file modules.
    let ys_root =
        entry_path.and_then(|p| std::path::Path::new(&p).parent().map(|d| d.to_path_buf()));
    std::process::exit(chrysalis::cli::run_command(
        args,
        std_core(),
        std_modules_at(ys_root),
    ));
}

/// `chrysalis add <name> [--path <p> | --native <p>] [--version <v>]` — add a
/// dependency to the nearest `project.ys` (walking up from the CWD), through the
/// homoiconic round-trip (`manifest::add_dependency` — parse → set → unparse → gate),
/// so a hand-edit can never wedge the manifest. #67 Phase 3; a registry source
/// (`chrysalis add foo` with no `--path`/`--native`) arrives in Phase 3b.
fn cmd_add(args: &[String]) {
    use chrysalis::manifest::{add_dependency_to_file, DependencySource, Manifest};

    let mut name: Option<String> = None;
    let mut path: Option<String> = None;
    let mut native: Option<String> = None;
    let mut version: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                path = args.get(i).cloned();
            }
            "--native" => {
                i += 1;
                native = args.get(i).cloned();
            }
            "--version" => {
                i += 1;
                version = args.get(i).cloned();
            }
            a if !a.starts_with("--") => name = Some(a.to_string()),
            other => die("add", format!("unknown flag `{other}`")),
        }
        i += 1;
    }
    let Some(name) = name else {
        eprintln!("usage: chrysalis add <name> [--path <p> | --native <p>] [--version <v>]");
        std::process::exit(2);
    };

    let cwd = std::env::current_dir().unwrap_or_else(|e| die("add", format!("cwd: {e}")));
    let manifest = Manifest::find(&cwd).unwrap_or_else(|| {
        die(
            "add",
            "no `project.ys` with a `def package = { … }` record found (run inside a chrysalis project)",
        )
    });
    let source = match (path, native) {
        (Some(p), None) => DependencySource::Path(p.into()),
        (None, Some(n)) => DependencySource::Native(n.into()),
        (None, None) => {
            // No `--path`/`--native` ⇒ a REGISTRY dependency (resolved by name +
            // version against the project's `registry:`); it needs a version.
            if version.is_none() {
                die(
                    "add",
                    "a registry dependency needs `--version <req>` (or use `--path <p>` / `--native <p>`)",
                );
            }
            DependencySource::Registry
        }
        (Some(_), Some(_)) => die("add", "`--path` and `--native` are mutually exclusive"),
    };
    match add_dependency_to_file(&manifest.dir, &name, &source, version.as_deref()) {
        Ok(()) => eprintln!("added `{name}` to {}", manifest.dir.join("project.ys").display()),
        Err(e) => die("add", e),
    }
}

/// `chrysalis remove <name>` — remove a dependency from the nearest `project.ys`
/// (the homoiconic round-trip, the reverse of `add`). #67 Phase 3.
fn cmd_remove(args: &[String]) {
    let name = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            eprintln!("usage: chrysalis remove <name>");
            std::process::exit(2);
        });
    let cwd = std::env::current_dir().unwrap_or_else(|e| die("remove", format!("cwd: {e}")));
    let manifest = chrysalis::manifest::Manifest::find(&cwd)
        .unwrap_or_else(|| die("remove", "no `project.ys` found (run inside a chrysalis project)"));
    match chrysalis::manifest::remove_dependency_from_file(&manifest.dir, &name) {
        Ok(()) => eprintln!("removed `{name}` from {}", manifest.dir.join("project.ys").display()),
        Err(e) => die("remove", e),
    }
}

/// `chrysalis install` — resolve the project's dependencies (honoring `project.lock`)
/// and write the lock, WITHOUT running. The "prepare" step. #67 Phase 3.
fn cmd_install(_args: &[String]) {
    resolve_and_lock("install", false);
}

/// `chrysalis update` — re-resolve every registry dependency to the newest satisfying
/// version (IGNORING the lock pins) and re-write `project.lock`. #67 Phase 3.
fn cmd_update(_args: &[String]) {
    resolve_and_lock("update", true);
}

/// Shared body of `install`/`update`: find the project, resolve (honoring or ignoring
/// the lock), and write `project.lock`.
fn resolve_and_lock(cmd: &str, update: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| die(cmd, format!("cwd: {e}")));
    let manifest = chrysalis::manifest::Manifest::find(&cwd)
        .unwrap_or_else(|| die(cmd, "no `project.ys` found (run inside a chrysalis project)"));
    if chrysalis::codegen::has_native_parts(&manifest) {
        die(
            cmd,
            "this project has native dependencies — use `chrysalis run` (the codegen path links them)",
        );
    }
    let resolution = if update {
        chrysalis::resolver::resolve_update(&manifest, std_modules())
    } else {
        chrysalis::resolver::resolve(&manifest, std_modules())
    }
    .unwrap_or_else(|e| die(cmd, e));
    chrysalis::lockfile::write(&resolution.graph, &manifest.dir).unwrap_or_else(|e| die(cmd, e));
    eprintln!(
        "{cmd}: resolved {} package(s) → wrote {}",
        resolution.graph.len(),
        manifest.dir.join("project.lock").display()
    );
}

fn cmd_check(args: &[String]) {
    let (path, _) = path_and_time(args);
    let prog = parse_file(&path).unwrap_or_else(|e| die(&format!("parse {path}"), e));
    match compile_with_core(&prog, std_core(), std_modules()) {
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
    chrysalis::runner::to_document(&prog, std_core(), std_modules())
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
