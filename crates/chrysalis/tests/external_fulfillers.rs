//! COPASI/Tellurium as rest-addressed contracted fulfillers. `lib/external.ys`
//! declares each as a composite with `fulfills DeterministicMassAction` + a
//! `rest<>` alias, and `fulfillers(DeterministicMassAction)` returns them
//! ALONGSIDE the native integrators — indistinguishable to the query, the point
//! of the contract index. The ALIAS (not the bare local composite) is returned,
//! because it carries the transport address (so it runs in COPASI/Tellurium).

use chrysalis::ast::ContractRef;
use chrysalis::contract::fulfillers;
use chrysalis::parse::parse_file;

fn ys_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ys").join(name)
}

#[test]
fn external_engines_join_the_deterministic_fulfillers() {
    // parse_file resolves `import Sim from 'simulators.ys'`, so the native
    // fulfillers + contracts are merged in alongside external.ys's own defs.
    let program = parse_file(&ys_path("lib/external.ys")).expect("parse_file lib/external.ys");

    let mut got = fulfillers(&program, &ContractRef::new("DeterministicMassAction"));
    got.sort();
    assert_eq!(
        got,
        vec!["CopasiCvode", "ForwardEuler", "Rk4", "RoadRunnerCvode"],
        "native (Rk4/ForwardEuler) + rest-addressed COPASI/Tellurium all fulfill \
         DeterministicMassAction, queried uniformly; the rest ALIASES are returned \
         (they carry the address), not the bare CopasiModel/RoadRunnerModel composites"
    );
}
