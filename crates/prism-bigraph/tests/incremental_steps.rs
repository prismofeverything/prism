//! Incremental step execution (#10 / "step triggers in general"): a workflow's
//! steps are SKIPPED when their output is cached and fresh, and selectively re-run
//! via the forced set, with staleness cascading down the step DAG. Executable
//! axiom for the [`StepCache`] mechanism that chrysalis surfaces as
//! `chrysalis run f.ys --steps a,b`.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use indexmap::IndexMap;

use prism_bigraph::process::{ProcessNode, Step};
use prism_bigraph::topology::{ProcessSpec, Topology};
use prism_bigraph::{Engine, Key, Schema, StepCache, Update, Value};

fn overwrite_float() -> Schema {
    Schema::Overwrite { inner: Box::new(Schema::float()) }
}

/// A step that records each firing in a shared counter, reads its (optional) input
/// port, and emits `input + add` on its output port.
#[derive(Debug)]
struct Recording {
    in_port: Option<&'static str>,
    out_port: &'static str,
    add: f64,
    fired: Arc<AtomicUsize>,
}

impl Step for Recording {
    fn inputs(&self) -> IndexMap<String, Schema> {
        match self.in_port {
            Some(p) => IndexMap::from([(p.to_string(), Schema::float())]),
            None => IndexMap::new(),
        }
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([(self.out_port.to_string(), overwrite_float())])
    }
    fn update(&self, state: &Value) -> Update {
        self.fired.fetch_add(1, Ordering::SeqCst);
        let input = self
            .in_port
            .and_then(|p| state.get_field(p))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Update::value(Value::tree([(self.out_port, Value::float(input + self.add))]))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn spec(inputs: &[(&str, &str)], outputs: &[(&str, &str)]) -> ProcessSpec {
    ProcessSpec {
        process_type: "Recording".to_string(),
        config: Value::None,
        inputs: inputs.iter().map(|(p, s)| (p.to_string(), vec![Key::from(*s)])).collect(),
        outputs: outputs.iter().map(|(p, s)| (p.to_string(), vec![Key::from(*s)])).collect(),
        interval: None,
        priority: 0.0,
    }
}

/// The chain a → b → c over state slots va/vb/vc: va=1, vb=va+10=11, vc=vb+100=111.
fn chain(
    a: &Arc<AtomicUsize>,
    b: &Arc<AtomicUsize>,
    c: &Arc<AtomicUsize>,
) -> (Topology, HashMap<String, ProcessNode>) {
    let mut topo = Topology::new();
    topo.initial_state = Value::tree([
        ("va", Value::float(0.0)),
        ("vb", Value::float(0.0)),
        ("vc", Value::float(0.0)),
    ]);
    topo.state_schema = Schema::tree([
        ("va", overwrite_float()),
        ("vb", overwrite_float()),
        ("vc", overwrite_float()),
    ]);
    topo.processes.insert("a".into(), spec(&[], &[("out", "va")]));
    topo.processes.insert("b".into(), spec(&[("in", "va")], &[("out", "vb")]));
    topo.processes.insert("c".into(), spec(&[("in", "vb")], &[("out", "vc")]));

    let step = |in_port, out_port, add, fired: &Arc<AtomicUsize>| {
        ProcessNode::Step(Box::new(Recording { in_port, out_port, add, fired: Arc::clone(fired) }))
    };
    let mut instances: HashMap<String, ProcessNode> = HashMap::new();
    instances.insert("a".into(), step(None, "out", 1.0, a));
    instances.insert("b".into(), step(Some("in"), "out", 10.0, b));
    instances.insert("c".into(), step(Some("in"), "out", 100.0, c));
    (topo, instances)
}

fn cache_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("prism-stepcache-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn assert_chain_state(engine: &Engine) {
    let f = |k: &str| engine.state().get_field(k).and_then(|v| v.as_f64());
    assert_eq!(
        (f("va"), f("vb"), f("vc")),
        (Some(1.0), Some(11.0), Some(111.0)),
        "chain settles to va=1, vb=11, vc=111"
    );
}

/// The whole incremental contract in one DAG: first build runs everything; a
/// second build with a fresh cache skips everything (reloading exact outputs);
/// forcing a middle step re-runs it and cascades to its dependent, leaving its
/// upstream cached.
#[test]
fn rerun_skips_and_force_cascades_downstream() {
    let dir = cache_dir("chain");
    let (a, b, c) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let fires = |a: &Arc<AtomicUsize>, b: &Arc<AtomicUsize>, c: &Arc<AtomicUsize>| {
        (a.load(Ordering::SeqCst), b.load(Ordering::SeqCst), c.load(Ordering::SeqCst))
    };

    // Run 1: empty cache ⇒ all three fire and persist.
    {
        let (topo, inst) = chain(&a, &b, &c);
        let engine = Engine::new_with_cache(topo, inst, Some(StepCache::new(&dir, None, HashSet::new())));
        assert_chain_state(&engine);
    }
    assert_eq!(fires(&a, &b, &c), (1, 1, 1), "run 1: all steps fire");

    // Run 2: cache fresh, nothing forced ⇒ all SKIP (counters unchanged), yet the
    // state is correct ⇒ cached outputs were reloaded (exact serde round-trip).
    {
        let (topo, inst) = chain(&a, &b, &c);
        let engine = Engine::new_with_cache(topo, inst, Some(StepCache::new(&dir, None, HashSet::new())));
        assert_chain_state(&engine);
    }
    assert_eq!(fires(&a, &b, &c), (1, 1, 1), "run 2: every step skips (reloaded from cache)");

    // Run 3: force `b` ⇒ b recomputes, c (downstream) cascades, a stays cached.
    {
        let (topo, inst) = chain(&a, &b, &c);
        let forced = HashSet::from(["b".to_string()]);
        let engine = Engine::new_with_cache(topo, inst, Some(StepCache::new(&dir, None, forced)));
        assert_chain_state(&engine);
    }
    assert_eq!(
        fires(&a, &b, &c),
        (1, 2, 2),
        "run 3: forcing b re-runs b and its dependent c; a stays cached"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// With no `StepCache` attached, behaviour is identical to before the cache existed
/// (the `Option`-gated default): every step fires on every build.
#[test]
fn no_cache_fires_every_step_every_build() {
    let (a, b, c) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    for _ in 0..2 {
        let (topo, inst) = chain(&a, &b, &c);
        let engine = Engine::new(topo, inst);
        assert_chain_state(&engine);
    }
    assert_eq!(
        (a.load(Ordering::SeqCst), b.load(Ordering::SeqCst), c.load(Ordering::SeqCst)),
        (2, 2, 2),
        "no cache ⇒ every step fires on every build"
    );
}
