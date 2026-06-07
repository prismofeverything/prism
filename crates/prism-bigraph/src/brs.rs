//! Bigraphical Reactive System Process.
//!
//! A [`Process`] that fires Milner-style parametric reaction rules on
//! each tick. Mirrors Python's
//! `process_bigraph.processes.bigraphical_reactive_system.BigraphicalReactiveSystem`.
//!
//! ## Modes
//!
//! - **Deterministic**: the first matching rule's first match fires.
//!   At most one firing per tick. Useful for worked examples where rule
//!   order encodes intent.
//! - **Stochastic**: one firing per tick, sampled from all candidate
//!   matches weighted by `rule.rate * 1` (each match counts once).
//! - **Gillespie**: proper SSA τ-leap. Inter-firing waits are
//!   exponentially distributed with parameter `λ = Σ_i rate_i ·
//!   |matches_i|`. Zero or more firings per tick — the wall-clock
//!   interval bounds time, not count.
//!
//! ## Output format
//!
//! The process applies firings in-memory to a working copy of the
//! input subtree, then emits the structural difference between the
//! initial and final subtrees. The diff is a nested `Value::Map` that
//! reuses the `_add` / `_remove` sentinels the engine already
//! understands — so unchanged subtrees never appear in the output and
//! Gillespie stays cheap on large states.

use std::any::Any;
use std::sync::Mutex;

use indexmap::IndexMap;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use prism_schema::reaction::{
    ControlStatus, FireUpdate, Match, ReactionRule, apply_fire, find_matches, fire_rule_at,
};
use prism_schema::{FOREIGN_REACTION, Key, Schema, StateMap, Value};

use crate::Core;
use crate::ports::PortSchema;
use crate::process::Process;
use crate::update::Update;

/// Firing mode for the BRS process.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BrsMode {
    Deterministic,
    Stochastic,
    Gillespie,
}

impl BrsMode {
    fn parse(s: &str) -> Self {
        match s {
            "stochastic" => Self::Stochastic,
            "gillespie" => Self::Gillespie,
            _ => Self::Deterministic,
        }
    }
}

/// Record of one firing — kept on `fired_log` so callers can inspect
/// what happened in the last tick.
#[derive(Clone, Debug)]
pub struct FiredEvent {
    pub sim_time: f64,
    pub rule_label: String,
    pub match_path: Vec<Key>,
}

pub struct BigraphicalReactiveSystem {
    pub rules: Vec<ReactionRule>,
    pub mode: BrsMode,
    pub control_status: Option<ControlStatus>,
    pub max_per_tick: usize,
    pub interval: f64,
    rng: Mutex<StdRng>,
    fired_log: Mutex<Vec<FiredEvent>>,
    /// The WHOLE shared [`Core`], captured via `set_core` (not a `TypeRegistry`
    /// subset). `apply_fire` reads `core.types` so a registry-driven fire (e.g. a
    /// `_divide` over branded `_type: Cell` nodes) enacts SCHEMA-AWARE — splitting
    /// via the same `divide_by_schema(CompositeLink)` the engine/Divider use,
    /// conserving mass. Holding the full Core (types + processes + methods +
    /// protocols, all `Arc`s) keeps a *reaction* informed by the same shared Core
    /// as the engine: a reactum evaluated against it can introspect the available
    /// processes/types and emit specs the engine then instantiates — the
    /// generative / reflective direction (#59/#60). `None` until `set_core` runs
    /// (purely structural fires over plain controls need no Core).
    core: Option<Core>,
}

impl std::fmt::Debug for BigraphicalReactiveSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BigraphicalReactiveSystem")
            .field("rules", &self.rules.len())
            .field("mode", &self.mode)
            .field("interval", &self.interval)
            .field("max_per_tick", &self.max_per_tick)
            .finish()
    }
}

impl BigraphicalReactiveSystem {
    pub fn new(rules: Vec<ReactionRule>) -> Self {
        Self::with_config(rules, BrsMode::Deterministic, None, 0, None, 1.0)
    }

    pub fn with_config(
        rules: Vec<ReactionRule>,
        mode: BrsMode,
        control_status: Option<ControlStatus>,
        seed: u64,
        max_per_tick: Option<usize>,
        interval: f64,
    ) -> Self {
        let default_cap = if mode == BrsMode::Gillespie {
            usize::MAX
        } else {
            1
        };
        Self {
            rules,
            mode,
            control_status,
            max_per_tick: max_per_tick.unwrap_or(default_cap),
            interval,
            rng: Mutex::new(StdRng::seed_from_u64(seed)),
            fired_log: Mutex::new(Vec::new()),
            core: None,
        }
    }

    /// Builder form of [`set_core`](Process::set_core) — attach the shared
    /// [`Core`] at construction so a standalone BRS (one built outside the
    /// engine's discovery, e.g. in a test or a host that drives `update`
    /// directly) fires through the SAME Core, not a registry-less fallback.
    /// Chains: `BigraphicalReactiveSystem::with_config(..).with_core(core)`.
    pub fn with_core(mut self, core: Core) -> Self {
        self.core = Some(core);
        self
    }

    /// Build a BRS from a `Value` config (mode, seed, interval, max_per_tick).
    /// Rules must be supplied separately — they don't live in serializable
    /// config (they contain Patterns, not Values).
    pub fn from_config(rules: Vec<ReactionRule>, config: &Value) -> Self {
        let cmap = config.as_map();
        let mode = cmap
            .and_then(|m| m.get("mode"))
            .and_then(|v| v.as_str())
            .map(BrsMode::parse)
            .unwrap_or(BrsMode::Deterministic);
        let seed = cmap
            .and_then(|m| m.get("seed"))
            .and_then(|v| v.as_i64())
            .map(|i| i as u64)
            .unwrap_or(0);
        let interval = cmap
            .and_then(|m| m.get("interval"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        let max_per_tick = cmap
            .and_then(|m| m.get("max_per_tick"))
            .and_then(|v| v.as_i64())
            .map(|i| i as usize);
        Self::with_config(rules, mode, None, seed, max_per_tick, interval)
    }

    pub fn fired_log(&self) -> Vec<FiredEvent> {
        self.fired_log.lock().unwrap().clone()
    }

    /// Enumerate every candidate (rule_index, match, rate) over the
    /// current subtree, against the ACTIVE ruleset `rules` (seed rules ∪
    /// rules-as-state). Rates default to 1.0 if `rule.rate` is None.
    fn enumerate_candidates(&self, rules: &[ReactionRule], subtree: &Value) -> Vec<(usize, Match, f64)> {
        let mut out = Vec::new();
        let status = self.control_status.as_ref();
        for (i, rule) in rules.iter().enumerate() {
            for m in find_matches(subtree, &rule.redex, status) {
                // A rule's guard (if any) gates which matches are candidates.
                if !rule.passes_guard(&m.bindings) {
                    continue;
                }
                // Propensity may depend on the match (rate_fn) or be the
                // constant rate; defaults to 1.0.
                let rate = rule.propensity(&m.bindings);
                out.push((i, m, rate));
            }
        }
        out
    }

    /// Pick a candidate by Gillespie-style cumulative-weight sampling.
    fn pick_candidate<'a>(
        &self,
        candidates: &'a [(usize, Match, f64)],
    ) -> Option<&'a (usize, Match, f64)> {
        if candidates.is_empty() {
            return None;
        }
        let total: f64 = candidates.iter().map(|(_, _, r)| *r).sum();
        if total <= 0.0 {
            return None;
        }
        let pick = self.rng.lock().unwrap().r#gen::<f64>() * total;
        let mut cum = 0.0;
        for c in candidates {
            cum += c.2;
            if cum >= pick {
                return Some(c);
            }
        }
        candidates.last()
    }

    /// Fire one rule (deterministic or stochastic). Returns the new subtree (for
    /// multi-fire iteration), the fire's localized DELTA nested under its match
    /// path (emitted for the engine to apply schema-aware), and the fired event —
    /// or None if nothing fires.
    fn fire_one(
        &self,
        rules: &[ReactionRule],
        subtree: &Value,
        sim_time: f64,
    ) -> Option<(Value, Value, FiredEvent)> {
        match self.mode {
            BrsMode::Deterministic => {
                let status = self.control_status.as_ref();
                for rule in rules {
                    let matches = find_matches(subtree, &rule.redex, status);
                    // First match whose guard passes (a guard can reject the
                    // structurally-first match).
                    if let Some(m) = matches.into_iter().find(|m| rule.passes_guard(&m.bindings)) {
                        let upd = fire_rule_at(rule, &m)?;
                        let nested = nest_fire_delta(&upd);
                        let new_subtree = apply_fire(subtree, &upd, self.core.as_ref().map(|c| c.types.as_ref()));
                        return Some((
                            new_subtree,
                            nested,
                            FiredEvent {
                                sim_time,
                                rule_label: rule.label.clone(),
                                match_path: m.path.clone(),
                            },
                        ));
                    }
                }
                None
            }
            BrsMode::Stochastic | BrsMode::Gillespie => {
                let candidates = self.enumerate_candidates(rules, subtree);
                let (rule_idx, m, _rate) = self.pick_candidate(&candidates)?.clone();
                let rule = &rules[rule_idx];
                let upd = fire_rule_at(rule, &m)?;
                let nested = nest_fire_delta(&upd);
                let new_subtree = apply_fire(subtree, &upd, self.core.as_ref().map(|c| c.types.as_ref()));
                Some((
                    new_subtree,
                    nested,
                    FiredEvent {
                        sim_time,
                        rule_label: rule.label.clone(),
                        match_path: m.path.clone(),
                    },
                ))
            }
        }
    }

    /// Proper Gillespie SSA τ-leap: sample exponential waits with
    /// parameter `λ = total propensity` until time would exceed
    /// `interval`. Each step picks a candidate by per-match weight.
    fn gillespie_step(&self, rules: &[ReactionRule], subtree: Value, interval: f64) -> (Value, bool) {
        let mut subtree = subtree;
        let mut t = 0.0;
        let mut fired_any = false;
        let mut steps = 0usize;

        while t < interval && steps < self.max_per_tick {
            let candidates = self.enumerate_candidates(rules, &subtree);
            if candidates.is_empty() {
                break;
            }
            let total: f64 = candidates.iter().map(|(_, _, r)| *r).sum();
            if total <= 0.0 {
                break;
            }
            let u = self.rng.lock().unwrap().r#gen::<f64>().max(1e-12);
            let dt = -u.ln() / total;
            if t + dt > interval {
                break;
            }
            t += dt;

            let picked = match self.pick_candidate(&candidates) {
                Some(c) => c.clone(),
                None => break,
            };
            let (rule_idx, m, _rate) = picked;
            let rule = &rules[rule_idx];
            let upd = match fire_rule_at(rule, &m) {
                Some(u) => u,
                None => break,
            };
            subtree = apply_fire(&subtree, &upd, self.core.as_ref().map(|c| c.types.as_ref()));
            self.fired_log.lock().unwrap().push(FiredEvent {
                sim_time: t,
                rule_label: rule.label.clone(),
                match_path: m.path.clone(),
            });
            fired_any = true;
            steps += 1;
        }
        (subtree, fired_any)
    }
}

impl Process for BigraphicalReactiveSystem {
    /// Capture the Core's type registry so fires apply SCHEMA-AWARE: a `_divide`
    /// (or any registry-driven sentinel) in a reactum then enacts through the
    /// same algebra apply the engine uses — splitting branded cells via
    /// `divide_by_schema(CompositeLink)` and conserving mass. (The engine calls
    /// `set_core` on every discovered process node.)
    fn set_core(&mut self, core: crate::Core) {
        self.core = Some(core);
    }

    fn inputs(&self) -> PortSchema {
        let mut p = IndexMap::new();
        p.insert("state".to_string(), Schema::Any);
        // Optional `rules` port — wire it to a shared `link reactions` (the rules
        // pool) and the BRS reads its active ruleset from there too (rules-as-
        // state over a link); unwired, the BRS behaves exactly as before.
        p.insert("rules".to_string(), Schema::Any);
        p
    }

    fn outputs(&self) -> PortSchema {
        let mut p = IndexMap::new();
        p.insert("state".to_string(), Schema::Any);
        p.insert("rules".to_string(), Schema::Any);
        p
    }

    fn interval(&self) -> f64 {
        self.interval
    }

    fn update(&self, state: &Value, interval: f64) -> Update {
        // Pull the wired subtree off the "state" input port.
        let subtree = state.get_field("state").cloned().unwrap_or(Value::None);

        // rules-as-state: the ACTIVE ruleset is the seed rules (`self.rules`, from
        // config) UNION the reaction-values living in (a) the state subtree (the
        // rules-in-soup `_rules` form) and (b) the `rules` input port (a shared
        // `link reactions` — the pool form). So a reactum can produce a reaction-
        // value and have it fire on a later tick — the reaction loop (#61 AlChemy),
        // local OR shared across reactors on a link. A seed-only, no-`rules`-port
        // state reduces to exactly the prior behavior.
        let active: Vec<ReactionRule> = self
            .rules
            .iter()
            .cloned()
            .chain(collect_reactions(&subtree))
            .chain(state.get_field("rules").map(collect_reactions).unwrap_or_default())
            .collect();

        let delta = match self.mode {
            // Gillespie τ-leap: many firings collapse into the net structural
            // change (chemical kinetics — no `_divide`), so a diff is apt.
            BrsMode::Gillespie => {
                let initial = subtree.clone();
                let (s, fired) = self.gillespie_step(&active, subtree, interval);
                if !fired {
                    return Update::Noop;
                }
                structural_diff(&initial, &s)
            }
            // Deterministic / Stochastic: emit the RECONCILED FIRE DELTAS — the
            // raw `_divide`/`_remove`/`_add` directives — for the ENGINE to apply
            // schema-aware, NOT a structural diff of an internal apply. So a
            // `_divide` reaches the engine and splits the LIVE node (this tick's
            // growth included), alongside other processes' deltas — conserving
            // mass across a divide (the Form-3 property, now for reaction-driven
            // division). The internal `apply_fire` below only advances `current`
            // so multi-fire iteration finds the NEXT match, not the same one.
            _ => {
                let mut current = subtree;
                let mut deltas: Vec<Value> = Vec::new();
                for _ in 0..self.max_per_tick {
                    match self.fire_one(&active, &current, interval) {
                        Some((next, nested_delta, ev)) => {
                            self.fired_log.lock().unwrap().push(ev);
                            current = next;
                            deltas.push(nested_delta);
                        }
                        None => break,
                    }
                }
                if deltas.is_empty() {
                    return Update::Noop;
                }
                prism_schema::reconcile::reconcile_with(
                    self.core.as_ref().map(|c| c.types.as_ref()),
                    &Schema::Any,
                    &deltas,
                )
                .unwrap_or(Value::None)
            }
        };

        if is_zero(&delta) {
            return Update::Noop;
        }
        let mut out = StateMap::new();
        // Shared-link form: when a `rules` port is wired, route a reactum's
        // rule-additions (the `_rules` component of the fire delta) to it — the
        // shared reaction pool / `link reactions`, so a reaction born here is read
        // by every reactor on the link. Otherwise keep `_rules` in the soup (the
        // local rules-as-state form). (Lifts a top-level `_rules`, i.e. a
        // soup-root match — the common AlChemy case; deeper matches keep it in
        // `state`.)
        let state_delta = if state.get_field("rules").is_some() {
            if let Value::Map(mut m) = delta {
                if let Some(rules_delta) = m.shift_remove("_rules") {
                    out.insert(Key::from("rules"), rules_delta);
                }
                Value::Map(m)
            } else {
                delta
            }
        } else {
            delta
        };
        out.insert(Key::from("state"), state_delta);
        Update::Value(Value::Map(out))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A reaction-value as data: `Value::Foreign(FOREIGN_REACTION, ReactionRule)` —
/// the SAME wire form #42 uses across bridges. Pushes the cloned rule if `v` is
/// one.
fn push_reaction(v: &Value, out: &mut Vec<ReactionRule>) {
    if let Value::Foreign(f) = v {
        if f.type_name == FOREIGN_REACTION {
            if let Some(r) = f.downcast_ref::<ReactionRule>() {
                out.push(r.clone());
            }
        }
    }
}

/// Reaction-values living in a wired value become ACTIVE rules — rules-as-state,
/// the load-bearing form of "a rule IS state" (#60). Two shapes, both read:
///   - reaction-values **directly under** the map — the *pool / link* form (a
///     `rules` port wired to a shared `link reactions :: map[Reaction]`, so a
///     reaction born in one reactor is read by every reactor on the link);
///   - reaction-values under a **`_rules`** meta-slot — the *rules-in-soup* form
///     (a reactum `_add`s into `_rules` within the state it rewrites).
/// So a reactum can produce a reaction-value and the BRS picks it up next tick —
/// the reaction loop (#61 AlChemy), local OR shared. `_rules` is a reserved
/// meta-key (like `_type`); molecular redexes never match it / a `Foreign`.
fn collect_reactions(value: &Value) -> Vec<ReactionRule> {
    let mut out = Vec::new();
    let Some(m) = value.as_map() else {
        return out;
    };
    for v in m.values() {
        push_reaction(v, &mut out);
    }
    match m.get("_rules") {
        Some(Value::Map(rm)) => rm.values().for_each(|v| push_reaction(v, &mut out)),
        Some(Value::List(xs)) => xs.iter().for_each(|v| push_reaction(v, &mut out)),
        Some(single) => push_reaction(single, &mut out),
        None => {}
    }
    out
}

/// Nest a fire's localized delta under its match path, so per-fire deltas can be
/// reconciled and emitted at the subtree root (the engine then applies the
/// combined delta schema-aware at the wired slot). `[a, b] + delta → {a: {b:
/// delta}}`; an empty path returns the delta unchanged.
fn nest_fire_delta(upd: &FireUpdate) -> Value {
    let mut v = upd.delta.clone();
    for seg in upd.path.iter().rev() {
        let mut m = StateMap::new();
        m.insert(seg.clone(), v);
        v = Value::Map(m);
    }
    v
}

/// Recursively diff two values, producing an update that uses
/// `_add`/`_remove` sentinels for structural map changes. Unchanged
/// subtrees produce no output (Value::None at that position).
///
/// This is what gives BRS firings their path-localized delta: keys
/// that aren't touched by any rule firing don't appear in the result.
fn structural_diff(old: &Value, new: &Value) -> Value {
    match (old, new) {
        (Value::Map(old_map), Value::Map(new_map)) => {
            let mut delta = StateMap::new();
            let old_keys: std::collections::HashSet<&Key> = old_map.keys().collect();
            let new_keys: std::collections::HashSet<&Key> = new_map.keys().collect();

            let removed: Vec<&Key> = old_keys.difference(&new_keys).copied().collect();
            let added: Vec<&Key> = new_keys.difference(&old_keys).copied().collect();

            if !removed.is_empty() {
                delta.insert(
                    Key::from("_remove"),
                    Value::List(
                        removed
                            .iter()
                            .map(|k| Value::String(k.to_string()))
                            .collect(),
                    ),
                );
            }
            if !added.is_empty() {
                let mut add_map = StateMap::new();
                for k in &added {
                    if let Some(v) = new_map.get(*k) {
                        add_map.insert((*k).clone(), v.clone());
                    }
                }
                delta.insert(Key::from("_add"), Value::Map(add_map));
            }

            // Recurse into shared keys.
            for (k, new_v) in new_map {
                if added.contains(&k) {
                    continue;
                }
                if let Some(old_v) = old_map.get(k) {
                    let sub = structural_diff(old_v, new_v);
                    if !is_zero(&sub) {
                        delta.insert(k.clone(), sub);
                    }
                }
            }

            Value::Map(delta)
        }
        _ if old == new => Value::None,
        _ => new.clone(),
    }
}

fn is_zero(v: &Value) -> bool {
    match v {
        Value::None => true,
        Value::Map(m) => m.is_empty() || m.values().all(is_zero),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_schema::reaction::Pattern;

    fn val_str(s: &str) -> Value {
        Value::String(s.to_string())
    }

    fn phos_rule() -> ReactionRule {
        // Same shape as MAPK rule_phosphorylate but without LinkVars:
        // ERK in a Compartment becomes pERK. Tests the BRS plumbing
        // around the matcher rather than link-graph mechanics.
        ReactionRule::new(
            Pattern::map([(
                "comp",
                Pattern::sort(
                    "Compartment",
                    [
                        (
                            "substrate",
                            Pattern::sort("ERK", Vec::<(&str, Pattern)>::new()),
                        ),
                        ("rest", Pattern::site()),
                    ],
                ),
            )]),
            Pattern::map([(
                "comp",
                Pattern::sort(
                    "Compartment",
                    [
                        (
                            "substrate",
                            Pattern::sort("pERK", Vec::<(&str, Pattern)>::new()),
                        ),
                        ("rest", Pattern::site()),
                    ],
                ),
            )]),
        )
        .with_label("phos")
        .with_rate(1.0)
    }

    fn cell_state() -> Value {
        Value::tree([(
            "cyto",
            Value::tree([
                ("_type", val_str("Compartment")),
                ("mek", Value::tree([("_type", val_str("MEK"))])),
                ("erk1", Value::tree([("_type", val_str("ERK"))])),
                ("erk2", Value::tree([("_type", val_str("ERK"))])),
            ]),
        )])
    }

    fn wrap_input(subtree: Value) -> Value {
        let mut m = StateMap::new();
        m.insert(Key::from("state"), subtree);
        Value::Map(m)
    }

    /// Apply a BRS-emitted delta value to a copy of `initial`, walking
    /// nested maps. Recognizes `_add`/`_remove` sentinels at any level.
    /// This mirrors what the composite apply pipeline does in practice.
    fn apply_delta(initial: &Value, delta: &Value) -> Value {
        fn walk(target: &mut Value, delta: &Value) {
            let delta_map = match delta.as_map() {
                Some(m) => m,
                None => {
                    *target = delta.clone();
                    return;
                }
            };
            let target_map = match target.as_map_mut() {
                Some(m) => m,
                None => {
                    *target = delta.clone();
                    return;
                }
            };
            if let Some(Value::List(rm)) = delta_map.get("_remove") {
                for k in rm {
                    if let Some(s) = k.as_str() {
                        target_map.shift_remove(s);
                    }
                }
            }
            if let Some(Value::Map(adds)) = delta_map.get("_add") {
                for (k, v) in adds {
                    target_map.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in delta_map {
                if k == "_add" || k == "_remove" {
                    continue;
                }
                match target_map.get_mut(k) {
                    Some(child) => walk(child, v),
                    None => {
                        target_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        let mut next = initial.clone();
        walk(&mut next, delta);
        next
    }

    fn count_perk(state: &Value) -> usize {
        let cyto = state.get_field("cyto").unwrap();
        let cm = cyto.as_map().unwrap();
        ["erk1", "erk2"]
            .iter()
            .filter(|k| {
                cm.get(**k)
                    .and_then(|v| v.get_field("_type"))
                    .and_then(|t| t.as_str())
                    == Some("pERK")
            })
            .count()
    }

    #[test]
    fn deterministic_one_firing_per_tick() {
        let brs = BigraphicalReactiveSystem::with_config(
            vec![phos_rule()],
            BrsMode::Deterministic,
            None,
            0,
            None,
            1.0,
        );
        let initial = cell_state();
        let update = brs.update(&wrap_input(initial.clone()), 1.0);
        let value = match update {
            Update::Value(v) => v,
            Update::Noop => panic!("expected an update"),
        };
        // Apply the BRS-emitted state delta to the initial subtree.
        let state_delta = value.get_field("state").unwrap();
        let after = apply_delta(&initial, state_delta);
        assert_eq!(
            count_perk(&after),
            1,
            "exactly one ERK→pERK in deterministic mode"
        );
    }

    #[test]
    fn gillespie_fires_zero_or_more_per_tick() {
        let brs = BigraphicalReactiveSystem::with_config(
            vec![phos_rule()],
            BrsMode::Gillespie,
            None,
            42,
            None,
            1.0,
        );
        let initial = cell_state();
        // Long interval, high propensity → both ERKs should fire.
        let update = brs.update(&wrap_input(initial.clone()), 1000.0);
        let value = match update {
            Update::Value(v) => v,
            Update::Noop => panic!("expected firings"),
        };
        let state_delta = value.get_field("state").unwrap();
        let after = apply_delta(&initial, state_delta);
        assert_eq!(
            count_perk(&after),
            2,
            "both ERKs phosphorylated over a long Gillespie interval"
        );
    }

    #[test]
    fn noop_when_nothing_matches() {
        // State has no ERKs, so the phos rule can never fire.
        let no_erk = Value::tree([(
            "cyto",
            Value::tree([
                ("_type", val_str("Compartment")),
                ("mek", Value::tree([("_type", val_str("MEK"))])),
            ]),
        )]);
        let brs = BigraphicalReactiveSystem::new(vec![phos_rule()]);
        let update = brs.update(&wrap_input(no_erk), 1.0);
        assert!(matches!(update, Update::Noop));
    }

    #[test]
    fn brs_runs_guarded_computed_reactum() {
        // The chrysalis-shaped rule: a guard + a computed reactum (no
        // structural reactum pattern). Proves prism's BRS runs the closure
        // form end-to-end, so chrysalis needs no BRS of its own.
        use prism_schema::reaction::{Bindings, GuardFn, ReactionRule, ReactumFn};
        use std::sync::Arc;

        let redex = Pattern::map([("cell", Pattern::sort("Cell", [("mass", Pattern::site())]))]);
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
            let cell = || Value::tree([("_type", val_str("Cell")), ("mass", Value::float(1.0))]);
            let mut add = StateMap::new();
            add.insert(Key::from(format!("{key}_0").as_str()), cell());
            add.insert(Key::from(format!("{key}_1").as_str()), cell());
            Value::tree([
                ("_remove", Value::List(vec![val_str(&key)])),
                ("_add", Value::Map(add)),
            ])
        });
        let rule = ReactionRule::new(redex, Pattern::Site)
            .with_label("divide")
            .with_guard(guard)
            .with_reactum_fn(reactum_fn);
        let brs = BigraphicalReactiveSystem::new(vec![rule]);

        // mass 3 > 2 → fires; mother removed, two daughters added.
        let big = Value::tree([(
            "0",
            Value::tree([("_type", val_str("Cell")), ("mass", Value::float(3.0))]),
        )]);
        let update = brs.update(&wrap_input(big.clone()), 1.0);
        let value = match update {
            Update::Value(v) => v,
            Update::Noop => panic!("expected a firing"),
        };
        let after = apply_delta(&big, value.get_field("state").unwrap());
        let am = after.as_map().unwrap();
        assert!(!am.contains_key("0"), "mother removed");
        assert!(
            am.contains_key("0_0") && am.contains_key("0_1"),
            "two daughters"
        );

        // mass 1 < 2 → guard blocks → Noop.
        let small = Value::tree([(
            "0",
            Value::tree([("_type", val_str("Cell")), ("mass", Value::float(1.0))]),
        )]);
        assert!(matches!(brs.update(&wrap_input(small), 1.0), Update::Noop));
    }
}
