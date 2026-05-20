//! Bigraphical reactive system (BRS) primitives — parametric reaction
//! rules over hierarchical state.
//!
//! Reference: Milner, *Space and Motion of Communicating Agents* (2008),
//! Defs. 8.2 (dynamic signature: atomic / passive / active) and
//! 8.5 (parametric reaction rule = (redex, reactum, instantiation)).
//!
//! Python upstream: `bigraph_schema.assembly` (the reaction half).
//!
//! ## Encoding
//!
//! - **Place graph** = nested `Value::Map` (already what state is).
//! - **Link graph** = wire paths embedded in state as `Value::List`
//!   under a node's `outputs` field, e.g. `outputs: {port: [..path]}`.
//! - **Redex / reactum** = [`Pattern`] tree mirroring `Value`, with
//!   three pattern-only constructors:
//!     - [`Pattern::Site`] — place-graph hole, matches any subtree.
//!     - [`Pattern::LinkVar`] — link variable, captures a wire path;
//!       same name in two positions binds to the same path.
//!     - [`Pattern::Absent`] — negative application condition: the
//!       named key must be missing or empty (e.g. "this port is not
//!       wired", "this enzyme is free").
//!
//! ## Match
//!
//! [`find_matches`] walks the state tree and at every map node tries
//! the redex. Each successful match records the path, the captured
//! site bindings, captured edge bindings, and the redex→state key
//! mapping needed to re-emit reactum keys under their original state
//! names.
//!
//! ## Fire
//!
//! [`fire_rule`] picks a match, builds the replacement via
//! [`instantiate`], and emits a path-localized update: a `_remove`
//! sentinel for keys the redex consumed plus an `_add` for the
//! reactum's new keys at the match path. No whole-tree overwrite.

use std::sync::atomic::{AtomicU64, Ordering};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::{Key, Path, StateMap, Value};

// ── Patterns ────────────────────────────────────────────────────────

/// A pattern in a redex or reactum.
///
/// Mirrors [`Value`] structurally but admits three pattern-only
/// constructors that never appear in concrete state:
/// [`Pattern::Site`], [`Pattern::LinkVar`], [`Pattern::Absent`].
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    /// Place-graph hole — matches any subtree.
    Site,
    /// Link-graph variable — captures a wire path. Two occurrences of
    /// the same name in a redex must bind to the same path; in a
    /// reactum, an unbound name mints a fresh edge anchor.
    LinkVar(Key),
    /// Negative application condition — the key must be absent in
    /// state, or present as an empty container.
    Absent,
    /// Literal value match (atom or list).
    Atom(Value),
    /// Map pattern. Keys starting with `_` are metadata (e.g.
    /// `_type` carries the control / sort label). Non-`_` keys are
    /// matched combinatorially against state keys.
    Map(IndexMap<Key, Pattern>),
    /// List pattern, element-wise.
    List(Vec<Pattern>),
    /// As-pattern: match `inner`, AND bind the whole matched node to
    /// `name` (captured as a site). The chrysalis surface form is a
    /// nested typed site, `?name::Sort` — e.g. `?cid : ?cell::Cell` binds
    /// the entry key to `?cid` and the matched cell value to `?cell`.
    Bind { name: Key, inner: Box<Pattern> },
}

impl Pattern {
    pub fn site() -> Self {
        Self::Site
    }

    pub fn link_var(name: impl Into<Key>) -> Self {
        Self::LinkVar(name.into())
    }

    pub fn absent() -> Self {
        Self::Absent
    }

    pub fn atom(value: impl Into<Value>) -> Self {
        Self::Atom(value.into())
    }

    /// Build a Map pattern from an iterator of (key, pattern) pairs.
    pub fn map<K, I>(entries: I) -> Self
    where
        K: Into<Key>,
        I: IntoIterator<Item = (K, Pattern)>,
    {
        Self::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// Build a Map pattern with a sort (`_type`) label, e.g.
    /// `Pattern::sort("Compartment", [("kind", ...)])`.
    pub fn sort<K, I>(label: impl Into<String>, entries: I) -> Self
    where
        K: Into<Key>,
        I: IntoIterator<Item = (K, Pattern)>,
    {
        let mut map: IndexMap<Key, Pattern> = IndexMap::new();
        map.insert(Key::from("_type"), Pattern::Atom(Value::String(label.into())));
        for (k, v) in entries {
            map.insert(k.into(), v);
        }
        Self::Map(map)
    }

    pub fn list(items: impl IntoIterator<Item = Pattern>) -> Self {
        Self::List(items.into_iter().collect())
    }
}

// ── Activity (Milner Def. 8.2) ──────────────────────────────────────

/// Per-control activity status. Controls not listed default to
/// [`Activity::Active`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Activity {
    Active,
    Passive,
    Atomic,
}

impl Default for Activity {
    fn default() -> Self {
        Self::Active
    }
}

pub type ControlStatus = IndexMap<Key, Activity>;

/// True if every ancestor of the matched position has an active
/// control. Reactions only fire at active locations (Milner Def. 8.2).
/// Lookups match by `_type` / `_control` first, then by the dict-key
/// the path step descends into (for plain dicts whose key IS the
/// control label, as in Milner's built-environment example).
///
/// The "ancestors" of a position at `path` are root and every node
/// along the path. We check each in turn — including the final node
/// at `path`, since that node contains the matched site.
pub fn is_active(root: &Value, path: &[Key], status: &ControlStatus) -> bool {
    fn check(node: &Value, step: Option<&Key>, status: &ControlStatus) -> bool {
        let by_control = control_of(node).and_then(|c| status.get(c).copied());
        let by_step = step.and_then(|s| status.get(s.as_str()).copied());
        let activity = by_control.or(by_step).unwrap_or(Activity::Active);
        activity == Activity::Active
    }

    if !check(root, None, status) {
        return false;
    }
    let mut node = root;
    for step in path {
        node = match node.get_field(step.as_str()) {
            Some(child) => child,
            None => return true,
        };
        if !check(node, Some(step), status) {
            return false;
        }
    }
    true
}

/// Read a node's control label from `_type` (canonical) or
/// `_control` (legacy).
fn control_of(node: &Value) -> Option<&str> {
    node.get_field("_type")
        .and_then(|v| v.as_str())
        .or_else(|| node.get_field("_control").and_then(|v| v.as_str()))
}

// ── Bindings + Match ────────────────────────────────────────────────

/// Bindings collected during one match attempt.
#[derive(Clone, Debug, Default)]
pub struct Bindings {
    /// Captured subtrees, keyed by redex site label.
    /// With surplus, a site captures `{state_key: state_value}` so the
    /// "rest" semantics can be preserved through reactum instantiation.
    /// Without surplus (1:1 assignment), a site captures the bare value.
    pub sites: IndexMap<Key, Value>,

    /// Captured wire paths, keyed by [`Pattern::LinkVar`] name.
    pub edges: IndexMap<Key, Value>,

    /// Redex key → state key, so the reactum can emit children under
    /// their original state names rather than the redex's pattern labels.
    /// Nested matches merge into this map (inner-precedence; we walk
    /// outside-in so outermost keys settle first).
    pub key_map: IndexMap<Key, Key>,
}

/// Result of a successful match.
#[derive(Clone, Debug)]
pub struct Match {
    /// Where in state the match was found.
    pub path: Path,
    /// Captured site / edge / key-map data.
    pub bindings: Bindings,
}

// ── Reaction rule ───────────────────────────────────────────────────

/// A parametric reaction rule (Milner Def. 8.5).
#[derive(Clone, Debug)]
pub struct ReactionRule {
    pub redex: Pattern,
    pub reactum: Pattern,
    /// `reactum_site_key → redex_site_key` — which captured subtree
    /// fills which reactum hole. If empty, defaults to identity by
    /// name (set up lazily on first use via [`Self::resolved_instantiation`]).
    pub instantiation: IndexMap<Key, Key>,
    /// Optional stochastic rate (Milner §11.4). Used as a propensity
    /// coefficient by Gillespie-style firing.
    pub rate: Option<f64>,
    /// Human-readable label for traces and logs.
    pub label: String,
}

impl ReactionRule {
    pub fn new(redex: Pattern, reactum: Pattern) -> Self {
        Self {
            redex,
            reactum,
            instantiation: IndexMap::new(),
            rate: None,
            label: String::new(),
        }
    }

    pub fn with_rate(mut self, rate: f64) -> Self {
        self.rate = Some(rate);
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn with_instantiation<K1, K2, I>(mut self, entries: I) -> Self
    where
        K1: Into<Key>,
        K2: Into<Key>,
        I: IntoIterator<Item = (K1, K2)>,
    {
        self.instantiation = entries
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    /// Resolved instantiation map — falls back to identity-by-site-key
    /// when not explicitly specified, matching the Python default.
    pub fn resolved_instantiation(&self) -> IndexMap<Key, Key> {
        if !self.instantiation.is_empty() {
            return self.instantiation.clone();
        }
        let mut result = IndexMap::new();
        let reactum_sites = collect_sites(&self.reactum);
        let redex_sites = collect_sites(&self.redex);
        for site_key in &reactum_sites {
            if redex_sites.contains(site_key) {
                result.insert(site_key.clone(), site_key.clone());
            }
        }
        result
    }
}

fn collect_sites(pat: &Pattern) -> Vec<Key> {
    let mut out = Vec::new();
    fn walk(pat: &Pattern, out: &mut Vec<Key>) {
        match pat {
            Pattern::Map(m) => {
                for (k, v) in m {
                    if matches!(v, Pattern::Site) {
                        out.push(k.clone());
                    } else {
                        walk(v, out);
                    }
                }
            }
            Pattern::List(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }
    walk(pat, &mut out);
    out
}

// ── Matching ────────────────────────────────────────────────────────

/// Find every position in `state` where `redex` matches.
///
/// `control_status`, when provided, gates matches by activity — a
/// position only matches if every ancestor along the path has an
/// active control.
pub fn find_matches(
    state: &Value,
    redex: &Pattern,
    control_status: Option<&ControlStatus>,
) -> Vec<Match> {
    let mut results = Vec::new();
    let mut path = Vec::new();
    walk_state(state, state, redex, &mut path, control_status, &mut results);
    results
}

fn walk_state(
    root: &Value,
    node: &Value,
    redex: &Pattern,
    path: &mut Vec<Key>,
    control_status: Option<&ControlStatus>,
    results: &mut Vec<Match>,
) {
    let is_map = matches!(node, Value::Map(_) | Value::Struct { .. });
    if is_map {
        let mut bindings = Bindings::default();
        if let Pattern::Map(redex_map) = redex {
            if match_against_map(node, redex_map, &mut bindings) {
                let active = control_status
                    .map(|cs| is_active(root, path, cs))
                    .unwrap_or(true);
                if active {
                    results.push(Match {
                        path: path.clone(),
                        bindings,
                    });
                }
            }
        }
        // Descend into children.
        if let Some(iter) = node.iter_fields() {
            for (key, child) in iter {
                if key.starts_with('_') {
                    continue;
                }
                path.push(key.clone());
                walk_state(root, child, redex, path, control_status, results);
                path.pop();
            }
        }
    }
}

/// Match a redex map against a state node (Map or Struct).
fn match_against_map(
    state: &Value,
    redex: &IndexMap<Key, Pattern>,
    bindings: &mut Bindings,
) -> bool {
    // Sort-label check (`_type` / `_control`) before doing any work.
    if let Some(Pattern::Atom(expected)) = redex
        .get(Key::from("_type").as_str())
        .or_else(|| redex.get(Key::from("_control").as_str()))
    {
        let actual = state
            .get_field("_type")
            .or_else(|| state.get_field("_control"));
        if actual != Some(expected) {
            return false;
        }
    }

    // Absent NACs first — fail fast if a forbidden key is present.
    for (k, v) in redex {
        if matches!(v, Pattern::Absent) {
            match state.get_field(k.as_str()) {
                None => continue,
                Some(Value::Map(m)) if m.is_empty() => continue,
                Some(Value::None) => continue,
                _ => return false,
            }
        }
    }

    // Collect assignable entries (skip _-prefixed keys and Absent NACs).
    let user_entries: Vec<(Key, &Pattern)> = redex
        .iter()
        .filter(|(k, v)| !k.starts_with('_') && !matches!(v, Pattern::Absent))
        .map(|(k, v)| (k.clone(), v))
        .collect();

    // Collect state's user keys (also skip _-prefixed).
    let state_keys: Vec<Key> = match state.iter_fields() {
        Some(it) => it
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, _)| k.clone())
            .collect(),
        None => return false,
    };

    let state_key_set: std::collections::HashSet<&Key> = state_keys.iter().collect();

    // ── Phase 1: name-aligned entries (match by KEY NAME) ───────────
    // A redex key that is also a state key is a field SELECTOR: bind its
    // pattern to that exact field. This makes field matching independent of
    // declaration / insertion order — `mass: ?m` always binds the `mass`
    // field, never a positional neighbour. Sites capture the bare value
    // (a named selector picks one value, it does not absorb a region).
    let mut consumed: std::collections::HashSet<Key> = std::collections::HashSet::new();
    let mut label_entries: Vec<(Key, &Pattern)> = Vec::new();
    for (rkey, rpat) in &user_entries {
        if state_key_set.contains(rkey) {
            let sval = state
                .get_field(rkey.as_str())
                .expect("state_key_set membership implies the field exists");
            if !try_pair(rkey, *rpat, rkey, sval, false, bindings) {
                return false;
            }
            bindings
                .key_map
                .entry(rkey.clone())
                .or_insert_with(|| rkey.clone());
            consumed.insert(rkey.clone());
        } else {
            label_entries.push((rkey.clone(), *rpat));
        }
    }

    // ── Phase 2: label entries (structural / positional) ────────────
    // Redex keys NOT present in the state are labels (e.g. `substrate:
    // ERK`, `rest: Site`): match their patterns against the REMAINING state
    // keys by structure, with Milner "rest capture" into the last label
    // Site. Surplus state keys with no label Site to absorb them are
    // tolerated — a named selector matches a node that has extra fields.
    let remaining_state: Vec<Key> = state_keys
        .iter()
        .filter(|k| !consumed.contains(*k))
        .cloned()
        .collect();

    let label_non_site = label_entries
        .iter()
        .filter(|(_, v)| !matches!(v, Pattern::Site))
        .count();
    if label_non_site > remaining_state.len() {
        return false;
    }
    let has_surplus = label_entries.len() < remaining_state.len();

    let mut assignment: Vec<Option<Key>> = vec![None; label_entries.len()];
    let mut used = vec![false; remaining_state.len()];

    if !try_assign(
        0,
        &label_entries,
        &remaining_state,
        state,
        &mut assignment,
        &mut used,
        has_surplus,
        bindings,
    ) {
        return false;
    }

    // Absorb surplus state keys into the last label Site (Milner: a site is
    // a hole that captures the leftover region). With no label Site, extra
    // fields are simply tolerated.
    if has_surplus {
        absorb_surplus(state, &label_entries, &assignment, &consumed, bindings);
    }

    for (i, (redex_key, _)) in label_entries.iter().enumerate() {
        if let Some(state_key) = &assignment[i] {
            bindings
                .key_map
                .entry(redex_key.clone())
                .or_insert_with(|| state_key.clone());
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn try_assign(
    i: usize,
    user_entries: &[(Key, &Pattern)],
    state_keys: &[Key],
    state: &Value,
    assignment: &mut [Option<Key>],
    used: &mut [bool],
    has_surplus: bool,
    bindings: &mut Bindings,
) -> bool {
    if i == user_entries.len() {
        return true;
    }

    let (redex_key, redex_pat) = &user_entries[i];
    let saved = bindings.clone();

    for j in 0..state_keys.len() {
        if used[j] {
            continue;
        }
        let state_key = &state_keys[j];
        let state_value = state
            .get_field(state_key.as_str())
            .expect("state_key from iter_fields must resolve");

        let ok = try_pair(redex_key, redex_pat, state_key, state_value, has_surplus, bindings);

        if ok {
            assignment[i] = Some(state_key.clone());
            used[j] = true;
            if try_assign(
                i + 1,
                user_entries,
                state_keys,
                state,
                assignment,
                used,
                has_surplus,
                bindings,
            ) {
                return true;
            }
            assignment[i] = None;
            used[j] = false;
            *bindings = saved.clone();
        }
    }

    // Site can also bind to zero state keys (empty capture).
    if matches!(redex_pat, Pattern::Site) {
        bindings
            .sites
            .entry(redex_key.clone())
            .or_insert_with(Value::map);
        assignment[i] = None;
        if try_assign(
            i + 1,
            user_entries,
            state_keys,
            state,
            assignment,
            used,
            has_surplus,
            bindings,
        ) {
            return true;
        }
        *bindings = saved;
    }

    false
}

/// Attempt to pair one redex (key, pattern) with one state (key, value).
fn try_pair(
    redex_key: &Key,
    redex_pat: &Pattern,
    state_key: &Key,
    state_value: &Value,
    has_surplus: bool,
    bindings: &mut Bindings,
) -> bool {
    match redex_pat {
        Pattern::Site => {
            // With surplus, sites capture as {state_key: value} dicts so
            // the last site can absorb the rest as a forest of trees;
            // without surplus (1:1), sites capture the bare value.
            let captured = if has_surplus {
                let mut m = StateMap::new();
                m.insert(state_key.clone(), state_value.clone());
                Value::Map(m)
            } else {
                state_value.clone()
            };
            bindings.sites.insert(redex_key.clone(), captured);
            true
        }
        Pattern::LinkVar(name) => match bindings.edges.get(name) {
            None => {
                bindings.edges.insert(name.clone(), state_value.clone());
                true
            }
            Some(existing) => existing == state_value,
        },
        Pattern::Absent => unreachable!("Absent handled at level above"),
        Pattern::Atom(expected) => state_value == expected,
        Pattern::Map(submap) => match_against_map(state_value, submap, bindings),
        Pattern::List(items) => match state_value {
            Value::List(state_list) if state_list.len() == items.len() => {
                let saved = bindings.clone();
                for (s, r) in state_list.iter().zip(items.iter()) {
                    // List positions don't have keys; pass dummies.
                    if !try_pair(redex_key, r, &Key::from(""), s, false, bindings) {
                        *bindings = saved;
                        return false;
                    }
                }
                true
            }
            _ => false,
        },
        // As-pattern: match `inner`, then capture the WHOLE matched node
        // under `name` (a site), so a reaction can name + reuse it
        // (`?cell::Cell` → guard/reactum use `?cell`).
        Pattern::Bind { name, inner } => {
            if try_pair(redex_key, inner, state_key, state_value, has_surplus, bindings) {
                bindings.sites.insert(name.clone(), state_value.clone());
                true
            } else {
                false
            }
        }
    }
}

/// Merge unassigned state keys into the last [`Pattern::Site`]'s
/// binding. This implements Milner's "rest capture": one site can
/// absorb the remaining region children as a forest.
fn absorb_surplus(
    state: &Value,
    user_entries: &[(Key, &Pattern)],
    assignment: &[Option<Key>],
    consumed: &std::collections::HashSet<Key>,
    bindings: &mut Bindings,
) {
    let assigned: std::collections::HashSet<&Key> = assignment
        .iter()
        .filter_map(|opt| opt.as_ref())
        .collect();
    let mut surplus = StateMap::new();
    if let Some(iter) = state.iter_fields() {
        for (k, v) in iter {
            // Skip private keys, keys bound by a label, and keys already
            // consumed by a name-aligned selector.
            if k.starts_with('_') || assigned.contains(k) || consumed.contains(k) {
                continue;
            }
            surplus.insert(k.clone(), v.clone());
        }
    }
    if surplus.is_empty() {
        return;
    }
    let last_site_idx = user_entries
        .iter()
        .rposition(|(_, v)| matches!(v, Pattern::Site));
    if let Some(idx) = last_site_idx {
        let key = &user_entries[idx].0;
        match bindings.sites.get_mut(key) {
            Some(Value::Map(existing)) => {
                for (k, v) in surplus {
                    existing.insert(k, v);
                }
            }
            _ => {
                bindings.sites.insert(key.clone(), Value::Map(surplus));
            }
        }
    }
}

// ── Instantiation ───────────────────────────────────────────────────

static FRESH_EDGE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn gensym_edge() -> String {
    let n = FRESH_EDGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("~e_{n}")
}

/// Build the concrete replacement subtree from a reactum pattern using
/// the bindings captured during matching.
pub fn instantiate(
    reactum: &Pattern,
    bindings: &Bindings,
    instantiation: &IndexMap<Key, Key>,
) -> Value {
    let mut edges = bindings.edges.clone();
    instantiate_walk(reactum, bindings, instantiation, &mut edges)
}

fn instantiate_walk(
    pat: &Pattern,
    bindings: &Bindings,
    instantiation: &IndexMap<Key, Key>,
    edges: &mut IndexMap<Key, Value>,
) -> Value {
    match pat {
        Pattern::Site => {
            // A bare Site at the value position is unusual but supported.
            Value::None
        }
        Pattern::LinkVar(name) => match edges.get(name) {
            Some(bound) => bound.clone(),
            None => {
                let fresh = Value::List(vec![
                    Value::String("_edges".to_string()),
                    Value::String(gensym_edge()),
                ]);
                edges.insert(name.clone(), fresh.clone());
                fresh
            }
        },
        Pattern::Absent => Value::None,
        Pattern::Atom(v) => v.clone(),
        Pattern::Map(m) => instantiate_map(m, bindings, instantiation, edges),
        Pattern::List(items) => Value::List(
            items
                .iter()
                .map(|p| instantiate_walk(p, bindings, instantiation, edges))
                .collect(),
        ),
        // A Bind on the reactum side is just its inner pattern.
        Pattern::Bind { inner, .. } => instantiate_walk(inner, bindings, instantiation, edges),
    }
}

fn instantiate_map(
    reactum: &IndexMap<Key, Pattern>,
    bindings: &Bindings,
    instantiation: &IndexMap<Key, Key>,
    edges: &mut IndexMap<Key, Value>,
) -> Value {
    let mut result = StateMap::new();
    for (key, value) in reactum {
        match value {
            Pattern::Absent => continue,
            Pattern::Site => {
                let source = instantiation.get(key).unwrap_or(key);
                let filler = bindings.sites.get(source);
                match filler {
                    None => continue,
                    Some(Value::Map(m)) => {
                        let has_sort_tag = m.contains_key("_type")
                            || m.contains_key("_control");
                        if !has_sort_tag {
                            // Forest of trees — splice in at this level.
                            for (fk, fv) in m {
                                result.insert(fk.clone(), fv.clone());
                            }
                        } else {
                            // Single tree with a sort label — nest under
                            // the redex-side site key; reactum-key →
                            // state-key remapping happens in fire_rule.
                            result.insert(key.clone(), Value::Map(m.clone()));
                        }
                    }
                    Some(other) => {
                        result.insert(key.clone(), other.clone());
                    }
                }
            }
            Pattern::LinkVar(name) => {
                let bound = match edges.get(name) {
                    Some(b) => b.clone(),
                    None => {
                        let fresh = Value::List(vec![
                            Value::String("_edges".to_string()),
                            Value::String(gensym_edge()),
                        ]);
                        edges.insert(name.clone(), fresh.clone());
                        fresh
                    }
                };
                result.insert(key.clone(), bound);
            }
            Pattern::Map(submap) => {
                let v = instantiate_map(submap, bindings, instantiation, edges);
                result.insert(key.clone(), v);
            }
            Pattern::Atom(v) => {
                result.insert(key.clone(), v.clone());
            }
            Pattern::List(items) => {
                let v = Value::List(
                    items
                        .iter()
                        .map(|p| instantiate_walk(p, bindings, instantiation, edges))
                        .collect(),
                );
                result.insert(key.clone(), v);
            }
            Pattern::Bind { inner, .. } => {
                let v = instantiate_walk(inner, bindings, instantiation, edges);
                result.insert(key.clone(), v);
            }
        }
    }
    Value::Map(result)
}

/// Rename map keys recursively according to `key_map`.
fn remap_keys(value: &Value, key_map: &IndexMap<Key, Key>) -> Value {
    match value {
        Value::Map(m) => {
            let mut renamed = StateMap::new();
            for (k, v) in m {
                let new_key = key_map.get(k).cloned().unwrap_or_else(|| k.clone());
                renamed.insert(new_key, remap_keys(v, key_map));
            }
            Value::Map(renamed)
        }
        _ => value.clone(),
    }
}

// ── Firing ──────────────────────────────────────────────────────────

/// A path-localized update produced by firing a rule.
///
/// Instead of overwriting the whole subtree, we emit one diff:
/// `removed_keys` are dropped from the map at `path`, and `added`
/// (a Value::Map of new entries) is merged in. Smaller deltas mean
/// Gillespie τ-leap stays fast even on large state trees.
#[derive(Clone, Debug)]
pub struct FireUpdate {
    pub path: Path,
    pub removed_keys: Vec<Key>,
    pub added: Value,
    pub label: String,
}

/// Fire a rule at a specific match.
///
/// Returns `Some(FireUpdate)` describing the localized change, or
/// `None` if instantiation produced nothing (shouldn't happen for
/// well-formed rules).
pub fn fire_rule_at(rule: &ReactionRule, m: &Match) -> Option<FireUpdate> {
    let instantiation = rule.resolved_instantiation();
    let replacement = instantiate(&rule.reactum, &m.bindings, &instantiation);

    // Reactum keys are the pattern's labels; rename them to the
    // original state keys captured during matching.
    let actual_map: IndexMap<Key, Key> = m
        .bindings
        .key_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let renamed = remap_keys(&replacement, &actual_map);

    let added = match renamed {
        Value::Map(_) => renamed,
        _ => return None,
    };

    // Removed keys = state keys consumed by the redex (everything in
    // key_map's range).
    let removed_keys: Vec<Key> = m.bindings.key_map.values().cloned().collect();

    Some(FireUpdate {
        path: m.path.clone(),
        removed_keys,
        added,
        label: rule.label.clone(),
    })
}

/// Find the first match (deterministic mode) and fire.
pub fn fire_rule(
    state: &Value,
    rule: &ReactionRule,
    control_status: Option<&ControlStatus>,
) -> Option<FireUpdate> {
    let matches = find_matches(state, &rule.redex, control_status);
    let m = matches.first()?;
    fire_rule_at(rule, m)
}

/// Apply a [`FireUpdate`] to a `Value`, returning the new value.
///
/// At the match path, removes the consumed keys then merges the
/// reactum's added keys. Used by the BRS process and by tests.
pub fn apply_fire(state: &Value, update: &FireUpdate) -> Value {
    let mut next = state.clone();
    let parent: &mut Value = if update.path.is_empty() {
        &mut next
    } else {
        match next.get_path_mut(&update.path) {
            Some(p) => p,
            None => return next,
        }
    };
    if let Some(map) = parent.as_map_mut() {
        for k in &update.removed_keys {
            map.shift_remove(k);
        }
        if let Value::Map(add_map) = &update.added {
            for (k, v) in add_map {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    next
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn val_str(s: &str) -> Value {
        Value::String(s.to_string())
    }

    #[test]
    fn named_field_matches_by_name_not_position() {
        // `Cell[mass: ?m]`: `mass` must bind the float field BY NAME —
        // regardless of where `mass` sits among the node's fields, and with
        // unmentioned fields (`body`, `a`, `b`, `c`) tolerated. This guards
        // against the matcher reverting to positional/surplus binding (which
        // would capture a `{mass, body}` dict and break guards on `?m`).
        let check = |cell: Value, label: &str| {
            let redex = Pattern::map([(
                "cell",
                Pattern::sort("Cell", [("mass", Pattern::site())]),
            )]);
            let state = Value::tree([("c0", cell)]);
            let matches = find_matches(&state, &redex, None);
            assert_eq!(matches.len(), 1, "{label}: should match Cell[mass]");
            assert_eq!(
                matches[0].bindings.sites.get("mass"),
                Some(&Value::float(2.5)),
                "{label}: `mass` must bind the bare float 2.5 by NAME, not position"
            );
        };

        check(
            Value::tree([
                ("_type", val_str("Cell")),
                ("mass", Value::float(2.5)),
                ("body", Value::tree([("x", Value::float(9.0))])),
            ]),
            "mass-first",
        );
        check(
            Value::tree([
                ("_type", val_str("Cell")),
                ("body", Value::tree([("x", Value::float(9.0))])),
                ("mass", Value::float(2.5)),
            ]),
            "mass-last",
        );
        check(
            Value::tree([
                ("_type", val_str("Cell")),
                ("a", Value::float(1.0)),
                ("b", Value::float(2.0)),
                ("mass", Value::float(2.5)),
                ("c", Value::float(3.0)),
            ]),
            "mass-middle-of-5",
        );
    }

    #[test]
    fn site_matches_any_subtree() {
        // redex: { container: { _type: Box, body: Site } }
        let redex = Pattern::map([(
            "container",
            Pattern::sort("Box", [("body", Pattern::site())]),
        )]);

        let state = Value::tree([(
            "outer",
            Value::tree([
                ("_type", val_str("Box")),
                ("body", Value::Int(42)),
            ]),
        )]);

        let matches = find_matches(&state, &redex, None);
        assert_eq!(matches.len(), 1, "should match the Box at root level");
        assert_eq!(matches[0].path, Vec::<Key>::new());
        assert_eq!(
            matches[0].bindings.sites.get("body"),
            Some(&Value::Int(42))
        );
        // Redex key `container` was assigned to state key `outer`.
        assert_eq!(
            matches[0].bindings.key_map.get("container"),
            Some(&Key::from("outer"))
        );
    }

    #[test]
    fn linkvar_binds_then_constrains() {
        // redex: { a: { outputs: { p: LinkVar(e) } }, b: { outputs: { p: LinkVar(e) } } }
        // — both ports must wire to the same path.
        let redex = Pattern::map([
            (
                "a",
                Pattern::map([(
                    "outputs",
                    Pattern::map([("p", Pattern::link_var("e"))]),
                )]),
            ),
            (
                "b",
                Pattern::map([(
                    "outputs",
                    Pattern::map([("p", Pattern::link_var("e"))]),
                )]),
            ),
        ]);

        let wire = Value::List(vec![val_str("_edges"), val_str("~e_0")]);
        let state = Value::tree([
            (
                "mek",
                Value::tree([(
                    "outputs",
                    Value::tree([("p", wire.clone())]),
                )]),
            ),
            (
                "erk",
                Value::tree([(
                    "outputs",
                    Value::tree([("p", wire.clone())]),
                )]),
            ),
        ]);
        let matches = find_matches(&state, &redex, None);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].bindings.edges.get("e"), Some(&wire));
    }

    #[test]
    fn absent_blocks_when_present() {
        // redex: { x: ERK { outputs: Absent } } — the _type tag prevents
        // spurious permutation matches against unrelated subtrees, so
        // this is the realistic shape for negative application conditions.
        let redex = Pattern::map([(
            "x",
            Pattern::sort("ERK", [("outputs", Pattern::absent())]),
        )]);

        let state_free = Value::tree([(
            "x",
            Value::tree([
                ("_type", val_str("ERK")),
                ("name", val_str("n")),
            ]),
        )]);
        let state_bound = Value::tree([(
            "x",
            Value::tree([
                ("_type", val_str("ERK")),
                (
                    "outputs",
                    Value::tree([("p", Value::List(vec![val_str("e")]))]),
                ),
            ]),
        )]);

        assert_eq!(find_matches(&state_free, &redex, None).len(), 1);
        assert_eq!(find_matches(&state_bound, &redex, None).len(), 0);
    }

    #[test]
    fn surplus_absorbed_by_last_site() {
        // redex has 1 explicit Site that should absorb the surplus state keys.
        let redex = Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    ("enzyme", Pattern::sort("MEK", Vec::<(&str, Pattern)>::new())),
                    ("rest", Pattern::site()),
                ],
            ),
        )]);

        let state = Value::tree([(
            "cyto",
            Value::tree([
                ("_type", val_str("Compartment")),
                ("mek", Value::tree([("_type", val_str("MEK"))])),
                ("erk1", Value::tree([("_type", val_str("ERK"))])),
                ("erk2", Value::tree([("_type", val_str("ERK"))])),
            ]),
        )]);

        let matches = find_matches(&state, &redex, None);
        assert_eq!(matches.len(), 1);
        let rest = matches[0].bindings.sites.get("rest").unwrap();
        let rest_map = rest.as_map().unwrap();
        assert!(rest_map.contains_key("erk1"));
        assert!(rest_map.contains_key("erk2"));
    }

    #[test]
    fn fire_emits_localized_delta_and_apply_works() {
        // Rule: rename ERK → pERK, keep other keys via Site.
        let rule = ReactionRule::new(
            Pattern::map([(
                "compartment",
                Pattern::sort(
                    "Compartment",
                    [
                        ("substrate", Pattern::sort("ERK", Vec::<(&str, Pattern)>::new())),
                        ("rest", Pattern::site()),
                    ],
                ),
            )]),
            Pattern::map([(
                "compartment",
                Pattern::sort(
                    "Compartment",
                    [
                        ("substrate", Pattern::sort("pERK", Vec::<(&str, Pattern)>::new())),
                        ("rest", Pattern::site()),
                    ],
                ),
            )]),
        )
        .with_label("phos");

        let state = Value::tree([(
            "cyto",
            Value::tree([
                ("_type", val_str("Compartment")),
                ("erk1", Value::tree([("_type", val_str("ERK"))])),
                ("erk2", Value::tree([("_type", val_str("ERK"))])),
            ]),
        )]);

        let matches = find_matches(&state, &rule.redex, None);
        assert_eq!(matches.len(), 1);
        let update = fire_rule_at(&rule, &matches[0]).expect("fire");

        // Path-localized: change happens at the root level (where the
        // compartment lives), and `cyto` is removed and re-added with
        // the modified content.
        assert_eq!(update.path, Vec::<Key>::new());
        assert!(update.removed_keys.contains(&Key::from("cyto")));

        let next = apply_fire(&state, &update);
        let cyto = next.get_path(&[Key::from("cyto")]).unwrap();
        // One of {erk1, erk2} is now pERK; the other is still ERK.
        let cm = cyto.as_map().unwrap();
        let p_count = ["erk1", "erk2"]
            .iter()
            .filter(|k| {
                cm.get(**k)
                    .and_then(|v| v.get_field("_type"))
                    .and_then(|t| t.as_str())
                    == Some("pERK")
            })
            .count();
        assert_eq!(p_count, 1, "exactly one ERK became pERK");
    }

    #[test]
    fn activity_blocks_passive_ancestor() {
        let redex = Pattern::map([(
            "x",
            Pattern::sort("Atom", Vec::<(&str, Pattern)>::new()),
        )]);
        // State: { passive_box: { _type: Box, x: { _type: Atom } } }
        let state = Value::tree([(
            "passive_box",
            Value::tree([
                ("_type", val_str("Box")),
                ("x", Value::tree([("_type", val_str("Atom"))])),
            ]),
        )]);

        let mut status = ControlStatus::new();
        status.insert(Key::from("Box"), Activity::Passive);

        let matches = find_matches(&state, &redex, Some(&status));
        assert!(matches.is_empty(), "passive ancestor must block the match");

        // Without status, the same redex matches inside the Box.
        let matches = find_matches(&state, &redex, None);
        assert!(!matches.is_empty());
    }
}
