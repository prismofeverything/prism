//! `from <module> import <names>` — native host imports (the `extern`
//! replacement). Parses to `Def::Use { module, names }`, distinct from the
//! file-import `import Name from "path.ys"` (`Def::Import`).

use chrysalis::ast::Def;
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;

fn uses(src: &str) -> Vec<(String, Vec<String>)> {
    parse_program(src)
        .expect("parse")
        .defs
        .into_iter()
        .filter_map(|d| match d {
            Def::Use { module, names } => Some((module, names)),
            _ => None,
        })
        .collect()
}

#[test]
fn parses_native_host_imports() {
    let src = "\
from core import RunProcess
from integrators import rk4, euler
from plot import overlay
";
    assert_eq!(
        uses(src),
        vec![
            ("core".into(), vec!["RunProcess".into()]),
            ("integrators".into(), vec!["rk4".into(), "euler".into()]),
            ("plot".into(), vec!["overlay".into()]),
        ]
    );
}

#[test]
fn from_is_still_a_usable_identifier() {
    // `from` stays a contextual keyword: `def from = …` is a normal binding, not
    // an import (guarded by peek2 != `import`).
    let prog = parse_program("def from = 3.0\n").expect("parse");
    assert!(
        prog.defs.iter().any(|d| matches!(d, Def::Binding { name, .. } if name == "from")),
        "`def from = 3.0` should parse as a binding named `from`, got {:?}",
        prog.defs
    );
}

#[test]
fn fulfills_parses_before_or_after_the_interface() {
    let before = "process P[x: any] fulfills C[method: M] ~{a: float} ->{b: float} ( b )";
    let after = "process P[x: any] ~{a: float} ->{b: float} fulfills C[method: M] ( b )";
    for src in [before, after] {
        let prog = parse_program(src).expect("parse");
        let p = prog
            .defs
            .iter()
            .find_map(|d| match d {
                Def::Process(p) => Some(p),
                _ => None,
            })
            .expect("a process def");
        let out = p.interface.outputs.get("b").expect("output port b");
        let contract = out
            .contract
            .as_ref()
            .unwrap_or_else(|| panic!("output `b` should carry the fulfills contract ({src})"));
        assert_eq!(contract.name, "C", "contract name ({src})");
    }
}

#[test]
fn use_import_round_trips_through_unparse() {
    let src = "from integrators import rk4, euler\n";
    let prog = parse_program(src).expect("parse");
    let out = unparse(&prog);
    assert!(
        out.contains("from integrators import rk4, euler"),
        "unparse should emit the use-import, got: {out}"
    );
}

#[test]
fn parses_dotted_hyphenated_module_paths() {
    // `.ys`-file imports (#25): a package name may contain `-` (cargo names) and
    // submodules are dotted, so `from spatio-flux.composites import comets` must
    // parse as ONE module string (the lexer splits `-`/`.`; the parser reassembles).
    let src = "from spatio-flux.composites.comets import Comet\n";
    assert_eq!(uses(src), vec![("spatio-flux.composites.comets".into(), vec!["Comet".into()])]);
    // …and round-trips through unparse.
    let prog = parse_program(src).expect("parse");
    assert!(
        unparse(&prog).contains("from spatio-flux.composites.comets import Comet"),
        "dotted/hyphenated module path should round-trip"
    );
}
