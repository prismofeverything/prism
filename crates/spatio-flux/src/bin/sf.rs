//! `sf` — run a spatio-flux `.ys` through chrysalis's runner over spatio-flux's
//! packages. This is the IN-TREE equivalent of what `chrysalis run` will do once
//! the codegen path (#10) can link a non-std package: the SAME run path
//! (`chrysalis::runner::run`), only the imported packages differ. (A generated
//! runner crate's `main` will look just like this.)
//!
//! ```text
//! cargo run -p spatio-flux --bin sf -- run <file.ys> [--time T]
//! ```

use chrysalis::parse::parse_file;
use spatio_flux::prelude::{sf_methods, sf_modules, sf_registry};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Accept either `sf run <file>` or `sf <file>`.
    let rest: Vec<String> = match args.split_first() {
        Some((cmd, r)) if cmd == "run" => r.to_vec(),
        _ => args,
    };

    let mut path = None;
    let mut time = 2.0_f64;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--time" => {
                i += 1;
                time = rest.get(i).and_then(|s| s.parse().ok()).unwrap_or(time);
            }
            p => path = Some(p.to_string()),
        }
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("usage: sf run <file.ys> [--time T]");
        std::process::exit(2);
    };

    let prog = parse_file(&path).unwrap_or_else(|e| {
        eprintln!("sf: parse {path}: {e}");
        std::process::exit(1);
    });
    let state = chrysalis::runner::run(&prog, sf_registry(), sf_methods(), sf_modules(), time)
        .unwrap_or_else(|e| {
            eprintln!("sf: run {path}: {e}");
            std::process::exit(1);
        });
    let keys: Vec<String> = state
        .as_map()
        .map(|m| m.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();
    println!("ran {path} (t={time}); final state: {keys:?}");
}
