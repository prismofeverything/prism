//! Unparser: generate `.ys` from AST fixtures + the round-trip property.
//!
//! `generate_and_print` shows the generated surface (run with --nocapture).
//! `roundtrip_*` assert the text fixpoint `unparse(parse(unparse(p))) ==
//! unparse(p)` — the parser reproduces what the unparser emits.

use chrysalis::fixtures;
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;

/// Generate `ys/<name>.ys` for the fixtures that have no hand-written surface
/// file — M/R and MAPK — straight from their AST (the "inverse compile").
#[test]
fn generate_mapk_and_mr_ys() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys");
    for (name, prog) in [
        ("mr", fixtures::mr::program()),
        ("mapk", fixtures::mapk::program()),
    ] {
        let header = format!(
            "# GENERATED from `fixtures::{name}::program()` by the unparser\n\
             # (chrysalis::unparse). Round-trip-verified: parse(this) re-unparses\n\
             # identically. Edit the fixture, not this file.\n\n"
        );
        let text = format!("{header}{}", unparse(&prog));
        std::fs::write(dir.join(format!("{name}.ys")), &text).unwrap();
        println!("wrote {name}.ys ({} bytes)", text.len());
    }
}

/// Text fixpoint: parsing the unparser's output and re-unparsing reproduces it.
fn assert_roundtrip(name: &str, prog: &chrysalis::ast::Program) {
    let text1 = unparse(prog);
    let parsed = match parse_program(&text1) {
        Ok(p) => p,
        Err(e) => panic!("{name}: re-parse of unparsed text failed: {e}\n--- text ---\n{text1}"),
    };
    let text2 = unparse(&parsed);
    assert_eq!(text1, text2, "{name}: not a round-trip fixpoint");
}

#[test]
fn roundtrip_grow_divide() {
    assert_roundtrip("grow_divide", &fixtures::grow_divide::program());
}

#[test]
fn roundtrip_graph() {
    assert_roundtrip("graph", &fixtures::graph::program());
}

#[test]
fn roundtrip_nuclear_shuttle() {
    assert_roundtrip("nuclear_shuttle", &fixtures::nuclear_shuttle::program());
}

#[test]
fn roundtrip_mr() {
    assert_roundtrip("mr", &fixtures::mr::program());
}

#[test]
fn roundtrip_mapk() {
    assert_roundtrip("mapk", &fixtures::mapk::program());
}
