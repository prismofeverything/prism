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

use std::sync::Arc;
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

/// A guard predicate over a match's [`Bindings`]: the rule only fires
/// where this returns `true`. Lets a caller (e.g. chrysalis) attach a
/// `where` clause without prism knowing the surface language.
pub type GuardFn = Arc<dyn Fn(&Bindings) -> bool + Send + Sync>;

/// A computed (parametric) reactum: instead of the structural `reactum`
/// pattern, the replacement is a *function of the match*. It returns a
/// delta `Value` — either an explicit `{_remove: [...], _add: {...}}`
/// map (localized at the match path) or a bare map treated as `_add`.
/// This is the general Milner-parametric form where the instantiation is
/// a function; chrysalis uses it for computed reactums like `?c.divide()`.
pub type ReactumFn = Arc<dyn Fn(&Bindings) -> Value + Send + Sync>;

/// A rate expression over a match's [`Bindings`] (propensity that can
/// depend on the matched contents), overriding the constant `rate`.
pub type RateFn = Arc<dyn Fn(&Bindings) -> f64 + Send + Sync>;

/// A parametric reaction rule (Milner Def. 8.5).
///
/// The base form is a structural `redex → reactum` pattern rewrite.
/// Three optional closures generalize it without prism depending on any
/// surface language: a [`GuardFn`] (firing condition), a [`ReactumFn`]
/// (computed reactum), and a [`RateFn`] (computed propensity).
#[derive(Clone)]
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
    /// Optional firing guard: a predicate over the match bindings.
    pub guard: Option<GuardFn>,
    /// Optional computed reactum: replaces structural `instantiate` with
    /// a function of the match (see [`ReactumFn`]).
    pub reactum_fn: Option<ReactumFn>,
    /// Optional computed propensity over the match bindings.
    pub rate_fn: Option<RateFn>,
}

impl std::fmt::Debug for ReactionRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReactionRule")
            .field("label", &self.label)
            .field("redex", &self.redex)
            .field("reactum", &self.reactum)
            .field("instantiation", &self.instantiation)
            .field("rate", &self.rate)
            .field("guard", &self.guard.as_ref().map(|_| "<fn>"))
            .field("reactum_fn", &self.reactum_fn.as_ref().map(|_| "<fn>"))
            .field("rate_fn", &self.rate_fn.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

impl ReactionRule {
    pub fn new(redex: Pattern, reactum: Pattern) -> Self {
        Self {
            redex,
            reactum,
            instantiation: IndexMap::new(),
            rate: None,
            label: String::new(),
            guard: None,
            reactum_fn: None,
            rate_fn: None,
        }
    }

    pub fn with_rate(mut self, rate: f64) -> Self {
        self.rate = Some(rate);
        self
    }

    /// Attach a firing guard (a predicate over the match bindings).
    pub fn with_guard(mut self, guard: GuardFn) -> Self {
        self.guard = Some(guard);
        self
    }

    /// Attach a computed reactum (a function of the match producing a
    /// delta value), replacing the structural `reactum` pattern.
    pub fn with_reactum_fn(mut self, reactum_fn: ReactumFn) -> Self {
        self.reactum_fn = Some(reactum_fn);
        self
    }

    /// Attach a computed propensity (a function of the match bindings).
    pub fn with_rate_fn(mut self, rate_fn: RateFn) -> Self {
        self.rate_fn = Some(rate_fn);
        self
    }

    /// Does this rule's guard admit `bindings`? `true` when there is no
    /// guard.
    pub fn passes_guard(&self, bindings: &Bindings) -> bool {
        match &self.guard {
            Some(g) => g(bindings),
            None => true,
        }
    }

    /// The propensity for a given match: `rate_fn` if present, else the
    /// constant `rate`, else `1.0`.
    pub fn propensity(&self, bindings: &Bindings) -> f64 {
        if let Some(rf) = &self.rate_fn {
            rf(bindings)
        } else {
            self.rate.unwrap_or(1.0)
        }
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

// ── Reaction as data — Pattern / ReactionRule ↔ Value (#61b) ──────────────
//
// A STRUCTURAL reaction (closure-free) is pure data: `redex`/`reactum` Patterns
// + an instantiation map + a label/rate. These codecs render it to a JSON-able
// `Value` so a reaction crosses a `rest:`/`stream:` bridge (or any serde
// boundary) and is reconstructed on the far side. The payoff: a `map[Reaction]`
// link — now schema-typed (#61a) — transports through the SAME schema-driven
// `serialize`/`apply` path as any other typed slot, no bridge special-case.
//
// Each node is tagged `_pat` — distinct from `_type` (which appears INSIDE a
// `Pattern::Map` as genuine sort content) and from the chrysalis SOURCE form
// `{_type: "Rule"}` (an `Expr` tree). Two representations of a reaction coexist
// by design: authored source (the Expr form `compile_reaction_value` reads) vs
// compiled structural rule (this form `from_data_value` reads). The runnable
// `Foreign(FOREIGN_REACTION, ReactionRule)` carrier can't cross a wire; this
// data form is what does.

/// Error from [`Pattern::from_value`] / [`ReactionRule::from_data_value`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternFromValue(pub String);

impl std::fmt::Display for PatternFromValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "reaction from value: {}", self.0)
    }
}
impl std::error::Error for PatternFromValue {}

fn pat_err(msg: impl Into<String>) -> PatternFromValue {
    PatternFromValue(msg.into())
}

impl Pattern {
    /// Serialize this pattern to a JSON-able data `Value` (the structural wire
    /// form). Inverse of [`Pattern::from_value`]; faithful + lossless for the
    /// closure-free structural patterns that cross a wire.
    pub fn to_value(&self) -> Value {
        let tagged = |variant: &str, fields: Vec<(&str, Value)>| -> Value {
            let mut m: StateMap = IndexMap::new();
            m.insert(Key::from("_pat"), Value::String(variant.to_string()));
            for (k, v) in fields {
                m.insert(Key::from(k), v);
            }
            Value::Map(m)
        };
        match self {
            Pattern::Site => tagged("Site", vec![]),
            Pattern::LinkVar(k) => tagged("LinkVar", vec![("name", Value::String(k.to_string()))]),
            Pattern::Absent => tagged("Absent", vec![]),
            Pattern::Atom(v) => tagged("Atom", vec![("value", v.clone())]),
            Pattern::Map(entries) => {
                let mut m: StateMap = IndexMap::new();
                for (k, p) in entries {
                    m.insert(k.clone(), p.to_value());
                }
                tagged("Map", vec![("entries", Value::Map(m))])
            }
            Pattern::List(items) => tagged(
                "List",
                vec![("items", Value::List(items.iter().map(Pattern::to_value).collect()))],
            ),
            Pattern::Bind { name, inner } => tagged(
                "Bind",
                vec![
                    ("name", Value::String(name.to_string())),
                    ("inner", inner.to_value()),
                ],
            ),
        }
    }

    /// Reconstruct a pattern from its [`Pattern::to_value`] data form.
    pub fn from_value(v: &Value) -> Result<Pattern, PatternFromValue> {
        let m = v.as_map().ok_or_else(|| pat_err("a pattern is a `_pat`-tagged map"))?;
        let tag = m
            .get("_pat")
            .and_then(|t| t.as_str())
            .ok_or_else(|| pat_err("a pattern map needs a `_pat` tag"))?;
        let field = |k: &str| m.get(k).ok_or_else(|| pat_err(format!("`{tag}` needs `{k}`")));
        match tag {
            "Site" => Ok(Pattern::Site),
            "Absent" => Ok(Pattern::Absent),
            "LinkVar" => Ok(Pattern::LinkVar(Key::from(
                field("name")?.as_str().ok_or_else(|| pat_err("LinkVar `name` is a string"))?,
            ))),
            "Atom" => Ok(Pattern::Atom(field("value")?.clone())),
            "Map" => {
                let entries = field("entries")?
                    .as_map()
                    .ok_or_else(|| pat_err("Map `entries` is a map"))?;
                let mut out: IndexMap<Key, Pattern> = IndexMap::new();
                for (k, p) in entries {
                    out.insert(k.clone(), Pattern::from_value(p)?);
                }
                Ok(Pattern::Map(out))
            }
            "List" => {
                let items = field("items")?
                    .as_list()
                    .ok_or_else(|| pat_err("List `items` is a list"))?;
                Ok(Pattern::List(
                    items.iter().map(Pattern::from_value).collect::<Result<_, _>>()?,
                ))
            }
            "Bind" => Ok(Pattern::Bind {
                name: Key::from(
                    field("name")?.as_str().ok_or_else(|| pat_err("Bind `name` is a string"))?,
                ),
                inner: Box::new(Pattern::from_value(field("inner")?)?),
            }),
            other => Err(pat_err(format!("unknown `_pat` tag `{other}`"))),
        }
    }
}

impl ReactionRule {
    /// Serialize a STRUCTURAL rule to a JSON-able data `Value`. `None` if the
    /// rule carries a closure (`guard` / `reactum_fn` / `rate_fn`) — those can't
    /// cross a wire, so the caller keeps the runnable form for in-process use.
    /// Wire form: `{_pat: "Rule", label, redex, reactum, instantiation?, rate?}`.
    pub fn to_data_value(&self) -> Option<Value> {
        if self.guard.is_some() || self.reactum_fn.is_some() || self.rate_fn.is_some() {
            return None;
        }
        let mut m: StateMap = IndexMap::new();
        m.insert(Key::from("_pat"), Value::String("Rule".to_string()));
        m.insert(Key::from("label"), Value::String(self.label.clone()));
        m.insert(Key::from("redex"), self.redex.to_value());
        m.insert(Key::from("reactum"), self.reactum.to_value());
        if !self.instantiation.is_empty() {
            let mut inst: StateMap = IndexMap::new();
            for (k, v) in &self.instantiation {
                inst.insert(k.clone(), Value::String(v.to_string()));
            }
            m.insert(Key::from("instantiation"), Value::Map(inst));
        }
        if let Some(rate) = self.rate {
            m.insert(Key::from("rate"), Value::Float(rate.into()));
        }
        Some(Value::Map(m))
    }

    /// Reconstruct a structural rule from its [`ReactionRule::to_data_value`]
    /// form (the closure fields stay `None` — the wire carries no closures).
    pub fn from_data_value(v: &Value) -> Result<ReactionRule, PatternFromValue> {
        let m = v.as_map().ok_or_else(|| pat_err("a rule is a `_pat: \"Rule\"` map"))?;
        if m.get("_pat").and_then(|t| t.as_str()) != Some("Rule") {
            return Err(pat_err("rule data needs `_pat: \"Rule\"`"));
        }
        let redex = Pattern::from_value(m.get("redex").ok_or_else(|| pat_err("Rule needs `redex`"))?)?;
        let reactum =
            Pattern::from_value(m.get("reactum").ok_or_else(|| pat_err("Rule needs `reactum`"))?)?;
        let mut rule = ReactionRule::new(redex, reactum);
        if let Some(label) = m.get("label").and_then(|v| v.as_str()) {
            rule.label = label.to_string();
        }
        if let Some(inst) = m.get("instantiation").and_then(|v| v.as_map()) {
            rule.instantiation = inst
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), Key::from(s))))
                .collect();
        }
        if let Some(rate) = m.get("rate").and_then(|v| v.as_f64()) {
            rule.rate = Some(rate);
        }
        Ok(rule)
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
        // A `Map` redex matches the node's fields by name/structure; a `List`
        // redex (anonymous parallel `a | b`) matches a SUB-MULTISET of the
        // node's children — "an F and a B somewhere in this soup", rest
        // tolerated. Both report at this node's path.
        let matched = match redex {
            Pattern::Map(redex_map) => match_against_map(node, redex_map, &mut bindings),
            Pattern::List(items) => match_list_in_container(node, items, &mut bindings),
            _ => false,
        };
        if matched {
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

// ── List (anonymous-parallel) sub-multiset matching ─────────────────

/// Match a `List` redex (anonymous parallel `a | b | …`) against a
/// container node as a SUB-MULTISET: assign each item pattern to a
/// *distinct* non-`_` child, leaving the rest untouched (a redex names
/// the reactants it consumes, not the whole soup). Records each matched
/// child's key in `key_map` (so firing can consume exactly those) plus any
/// site / edge / as-pattern captures. Unlike a `Map` redex, items have no
/// keys, so two items capturing the same field don't collide — use
/// as-patterns (`?f::F`) to bind whole ions under distinct names.
fn match_list_in_container(node: &Value, items: &[Pattern], bindings: &mut Bindings) -> bool {
    let children: Vec<(Key, &Value)> = match node.iter_fields() {
        Some(it) => it
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, v)| (k.clone(), v))
            .collect(),
        None => return false,
    };
    if items.len() > children.len() {
        return false;
    }
    let mut used = vec![false; children.len()];
    assign_list_items(0, items, &children, &mut used, bindings)
}

fn assign_list_items(
    i: usize,
    items: &[Pattern],
    children: &[(Key, &Value)],
    used: &mut [bool],
    bindings: &mut Bindings,
) -> bool {
    if i == items.len() {
        return true;
    }
    let saved = bindings.clone();
    for j in 0..children.len() {
        if used[j] {
            continue;
        }
        let (ckey, cval) = &children[j];
        // The child key doubles as the redex key, so a bare `Site` item
        // captures under the (distinct) child key rather than colliding.
        if try_pair(ckey, &items[i], ckey, cval, false, bindings) {
            bindings
                .key_map
                .entry(ckey.clone())
                .or_insert_with(|| ckey.clone());
            used[j] = true;
            if assign_list_items(i + 1, items, children, used, bindings) {
                return true;
            }
            used[j] = false;
        }
        *bindings = saved.clone();
    }
    false
}

// ── Instantiation ───────────────────────────────────────────────────

static FRESH_EDGE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn gensym_edge() -> String {
    let n = FRESH_EDGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("~e_{n}")
}

static FRESH_NODE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh container key for a multiset reaction's product (`a | b => c | d`),
/// used by [`localize_fire`]. Prefix `g` (NOT `_`, which marks meta keys the
/// matcher / discovery skip) — the same convention as chrysalis's `fresh_id`.
fn gensym_node() -> String {
    let n = FRESH_NODE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("g{n}")
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
    /// The localized delta at `path` — a `{_remove, _add, _divide, …}` map (or
    /// any value `apply` accepts). [`apply_fire`] enacts it through the
    /// SCHEMA-AWARE algebra apply (the SAME mechanism the engine uses), so a
    /// `_divide` sentinel splits via `divide_by_schema`, numeric leaves add,
    /// `_add` entries realize, etc. Fire-application IS algebra apply — there is
    /// no schemaless structural mutator that would silently drop a sentinel.
    pub delta: Value,
    pub label: String,
}

/// Fire a rule at a specific match.
///
/// Returns `Some(FireUpdate)` describing the localized change, or
/// `None` if instantiation produced nothing (shouldn't happen for
/// well-formed rules).
pub fn fire_rule_at(rule: &ReactionRule, m: &Match) -> Option<FireUpdate> {
    // Computed (parametric) reactum: the replacement is a function of the
    // match, returning a delta value directly. The caller owns which keys
    // to remove (it knows the surface-level binding roles), so prism reads
    // the explicit `_remove`/`_add` sentinels rather than guessing from
    // key_map. See [`ReactumFn`].
    if let Some(reactum_fn) = &rule.reactum_fn {
        let produced = reactum_fn(&m.bindings);
        return Some(computed_fire_update(rule, m, produced));
    }

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

    // Removed keys = state keys consumed by the redex (everything in key_map's
    // range). The keying — what's removed, how the products are keyed — runs
    // through [`localize_fire`], the ONE convention shared with the chrysalis
    // computed path (`reaction_delta`). A LIST reactum (a multiset reaction
    // `a | b => c | d`) thereby FIRES (fresh-keyed products) instead of silently
    // no-op'ing, exactly as the computed path does.
    let removed: Vec<Value> = m
        .bindings
        .key_map
        .values()
        .map(|k| Value::String(k.to_string()))
        .collect();

    Some(FireUpdate {
        path: m.path.clone(),
        delta: localize_fire(removed, renamed),
        label: rule.label.clone(),
    })
}

/// Localize a fired reactum into a `{_remove, _add, …}` delta — THE single
/// keying convention, shared by the STRUCTURAL path ([`fire_rule_at`]) and the
/// chrysalis COMPUTED path (`reaction_delta`). It consumes `removed` (the matched
/// keys the reaction replaces) and keys the `products`:
///
/// - a **`List`** → a multiset of products under FRESH keys (`a | b => c | d`);
/// - a **`Map` carrying `_add`/`_remove`** sentinels → passed through (the
///   reactum authored its own delta); its `_remove` overrides `removed`;
/// - any other **`Map`** → added wholesale (named products, MAPK-style);
/// - a scalar / other → added as the single product.
///
/// Both firing paths feed `(removed, products)` here, so a reaction's keying is
/// its meaning — not an artifact of the structural-vs-computed classification.
pub fn localize_fire(removed: Vec<Value>, products: Value) -> Value {
    let mut delta: StateMap = StateMap::new();
    if !removed.is_empty() {
        delta.insert(Key::from("_remove"), Value::List(removed));
    }
    match products {
        // Multiset products → fresh keys (Milner: the reactum's ions are fresh).
        Value::List(items) => {
            let mut add: StateMap = StateMap::new();
            for item in items {
                add.insert(Key::from(gensym_node().as_str()), item);
            }
            delta.insert(Key::from("_add"), Value::Map(add));
        }
        // The reactum authored its own delta (`{_remove, _add, _divide, …}`):
        // pass the sentinels through; its `_remove` overrides the matched keys.
        Value::Map(mut m) if m.contains_key("_add") || m.contains_key("_remove") => {
            if let Some(rem) = m.shift_remove("_remove") {
                delta.insert(Key::from("_remove"), rem);
            }
            if let Some(add) = m.shift_remove("_add") {
                delta.insert(Key::from("_add"), add);
            }
            for (k, v) in m {
                delta.insert(k, v);
            }
        }
        // Named products (a MAPK-style template / a `{key: value}` reactum).
        Value::Map(m) => {
            delta.insert(Key::from("_add"), Value::Map(m));
        }
        other => {
            delta.insert(Key::from("_add"), other);
        }
    }
    Value::Map(delta)
}

/// Build a [`FireUpdate`] from a computed reactum's produced delta.
///
/// The produced value IS the localized delta at the match path — it already
/// carries its own `_remove`/`_add`/`_divide` sentinels (set by the surface
/// firing convention). [`apply_fire`] enacts it through the schema-aware algebra
/// apply, so every sentinel is handled by the one apply mechanism (no bespoke
/// extraction here that could drop a sentinel like `_divide`).
fn computed_fire_update(rule: &ReactionRule, m: &Match, produced: Value) -> FireUpdate {
    FireUpdate {
        path: m.path.clone(),
        delta: produced,
        label: rule.label.clone(),
    }
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
/// Enacts the localized delta at the match path through the ONE schema-aware
/// algebra apply — the SAME mechanism the engine uses — so every sentinel
/// (`_remove`/`_add`/`_divide`) is handled uniformly. The schema is inferred
/// from the live subtree; `infer` honors `_type` brands, so a `_divide` over
/// branded cells (`_type: Cell`) resolves `Cell → CompositeLink` via the
/// registry and splits by `divide_by_schema` (conserving mass across the
/// dividing tick). `registry` (the Core's `TypeRegistry`, threaded via the
/// firing process's `set_core`) is required for `Custom`-typed splits; pass
/// `None` for purely structural fires. There is NO schemaless structural path
/// that could silently drop a sentinel.
pub fn apply_fire(
    state: &Value,
    update: &FireUpdate,
    registry: Option<&crate::registry::TypeRegistry>,
) -> Value {
    let mut next = state.clone();
    let slot: &mut Value = if update.path.is_empty() {
        &mut next
    } else {
        match next.get_path_mut(&update.path) {
            Some(p) => p,
            None => return next,
        }
    };
    // A registry-driven sentinel (`_divide`) needs the SCHEMA-AWARE algebra
    // apply: it brand-resolves the target (`_type: Cell → CompositeLink`) and
    // splits by `divide_by_schema`, conserving mass when the engine applies it
    // late alongside the same-tick growth. A purely structural delta
    // (`_remove`/`_add` of opaque children) applies directly — the proven path
    // that never re-infers a child's type, so it can't degrade an opaque node.
    let cur = slot.clone();
    let schema = crate::algebra::infer(&cur);
    *slot = crate::algebra::apply_with(registry, &schema, &cur, &update.delta);
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
        let removed = update
            .delta
            .get_field("_remove")
            .and_then(|v| v.as_list())
            .map(|l| l.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
            .unwrap_or_default();
        assert!(removed.iter().any(|k| k == "cyto"));

        let next = apply_fire(&state, &update, None);
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

    #[test]
    fn computed_reactum_with_guard_fires_explicit_delta() {
        // A divide-style rule expressed with the closure hooks (the shape
        // chrysalis produces): bind the Cell entry's key + its mass; guard
        // on mass > 2; computed reactum removes the matched key and adds two
        // daughters keyed `<key>_0` / `<key>_1`. No structural reactum.
        let redex = Pattern::map([(
            "cell",
            Pattern::sort("Cell", [("mass", Pattern::site())]),
        )]);

        let guard: GuardFn = Arc::new(|b: &Bindings| {
            b.sites
                .get("mass")
                .and_then(|v| v.as_f64())
                .map(|m| m > 2.0)
                .unwrap_or(false)
        });

        let reactum_fn: ReactumFn = Arc::new(|b: &Bindings| {
            let key = b
                .key_map
                .get("cell")
                .map(|k| k.to_string())
                .unwrap_or_default();
            let mass = b.sites.get("mass").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let daughter = |suffix: &str| {
                Value::tree([
                    ("_type", val_str("Cell")),
                    ("mass", Value::float(mass / 2.0)),
                    ("id", val_str(&format!("{key}{suffix}"))),
                ])
            };
            let mut add = StateMap::new();
            add.insert(Key::from(format!("{key}_0").as_str()), daughter("_0"));
            add.insert(Key::from(format!("{key}_1").as_str()), daughter("_1"));
            Value::tree([
                ("_remove", Value::List(vec![val_str(&key)])),
                ("_add", Value::Map(add)),
            ])
        });

        let rule = ReactionRule::new(redex, Pattern::Site)
            .with_label("divide")
            .with_guard(guard)
            .with_reactum_fn(reactum_fn);

        // Above threshold → fires, splitting the cell into two daughters.
        let big = Value::tree([(
            "0",
            Value::tree([("_type", val_str("Cell")), ("mass", Value::float(3.0))]),
        )]);
        let matches = find_matches(&big, &rule.redex, None);
        assert_eq!(matches.len(), 1);
        assert!(rule.passes_guard(&matches[0].bindings));
        let upd = fire_rule_at(&rule, &matches[0]).expect("fire");
        assert_eq!(
            upd.delta
                .get_field("_remove")
                .and_then(|v| v.as_list())
                .map(<[Value]>::to_vec),
            Some(vec![Value::String("0".into())]),
        );
        let after = apply_fire(&big, &upd, None);
        let am = after.as_map().unwrap();
        assert!(!am.contains_key("0"), "mother removed");
        assert!(am.contains_key("0_0") && am.contains_key("0_1"), "two daughters");
        assert_eq!(
            am.get("0_0").and_then(|v| v.get_field("mass")).and_then(|v| v.as_f64()),
            Some(1.5),
            "daughter mass halved by the computed reactum"
        );

        // Below threshold → the guard blocks it.
        let small = Value::tree([(
            "0",
            Value::tree([("_type", val_str("Cell")), ("mass", Value::float(1.0))]),
        )]);
        let matches = find_matches(&small, &rule.redex, None);
        assert_eq!(matches.len(), 1);
        assert!(!rule.passes_guard(&matches[0].bindings), "guard blocks mass 1.0");
    }

    #[test]
    fn list_redex_matches_submultiset() {
        // `F | B` (anonymous parallel) matches a soup containing an F and a B,
        // consuming exactly those two — the bystander G is left alone.
        let redex = Pattern::list([
            Pattern::sort("F", Vec::<(&str, Pattern)>::new()),
            Pattern::sort("B", Vec::<(&str, Pattern)>::new()),
        ]);
        let state = Value::tree([
            ("x", Value::tree([("_type", val_str("F"))])),
            ("y", Value::tree([("_type", val_str("B"))])),
            ("z", Value::tree([("_type", val_str("G"))])),
        ]);
        let matches = find_matches(&state, &redex, None);
        assert!(!matches.is_empty(), "F|B should match the soup");
        let consumed: std::collections::HashSet<Key> =
            matches[0].bindings.key_map.values().cloned().collect();
        assert!(
            consumed.contains(&Key::from("x")) && consumed.contains(&Key::from("y")),
            "the F and the B are consumed; got {consumed:?}"
        );
        assert!(
            !consumed.contains(&Key::from("z")),
            "the bystander G is not consumed"
        );

        // No B present → no match.
        let no_b = Value::tree([("x", Value::tree([("_type", val_str("F"))]))]);
        assert!(find_matches(&no_b, &redex, None).is_empty());
    }

    #[test]
    fn list_redex_as_patterns_capture_distinct_ions() {
        // `?f::F | ?b::B` binds the two whole ions under DISTINCT names — so a
        // guard can compare `?f.blueprint` vs `?b.blueprint` without the
        // same-field-name collision a bare `blueprint` site would suffer.
        let redex = Pattern::list([
            Pattern::Bind {
                name: Key::from("?f"),
                inner: Box::new(Pattern::sort("F", Vec::<(&str, Pattern)>::new())),
            },
            Pattern::Bind {
                name: Key::from("?b"),
                inner: Box::new(Pattern::sort("B", Vec::<(&str, Pattern)>::new())),
            },
        ]);
        let state = Value::tree([
            (
                "x",
                Value::tree([("_type", val_str("F")), ("blueprint", Value::Int(7))]),
            ),
            (
                "y",
                Value::tree([("_type", val_str("B")), ("blueprint", Value::Int(7))]),
            ),
        ]);
        let m = &find_matches(&state, &redex, None)[0];
        assert_eq!(
            m.bindings.sites.get("?f").and_then(|v| v.get_field("blueprint")),
            Some(&Value::Int(7)),
            "?f captured the whole F ion (with its blueprint)"
        );
        assert_eq!(
            m.bindings.sites.get("?b").and_then(|v| v.get_field("blueprint")),
            Some(&Value::Int(7)),
            "?b captured the whole B ion, distinctly from ?f"
        );
    }
}
