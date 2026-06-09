//! #62 / Demo 2 PREP — the mesh carries the Kuramoto MEAN-FIELD carrier.
//!
//! Demo 2 couples oscillators through a per-source `map[id -> array[[2], float]]`
//! mesh link: each oscillator owns its key, holding its phase vector `[cos θ, sin θ]`;
//! the mean field = the sum over the keys (the dynamics reads + sums; the mesh just
//! REPLICATES). This verifies that exact carrier converges over the live bridge — the
//! array VALUE crosses the rest codec and the per-source merge holds — so the
//! distributed Demo 2 wiring is proven before manifold's tile composite arrives.

use std::sync::Arc;
use std::time::Duration;

use prism_bigraph::protocols::MeshAgent;
use prism_schema::{Schema, TypeRegistry, Value};

/// A phase vector `[x, y]` (an `array[[2], float]` value).
fn vec2(x: f64, y: f64) -> Value {
    Value::List(vec![Value::float(x), Value::float(y)])
}

/// One oscillator's per-source entry: `{ id: [x, y] }`.
fn osc(id: &str, x: f64, y: f64) -> Value {
    Value::tree([(id, vec2(x, y))])
}

/// The carrier schema — a map of 2-vectors. `mesh_safety` blesses it (Map = the
/// per-source key-union); each oscillator is the single writer of its own key.
fn carrier_schema() -> Schema {
    Schema::map(Schema::Array {
        shape: vec![2],
        element: Box::new(Schema::float()),
    })
}

fn types() -> Arc<TypeRegistry> {
    Arc::new(TypeRegistry::new())
}

#[test]
fn the_kuramoto_mean_field_carrier_converges_over_the_bridge() {
    let schema = carrier_schema();
    // The carrier is admissible as a mesh link (Map of arrays — per-source safe).
    assert!(prism_schema::algebra::is_mesh_safe(&schema));

    // Two tiles, each contributing its oscillator's phase vector to its OWN key.
    let a = MeshAgent::host(schema.clone(), osc("osc0", 1.0, 0.0), types()).unwrap();
    let b = MeshAgent::host(schema.clone(), osc("osc1", 0.0, 1.0), types()).unwrap();
    std::thread::sleep(Duration::from_millis(50)); // servers up

    // One gossip round over the live bridge → both hold BOTH oscillators' vectors
    // (the array value crossed the rest codec; the per-source key-union merged).
    a.sync_round(&[b.port()]);

    let want = Value::tree([("osc0", vec2(1.0, 0.0)), ("osc1", vec2(0.0, 1.0))]);
    assert_eq!(a.replica(), want, "tile A has the full mean-field map: {:?}", a.replica());
    assert_eq!(b.replica(), want, "tile B has the full mean-field map: {:?}", b.replica());

    // The mean field each tile reads = the element-wise SUM over the keys: [1,0]+[0,1]
    // = [1,1] here. (The dynamics computes this; the mesh's job — replication — is done.)
    let mean: Vec<f64> = a
        .replica()
        .as_map()
        .unwrap()
        .values()
        .filter_map(|v| v.as_list())
        .fold(vec![0.0, 0.0], |mut acc, xy| {
            acc[0] += xy[0].as_f64().unwrap_or(0.0);
            acc[1] += xy[1].as_f64().unwrap_or(0.0);
            acc
        });
    assert_eq!(mean, vec![1.0, 1.0], "the replicated field sums to the mean field");
}

/// The crux (manifold's Demo-2 question): for an oscillator that REFRESHES its key
/// every tick, what value schema is correct on BOTH the engine `apply` path and the
/// mesh `merge` path? Answer: `map[overwrite[array]]`.
///
/// `merge` for a bare `array` already OVERWRITES (LWW — "merge combines two complete
/// arrays"), so re-broadcast over gossip is idempotent. BUT the engine's `apply` for
/// a bare `array` is element-wise ADDITIVE — so an oscillator writing its key each
/// tick through the engine would ACCUMULATE. Wrapping the value in `overwrite` makes
/// BOTH paths overwrite-per-value: the per-tick refresh replaces (no accumulation)
/// and the gossip re-broadcast stays idempotent.
#[test]
fn overwrite_array_is_the_right_carrier_value_apply_and_merge_agree() {
    use prism_schema::algebra::{apply_with, merge};

    let arr = || Schema::Array { shape: vec![2], element: Box::new(Schema::float()) };

    // ENGINE `apply`: bare array ADDS (the accumulation footgun); overwrite REPLACES.
    let added = apply_with(None, &arr(), &vec2(1.0, 0.0), &vec2(0.5, 0.5));
    assert_eq!(added, vec2(1.5, 0.5), "bare array apply is element-wise ADDITIVE");
    let replaced = apply_with(None, &Schema::overwrite(arr()), &vec2(1.0, 0.0), &vec2(0.5, 0.5));
    assert_eq!(replaced, vec2(0.5, 0.5), "overwrite[array] apply REPLACES — the per-tick refresh");

    // MESH `merge`: both overwrite (so re-broadcast is idempotent), confirming the
    // wrapper costs nothing on the gossip path.
    let m = merge(&Schema::overwrite(arr()), &vec2(1.0, 0.0), &vec2(0.5, 0.5));
    assert_eq!(m, vec2(0.5, 0.5), "overwrite[array] merge REPLACES");

    // End-to-end over the bridge: a per-source `map[overwrite[array]]` field
    // converges, re-broadcast is idempotent, and a key REFRESH overwrites (not adds).
    let schema = Schema::map(Schema::overwrite(arr()));
    assert!(prism_schema::algebra::is_mesh_safe(&schema));
    let a = MeshAgent::host(schema.clone(), osc("osc0", 1.0, 0.0), types()).unwrap();
    let b = MeshAgent::host(schema.clone(), osc("osc1", 0.0, 1.0), types()).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    a.sync_round(&[b.port()]);
    a.sync_round(&[b.port()]); // re-broadcast (anti-entropy)
    let want = Value::tree([("osc0", vec2(1.0, 0.0)), ("osc1", vec2(0.0, 1.0))]);
    assert_eq!(b.replica(), want, "converged; re-broadcast did NOT accumulate (idempotent)");

    // osc0 refreshes its phase vector → its key OVERWRITES (no accumulation).
    a.contribute(&osc("osc0", 0.5, 0.5));
    a.sync_round(&[b.port()]);
    let refreshed = Value::tree([("osc0", vec2(0.5, 0.5)), ("osc1", vec2(0.0, 1.0))]);
    assert_eq!(b.replica(), refreshed, "a per-tick key refresh overwrites, not accumulates");
}
