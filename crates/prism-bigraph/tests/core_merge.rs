//! `Core::merge` — the package **linker** as an idempotent join-semilattice (#67,
//! the `pkg` agent's foundation). A package is a `Core` (types · processes ·
//! methods · protocols); *depending on* a package is **merging its Core**. For the
//! colimit over a dependency DAG to be well-defined — so a diamond dependency
//! merges its shared apex ONCE — `merge` must be commutative + associative +
//! idempotent (the CALM property `mesh_safety` already gates for the mesh; here it
//! governs theory COMPOSITION instead of replica convergence).
//!
//! These are the executable laws. The key insight under test: the join's identity
//! per registry is what makes it idempotent — TYPES reconcile structurally (the
//! builtins every registry shares are equal, so they never false-conflict),
//! PROCESSES/METHODS by `Arc` identity (a shared dep is pointer-identical),
//! PROTOCOLS by name. A genuine same-name *incompatible* definition is a conflict.

use std::any::Any;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{ProcessNode, Step};
use prism_bigraph::update::Update;
use prism_bigraph::Core;
use prism_schema::{MethodRegistry, Schema, TypeRegistry, Value};

// ── A trivial node so a factory closure has something to return. `merge` never
//    invokes factories (it joins the `Arc`s), so this is never run. ──
#[derive(Debug)]
struct NoopStep;
impl Step for NoopStep {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }
    fn update(&self, _state: &Value) -> Update {
        Update::value(Value::None)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
fn noop() -> ProcessNode {
    ProcessNode::Step(Box::new(NoopStep))
}

/// A package-like core: one custom type, one process class, one method — each name
/// distinct, so independently-built cores are disjoint (no conflicts) yet each
/// `register` mints a FRESH `Arc` (the property the conflict tests exercise).
fn pkg_core(type_name: &str, proc_name: &str, method: (&str, &str)) -> Core {
    let mut types = TypeRegistry::new();
    types.register(type_name, Schema::float(), Some(Value::float(0.0)));
    let mut procs = ProcessRegistry::new();
    procs.register(proc_name, |_| noop());
    let mut methods = MethodRegistry::new();
    let (mty, mname) = method;
    methods.register(mty, mname, |r, _| Ok(r.clone()));
    Core::new()
        .with_types(Arc::new(types))
        .with_processes(Arc::new(procs))
        .with_methods(Arc::new(methods))
}

/// The four key-sets (sorted), the lattice carrier the laws are stated over: the
/// join is a key-union, so commutativity / associativity / idempotence are testable
/// at the level of WHICH names are present.
fn key_sets(core: &Core) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let mut types: Vec<String> = core.types.type_names().iter().map(|s| s.to_string()).collect();
    let mut procs: Vec<String> = core
        .processes
        .type_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut methods: Vec<String> = core
        .methods
        .iter()
        .map(|(t, m)| format!("{t}.{m}"))
        .collect();
    let mut protos: Vec<String> = core.protocols.names().iter().map(|s| s.to_string()).collect();
    types.sort();
    procs.sort();
    methods.sort();
    protos.sort();
    (types, procs, methods, protos)
}

#[test]
fn disjoint_cores_link_to_the_union() {
    let a = pkg_core("Alpha", "PA", ("Alpha", "ma"));
    let b = pkg_core("Beta", "PB", ("Beta", "mb"));
    let ab = a.merge(&b).expect("disjoint packages link with no conflict");

    // Both packages' contributions are present in the linked core.
    assert!(ab.types.schema("Alpha").is_some());
    assert!(ab.types.schema("Beta").is_some());
    assert!(ab.processes.contains("PA"));
    assert!(ab.processes.contains("PB"));
    assert!(ab.methods.lookup("Alpha", "ma").is_some());
    assert!(ab.methods.lookup("Beta", "mb").is_some());
    // And the shared builtins survive (no duplication, no conflict).
    assert!(ab.types.schema("float").is_some());
}

#[test]
fn merge_is_commutative_on_key_sets() {
    let a = pkg_core("Alpha", "PA", ("Alpha", "ma"));
    let b = pkg_core("Beta", "PB", ("Beta", "mb"));
    let ab = a.merge(&b).unwrap();
    let ba = b.merge(&a).unwrap();
    assert_eq!(key_sets(&ab), key_sets(&ba), "A⊔B and B⊔A carry the same names");
}

#[test]
fn merge_is_associative_on_key_sets() {
    let a = pkg_core("Alpha", "PA", ("Alpha", "ma"));
    let b = pkg_core("Beta", "PB", ("Beta", "mb"));
    let c = pkg_core("Gamma", "PC", ("Gamma", "mc"));
    let left = a.merge(&b).unwrap().merge(&c).unwrap();
    let right = a.merge(&b.merge(&c).unwrap()).unwrap();
    assert_eq!(key_sets(&left), key_sets(&right), "(A⊔B)⊔C = A⊔(B⊔C)");
}

#[test]
fn merge_is_idempotent() {
    // The crux of the diamond: A⊔A = A. Self-merge must NOT conflict — types
    // reconcile structurally; processes/methods are the SAME `Arc`s (ptr_eq).
    let a = pkg_core("Alpha", "PA", ("Alpha", "ma"));
    let aa = a.merge(&a).expect("a core merges with itself with no conflict (idempotent)");
    assert_eq!(key_sets(&aa), key_sets(&a), "A⊔A carries exactly A's names");
}

#[test]
fn diamond_dependency_merges_the_shared_apex_once() {
    // prog → A, B and A → D, B → D. D is built ONCE and Arc-shared into both A and
    // B (the resolver's job — here we model it directly), so when A⊔B re-unions D's
    // entries they are pointer-identical → no false self-conflict. This is the whole
    // reason the join must be idempotent at the registry level.
    let shared_dep = Arc::new({
        let mut procs = ProcessRegistry::new();
        procs.register("D", |_| noop());
        procs
    });
    // A and B each carry the SAME shared-dep registry (Arc-cloned) plus their own.
    let a = Core::new().with_processes(Arc::new({
        let mut p = (*shared_dep).clone(); // clones the map → Arc VALUES ptr-preserved
        p.register("A_only", |_| noop());
        p
    }));
    let b = Core::new().with_processes(Arc::new({
        let mut p = (*shared_dep).clone();
        p.register("B_only", |_| noop());
        p
    }));
    let linked = a
        .merge(&b)
        .expect("the diamond links cleanly — the shared apex D is pointer-identical");
    assert!(linked.processes.contains("D"));
    assert!(linked.processes.contains("A_only"));
    assert!(linked.processes.contains("B_only"));
}

#[test]
fn incompatible_type_redefinition_is_a_conflict() {
    // Two packages each export a DIFFERENT `Cell` schema — a genuine conflict the
    // version solver (Phase 2) must prevent. The shared builtins do NOT conflict.
    let mut ta = TypeRegistry::new();
    ta.register("Cell", Schema::float(), None);
    let mut tb = TypeRegistry::new();
    tb.register("Cell", Schema::string(), None);
    let a = Core::new().with_types(Arc::new(ta));
    let b = Core::new().with_types(Arc::new(tb));

    let conflicts = a.merge(&b).expect_err("two different `Cell` types must conflict");
    assert!(
        conflicts.iter().any(|c| c.registry == "type" && c.name == "Cell"),
        "expected a type conflict on `Cell`, got {conflicts:?}"
    );
    // ONLY `Cell` conflicts — the builtins reconcile structurally.
    assert_eq!(conflicts.len(), 1, "builtins must not false-conflict: {conflicts:?}");
}

#[test]
fn same_named_different_process_factories_conflict() {
    // Same class name, two DIFFERENT factory closures (distinct `Arc`s) ⇒ conflict,
    // because a factory is opaque behaviour we cannot compare except by identity.
    let mut pa = ProcessRegistry::new();
    pa.register("PX", |_| noop());
    let mut pb = ProcessRegistry::new();
    pb.register("PX", |_| noop());
    let a = Core::new().with_processes(Arc::new(pa));
    let b = Core::new().with_processes(Arc::new(pb));

    let conflicts = a.merge(&b).expect_err("two different `PX` factories must conflict");
    assert!(conflicts.iter().any(|c| c.registry == "process" && c.name == "PX"));
}

#[test]
fn same_named_different_methods_conflict() {
    let mut ma = MethodRegistry::new();
    ma.register("T", "m", |r, _| Ok(r.clone()));
    let mut mb = MethodRegistry::new();
    mb.register("T", "m", |r, _| Ok(r.clone()));
    let a = Core::new().with_methods(Arc::new(ma));
    let b = Core::new().with_methods(Arc::new(mb));

    let conflicts = a.merge(&b).expect_err("two different `T.m` methods must conflict");
    assert!(conflicts.iter().any(|c| c.registry == "method" && c.name == "T.m"));
}

#[test]
fn builtin_only_cores_merge_cleanly() {
    // The decisive idempotence case: two FRESH cores share nothing but the builtins
    // + the `local` protocol. If those false-conflicted, every two-package link
    // would fail. They must reconcile (structurally for types, by-name for protocols).
    let a = Core::new();
    let b = Core::new();
    let merged = a
        .merge(&b)
        .expect("the shared base (builtins + local protocol) must never conflict");
    assert!(merged.types.schema("float").is_some());
    assert!(merged.protocols.names().contains(&"local"));
}

#[test]
fn own_over_strips_the_shared_base_so_compiled_cores_link() {
    // The package resolver's projection (the dual of merge). A *compiled* Core is
    // not a package's theory — it bundles the theory WITH the base it compiled
    // against AND per-compile infrastructure that re-registers the SAME names with
    // FRESH closures (the real `Composite`/`Brs` factories). Modelled here: the base
    // has `Composite`; a "compiled" core re-registers `Composite` (a fresh Arc) and
    // adds the package's own `Tick`.
    let base = {
        let mut p = ProcessRegistry::new();
        p.register("Composite", |_| noop()); // base infrastructure
        Core::new().with_processes(Arc::new(p))
    };
    let compiled = {
        let mut p = ProcessRegistry::new();
        p.register("Composite", |_| noop()); // FRESH Arc — the per-compile re-registration
        p.register("Tick", |_| noop()); // the package's own export
        Core::new().with_processes(Arc::new(p))
    };

    // Merging the FULL compiled core false-conflicts on the re-registered `Composite`.
    let conflict = base.merge(&compiled).expect_err("re-registered infrastructure conflicts");
    assert!(conflict.iter().any(|c| c.name == "Composite"));

    // `own_over` recovers just the package's own contribution — infrastructure gone.
    let own = compiled.own_over(&base);
    assert!(own.processes.contains("Tick"), "the package's own export survives");
    assert!(
        !own.processes.contains("Composite"),
        "the shared infrastructure is stripped"
    );

    // So the resolver's `base.merge(&own)` links cleanly and carries `Tick`.
    let linked = base
        .merge(&own)
        .expect("a package's own theory links over the shared base");
    assert!(linked.processes.contains("Tick"));
    assert!(linked.processes.contains("Composite"));
}

// ── Phase 2: the n-ary colimit — transitive resolution as a confluent join. ─────
//
// `Core::colimit(base, parts)` is the n-ary extension of `merge`: link a whole
// resolved dependency set onto the shared floor. The categorical claim it realizes
// — *transitive resolution = a colimit over the dep DAG; resolution = confluence* —
// is executable here: the fold is order-independent (any topological order links to
// the same theory), the diamond's shared apex is merged once, and conflicts across
// the whole set are surfaced together. The resolver (pkg) does the upstream work
// (version-solve to one-per-name, memoize so a shared dep is Arc-identical); the
// composition is this one prism call.

/// A package core carrying a SHARED dependency (Arc-cloned in, so it is
/// pointer-identical across packages — what the resolver's memoized compilation
/// guarantees) plus its own distinct process, and a `Shared` type identical by
/// name+schema across packages (deduped STRUCTURALLY, the other dedup path). Models
/// the lower packages of a diamond.
fn pkg_over_shared(shared: &Arc<ProcessRegistry>, own_proc: &str, own_type: &str) -> Core {
    let mut procs = (**shared).clone(); // clones the map → Arc VALUES ptr-preserved
    procs.register(own_proc, |_| noop());
    let mut types = TypeRegistry::new();
    types.register(own_type, Schema::float(), Some(Value::float(0.0)));
    types.register("Shared", Schema::float(), Some(Value::float(0.0)));
    Core::new()
        .with_processes(Arc::new(procs))
        .with_types(Arc::new(types))
}

#[test]
fn colimit_is_the_iterated_binary_join() {
    // colimit IS the n-ary extension of merge — not a parallel codepath. Linking
    // [a, b] onto base equals base.merge(a).merge(b).
    let a = pkg_core("Alpha", "PA", ("Alpha", "ma"));
    let b = pkg_core("Beta", "PB", ("Beta", "mb"));
    let base = Core::new();

    let by_colimit = base.colimit(&[a.clone(), b.clone()]).unwrap();
    let by_fold = base.merge(&a).unwrap().merge(&b).unwrap();
    assert_eq!(key_sets(&by_colimit), key_sets(&by_fold), "colimit = iterated merge");
}

#[test]
fn colimit_is_confluent_over_every_order() {
    // The executable "resolution = confluence": the resolver may present the resolved
    // packages in ANY topological order; the linked theory is identical. Three
    // packages share a dep D (Arc-identical) + a structural type `Shared`, each with
    // its own gens — every one of the 3! orders yields the same key-sets, no conflict.
    let shared = Arc::new({
        let mut p = ProcessRegistry::new();
        p.register("D", |_| noop());
        p
    });
    let a = pkg_over_shared(&shared, "A_only", "Alpha");
    let b = pkg_over_shared(&shared, "B_only", "Beta");
    let c = pkg_over_shared(&shared, "C_only", "Gamma");
    let base = Core::new();

    let reference = key_sets(
        &base
            .colimit(&[a.clone(), b.clone(), c.clone()])
            .expect("the colimit links cleanly"),
    );
    let orders = [
        [&a, &b, &c],
        [&a, &c, &b],
        [&b, &a, &c],
        [&b, &c, &a],
        [&c, &a, &b],
        [&c, &b, &a],
    ];
    for order in orders {
        let parts: Vec<Core> = order.iter().map(|c| (**c).clone()).collect();
        let linked = base.colimit(&parts).expect("every order links cleanly");
        assert_eq!(
            key_sets(&linked),
            reference,
            "colimit is order-independent (confluent)"
        );
    }
}

#[test]
fn colimit_merges_the_shared_apex_once() {
    // The n-ary diamond: a shared dep D reached through three packages contributes
    // ONCE (idempotent), every package's own export is present, and the structural
    // shared type deduped to a single entry.
    let shared = Arc::new({
        let mut p = ProcessRegistry::new();
        p.register("D", |_| noop());
        p
    });
    let a = pkg_over_shared(&shared, "A_only", "Alpha");
    let b = pkg_over_shared(&shared, "B_only", "Beta");
    let c = pkg_over_shared(&shared, "C_only", "Gamma");

    let linked = Core::new()
        .colimit(&[a, b, c])
        .expect("the n-ary diamond links cleanly — the shared apex D is pointer-identical");
    assert!(linked.processes.contains("D"));
    assert!(linked.processes.contains("A_only"));
    assert!(linked.processes.contains("B_only"));
    assert!(linked.processes.contains("C_only"));
    assert!(linked.types.schema("Shared").is_some(), "the structural shared type is present");
}

#[test]
fn colimit_accumulates_every_conflict() {
    // n-ary multi-conflict: two packages each redefine a DIFFERENT base name
    // incompatibly. The colimit does NOT stop at the first — Err lists BOTH, so the
    // user and the version solver's diagnostics see every conflicting package at once.
    let base = {
        let mut t = TypeRegistry::new();
        t.register("X", Schema::float(), None);
        t.register("Y", Schema::float(), None);
        Core::new().with_types(Arc::new(t))
    };
    let redefines_x = {
        let mut t = TypeRegistry::new();
        t.register("X", Schema::string(), None); // incompatible with base X
        Core::new().with_types(Arc::new(t))
    };
    let redefines_y = {
        let mut t = TypeRegistry::new();
        t.register("Y", Schema::string(), None); // incompatible with base Y
        Core::new().with_types(Arc::new(t))
    };

    let conflicts = base
        .colimit(&[redefines_x, redefines_y])
        .expect_err("two incompatible packages must both surface");
    assert!(conflicts.iter().any(|c| c.name == "X"), "X conflict missing: {conflicts:?}");
    assert!(conflicts.iter().any(|c| c.name == "Y"), "Y conflict missing: {conflicts:?}");
}

#[test]
fn colimit_links_own_over_projected_theories_over_the_floor_once() {
    // The resolver's actual loop, end to end (own_over → colimit). Two "compiled"
    // cores each bundle shared infrastructure `Composite` (fresh Arcs that would
    // false-conflict) plus their own export. Projected with `own_over(base)` then
    // linked with `colimit`, the floor is contributed ONCE (no conflict) and both
    // exports survive — resolver.rs's `linked.merge(own)` lifted to n parts.
    let base = {
        let mut p = ProcessRegistry::new();
        p.register("Composite", |_| noop());
        Core::new().with_processes(Arc::new(p))
    };
    let compiled = |export: &'static str| {
        let mut p = ProcessRegistry::new();
        p.register("Composite", |_| noop()); // fresh per-compile infrastructure
        p.register(export, |_| noop());
        Core::new().with_processes(Arc::new(p))
    };
    let foo = compiled("Foo");
    let bar = compiled("Bar");

    let parts = [foo.own_over(&base), bar.own_over(&base)];
    let linked = base
        .colimit(&parts)
        .expect("projected package theories link over the shared floor with no false conflict");
    assert!(linked.processes.contains("Foo"));
    assert!(linked.processes.contains("Bar"));
    assert!(linked.processes.contains("Composite"));
}
