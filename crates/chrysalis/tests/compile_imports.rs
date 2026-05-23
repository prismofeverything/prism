//! Native host imports resolve through the `ModuleRegistry` (`from <module>
//! import …`): object imports bind values, type imports become first-class
//! types, and an undeclared import is a compile error (not a silent opaque
//! fallback). This pins the `extern` replacement at the compile boundary.

use chrysalis::compile::{compile_with_modules, ModuleRegistry};
use chrysalis::parse::parse_program;
use prism_bigraph::ProcessRegistry;
use prism_schema::{MethodRegistry, Value};

const SRC: &str = r#"
from integrators import rk4
from chem import CRN

process Step1[network: CRN] ~{state: map[float]} ->{state: map[float]} (
  rk4.integrate(network, state, interval)
)

composite W ->{state: map[float]} (
  init: {A: 1.0} |
  s: Step1[network: {species: ['A'], reactions: []}] ~{state: init} ->{state: state}
)

W[]
"#;

fn rk4_object() -> Value {
    Value::tree([
        ("_type", Value::from("Integrator")),
        ("method", Value::from("rk4")),
    ])
}

const CRN_REPR: &str =
    "{species: list[string], reactions: list[{reactants: map[float], products: map[float], k: float}]}";

#[test]
fn resolves_object_and_type_imports() {
    let prog = parse_program(SRC).expect("parse");
    let modules = ModuleRegistry::new()
        .object("integrators", "rk4", rk4_object())
        .type_("chem", "CRN", CRN_REPR);
    let result =
        compile_with_modules(&prog, ProcessRegistry::new(), MethodRegistry::new(), modules);
    assert!(
        result.is_ok(),
        "should compile with rk4 (object) + CRN (type) declared: {:?}",
        result.err()
    );
}

#[test]
fn undeclared_import_is_a_compile_error() {
    let prog = parse_program(SRC).expect("parse");
    // Omit the `chem::CRN` declaration: `from chem import CRN` is now unresolved.
    let modules = ModuleRegistry::new().object("integrators", "rk4", rk4_object());
    let err = compile_with_modules(&prog, ProcessRegistry::new(), MethodRegistry::new(), modules)
        .err()
        .expect("compile should fail when an imported name isn't declared");
    assert!(
        format!("{err}").contains("CRN"),
        "error should name the unresolved import `CRN`, got: {err}"
    );
}
