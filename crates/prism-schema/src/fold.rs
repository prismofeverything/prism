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
//! The parent-context lift — [`unfurl_into`] / [`fold_at`] — operates
//! on a composite at a path in a parent place graph: inlines the spec
//! into its slot and reseals from a boundary descriptor. Round-trip
//! identity: `fold_at(unfurl_into(parent, p)?.parent, p, &…boundary) ≡
//! parent`.
//!
//! The link-graph closure — [`refuse_links`] — completes BATWD §IV's
//! "re-fuse the cut links" claim: after `unfurl_into` hoists inner
//! state to the slot, `refuse_links` walks the inlined state and
//! rewrites every inner process's wire whose path matched a bridge
//! entry, replacing the prefix with `[".."] + outer_wire + suffix`.
//! The leading `..` shifts the reference frame from the inner
//! process's new container (one level deeper after inline) back to
//! the composite's former container, so the outer wire resolves
//! correctly. Together, `unfurl_into` + `refuse_links` produce a flat
//! parent that runs the SAME computation as the original composite-
//! containing parent — the place graph AND the link graph are
//! continuous across the dissolved boundary.
//!
//! These are the **composite-level lift** of the value-level
//! divide↔tensor duality (see `divide_by_schema` / `tensor_by_schema`):
//! divide splits a value into independent parts, unfurl opens a
//! composite; tensor combines two values, fold seals a sub-region. One
//! rung up the bigraph ladder; same algebraic shape.

use indexmap::IndexMap;

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

// ════════════════════════════════════════════════════════════════════
// Wire re-fusion (S1 part C — BATWD §IV "re-fuse the cut links")
// ════════════════════════════════════════════════════════════════════

/// After [`unfurl_into`] has hoisted a composite's inner state up to
/// `composite_path`, **re-fuse the cut links**: rewrite each inner
/// process's wire whose path matched a bridge entry's internal path
/// so it points directly to the composite's former outer endpoint.
///
/// The substitution per wire:
///
/// ```text
///   if wire_path starts with bridge.{inputs,outputs}[port] = internal_path:
///     wire_path  →  [".."] + boundary.{inputs,outputs}[port] + suffix
/// ```
///
/// The leading `".."` shifts the reference frame from the inner
/// process's container (one level deeper after inline) back to the
/// composite's former container — so the outer wire (which was
/// relative TO THE COMPOSITE'S CONTAINER) resolves correctly from the
/// inner process's NEW container.
///
/// Pre-conditions: `composite_path` is non-empty (a root-level inline
/// has no `..` to shift); `boundary` is the [`UnfurlAt::boundary`]
/// produced by `unfurl_into` (`_type:"unfurled"`). Returns `None`
/// otherwise.
///
/// Process specs are recognised by `_type ∈ {"process", "step",
/// "link", "composite"}`. Wires are walked recursively — nested process
/// specs anywhere in the inlined state get their wires rewritten too.
///
/// This is the link-graph half of BATWD §IV: after `unfurl_into` does
/// the place-graph half (inline inner state into the slot),
/// `refuse_links` makes the link graph continuous across the dissolved
/// boundary. Together: the flat parent runs the same computation as
/// the original composite-containing parent.
pub fn refuse_links(parent: &Value, composite_path: &[Key], boundary: &Value) -> Option<Value> {
    if composite_path.is_empty() {
        return None;
    }
    let b_map = boundary.as_map()?;
    if b_map.get("_type").and_then(|v| v.as_str()) != Some(UNFURLED_TYPE) {
        return None;
    }

    let bridge = b_map.get("bridge").and_then(|v| v.as_map());
    let bridge_inputs = bridge
        .and_then(|m| m.get("inputs"))
        .and_then(|v| v.as_map())
        .cloned()
        .unwrap_or_default();
    let bridge_outputs = bridge
        .and_then(|m| m.get("outputs"))
        .and_then(|v| v.as_map())
        .cloned()
        .unwrap_or_default();
    let outer_inputs = b_map
        .get("inputs")
        .and_then(|v| v.as_map())
        .cloned()
        .unwrap_or_default();
    let outer_outputs = b_map
        .get("outputs")
        .and_then(|v| v.as_map())
        .cloned()
        .unwrap_or_default();

    let mut new_parent = parent.clone();
    let inlined = new_parent.get_path_mut(composite_path)?;
    rewire_process_specs(
        inlined,
        &bridge_inputs,
        &outer_inputs,
        &bridge_outputs,
        &outer_outputs,
    );
    Some(new_parent)
}

/// Recurse over `value` and rewrite every process-spec's wires.
fn rewire_process_specs(
    value: &mut Value,
    bridge_inputs: &IndexMap<Key, Value>,
    outer_inputs: &IndexMap<Key, Value>,
    bridge_outputs: &IndexMap<Key, Value>,
    outer_outputs: &IndexMap<Key, Value>,
) {
    let Some(map) = value.as_map_mut() else {
        return;
    };
    let is_spec = matches!(
        map.get("_type").and_then(|v| v.as_str()),
        Some("process" | "step" | "link" | "composite")
    );
    if is_spec {
        if let Some(Value::Map(inputs)) = map.get_mut("inputs") {
            rewire_wires(inputs, bridge_inputs, outer_inputs);
        }
        if let Some(Value::Map(outputs)) = map.get_mut("outputs") {
            rewire_wires(outputs, bridge_outputs, outer_outputs);
        }
    }
    // Recurse — process specs can be nested (composites containing
    // composites), and non-spec containers can still carry specs below.
    for v in map.values_mut() {
        rewire_process_specs(v, bridge_inputs, outer_inputs, bridge_outputs, outer_outputs);
    }
}

fn rewire_wires(
    wires: &mut IndexMap<Key, Value>,
    bridge_entries: &IndexMap<Key, Value>,
    outer_wires: &IndexMap<Key, Value>,
) {
    for wire in wires.values_mut() {
        if let Some(rewired) = rewire_one(wire, bridge_entries, outer_wires) {
            *wire = rewired;
        }
    }
}

/// If `wire`'s path starts with any `bridge_entries[port]`'s internal
/// path, replace the prefix with `[".."] + outer_wires[port]`. Returns
/// `None` if no entry matched (caller leaves the wire unchanged).
fn rewire_one(
    wire: &Value,
    bridge_entries: &IndexMap<Key, Value>,
    outer_wires: &IndexMap<Key, Value>,
) -> Option<Value> {
    let wire_list = wire.as_list()?;
    let wire_strs: Vec<&str> = wire_list.iter().filter_map(|v| v.as_str()).collect();
    if wire_strs.len() != wire_list.len() {
        return None;
    }

    // Longest-prefix match wins — a bridge entry [a, b] is more
    // specific than [a]; we want the more specific to take precedence.
    let mut best: Option<(usize, &Key)> = None;
    for (port, internal) in bridge_entries {
        let internal_list = match internal.as_list() {
            Some(l) => l,
            None => continue,
        };
        let internal_strs: Vec<&str> = internal_list.iter().filter_map(|v| v.as_str()).collect();
        if internal_strs.len() != internal_list.len() {
            continue;
        }
        if wire_strs.len() < internal_strs.len() {
            continue;
        }
        if wire_strs[..internal_strs.len()] != internal_strs[..] {
            continue;
        }
        match best {
            None => best = Some((internal_strs.len(), port)),
            Some((len, _)) if internal_strs.len() > len => best = Some((internal_strs.len(), port)),
            _ => {}
        }
    }

    let (prefix_len, port) = best?;
    let outer_wire = outer_wires.get(port)?.as_list()?;
    let suffix: Vec<Value> = wire_list[prefix_len..].to_vec();

    let mut new_path: Vec<Value> = Vec::with_capacity(1 + outer_wire.len() + suffix.len());
    new_path.push(Value::String("..".to_string()));
    new_path.extend(outer_wire.iter().cloned());
    new_path.extend(suffix);
    Some(Value::List(new_path))
}
