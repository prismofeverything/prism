//! Regression — `load(path)` and `load(path).run(t)` must resolve the LOADED
//! file's OWN relative `.ys` imports against ITS directory, the same way the
//! bin's `run` does (via `parse_file`).
//!
//! The bug: `load_program_as_document` + `run_from_source` (prelude.rs) parsed
//! with the path-blind `parse_program`, so a loaded program's `from .sibling
//! import …` was left unresolved and the compile rejected it as an unknown
//! native module (`unknown import `.mesh``). A program runs fine *directly*
//! (`chrysalis run board.ys`) but breaks the moment another program `load`s it.
//!
//! This is exactly what blocked the **metacircular orchestrator**
//! (`coord/orchestrator.ys` doing `load('board.ys').run(1.0)`, where `board.ys`
//! is `from .mesh import …` over the agent heartbeats). The orchestrator shape
//! in miniature: a leaf, a "board" importing it, an "orchestrator" load+run-ing
//! the board — and the loaded value flows all the way through.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrysalis::parse::parse_file;
use chrysalis::prelude::{std_core, std_modules_at};
use chrysalis::runner::invoke;

#[test]
fn load_then_run_resolves_a_loaded_files_sibling_imports() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("load-sibling-imports");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");

    // leaf.ys exports a value; board.ys imports it via a RELATIVE sibling import
    // and exposes it; orch.ys load()s + run()s the board from inside `.ys`.
    std::fs::write(dir.join("leaf.ys"), "def leaf = 7.0\n").expect("write leaf");
    std::fs::write(
        dir.join("board.ys"),
        "from .leaf import leaf\ncomposite Board ->{ out :: Float } (\n  out: leaf\n)\nBoard[]\n",
    )
    .expect("write board");
    std::fs::write(
        dir.join("orch.ys"),
        "from io import load\ncomposite Orch ~{} ->{ r :: Float } (\n  r: load('board.ys').run(1.0).out\n)\nOrch[]\n",
    )
    .expect("write orch");

    // `invoke` serializes the entry's OUTPUT ports (values, not the bin's
    // keys-only display) and runs through both fixed paths: `load(board)` (its
    // `.leaf` import) and the subsequent `.run(1.0)` (`run_from_source`). The
    // module ys_root is `dir`, so `load('board.ys')` resolves to `dir/board.ys`.
    let prog = parse_file(dir.join("orch.ys")).expect("parse orch");
    let out = invoke(
        &prog,
        std_core(),
        std_modules_at(Some(dir.clone())),
        &BTreeMap::new(),
        0.0,
    )
    .expect("invoke orch: load()+run() of a sibling-importing board must resolve, not error");

    assert_eq!(
        out.get_field("r").and_then(|v| v.as_f64()),
        Some(7.0),
        "the loaded board's `out` (7.0, via its `.leaf` sibling import) must flow through \
         load → run → field access: {out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
