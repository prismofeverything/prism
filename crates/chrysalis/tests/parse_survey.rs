//! Survey: which `.ys` files in `ys/` parse with the current parser?
//! Prints a pass/fail line per file (run with `--nocapture`). Not an
//! assertion of completeness — a gap report.

use std::path::PathBuf;

#[test]
fn survey_ys_files() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ys"))
        .collect();
    files.sort();
    println!("\n=== .ys parse survey ===");
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match chrysalis::parse::parse_file(&path) {
            Ok(p) => println!("  OK    {name}  ({} defs)", p.defs.len()),
            Err(e) => println!("  FAIL  {name}  — {}", e),
        }
    }
}
