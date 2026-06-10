//! **Canonical run-Core** (`docs/canonical-run-core.md`) — the #59 Core-threading
//! rule reaching its last seam, the chrysalis `run()`/`compile` boundary. The
//! 3-door `run(program, registry, methods, modules)` splits the one runtime apart
//! and HARD-CODES protocols; `run_with_core(program, domain_core, modules)` threads
//! ONE `Core` (procs + types + methods + protocols) and opens the **protocol door**
//! (a domain injects its own — what `synth` A6 / A8 `net:` need).
//!
//! Proven here: (1) the canonical is a *faithful additive sibling* (same result as
//! the 3-door run); (2) the protocol door carries the DOMAIN's protocols, not the
//! hard-coded `stream_protocols()`; (3) a domain's own native process reaches a
//! `.ys` program through the one Core (the synth pattern).

use std::any::Any;
use std::sync::Arc;

use chrysalis::compile::{compile_with_core, compile_with_modules};
use chrysalis::parse::parse_program;
use chrysalis::prelude::{std_core, std_methods, std_modules, std_registry};
use chrysalis::runner::{run, run_with_core};
use indexmap::IndexMap;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::{Core, ProcessRegistry, Schema, Update, Value};

const TICK: &str = "\
process Tick ~{n :: Float} ->{n :: Float} (
  {n: 1.0}
)
composite Main ->{ n :: Float } (
  n: 0.0 |
  tick: Tick ~{n: n} ->{n: n}
)
";

#[test]
fn run_with_core_is_a_faithful_sibling_of_the_three_door_run() {
    // The canonical run-Core and the 3-door run reach the SAME final state — so
    // collapsing the 3 doors into one Core changes nothing for existing callers
    // (the additive-sibling, keep-it-green property). `std_core()` carries the
    // same procs/types/methods/protocols the three separate args do.
    let prog = parse_program(TICK).expect("parse");
    let via_three_door =
        run(&prog, std_registry(), std_methods(), std_modules(), 5.0).expect("3-door run");
    let via_core = run_with_core(&prog, std_core(), std_modules(), 5.0).expect("canonical run");
    assert_eq!(
        via_three_door, via_core,
        "run_with_core(std_core()) == run(std_registry(), std_methods(), …): faithful sibling"
    );
    // Sanity: it actually ran (Tick accumulated to 5.0).
    assert_eq!(via_core.get_field("n").and_then(|v| v.as_f64()), Some(5.0));
}

#[test]
fn the_protocol_door_carries_the_domains_protocols_not_a_hard_coded_default() {
    // A domain Core with NON-default protocols (here `Core::new()` = `local` only,
    // no stream/rest/parallel) flows ITS protocols through compile — retiring the
    // hard-coded `stream_protocols()` of the 3-door path. This is the door A8
    // `net:` needs (inject a `net` protocol) and synth needs (audio protocols).
    let prog =
        parse_program("composite Main ->{ x :: Float } (\n  x: 1.0\n)\nMain[]\n").expect("parse");
    let domain = Core::new(); // local protocol only

    let via_core = compile_with_core(&prog, domain.clone(), std_modules()).expect("compile_with_core");
    let mut got: Vec<String> = via_core.core.protocols.names().iter().map(|s| s.to_string()).collect();
    got.sort();
    let mut want: Vec<String> = domain.protocols.names().iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(
        got, want,
        "the compiled Core carries the DOMAIN's protocols (the door), not a hard-coded set"
    );

    // And it genuinely differs from the 3-door default (which hard-codes the
    // stream set) — proving the choice is real, not incidental.
    let three_door =
        compile_with_modules(&prog, std_registry(), std_methods(), std_modules()).expect("3-door");
    assert!(
        three_door.core.protocols.names().len() > via_core.core.protocols.names().len(),
        "3-door hard-codes the larger stream set ({:?}); the door let the domain pick fewer ({:?})",
        three_door.core.protocols.names(),
        via_core.core.protocols.names(),
    );
}

/// A minimal domain-native process: emits +1.0 to its `n` output each tick.
#[derive(Debug)]
struct Pulse;
impl Process for Pulse {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("n".to_string(), Schema::float())])
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([("n".to_string(), Schema::float())])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn update(&self, _state: &Value, _interval: f64) -> Update {
        Update::value(Value::tree([("n", Value::float(1.0))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn a_domains_native_process_reaches_a_ys_program_through_the_one_core() {
    // The synth pattern: a DOMAIN exposes its native modules in ONE Core
    // (`Pulse` here ≈ prism-audio's `Oscillator`), and a `.ys` program reaches it
    // through `run_with_core`. The import NAME surface is `modules`
    // (`from pulses import Pulse`); the FACTORY rides the Core. Both thread to the
    // program — no per-domain prelude workaround, no registry-subset door.
    let mut registry = ProcessRegistry::new();
    registry.register("Pulse", |_config| ProcessNode::Process(Box::new(Pulse)));
    let domain = Core::new().with_processes(Arc::new(registry));
    let modules = chrysalis::compile::ModuleRegistry::new().process("pulses", "Pulse");

    let prog = parse_program(
        "from pulses import Pulse\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  beat: Pulse ~{n: n} ->{n: n}\n)\n",
    )
    .expect("parse");

    let out = run_with_core(&prog, domain, modules, 4.0).expect("run a domain process via one Core");
    assert_eq!(
        out.get_field("n").and_then(|v| v.as_f64()),
        Some(4.0),
        "the domain's native Pulse ran 4 ticks through the one Core: {out:?}"
    );
}
