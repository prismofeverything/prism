//! Chrysalis-typed bigraphical reactive system Process.
//!
//! Wraps prism's matcher with a chrysalis [`Rule`] firing model: the
//! redex is a static [`Pattern`] (matched by prism), but the reactum,
//! guard, and rate are chrysalis [`Expr`]s evaluated against the
//! match bindings at fire time.
//!
//! The output is an `_add` / `_remove` delta at the *parent of the
//! match path*: the matched outer key is removed and the reactum
//! value's entries are added. This keeps to-the-letter compatibility
//! with prism's existing structural-diff machinery — chrysalis adds
//! the expression layer without changing what gets emitted.

use std::any::Any;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use prism_bigraph::{PortSchema, Process, Update};
use prism_schema::{find_matches, value_type_name, Key, Match, Schema, Value};
use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::eval::{EvalError, Evaluator};
use crate::runtime::rule::{bind_environment, Rule};

/// Type name used for `Value::Foreign` carriers of a [`Rule`]. Lets
/// rules flow through state as values (which is what makes them
/// first-class in chrysalis).
pub const FOREIGN_RULE: &str = "ChrysalisRule";

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum BrsMode {
    /// First match of the first rule fires. At most one firing per
    /// tick — matches the surface-design intent for tier-1 examples.
    #[default]
    Deterministic,
    /// One firing per tick, sampled from all candidate matches
    /// weighted by the rule's evaluated rate.
    Stochastic,
}

#[derive(Clone, Debug)]
pub struct BrsConfig {
    pub rules: Vec<Rule>,
    pub mode: BrsMode,
    pub seed: u64,
    pub interval: f64,
    /// Maximum firings per tick. Caps unbounded modes.
    pub max_per_tick: usize,
}

impl Default for BrsConfig {
    fn default() -> Self {
        Self {
            rules: vec![],
            mode: BrsMode::Deterministic,
            seed: 0,
            interval: 1.0,
            max_per_tick: 1,
        }
    }
}

/// A `prism_bigraph::Process` that fires chrysalis [`Rule`]s.
pub struct ChrysalisBrs {
    pub config: BrsConfig,
    pub evaluator: Arc<Evaluator>,
    rng: Mutex<StdRng>,
}

impl std::fmt::Debug for ChrysalisBrs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChrysalisBrs")
            .field("n_rules", &self.config.rules.len())
            .field("mode", &self.config.mode)
            .field("interval", &self.config.interval)
            .finish()
    }
}

impl ChrysalisBrs {
    pub fn new(config: BrsConfig, evaluator: Arc<Evaluator>) -> Self {
        let seed = config.seed;
        Self {
            config,
            evaluator,
            rng: Mutex::new(StdRng::seed_from_u64(seed)),
        }
    }

    /// Fire deterministically: pick the first match of the first rule
    /// whose guard passes; emit the update; stop.
    fn fire_deterministic(&self, subtree: &Value) -> Option<Update> {
        for rule in &self.config.rules {
            let candidates = find_matches(subtree, &rule.redex, None);
            for m in &candidates {
                if let Some(update) = self.try_fire(rule, m).ok().flatten() {
                    return Some(update);
                }
            }
        }
        None
    }

    /// Build a candidate (rule_idx, match, rate) for stochastic mode.
    fn enumerate_candidates(&self, subtree: &Value) -> Vec<(usize, Match, f64)> {
        let mut out = Vec::new();
        for (i, rule) in self.config.rules.iter().enumerate() {
            for m in find_matches(subtree, &rule.redex, None) {
                // Filter by guard up front to avoid sampling impossible
                // matches.
                let env = bind_environment(&m.bindings, &rule.bindings, &rule.closure);
                if let Some(guard_expr) = &rule.guard {
                    match self.evaluator.eval_value(guard_expr, &env) {
                        Ok(Value::Bool(true)) => {}
                        Ok(_) => continue,
                        Err(e) => {
                            eprintln!("guard eval failed for rule {}: {e}", rule.label);
                            continue;
                        }
                    }
                }
                let rate = match &rule.rate {
                    Some(rate_expr) => match self
                        .evaluator
                        .eval_value(rate_expr, &env)
                        .ok()
                        .and_then(|v| v.as_f64())
                    {
                        Some(r) => r,
                        None => 1.0,
                    },
                    None => 1.0,
                };
                out.push((i, m, rate));
            }
        }
        out
    }

    /// Try to fire `rule` at `m`. Returns `Ok(Some(update))` if the
    /// guard passed and the reactum eval succeeded; `Ok(None)` if the
    /// guard rejected it; `Err` if an Expr evaluation failed.
    fn try_fire(
        &self,
        rule: &Rule,
        m: &Match,
    ) -> Result<Option<Update>, EvalError> {
        let env = bind_environment(&m.bindings, &rule.bindings, &rule.closure);

        // Guard predicate.
        if let Some(guard_expr) = &rule.guard {
            let g = self.evaluator.eval_value(guard_expr, &env)?;
            match g {
                Value::Bool(true) => {}
                Value::Bool(false) => return Ok(None),
                other => {
                    return Err(EvalError::TypeMismatch {
                        expected: "Bool (from guard)".into(),
                        got: value_type_name(&other).into(),
                    });
                }
            }
        }

        // Reactum value (the new content for the match site).
        let reactum_val = self.evaluator.eval_value(&rule.reactum, &env)?;

        // Build the update.
        //
        // Tier-1 firing model: the outermost map pattern in the redex
        // contains a single non-Site key; the matched state key (from
        // key_map) is removed from the parent map; the reactum value
        // (which should be a Map) is `_add`-merged in. The path of
        // the parent IS `m.path` — find_matches reports the path to
        // the map node where the redex matched, so `m.path` is the
        // parent.
        let matched_keys: Vec<Key> = rule
            .bindings
            .values()
            .filter_map(|src| match src {
                crate::runtime::rule::BindingSource::OuterKey(redex_key) => m
                    .bindings
                    .key_map
                    .get(redex_key)
                    .cloned(),
                _ => None,
            })
            .collect();

        let mut delta_map: IndexMap<Key, Value> = IndexMap::new();
        if !matched_keys.is_empty() {
            delta_map.insert(
                Key::from("_remove"),
                Value::List(
                    matched_keys
                        .into_iter()
                        .map(|k| Value::String(k.to_string()))
                        .collect(),
                ),
            );
        }
        // The reactum result becomes the `_add` payload. If the user
        // explicitly produced a `{_add: ..., _remove: ...}` map, use
        // that verbatim (homoiconic escape hatch).
        match reactum_val {
            Value::Map(mut m_val) if m_val.contains_key("_add") || m_val.contains_key("_remove") => {
                if let Some(rem) = m_val.shift_remove("_remove") {
                    delta_map.insert(Key::from("_remove"), rem);
                }
                if let Some(add) = m_val.shift_remove("_add") {
                    delta_map.insert(Key::from("_add"), add);
                }
                // Anything else in m_val is a plain field update — merge
                // it in alongside the sentinels.
                for (k, v) in m_val {
                    delta_map.insert(k, v);
                }
            }
            Value::Map(m_val) => {
                delta_map.insert(Key::from("_add"), Value::Map(m_val));
            }
            other => {
                // Non-map reactum: emit at root as a single replace.
                delta_map.insert(Key::from("_add"), other);
            }
        }

        // Wrap the delta in the path the match was found at, so it
        // applies to the right subtree relative to our `state` port.
        let mut wrapped = Value::Map(delta_map);
        for segment in m.path.iter().rev() {
            wrapped = Value::Map(IndexMap::from_iter([(segment.clone(), wrapped)]));
        }

        // The BRS's "state" output port receives this delta — see
        // outputs() below.
        Ok(Some(Update::value(Value::Map(IndexMap::from_iter([(
            Key::from("state"),
            wrapped,
        )])))))
    }
}

impl Process for ChrysalisBrs {
    fn inputs(&self) -> PortSchema {
        IndexMap::from_iter([("state".to_string(), Schema::Any)])
    }

    fn outputs(&self) -> PortSchema {
        IndexMap::from_iter([("state".to_string(), Schema::Any)])
    }

    fn interval(&self) -> f64 {
        self.config.interval
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        // Engine delivered state keyed by input port names — pull the
        // `state` subtree.
        let subtree = state
            .as_map()
            .and_then(|m| m.get("state"))
            .cloned()
            .unwrap_or(Value::None);

        match self.config.mode {
            BrsMode::Deterministic => {
                self.fire_deterministic(&subtree).unwrap_or(Update::Noop)
            }
            BrsMode::Stochastic => {
                let candidates = self.enumerate_candidates(&subtree);
                if candidates.is_empty() {
                    return Update::Noop;
                }
                let total: f64 = candidates.iter().map(|(_, _, r)| *r).sum();
                if total <= 0.0 {
                    return Update::Noop;
                }
                use rand::Rng;
                let pick = self.rng.lock().unwrap().r#gen::<f64>() * total;
                let mut cum = 0.0;
                for (i, m, r) in &candidates {
                    cum += *r;
                    if cum >= pick {
                        let rule = &self.config.rules[*i];
                        return self
                            .try_fire(rule, m)
                            .ok()
                            .flatten()
                            .unwrap_or(Update::Noop);
                    }
                }
                Update::Noop
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
