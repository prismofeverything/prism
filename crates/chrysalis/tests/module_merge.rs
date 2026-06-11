//! `ModuleRegistry::merge` (#67 Phase 5) — two import surfaces COMPOSE. The mixed-case
//! need it unblocks: a package's own native `modules()` ⊔ `std_modules()` ⊔ a
//! dependency's surface, so a `.ys` program can import `from <ownmod> import X` AND
//! `from <depmod> import Y` against one resolved surface.

use std::any::Any;
use std::sync::Arc;

use chrysalis::compile::ModuleRegistry;
use chrysalis::parse::parse_program;
use chrysalis::runner::run;
use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::update::Update;
use prism_bigraph::Core;
use prism_schema::{Schema, Value};

/// A native process: emits +1.0 to its `n` output each tick.
#[derive(Debug)]
struct Tick;
impl Process for Tick {
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
fn two_module_surfaces_compose_via_merge() {
    // A Core with two native processes (reached under DIFFERENT module names).
    let mut reg = ProcessRegistry::new();
    reg.register("Alpha", |_| ProcessNode::Process(Box::new(Tick)));
    reg.register("Beta", |_| ProcessNode::Process(Box::new(Tick)));
    let core = Core::new().with_processes(Arc::new(reg));

    // Two SEPARATE import surfaces (≈ an own native crate's `modules()` and a
    // dependency's), merged into one.
    let own = ModuleRegistry::new().process("mod_a", "Alpha");
    let dep = ModuleRegistry::new().process("mod_b", "Beta");
    let merged = own.merge(dep);
    assert!(
        merged.has_module("mod_a") && merged.has_module("mod_b"),
        "both surfaces' modules survive the merge"
    );

    // A program importing from BOTH merged modules resolves + runs — each +1/tick, so
    // n = 2/tick = 10.0 over 5 ticks. The merge composed the two import surfaces.
    let prog = parse_program(
        "from mod_a import Alpha\nfrom mod_b import Beta\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  a: Alpha ~{n: n} ->{n: n} |\n  b: Beta ~{n: n} ->{n: n}\n)\nMain[]\n",
    )
    .expect("parse");
    let state = run(&prog, core, merged, 5.0).expect("run against the merged surface");
    assert_eq!(
        state.get_field("n").and_then(|v| v.as_f64()),
        Some(10.0),
        "both imported processes ran through the merged module surface: {state:?}"
    );
}

#[test]
fn merge_unions_exports_within_a_shared_module() {
    // When both surfaces declare the SAME module, their exports UNION (not a
    // module-level overwrite) — `core`-style modules from std + a domain compose.
    let mut reg = ProcessRegistry::new();
    reg.register("Alpha", |_| ProcessNode::Process(Box::new(Tick)));
    reg.register("Beta", |_| ProcessNode::Process(Box::new(Tick)));
    let core = Core::new().with_processes(Arc::new(reg));

    let a = ModuleRegistry::new().process("shared", "Alpha");
    let b = ModuleRegistry::new().process("shared", "Beta");
    let merged = a.merge(b);

    // Both `Alpha` and `Beta` are importable from the SAME `shared` module post-merge.
    let prog = parse_program(
        "from shared import Alpha\nfrom shared import Beta\ncomposite Main ->{ n :: Float } (\n  n: 0.0 |\n  a: Alpha ~{n: n} ->{n: n} |\n  b: Beta ~{n: n} ->{n: n}\n)\nMain[]\n",
    )
    .expect("parse");
    let state = run(&prog, core, merged, 3.0).expect("run");
    assert_eq!(
        state.get_field("n").and_then(|v| v.as_f64()),
        Some(6.0),
        "exports unioned within the shared module: {state:?}"
    );
}
