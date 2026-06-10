//! A generic `.ys` runner over chrysalis's std prelude (the bundled prism-std
//! natives), via the shared runner:
//!
//! ```text
//! cargo run -p chrysalis --example run -- path/to/file.ys [time]
//! ```
//!
//! Parses (resolving file imports), compiles against the std modules
//! (`core`/`integrators`/`chem`/`io`), runs for `time` (default 2.0); the
//! program's own `Output` steps write any artifacts. This is the std-only
//! runner; the full `chrysalis` CLI (see the plan) will resolve extra packages.

use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_core, std_modules};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: cargo run -p chrysalis --example run -- <file.ys> [time]");
            std::process::exit(2);
        }
    };
    let time: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2.0);

    let prog = parse_file(&path).unwrap_or_else(|e| panic!("parse {path}: {e}"));
    let state = chrysalis::runner::run(&prog, std_core(), std_modules(), time)
        .unwrap_or_else(|e| panic!("run {path}: {e}"));

    let keys: Vec<String> = state
        .as_map()
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();
    println!("ran {path} (t={time}); final state: {keys:?}");
}
