//! REPL coherence (the "every name has a value, every value composes" theory):
//!   - a bare composite/process/step definer evaluates to its no-arg
//!     instantiation (`all` ≡ `all[]`), not "unbound";
//!   - value field access (`.field`) composes on *any* expression, not just
//!     place-paths — `all[].config.bridge`, `all.config.state.field.what`.
//! Mirrors the recorded REPL session that surfaced both gaps.

use std::sync::Arc;

use chrysalis::ast::Def;
use chrysalis::eval::Evaluator;
use chrysalis::parse::parse_program;
use prism_schema::{MethodRegistry, Value};

const DEFS: &str = "\
process be[is: boolean] ~{field: map[int]} ->{choice: int} ({choice: if is then field['what'] else field['okay']})
composite all ~{} ->{choices: map[int]} (
  world: be[is: true] ~{field: field} ->{choice: choices.world} |
  under: be[is: false] ~{field: field} ->{choice: choices.under} |
  field: {'what': 5, 'okay': 9}
)
";

/// Evaluate `expr_src` against the DEFS program (a trailing bare expression
/// becomes the implicit `main`).
fn eval_expr(expr_src: &str) -> Result<Value, String> {
    let src = format!("{DEFS}{expr_src}\n");
    let prog = parse_program(&src).map_err(|e| e.to_string())?;
    let main = match prog.lookup("main") {
        Some(Def::Binding { value, .. }) => value.clone(),
        _ => panic!("no main"),
    };
    let ev = Evaluator::new(Arc::new(prog), Arc::new(MethodRegistry::new()));
    ev.eval_value(&main, &Default::default())
        .map_err(|e| e.to_string())
}

#[test]
fn bare_composite_definer_is_its_instantiation() {
    // `all` ≡ `all[]`: a value, not "unbound variable".
    let v = eval_expr("all").expect("bare `all` should evaluate");
    assert_eq!(
        v.get_field("address").and_then(|a| a.as_str()),
        Some("local:Composite"),
        "bare `all` is the composite-as-data spec"
    );
}

#[test]
fn field_access_composes_on_a_term_result() {
    // `.config.bridge` on the `all[]` term result — the parser used to reject
    // field access on a non-path expression.
    let bridge = eval_expr("all[].config.bridge").expect("field access on term result");
    let choices = bridge
        .get_field("outputs")
        .and_then(|o| o.get_field("choices"))
        .and_then(|c| c.as_list())
        .map(|l| l.len());
    assert_eq!(
        choices,
        Some(1),
        "bridge.outputs.choices is the wire ['choices']; got {bridge:?}"
    );
}

#[test]
fn bracketless_and_bracketed_navigation_agree() {
    // `all.config.bridge` ≡ `all[].config.bridge` — path-root resolution and
    // var resolution are unified.
    let a = eval_expr("all.config.bridge").expect("bracketless");
    let b = eval_expr("all[].config.bridge").expect("bracketed");
    assert_eq!(
        a, b,
        "with and without `[]` should navigate to the same value"
    );
}

#[test]
fn deep_value_field_access_reaches_inner_state() {
    // Navigate into the composite's inner state: field map → key.
    let v = eval_expr("all.config.state.field.what").expect("deep navigation");
    assert_eq!(
        v.as_i64(),
        Some(5),
        "all.config.state.field.what == 5; got {v:?}"
    );
}

#[test]
fn missing_field_is_none_not_error() {
    // Field access never panics on an absent field — it yields `none`.
    let v = eval_expr("all.config.nope").expect("absent field is none, not error");
    assert_eq!(v, Value::None);
}

#[test]
fn definer_needing_args_reports_the_arg_not_unbound() {
    // `be` needs `is` — the error names the missing arg (and signals it's a
    // definer), rather than the misleading "unbound variable".
    let err = eval_expr("be").expect_err("bare `be` needs `is`");
    assert!(
        err.contains("is"),
        "error should name the missing arg `is`; got: {err}"
    );
    assert!(
        !err.contains("unbound"),
        "should not be an unbound-variable error; got: {err}"
    );
}
