//! Cross-composite reactor — a per-tick BRS that fires reactions whose redex
//! reaches INSIDE (and across) the composites in a container.
//!
//! Where [`BigraphicalReactiveSystem`](crate::BigraphicalReactiveSystem) matches
//! against the subtree it's wired to AS-IS (so a redex sees a composite's
//! published FACE, e.g. `cells.N.mass`), this reactor first **unfurls** every
//! composite child — dissolving each `{_type:"composite", config:{state}}`
//! boundary so the redex can match the composite's INTERIOR and span SEVERAL
//! composites at once (the BATWD §V "one BRS rewrites both levels" maneuver).
//!
//! ## The output=delta contract (the CRUX)
//!
//! The reactor emits a `{state: <delta>}` UPDATE, never a whole new state. The
//! delta is produced by [`cross_fire_delta`]: the localized fire update, with
//! each path mapped back through the composite boundaries it crossed (insert
//! `config.state`). So the cross-composite fire COMPOSES with the composites'
//! own per-tick dynamics — a cell inside one composite grows the SAME tick, and
//! the reactor's field-localized `_add`/`_remove`/`_divide` at `config.state`
//! reconciles field-by-field with that grow delta rather than clobbering it
//! (overwrite) or losing structure (diff). See `docs/execution-model.md`
//! "The boundary contract: state in, delta out".
//!
//! ## Auto-detection
//!
//! The composite paths are **auto-detected** ([`detect_composite_paths`]) from
//! the wired subtree — the caller no longer supplies them (the one-shot
//! `fire_across_composites` / `cross_fire_delta` take explicit paths; the
//! per-tick reactor finds them). A redex that names no composite still fires:
//! with no composites detected the reactor is a no-op (face-level reactions are
//! the plain [`BigraphicalReactiveSystem`]'s job).
//!
//! ## Scope (first slice)
//!
//! Built on the WORKING place-graph matcher (`find_matches`); the link-graph
//! surface form (`?west ~{edge: ~e} | ?east ~{edge: ~e}`, #40) and the
//! distributed form (reactions over a real `rest:`/`stream:` bridge, #42) are
//! later slices. Composites are unfurled ONE level (outermost in the subtree);
//! and the reactor reads `config.state` as the matchable interior — wiring it to
//! LIVE sub-engine composites (whose evolving inner state is private to the node)
//! needs `config.state` kept as the live source, a documented follow-on.

use std::any::Any;

use indexmap::IndexMap;

use prism_schema::reaction::ReactionRule;
use prism_schema::{cross_fire_delta, Key, Schema, StateMap, Value, COMPOSITE_TYPE};

use crate::ports::PortSchema;
use crate::process::Process;
use crate::update::Update;

/// A [`Process`] that, each tick, unfurls the composites in its wired subtree,
/// fires its [`ReactionRule`]s against the flat union, and emits a re-folded
/// structural fire-delta. The cross-composite sibling of
/// [`BigraphicalReactiveSystem`](crate::BigraphicalReactiveSystem); both are
/// thin drivers over the shared `prism_schema` reaction mechanism (no clone).
pub struct CrossCompositeReactor {
    /// Seed reaction rules fired (in order) each tick. Deterministic: the first
    /// match of each rule fires once per tick.
    pub rules: Vec<ReactionRule>,
    /// Wall-clock interval between firings.
    pub interval: f64,
}

impl std::fmt::Debug for CrossCompositeReactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrossCompositeReactor")
            .field("rules", &self.rules.len())
            .field("interval", &self.interval)
            .finish()
    }
}

impl CrossCompositeReactor {
    pub fn new(rules: Vec<ReactionRule>) -> Self {
        Self {
            rules,
            interval: 1.0,
        }
    }

    pub fn with_interval(mut self, interval: f64) -> Self {
        self.interval = interval;
        self
    }
}

impl Process for CrossCompositeReactor {
    fn inputs(&self) -> PortSchema {
        let mut p = IndexMap::new();
        p.insert("state".to_string(), Schema::Any);
        p
    }

    fn outputs(&self) -> PortSchema {
        let mut p = IndexMap::new();
        p.insert("state".to_string(), Schema::Any);
        p
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let Some(subtree) = state.get_field("state") else {
            return Update::Noop;
        };

        // Auto-detect which children are composites — the caller no longer
        // supplies paths (the one-shot fns take them; the per-tick reactor finds
        // them). No composites → nothing for THIS reactor to do.
        let paths = detect_composite_paths(subtree);
        if paths.is_empty() {
            return Update::Noop;
        }
        let path_refs: Vec<&[Key]> = paths.iter().map(Vec::as_slice).collect();

        // Fire each rule; collect the re-folded delta of each that matched.
        let mut deltas: Vec<Value> = Vec::new();
        for rule in &self.rules {
            if let Some(cfd) = cross_fire_delta(subtree, rule, &path_refs) {
                if cfd.fired {
                    deltas.push(cfd.delta);
                }
            }
        }
        if deltas.is_empty() {
            return Update::Noop;
        }

        // Combine the per-rule deltas. A recursing reconcile (RecursiveTree)
        // collates `_add`/`_remove`/`_divide` + per-key deltas at every map
        // level — the schema-faithful merge (NOT opaque `Any` last-wins, which
        // would drop a whole rule's delta). The ENGINE then reconciles THIS
        // update with other processes' updates against the real state schema.
        let combined = if deltas.len() == 1 {
            deltas.into_iter().next().unwrap()
        } else {
            let nested = Schema::RecursiveTree {
                leaf: Box::new(Schema::Any),
            };
            prism_schema::reconcile::reconcile_with(None, &nested, &deltas).unwrap_or(Value::None)
        };
        if matches!(combined, Value::None) {
            return Update::Noop;
        }

        let mut out = StateMap::new();
        out.insert(Key::from("state"), combined);
        Update::value(Value::Map(out))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Collect the path (relative to `subtree`'s root) of every composite spec
/// (`_type: "composite"`) reachable in `subtree`, WITHOUT descending into a
/// composite's interior (its inner state is unfurled by [`cross_fire_delta`],
/// not scanned). These are the auto-detected `composite_paths` the reactor hands
/// to `cross_fire_delta` — the one-level (outermost) composites in the subtree.
pub fn detect_composite_paths(subtree: &Value) -> Vec<Vec<Key>> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    collect(subtree, &mut path, &mut out);
    out
}

fn collect(node: &Value, path: &mut Vec<Key>, out: &mut Vec<Vec<Key>>) {
    let Some(map) = node.as_map() else {
        return;
    };
    if map.get("_type").and_then(|v| v.as_str()) == Some(COMPOSITE_TYPE) {
        out.push(path.clone());
        // Don't descend: a composite's interior is dissolved by unfurl, not
        // scanned for nested composites (the first slice unfurls one level).
        return;
    }
    for (k, v) in map {
        if k.starts_with('_') {
            continue;
        }
        path.push(k.clone());
        collect(v, path, out);
        path.pop();
    }
}
