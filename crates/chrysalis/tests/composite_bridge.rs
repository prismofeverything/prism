//! `port :: Type @ internal.path` — explicit composite-bridge syntax.
//!
//! The `@` operator maps an interface port to an inner-state path; absent, the
//! bridge is name-inferred (the same-named top-level field), the
//! backward-compatible default. `%` (not `@`) is now the self/here place-path.
//! See `eval::build_composite_outer` and docs/chrysalis-design.md (decision #23).

use std::sync::Arc;

use chrysalis::ast::{Def, Expr, Interface};
use chrysalis::eval::Evaluator;
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;
use prism_schema::MethodRegistry;

const SRC: &str = "\
composite Group ~{values :: Map[Float] @ fields.values} ->{total :: Float @ stats.total} (
  fields: { values: {} } |
  stats: { total: 0.0 }
)
";

fn group_iface(src: &str) -> Interface {
    let prog = parse_program(src).expect("parse");
    for d in &prog.defs {
        if let Def::Composite(c) = d {
            return c.interface.clone();
        }
    }
    panic!("no composite in program");
}

#[test]
fn explicit_bridge_paths_parse() {
    let iface = group_iface(SRC);
    assert_eq!(
        iface.inputs["values"].bridge,
        Some(vec!["fields".to_string(), "values".to_string()]),
        "input `values` should bridge to fields.values"
    );
    assert_eq!(
        iface.outputs["total"].bridge,
        Some(vec!["stats".to_string(), "total".to_string()]),
        "output `total` should bridge to stats.total"
    );
}

#[test]
fn absent_bridge_is_name_inferred() {
    // No `@` ⇒ bridge None; `build_composite_outer` falls back to `[port]`.
    let iface = group_iface("composite G ~{x :: Float} ->{y :: Float} ( x: 0.0 | y: 0.0 )\n");
    assert_eq!(iface.inputs["x"].bridge, None);
    assert_eq!(iface.outputs["y"].bridge, None);
}

#[test]
fn bridge_roundtrips_through_unparse() {
    let prog = parse_program(SRC).expect("parse");
    let text = unparse(&prog);
    // (Schema names canonicalize to lowercase primitives: `Float` → `float`.)
    assert!(
        text.contains("values :: Map[float] @ fields.values"),
        "unparse should emit the input bridge; got:\n{text}"
    );
    assert!(
        text.contains("total :: float @ stats.total"),
        "unparse should emit the output bridge; got:\n{text}"
    );
    // Fixpoint: re-parsing the unparse preserves the bridge paths.
    let reparsed = parse_program(&text).expect("reparse");
    let iface = reparsed
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Composite(c) => Some(c.interface.clone()),
            _ => None,
        })
        .expect("composite");
    assert_eq!(
        iface.inputs["values"].bridge,
        Some(vec!["fields".to_string(), "values".to_string()])
    );
    assert_eq!(
        iface.outputs["total"].bridge,
        Some(vec!["stats".to_string(), "total".to_string()])
    );
}

/// The eval consumer: a composite call compiles to a subengine spec whose
/// `config.bridge` carries the *explicit* inner path, not the port name. This
/// is the branch name-inference (grow_divide / nuclear-shuttle) never reaches.
#[test]
fn explicit_bridge_lands_in_compiled_config() {
    let prog = parse_program(SRC).expect("parse");
    let ev = Evaluator::new(Arc::new(prog), Arc::new(MethodRegistry::new()));
    // A bare call to the composite → its `Composite` subengine spec value.
    let spec = ev
        .eval_value(&Expr::term("Group").build(), &Default::default())
        .expect("eval composite call");

    // Dig spec.config.bridge.{inputs,outputs} and read each wire's segments.
    let wire = |kind: &str, port: &str| -> Vec<String> {
        spec.as_map()
            .and_then(|m| m.get("config"))
            .and_then(|v| v.as_map())
            .and_then(|m| m.get("bridge"))
            .and_then(|v| v.as_map())
            .and_then(|m| m.get(kind))
            .and_then(|v| v.as_map())
            .and_then(|m| m.get(port))
            .and_then(|v| v.as_list())
            .expect("bridge wire")
            .iter()
            .map(|v| v.as_str().expect("segment").to_string())
            .collect()
    };
    assert_eq!(wire("inputs", "values"), vec!["fields", "values"]);
    assert_eq!(wire("outputs", "total"), vec!["stats", "total"]);
}
