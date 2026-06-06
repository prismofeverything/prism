//! `fold` / `unfurl` — the BATWD §IV maneuver.
//!
//! Two named, **inverse** operations over composite-shaped values:
//!
//! - [`unfurl`] takes a composite spec map (the
//!   `{_type:"composite", address, config:{state, bridge, schema},
//!   inputs, outputs, …face…}` shape produced by chrysalis's
//!   `build_composite_outer` after the #47 unification) and **opens** it
//!   into an `_type:"unfurled"` envelope that exposes the inner state,
//!   bridge wiring, declared inner schema, outer interface, and the
//!   self-exported face as siblings of one map. The boundary is *visible*
//!   but not yet dissolved — the inner state is no longer hidden behind
//!   `config.state`.
//! - [`fold`] is the inverse: take an unfurled envelope and reseal it
//!   as a composite spec.
//!
//! The defining law — proved as a property test in
//! `prism-schema/tests/fold_unfurl.rs`:
//!
//! ```text
//!   fold(unfurl(spec)) ≡ spec                  // round-trip identity
//! ```
//!
//! The richer parent-context form (`unfurl_into(parent, path)` that
//! actually inlines a composite *into* its parent and rewrites the
//! bridge wires to relative paths) is the next slice — see
//! `docs/bigraphs-all-the-way-down.md` §IV. This slice gives the
//! algebra the two named ops and the inverse law; the parent-context
//! lift then uses the same pair on the sub-bigraph at a specific path.
//!
//! Sketch of the next slice (recorded for continuity):
//! ```text
//! unfurl_into(parent, composite_path) →
//!   replace parent[composite_path] with the composite's inner state
//!   (its `config.state`), then for each bridge entry mapping a port
//!   to an internal path, take the composite's outer wire for that
//!   port (its `inputs[port]` or `outputs[port]`) and connect it to
//!   the now-exposed internal path.
//!
//! fold_at(parent, region_path, boundary) →
//!   move the subtree at `region_path` into a fresh composite's
//!   `config.state`; for each wire in `boundary`, generate a face name
//!   (the cut-link → port identification) and record the bridge
//!   entry; the parent's slot at `region_path` becomes the composite
//!   spec.
//! ```
//!
//! These are the **composite-level lift** of the value-level
//! divide↔tensor duality (see `divide_by_schema` / `tensor_by_schema`):
//! divide splits a value into independent parts, unfurl opens a
//! composite; tensor combines two values, fold seals a sub-region. One
//! rung up the bigraph ladder; same algebraic shape.

use crate::value::{Key, StateMap, Value};

/// The `_type` sentinel for an unfurled composite envelope.
pub const UNFURLED_TYPE: &str = "unfurled";

/// The `_type` sentinel for a composite spec (the form chrysalis's
/// `build_composite_outer` emits after #47).
pub const COMPOSITE_TYPE: &str = "composite";

/// Reserved top-level keys in a composite spec — every OTHER key is
/// part of the self-exported face (the seed_self_face slots).
const SPEC_RESERVED: &[&str] = &[
    "_type", "address", "config", "inputs", "outputs", "interval",
];

/// Reserved top-level keys in an unfurled envelope — every OTHER key is
/// part of the face (kept as a sibling for round-trip identity).
const UNFURLED_RESERVED: &[&str] = &[
    "_type",
    "address",
    "state",
    "bridge",
    "schema",
    "inputs",
    "outputs",
    "interval",
    "face",
];

/// Open a composite spec into an `_type:"unfurled"` envelope.
///
/// Pre-condition: `spec` is a `Value::Map` whose `_type` is
/// [`COMPOSITE_TYPE`]. Returns `None` otherwise — `unfurl` is a
/// type-aware operation, not a generic map reshape.
///
/// The returned envelope has shape:
///
/// ```text
/// {
///   _type:   "unfurled",
///   address: <spec.address>,
///   state:   <spec.config.state>,
///   bridge:  <spec.config.bridge>,
///   schema:  <spec.config.schema>,
///   inputs:  <spec.inputs>,
///   outputs: <spec.outputs>,
///   face:    { <self-face slots from spec, e.g. mass, glucose> },
///   <preserved-spec-extras>:  …  // anything else the spec carried
/// }
/// ```
///
/// Inverse: [`fold`].
pub fn unfurl(spec: &Value) -> Option<Value> {
    let m = spec.as_map()?;
    if m.get("_type").and_then(|v| v.as_str()) != Some(COMPOSITE_TYPE) {
        return None;
    }

    let address = m.get("address").cloned().unwrap_or(Value::None);
    let inputs = m.get("inputs").cloned().unwrap_or_else(Value::map);
    let outputs = m.get("outputs").cloned().unwrap_or_else(Value::map);
    let interval = m.get("interval").cloned();

    // Pull the inner state, bridge, and schema out from under `config`.
    let (state, bridge, schema) = match m.get("config").and_then(|c| c.as_map()) {
        Some(cfg) => (
            cfg.get("state").cloned().unwrap_or(Value::None),
            cfg.get("bridge").cloned().unwrap_or_else(Value::map),
            cfg.get("schema").cloned().unwrap_or(Value::None),
        ),
        None => (Value::None, Value::map(), Value::None),
    };

    // Collect the face — every non-reserved top-level key on the spec.
    let mut face: StateMap = StateMap::new();
    for (k, v) in m {
        if !SPEC_RESERVED.contains(&k.as_str()) {
            face.insert(k.clone(), v.clone());
        }
    }

    let mut unfurled: StateMap = StateMap::new();
    unfurled.insert(Key::from("_type"), Value::String(UNFURLED_TYPE.into()));
    unfurled.insert(Key::from("address"), address);
    unfurled.insert(Key::from("state"), state);
    unfurled.insert(Key::from("bridge"), bridge);
    unfurled.insert(Key::from("schema"), schema);
    unfurled.insert(Key::from("inputs"), inputs);
    unfurled.insert(Key::from("outputs"), outputs);
    if let Some(iv) = interval {
        unfurled.insert(Key::from("interval"), iv);
    }
    unfurled.insert(Key::from("face"), Value::Map(face));
    Some(Value::Map(unfurled))
}

/// The result of unfurling a composite IN A PARENT place graph: the
/// inlined `parent` (with the composite's inner state hoisted up to its
/// slot) plus a `boundary` value capturing everything needed to
/// reconstruct the original composite spec via [`fold_at`].
///
/// `boundary` is a `Value::Map` carrying `address` / `bridge` /
/// `schema` / `inputs` / `outputs` / `face` (plus `interval` when
/// declared) — the same data the spec-level [`unfurl`] surfaces minus
/// the inner state itself (which now lives in the parent).
#[derive(Clone, Debug, PartialEq)]
pub struct UnfurlAt {
    /// The parent state with the composite's spec at `composite_path`
    /// replaced by the inner state (`config.state`).
    pub parent: Value,
    /// Metadata required to reconstruct the original composite via
    /// [`fold_at`]. Treat as opaque — its shape is part of fold/unfurl
    /// internals.
    pub boundary: Value,
}

/// Inline a composite spec at `composite_path` IN a parent place graph.
///
/// Pre-condition: `parent.get_path(composite_path)` is a composite spec
/// (i.e. a `_type:"composite"` map). Returns `None` otherwise.
///
/// Post-condition: the returned `parent` has the composite's INNER STATE
/// (`config.state`) at `composite_path`, with the spec wrapper removed.
/// The returned `boundary` captures the spec's address / bridge / inner
/// schema / outer interface / self-face — everything needed to reseal.
///
/// Inner processes' wires are PRESERVED unchanged — they were relative
/// to their own slot, which (after inline) still sits at the same
/// position in the place graph. The bridge wires (cross-boundary
/// connections from outer ports to internal paths) are deferred to the
/// boundary value — the structural inline does not yet "re-fuse" them
/// into direct relative-path wires; that's the third slice of S1 (see
/// `docs/bigraphs-all-the-way-down.md` §IV "rewrite each bridge wire
/// back into an ordinary relative-path wire").
///
/// Law: `fold_at(unfurl_into(parent, p)?.parent, p, &unfurl_into(parent, p)?.boundary) ≡ parent`.
pub fn unfurl_into(parent: &Value, composite_path: &[Key]) -> Option<UnfurlAt> {
    let spec = parent.get_path(composite_path)?;
    let envelope = unfurl(spec)?;
    let env_map = envelope.as_map()?;

    // Pull the inner state out of the envelope; everything else becomes
    // the boundary record (a map carrying the spec metadata sans state).
    let inner_state = env_map.get("state").cloned().unwrap_or(Value::None);
    let mut boundary: StateMap = StateMap::new();
    for (k, v) in env_map {
        if k.as_str() == "state" {
            continue;
        }
        boundary.insert(k.clone(), v.clone());
    }

    // Replace the composite spec at `composite_path` with its inner state.
    let mut new_parent = parent.clone();
    new_parent.set_path(composite_path, inner_state);

    Some(UnfurlAt {
        parent: new_parent,
        boundary: Value::Map(boundary),
    })
}

/// Reseal the inlined state at `composite_path` back into a composite
/// spec, using the `boundary` produced by [`unfurl_into`].
///
/// Pre-condition: `boundary` is a `Value::Map` whose `_type` is
/// [`UNFURLED_TYPE`] (the shape `unfurl_into` produces); `parent` has
/// inlined inner state at `composite_path`. Returns `None` otherwise.
///
/// Inverse: [`unfurl_into`]. Together they satisfy
/// `fold_at(unfurl_into(parent, p)?.parent, p, &boundary) ≡ parent`.
pub fn fold_at(parent: &Value, composite_path: &[Key], boundary: &Value) -> Option<Value> {
    let state = parent.get_path(composite_path)?.clone();
    // Stitch the inner state back into the unfurled envelope shape, then
    // call the spec-level `fold` for the wrapping.
    let b_map = boundary.as_map()?;
    if b_map.get("_type").and_then(|v| v.as_str()) != Some(UNFURLED_TYPE) {
        return None;
    }
    let mut envelope: StateMap = StateMap::new();
    for (k, v) in b_map {
        envelope.insert(k.clone(), v.clone());
    }
    envelope.insert(Key::from("state"), state);
    let spec = fold(&Value::Map(envelope))?;

    let mut new_parent = parent.clone();
    new_parent.set_path(composite_path, spec);
    Some(new_parent)
}

/// Reseal an unfurled envelope into a composite spec.
///
/// Pre-condition: `unfurled` is a `Value::Map` whose `_type` is
/// [`UNFURLED_TYPE`]. Returns `None` otherwise.
///
/// Inverse: [`unfurl`]. Together they satisfy
/// `fold(unfurl(spec)) ≡ spec`.
pub fn fold(unfurled: &Value) -> Option<Value> {
    let m = unfurled.as_map()?;
    if m.get("_type").and_then(|v| v.as_str()) != Some(UNFURLED_TYPE) {
        return None;
    }

    let address = m.get("address").cloned().unwrap_or(Value::None);
    let state = m.get("state").cloned().unwrap_or(Value::None);
    let bridge = m.get("bridge").cloned().unwrap_or_else(Value::map);
    let schema = m.get("schema").cloned().unwrap_or(Value::None);
    let inputs = m.get("inputs").cloned().unwrap_or_else(Value::map);
    let outputs = m.get("outputs").cloned().unwrap_or_else(Value::map);
    let interval = m.get("interval").cloned();
    let face = m.get("face").cloned().unwrap_or_else(Value::map);

    let mut config: StateMap = StateMap::new();
    config.insert(Key::from("state"), state);
    config.insert(Key::from("bridge"), bridge);
    config.insert(Key::from("schema"), schema);

    let mut spec: StateMap = StateMap::new();
    spec.insert(Key::from("_type"), Value::String(COMPOSITE_TYPE.into()));
    spec.insert(Key::from("address"), address);
    spec.insert(Key::from("config"), Value::Map(config));
    spec.insert(Key::from("inputs"), inputs);
    spec.insert(Key::from("outputs"), outputs);
    if let Some(iv) = interval {
        spec.insert(Key::from("interval"), iv);
    }
    // Restore the face — these slots sit at the spec's top level,
    // outside `config` (the seed_self_face contract).
    if let Some(face_map) = face.as_map() {
        for (k, v) in face_map {
            if !UNFURLED_RESERVED.contains(&k.as_str()) {
                spec.insert(k.clone(), v.clone());
            }
        }
    }
    Some(Value::Map(spec))
}
