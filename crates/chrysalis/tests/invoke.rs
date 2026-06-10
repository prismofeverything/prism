//! Compositional invocation (decision #24): `invoke` runs a file's entry
//! composite with the command line bound to its interface — `[config]` params +
//! `~{inputs}` filled from args (decoded via the schema codec), `->{outputs}`
//! serialized back into a record. The codec is `realize`/`serialize`, so a run's
//! output round-trips as another run's input.

use std::collections::BTreeMap;

use chrysalis::prelude::{std_core, std_modules};
use chrysalis::runner::invoke;
use prism_schema::Value;

/// Entry composite: result = value * factor, via a bridged inner field.
const SRC: &str = "\
composite Scale[factor :: Float = 1.0] ~{value :: Float @ amount} ->{result :: Float @ amount} (
  amount: value * factor
)
";

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn run_args(src: &str, pairs: &[(&str, &str)]) -> Result<Value, String> {
    let prog = chrysalis::parse::parse_program(src).expect("parse");
    invoke(
        &prog,
        std_core(),
        std_modules(),
        &args(pairs),
        1.0,
    )
    .map_err(|e| e.to_string())
}

#[test]
fn binds_config_and_input_renders_output() {
    // Literal sources for both the config param and the input port.
    let out = run_args(SRC, &[("value", "10.0"), ("factor", "3.0")]).expect("invoke");
    let result = out
        .as_map()
        .and_then(|m| m.get("result"))
        .and_then(|v| v.as_f64());
    assert_eq!(result, Some(30.0), "result = value*factor; got {out:?}");
}

#[test]
fn config_falls_back_to_default() {
    // `factor` omitted ⇒ its default 1.0 is used.
    let out = run_args(SRC, &[("value", "7.5")]).expect("invoke");
    let result = out
        .as_map()
        .and_then(|m| m.get("result"))
        .and_then(|v| v.as_f64());
    assert_eq!(result, Some(7.5), "factor defaults to 1.0; got {out:?}");
}

#[test]
fn missing_required_input_errors() {
    // `value` has no default and isn't supplied ⇒ a clear error, not a panic.
    let err = run_args(SRC, &[("factor", "2.0")]).expect_err("should error");
    assert!(
        err.contains("value"),
        "error should name the missing input; got: {err}"
    );
}

#[test]
fn output_round_trips_as_input() {
    // The serialized output of one run feeds the next run's input unchanged.
    let out1 = run_args(SRC, &[("value", "4.0"), ("factor", "2.0")]).expect("run 1");
    let r1 = out1
        .as_map()
        .and_then(|m| m.get("result"))
        .and_then(|v| v.as_f64())
        .unwrap();
    // Feed r1 back as `value` (factor 1.0) ⇒ identity, so result == r1.
    let out2 = run_args(SRC, &[("value", &r1.to_string())]).expect("run 2");
    let r2 = out2
        .as_map()
        .and_then(|m| m.get("result"))
        .and_then(|v| v.as_f64())
        .unwrap();
    assert_eq!(r1, 8.0);
    assert_eq!(r2, r1, "serialized output should round-trip as input");
}
