//! The std prelude — prism-std's native capabilities assembled into the
//! registries the runner/CLI need. chrysalis bundles prism-std as its standard
//! library (prism-std → chrysalis), so a `.ys` using std imports
//! (`core`/`integrators`/`chem`/`io`) runs with no extra packages. Downstream
//! packages (e.g. spatio-flux) extend these with their own.

use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;
use prism_bigraph::composite::Composite;
use prism_bigraph::{Core, ProcessNode, ProcessRegistry};
use prism_schema::{Key, MethodError, MethodRegistry, Value};

use crate::compile::ModuleRegistry;

/// Process factories from prism-std (currently `RunProcess`).
pub fn std_registry() -> ProcessRegistry {
    let mut r = ProcessRegistry::new();
    prism_std::register_processes(&mut r);
    r
}

/// Value-methods from prism-std (`TimeSeries`/`Integrator`/`Figure`/`Map`)
/// plus chrysalis's homoiconic `Document.run(time)` (the run-half of the
/// `load(path).run(time)` pair).
pub fn std_methods() -> MethodRegistry {
    let mut m = MethodRegistry::new();
    prism_std::register_methods(&mut m);
    register_document_methods(&mut m);
    crate::quantum::register_quantum_methods(&mut m);
    m
}

/// Register `Document.run(time)` — the run-half of `load(path).run(time)`.
/// Takes a Document value (the program-as-data shape `load()` returns:
/// `{_type: "Document", schema, state}`), reconstructs the `prism_bigraph::Document`,
/// runs it against `std_core()`, and returns the final state value.
fn register_document_methods(m: &mut MethodRegistry) {
    m.register("Document", "run", |recv, args| {
        let time = args
            .first()
            .and_then(|v| v.as_f64())
            .ok_or_else(|| MethodError::BadArgs {
                type_name: "Document".into(),
                method: "run".into(),
                message: "expected a numeric `time` argument (Document.run(t))".into(),
            })?;
        // If the Document remembers either its source (`load(path)` stashes
        // `_source`) OR the original program value (`compile_value(prog)`
        // stashes `_program`), re-compile so user-defined factories are in
        // the Core. Hand-constructed Documents without either fall through
        // to `std_core()` — works for std-only programs.
        if let Some(source_path) = recv.get_field("_source").and_then(|v| v.as_str()) {
            return run_from_source(source_path, time);
        }
        if let Some(program_value) = recv.get_field("_program") {
            return run_from_program_value(program_value, time);
        }
        let doc = value_to_document(recv).ok_or_else(|| MethodError::BadArgs {
            type_name: "Document".into(),
            method: "run".into(),
            message: "receiver is not a Document value (expected {_type: 'Document', schema, state})".into(),
        })?;
        crate::runner::run_document(&doc, std_core(), time).map_err(|e| MethodError::Failed {
            type_name: "Document".into(),
            method: "run".into(),
            message: format!("run_document failed: {e}"),
        })
    });
}

/// Re-compile + run a hand-built Program value — used by `Document.run(time)`
/// when the Document carries `_program` (from `compile_value(prog_value)`).
/// Mirrors [`run_from_source`] but reads the program-shape directly from
/// the Value rather than re-reading a source file.
fn run_from_program_value(program_value: &Value, time: f64) -> Result<Value, MethodError> {
    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "Document".into(),
        method: "run".into(),
        message: format!("{context}: {msg}"),
    };
    let prog = crate::ast::Program::from_value(program_value)
        .map_err(|e| mk_err("Program from_value", e.to_string()))?;
    crate::runner::run(&prog, std_registry(), std_methods(), std_modules(), time)
        .map_err(|e| mk_err("run", format!("{e:?}")))
}

/// Re-compile + run the source at `path` — used by `Document.run(time)` when
/// the Document remembers its origin file. Each call gets fresh registries
/// so a loaded program's own user-defined processes are in scope; without
/// this re-compile step, `std_core()` wouldn't know how to build them.
fn run_from_source(path: &str, time: f64) -> Result<Value, MethodError> {
    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "Document".into(),
        method: "run".into(),
        message: format!("{context}: {msg}"),
    };
    // `parse_file` (not `parse_program`) so the source's OWN relative `.ys`
    // imports (`from .mesh import …`) resolve against ITS directory — exactly
    // what the bin's `run` does (`cli.rs` via `parse_file`). Path-blind
    // `parse_program` would leave `.mesh` unresolved and the compile would reject
    // it as an unknown native module (the bug that broke `load(p).run()` on a
    // program with sibling imports — e.g. the coordination board).
    let prog =
        crate::parse::parse_file(path).map_err(|e| mk_err(&format!("parse {path}"), e.to_string()))?;
    // Use the source file's directory as ys_root so a nested `load('sibling.ys')`
    // inside this program resolves relative to where it lives.
    let ys_root = std::path::Path::new(path).parent().map(|p| p.to_path_buf());
    crate::runner::run(&prog, std_registry(), std_methods(), std_modules_at(ys_root), time)
        .map_err(|e| mk_err(&format!("run {path}"), format!("{e:?}")))
}

/// Materialize a `prism_bigraph::Document` from a chrysalis Value of the
/// shape `{_type: "Document", schema, state}`. The inverse of [`document_to_value`].
fn value_to_document(v: &Value) -> Option<prism_bigraph::Document> {
    let map = v.as_map()?;
    if map.get("_type")?.as_str()? != "Document" {
        return None;
    }
    Some(prism_bigraph::Document {
        schema: map.get("schema").cloned(),
        state: map.get("state")?.clone(),
        ..prism_bigraph::Document::new()
    })
}

/// Render a `prism_bigraph::Document` as a chrysalis Value — the
/// program-as-data shape `load(path)` returns. The Document already stores
/// its schema + state as values, so the conversion is just key wrapping.
fn document_to_value(doc: &prism_bigraph::Document) -> Value {
    let mut fields: IndexMap<Key, Value> = IndexMap::new();
    fields.insert(Key::from("_type"), Value::String("Document".into()));
    if let Some(schema) = &doc.schema {
        fields.insert(Key::from("schema"), schema.clone());
    }
    fields.insert(Key::from("state"), doc.state.clone());
    Value::Map(fields)
}

/// The std library assembled as a runnable [`Core`]: the std process factories +
/// the generic `Composite` factory (so a server can build composites from a doc)
/// + std value-methods. One object carrying the full std capability set — used by
/// `chrysalis server` and any in-process host that wants it whole.
pub fn std_core() -> Core {
    // The Composite factory needs the whole Core (to build subengines); the Core
    // contains the registry that contains this factory — a cycle resolved by a
    // OnceLock set once the Core is built.
    let handle: Arc<OnceLock<Core>> = Arc::new(OnceLock::new());
    let mut registry = ProcessRegistry::new();
    prism_std::register_processes(&mut registry);
    {
        let handle = Arc::clone(&handle);
        registry.register("Composite", move |config| {
            let core = handle.get().expect("core handle not initialized");
            ProcessNode::Process(Box::new(
                Composite::from_config(&config, core).expect("Composite::from_config"),
            ))
        });
    }
    let mut methods = MethodRegistry::new();
    prism_std::register_methods(&mut methods);
    let core = Core::new()
        .with_processes(Arc::new(registry))
        .with_methods(Arc::new(methods))
        .with_protocols(Arc::new(crate::stream::stream_protocols()));
    let _ = handle.set(core.clone());
    core
}

/// The std importable modules: `core` (RunProcess/Simulate), `integrators`
/// (rk4/euler), `chem` (CRN), `io` (Path, `load`). Paths passed to `load`
/// resolve against the **CWD** — see [`std_modules_at`] for the ys-root
/// resolving variant the bin uses.
pub fn std_modules() -> ModuleRegistry {
    std_modules_at(None)
}

/// Std importable modules with `load(path)` resolving relative to `ys_root`
/// (the entry `.ys` file's directory) when the supplied path is relative —
/// the same convention `from … import` already uses for sibling files. With
/// `ys_root = None`, behaves like [`std_modules`] (CWD-relative). The bin's
/// `run` path and `Document.run`'s re-compile both call this with the right
/// root, so a `.ys` author can write `load('sibling.ys')` and have it Just Work.
pub fn std_modules_at(ys_root: Option<std::path::PathBuf>) -> ModuleRegistry {
    let load_fn: crate::compile::HostFn = Arc::new(move |args| {
        let path = args.first().and_then(|v| v.as_str()).ok_or_else(|| {
            MethodError::BadArgs {
                type_name: "io".into(),
                method: "load".into(),
                message: "expected a path string argument (load('file.ys'))".into(),
            }
        })?;
        // Relative path + a known ys_root → resolve relative to that root.
        // Absolute paths and the no-root case pass through unchanged.
        let resolved: std::path::PathBuf = match (ys_root.as_ref(), std::path::Path::new(path).is_absolute()) {
            (Some(root), false) => root.join(path),
            _ => std::path::PathBuf::from(path),
        };
        load_program_as_document(resolved.to_str().unwrap_or(path))
    });
    // `meta::eval` — interpret an Expr-shape Value as a program (tier-2).
    // One-arg form `eval(expr)` evaluates against an empty env; two-arg form
    // `eval(expr, env)` resolves `Var` references against the bindings map.
    let eval_fn: crate::compile::HostFn = Arc::new(|args| {
        let v = args.first().ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "eval".into(),
            message: "expected at least one argument: an Expr-shape Value".into(),
        })?;
        let env = args.get(1);
        eval_with(v, env)
    });
    // `meta::compile_value(program_value)` — the in-memory sibling of
    // `io::load(path)`. Produces a Document VALUE you can `.run(time)`.
    let compile_value_fn: crate::compile::HostFn = Arc::new(|args| {
        let v = args.first().ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "compile_value".into(),
            message: "expected one argument: a Program-shape Value".into(),
        })?;
        compile_value(v)
    });
    // `meta::handle(expr, handlers)` — algebraic effects via the homoiconic
    // substrate. Handlers map operation names to `{params, body}` records;
    // Calls to those names dispatch through the handler bodies. The first
    // slice of #35 — eval-time effects only.
    let handle_fn: crate::compile::HostFn = Arc::new(|args| {
        let expr = args.first().ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "handle".into(),
            message: "expected two arguments: handle(expr, handlers)".into(),
        })?;
        let handlers = args.get(1).ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "handle".into(),
            message: "expected two arguments: handle(expr, handlers)".into(),
        })?;
        handle(expr, handlers)
    });
    // `meta::sample(distribution, seed)` — quantum measurement primitive.
    // Samples one outcome from a probability-weighted map. Deterministic
    // given the seed.
    let sample_fn: crate::compile::HostFn = Arc::new(|args| {
        let dist = args.first().ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "sample".into(),
            message: "expected sample(distribution, seed)".into(),
        })?;
        let seed = args.get(1).ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "sample".into(),
            message: "expected sample(distribution, seed)".into(),
        })?;
        sample(dist, seed)
    });
    // `meta::tensor(state_a, state_b)` — the inverse of `divide`. Combines
    // two separable quantum-state maps into one joint-state map (the tensor
    // product). See docs/quantum-bigraphs.md §III/VII.
    let tensor_fn: crate::compile::HostFn = Arc::new(|args| {
        let a = args.first().ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "tensor".into(),
            message: "expected tensor(state_a, state_b)".into(),
        })?;
        let b = args.get(1).ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "tensor".into(),
            message: "expected tensor(state_a, state_b)".into(),
        })?;
        tensor(a, b)
    });
    // `meta::factorize(joint, split_k)` — the inverse of `tensor`. Detects
    // whether a joint-state Map can be split at bit `k` into two separable
    // sub-states. Returns `{separable: bool, a: {...}, b: {...}}`.
    let factorize_fn: crate::compile::HostFn = Arc::new(|args| {
        let joint = args.first().ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "factorize".into(),
            message: "expected factorize(joint_state, split_k)".into(),
        })?;
        let split = args.get(1).ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "factorize".into(),
            message: "expected factorize(joint_state, split_k)".into(),
        })?;
        factorize(joint, split)
    });
    ModuleRegistry::new()
        .process("core", "RunProcess")
        .process("core", "Simulate")
        .object("integrators", "rk4", prism_std::integrator("rk4"))
        .object("integrators", "euler", prism_std::integrator("euler"))
        .object("stochastic", "ssa", prism_std::stochastic("ssa"))
        .type_(
            "chem",
            "CRN",
            "{species: list[string], reactions: list[{reactants: map[float], products: map[float], k: float}]}",
        )
        .type_("io", "Path", "string")
        .function("io", "load", load_fn)
        .function("meta", "eval", eval_fn)
        .function("meta", "compile_value", compile_value_fn)
        .function("meta", "handle", handle_fn)
        .function("meta", "sample", sample_fn)
        .function("meta", "tensor", tensor_fn)
        .function("meta", "factorize", factorize_fn)
}

/// Read + parse + compile a `.ys` file from disk and return it as a chrysalis
/// Value (`{_type: "Document", schema, state}`). The homoiconic move: a
/// program is the same kind of value the runtime operates on, reachable as
/// data from inside `.ys` itself via `load(path)`.
///
/// Each call constructs fresh std registries — so a loaded program's own
/// `load` imports get a clean module scope, exactly like a fresh
/// `chrysalis run`. The returned Document is runnable via `Document.run(time)`.
/// Public form of the `io::load` native function — the Rust-side door for
/// the surface `load(path)`. Returns the loaded program as a Document Value
/// (`{_type: "Document", schema, state, _source}`); the same value the
/// surface caller gets, callable from Rust without going through method
/// dispatch. Pairs with the `Document.run(time)` method.
pub fn load(path: &str) -> Result<Value, MethodError> {
    load_program_as_document(path)
}

/// Public form of the `meta::eval` native function — the tier-2 substrate:
/// take an `Expr`-shape `Value` (built by hand via map literals, or via
/// `Expr::to_value`), reify it into a real `Expr`, evaluate against an
/// empty environment, return the result. The chrysalis `(eval '(+ 2 3))`.
///
/// The interpreter sees no difference between a parsed expression and a
/// programmatically-built one — that's the homoiconic identity made
/// callable from `.ys` itself via `from meta import eval`.
pub fn eval(value: &Value) -> Result<Value, MethodError> {
    eval_with(value, None)
}

/// `tensor(state_a, state_b) → joint_state` — the **reverse of divide**.
/// Combines two SEPARABLE quantum-state maps into one joint-state map by
/// the tensor product: every (key_a, key_b) pair gets joint amplitude
/// `amp_a(key_a) * amp_b(key_b)`, keyed by `<key_a><key_b>` (string
/// concatenation; works cleanly when both states use bitstring keys).
///
/// Quantum semantics: two separable systems combine into one. The result
/// is FACTORIZABLE — equivalent to keeping them apart, just bundled. To
/// create real entanglement, apply a coupling gate AFTER tensoring.
///
/// Bigraph semantics: this is the inverse of `divide`. Two independent
/// composites' states tensor into one joint composite's state. See
/// `docs/quantum-bigraphs.md` §III + §VII.
pub fn tensor(a: &Value, b: &Value) -> Result<Value, MethodError> {
    use indexmap::IndexMap;
    use prism_schema::Key;
    let map_a = a.as_map().ok_or_else(|| MethodError::BadArgs {
        type_name: "meta".into(),
        method: "tensor".into(),
        message: "first argument must be a Map of {bitstring: amplitude}".into(),
    })?;
    let map_b = b.as_map().ok_or_else(|| MethodError::BadArgs {
        type_name: "meta".into(),
        method: "tensor".into(),
        message: "second argument must be a Map of {bitstring: amplitude}".into(),
    })?;
    let mut joint: IndexMap<Key, Value> = IndexMap::new();
    for (ka, va) in map_a {
        let amp_a = va.as_f64().unwrap_or(0.0);
        for (kb, vb) in map_b {
            let amp_b = vb.as_f64().unwrap_or(0.0);
            let combined_key = format!("{}{}", ka.as_str(), kb.as_str());
            joint.insert(Key::from(combined_key.as_str()), Value::float(amp_a * amp_b));
        }
    }
    Ok(Value::Map(joint))
}

/// `factorize(joint_state, split_k) → {separable: bool, a, b}` — the inverse of
/// `tensor`. Takes a joint-state Map (bitstrings of uniform length → amplitude)
/// and a split index `k`; tries to find two states `a` (over the first `k`
/// bits) and `b` (over the remaining bits) such that `joint = tensor(a, b)`.
///
/// Algorithm: the joint amplitudes form an m×n matrix indexed by (left_bits,
/// right_bits). If the joint is separable, this matrix has rank 1 — there's
/// a row vector `a` and column vector `b` with `joint[i,j] = a[i] * b[j]`.
/// We pick any row with nonzero magnitude as the seed, derive a candidate
/// `(a, b)`, then verify every other entry matches `a[i] * b[j]`.
///
/// Returns `{separable: true, a: {...}, b: {...}}` if factorizable;
/// `{separable: false, a: {}, b: {}}` otherwise. This is the substrate for
/// Q5 of #36 — composites can call this after a measurement to check whether
/// their qubits factor, and then `divide` into independent sub-composites
/// when they do. See `docs/quantum-bigraphs.md` §V/VII.
pub fn factorize(joint: &Value, split: &Value) -> Result<Value, MethodError> {
    use indexmap::IndexMap;
    use prism_schema::Key;
    let map = joint.as_map().ok_or_else(|| MethodError::BadArgs {
        type_name: "meta".into(),
        method: "factorize".into(),
        message: "first argument must be a Map of {bitstring: amplitude}".into(),
    })?;
    let k = split
        .as_i64()
        .or_else(|| split.as_f64().map(|f| f as i64))
        .ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "factorize".into(),
            message: "split index must be numeric".into(),
        })?;
    if k <= 0 {
        return Err(MethodError::BadArgs {
            type_name: "meta".into(),
            method: "factorize".into(),
            message: "split index must be >= 1".into(),
        });
    }
    let k = k as usize;
    if map.is_empty() {
        return Ok(separable_result(false, IndexMap::new(), IndexMap::new()));
    }

    // Verify every key is a bitstring of length > k.
    let total_len = map.keys().next().unwrap().as_str().len();
    if k >= total_len {
        return Err(MethodError::BadArgs {
            type_name: "meta".into(),
            method: "factorize".into(),
            message: format!(
                "split index {k} must be less than bitstring length {total_len}"
            ),
        });
    }
    for key in map.keys() {
        if key.as_str().len() != total_len {
            return Err(MethodError::BadArgs {
                type_name: "meta".into(),
                method: "factorize".into(),
                message: "all keys must have the same length".into(),
            });
        }
    }

    // Build the joint matrix: (left_bits, right_bits) → amplitude. Implicit
    // zero for missing entries.
    let mut left_keys: Vec<String> = Vec::new();
    let mut right_keys: Vec<String> = Vec::new();
    let mut matrix: IndexMap<(String, String), f64> = IndexMap::new();
    for (key, val) in map {
        let s = key.as_str();
        let l = s[..k].to_string();
        let r = s[k..].to_string();
        if !left_keys.contains(&l) {
            left_keys.push(l.clone());
        }
        if !right_keys.contains(&r) {
            right_keys.push(r.clone());
        }
        let amp = val.as_f64().unwrap_or(0.0);
        matrix.insert((l, r), amp);
    }

    // Find a seed row: any left_key whose row has nonzero norm.
    let row_norm = |li: &str| -> f64 {
        right_keys
            .iter()
            .map(|rj| matrix.get(&(li.to_string(), rj.clone())).copied().unwrap_or(0.0).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let seed_row = match left_keys.iter().find(|li| row_norm(li) > 1e-12) {
        Some(li) => li.clone(),
        None => return Ok(separable_result(false, IndexMap::new(), IndexMap::new())),
    };
    let a_seed = row_norm(&seed_row);

    // Candidate factors: a[i] = sqrt(Σ_j |M[i,j]|²); b[j] = M[seed, j] / a[seed].
    let mut a_map: IndexMap<Key, Value> = IndexMap::new();
    for li in &left_keys {
        a_map.insert(Key::from(li.as_str()), Value::float(row_norm(li)));
    }
    let mut b_map: IndexMap<Key, Value> = IndexMap::new();
    for rj in &right_keys {
        let v = matrix.get(&(seed_row.clone(), rj.clone())).copied().unwrap_or(0.0) / a_seed;
        b_map.insert(Key::from(rj.as_str()), Value::float(v));
    }

    // Verification: for every (i, j), check M[i,j] ≈ a[i] * b[j] (with the row
    // sign — `row_norm` is always nonneg, so any negative `b[j]` from the seed
    // row gets propagated). For each non-seed row, also check that all
    // entries share a consistent sign relative to the seed row.
    const TOL: f64 = 1e-6;
    for li in &left_keys {
        let a_i = a_map.get(&Key::from(li.as_str())).and_then(|v| v.as_f64()).unwrap_or(0.0);
        if a_i < 1e-12 {
            continue;
        }
        // Determine the sign multiplier from the first nonzero column.
        let mut sign: Option<f64> = None;
        for rj in &right_keys {
            let m_ij = matrix.get(&(li.clone(), rj.clone())).copied().unwrap_or(0.0);
            let b_j = b_map.get(&Key::from(rj.as_str())).and_then(|v| v.as_f64()).unwrap_or(0.0);
            if b_j.abs() < 1e-12 {
                if m_ij.abs() > TOL {
                    return Ok(separable_result(false, IndexMap::new(), IndexMap::new()));
                }
                continue;
            }
            let s_ij = m_ij / (a_i * b_j);
            if sign.is_none() {
                sign = Some(s_ij);
            } else if (sign.unwrap() - s_ij).abs() > TOL {
                return Ok(separable_result(false, IndexMap::new(), IndexMap::new()));
            }
        }
        // Apply the sign to a[i].
        if let Some(s) = sign {
            if s.abs() < 1e-12 {
                continue;
            }
            let signed = a_i * s.signum();
            a_map.insert(Key::from(li.as_str()), Value::float(signed));
        }
    }

    Ok(separable_result(true, a_map, b_map))
}

fn separable_result(
    separable: bool,
    a: indexmap::IndexMap<prism_schema::Key, Value>,
    b: indexmap::IndexMap<prism_schema::Key, Value>,
) -> Value {
    use indexmap::IndexMap;
    use prism_schema::Key;
    let mut result: IndexMap<Key, Value> = IndexMap::new();
    result.insert(Key::from("separable"), Value::Bool(separable));
    result.insert(Key::from("a"), Value::Map(a));
    result.insert(Key::from("b"), Value::Map(b));
    Value::Map(result)
}

/// `sample(distribution, seed) → outcome` — quantum measurement primitive.
/// `distribution` is a `Value::Map` of `outcome_key → probability_weight`;
/// weights are normalized internally. `seed` is an integer (any sign).
/// Returns the sampled key as a `Value::String`. Deterministic given the
/// seed — the same (distribution, seed) always returns the same outcome.
///
/// Used for quantum measurement in the eval-time effects layer: build a
/// quantum state via handlers, compute the Born probabilities (|amp|² per
/// basis state), and `sample(probs, seed)` collapses to an observed
/// outcome. Pre-quantum-handler-bundle work; `slice 9` will fold this
/// into a `measure` handler with proper state-collapse semantics.
pub fn sample(distribution: &Value, seed: &Value) -> Result<Value, MethodError> {
    use rand::{Rng, SeedableRng, rngs::StdRng};

    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "meta".into(),
        method: "sample".into(),
        message: format!("{context}: {msg}"),
    };
    let map = distribution.as_map().ok_or_else(|| MethodError::BadArgs {
        type_name: "meta".into(),
        method: "sample".into(),
        message: "expected a Map of {outcome_key: probability_weight, …}".into(),
    })?;
    let seed_int = seed.as_i64().or_else(|| seed.as_f64().map(|f| f as i64)).ok_or_else(|| {
        MethodError::BadArgs {
            type_name: "meta".into(),
            method: "sample".into(),
            message: "seed must be numeric".into(),
        }
    })?;
    if map.is_empty() {
        return Err(mk_err("normalize", "distribution is empty".into()));
    }
    let weights: Vec<(String, f64)> = map
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_f64().unwrap_or(0.0)))
        .collect();
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        // Zero-weight distribution (e.g. an uninitialized quantum state at
        // warmup tick): return the first key gracefully so warmup-tick
        // measurements don't crash. The caller can also use this as a
        // sentinel meaning "no signal yet."
        let _ = mk_err;
        return Ok(Value::String(weights[0].0.clone()));
    }
    let mut rng = StdRng::seed_from_u64(seed_int as u64);
    let mut u = rng.r#gen::<f64>() * total;
    for (k, w) in &weights {
        u -= w;
        if u <= 0.0 {
            return Ok(Value::String(k.clone()));
        }
    }
    // Fallback (shouldn't reach due to floating-point summation).
    Ok(Value::String(weights.last().unwrap().0.clone()))
}

/// `handle(expr_value, handlers_value)` — evaluate `expr` with `Call(name, args)`
/// dispatched through user-provided handlers when `name` is one of the keys
/// in `handlers`. Handlers are `{params: ['p1', …], body: <expr>}` values;
/// they materialize as synthetic `Def::Function`s scoped to this `handle`
/// call, so the existing call-resolution path picks them up.
///
/// The first slice of #35 (algebraic effects via the homoiconic substrate):
/// same expression evaluates differently under different handler bundles.
/// Eval-time effects only — runtime-effects (apply/dispatch interception
/// at the engine layer) need a deeper engine pass (later slice).
pub fn handle(expr_value: &Value, handlers_value: &Value) -> Result<Value, MethodError> {
    use std::sync::Arc;

    use indexmap::IndexMap;
    use prism_schema::MethodRegistry;

    use crate::ast::{Def, Expr, FunctionDef, Name, Param, Program, SchemaExpr};
    use crate::eval::Evaluator;

    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "meta".into(),
        method: "handle".into(),
        message: format!("{context}: {msg}"),
    };

    let expr = Expr::from_value(expr_value)
        .map_err(|e| mk_err("expr from_value", e.to_string()))?;

    let handlers_map = handlers_value.as_map().ok_or_else(|| MethodError::BadArgs {
        type_name: "meta".into(),
        method: "handle".into(),
        message: "handlers must be a Map of {name: {params: [...], body: <expr>}}".into(),
    })?;

    // Each handler entry → a synthetic Def::Function. Body comes from the
    // handler's `body` field via `Expr::from_value`; params from the handler's
    // `params` list. The function's name is the map key.
    let mut prog = Program::new();
    for (name, handler_value) in handlers_map {
        let h = handler_value.as_map().ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "handle".into(),
            message: format!("handler `{name}` must be a Map with `params` and `body` keys"),
        })?;
        let param_names: Vec<String> = h
            .get("params")
            .and_then(|v| v.as_list())
            .map(|l| {
                l.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let body_value = h.get("body").ok_or_else(|| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "handle".into(),
            message: format!("handler `{name}` missing `body`"),
        })?;
        let body = Expr::from_value(body_value)
            .map_err(|e| mk_err(&format!("handler `{name}` body from_value"), e.to_string()))?;
        let params: Vec<Param> = param_names
            .into_iter()
            .map(|n| Param {
                name: n,
                schema: SchemaExpr::Any,
                default: None,
            })
            .collect();
        prog.push(Def::Function(FunctionDef {
            name: Name::from(name.as_str()),
            params,
            body,
        }));
    }

    let evaluator = Evaluator::new(Arc::new(prog), Arc::new(MethodRegistry::new()));
    let env: IndexMap<String, Value> = IndexMap::new();
    evaluator
        .eval_value(&expr, &env)
        .map_err(|e| mk_err("eval", e.to_string()))
}

/// `compile_value(program_value)` — the in-memory sibling of `load(path)`.
/// Takes a hand-built `Program`-shape Value (`{_type: 'Program', entities:
/// [EntityDef…]}`), reifies it into a real `Program` via
/// [`crate::ast::Program::from_value`], compiles against fresh std
/// registries, and returns the resulting Document VALUE. Pairs with
/// `Document.run(time)` — same dispatcher as `load(path).run(time)`.
///
/// This is the chrysalis `(eval (cons 'program ...))` at the program level —
/// build a program from map literals, run it. The substrate for #34 (run
/// the streaming env with a hand-constructed Cell).
pub fn compile_value(program_value: &Value) -> Result<Value, MethodError> {
    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "meta".into(),
        method: "compile_value".into(),
        message: format!("{context}: {msg}"),
    };
    let prog = crate::ast::Program::from_value(program_value)
        .map_err(|e| mk_err("Program from_value", e.to_string()))?;
    let result =
        crate::compile::compile_with_modules(&prog, std_registry(), std_methods(), std_modules())
            .map_err(|e| mk_err("compile", format!("{e:?}")))?;
    let doc = crate::runner::document_of(&result);
    let mut value = document_to_value(&doc);
    // Stash `_program` so `Document.run(time)` can re-compile this exact
    // program for its own Core (which knows the user-defined process /
    // composite factories the hand-built entities define). Mirrors the
    // `_source` field `load()` writes; same idea, in-memory variant.
    if let Value::Map(m) = &mut value {
        m.insert(Key::from("_program"), program_value.clone());
    }
    Ok(value)
}

/// `eval(value, env)` — the two-arg form with explicit bindings. `env` is
/// a `Value::Map` of `name → value`; references in the expression resolve
/// against it (so a hand-built `Var("x")` finds `env.x`). When `env` is
/// `None`, behaves identically to [`eval`] (empty bindings).
pub fn eval_with(value: &Value, env: Option<&Value>) -> Result<Value, MethodError> {
    use std::sync::Arc;

    use indexmap::IndexMap;
    use prism_schema::MethodRegistry;

    use crate::ast::{Expr, Program};
    use crate::eval::Evaluator;

    let expr =
        Expr::from_value(value).map_err(|e| MethodError::BadArgs {
            type_name: "meta".into(),
            method: "eval".into(),
            message: format!("expr from_value: {e}"),
        })?;
    let env_map: IndexMap<String, Value> = match env {
        None | Some(Value::None) => IndexMap::new(),
        Some(v) => v
            .as_map()
            .ok_or_else(|| MethodError::BadArgs {
                type_name: "meta".into(),
                method: "eval".into(),
                message: "env must be a Map of {name: value, …}".into(),
            })?
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    };
    let evaluator = Evaluator::new(Arc::new(Program::default()), Arc::new(MethodRegistry::new()));
    evaluator
        .eval_value(&expr, &env_map)
        .map_err(|e| MethodError::Failed {
            type_name: "meta".into(),
            method: "eval".into(),
            message: format!("eval: {e}"),
        })
}

fn load_program_as_document(path: &str) -> Result<Value, MethodError> {
    let mk_err = |context: &str, msg: String| MethodError::Failed {
        type_name: "io".into(),
        method: "load".into(),
        message: format!("{context}: {msg}"),
    };
    // `parse_file` (not path-blind `parse_program`) so the loaded program's OWN
    // relative `.ys` imports (`from .mesh import …`) resolve against ITS directory
    // — the same resolution the bin's `run` uses. Without this, loading a program
    // with sibling imports (the coordination board) failed at compile with
    // "unknown import `.mesh`". And `std_modules_at(ys_root)` so a nested
    // `load('sibling.ys')` inside the loaded program resolves relative to it too.
    let prog =
        crate::parse::parse_file(path).map_err(|e| mk_err(&format!("parse {path}"), e.to_string()))?;
    let ys_root = std::path::Path::new(path).parent().map(|p| p.to_path_buf());
    let result =
        crate::compile::compile_with_modules(&prog, std_registry(), std_methods(), std_modules_at(ys_root))
            .map_err(|e| mk_err(&format!("compile {path}"), format!("{e:?}")))?;
    let doc = crate::runner::document_of(&result);
    let mut value = document_to_value(&doc);
    // Stash:
    //  - `_source` so `Document.run(time)` can re-compile for the program's
    //    own Core (it knows the user-defined process/composite factories).
    //  - `_entities` so a `.ys` caller can inspect the program's entity
    //    registry from inside the surface language (`loaded._entities.…`).
    //    The homoiconic principle: the *list of named things* is itself
    //    data, walkable like any other Map/List.
    if let Value::Map(m) = &mut value {
        m.insert(Key::from("_source"), Value::String(path.into()));
        m.insert(Key::from("_entities"), prog.to_value().get_field("entities").cloned().unwrap_or(Value::None));
    }
    Ok(value)
}
