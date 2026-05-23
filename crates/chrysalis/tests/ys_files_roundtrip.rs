//! Every shipped `.ys` example parses under the current grammar and is a
//! round-trip fixpoint of the unparser (`unparse(parse(unparse(x))) ==
//! unparse(x)`). Guards against a language change silently breaking an example.

use std::fs;
use std::path::PathBuf;

use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;

#[test]
fn all_ys_examples_parse_and_roundtrip() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys");
    let mut checked = Vec::new();
    for entry in fs::read_dir(&dir).expect("ys dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("ys") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        // `*-update.ys` are WIP scratchpads, intentionally not kept valid.
        if name.contains("-update") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let prog = parse_program(&src).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"));
        let text1 = unparse(&prog);
        let reparsed = parse_program(&text1)
            .unwrap_or_else(|e| panic!("{name}: re-parse of unparse failed: {e}\n--- text ---\n{text1}"));
        let text2 = unparse(&reparsed);
        assert_eq!(text1, text2, "{name}: not a round-trip fixpoint");
        checked.push(name);
    }
    checked.sort();
    assert!(
        checked.len() >= 8,
        "expected to check the example .ys files, only did {}: {checked:?}",
        checked.len()
    );
}
