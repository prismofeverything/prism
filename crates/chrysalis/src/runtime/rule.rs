//! A chrysalis-typed reaction rule.
//!
//! Unlike `prism_schema::ReactionRule` (purely structural rewrite), a
//! chrysalis [`Rule`] carries chrysalis [`Expr`]s for its reactum,
//! guard, and rate. At fire time those Exprs are evaluated against
//! the matched bindings, producing a delta Value that the engine then
//! applies through the usual `_add` / `_remove` sentinel mechanism.
//!
//! The redex remains a static [`prism_schema::Pattern`] so prism's
//! matcher does the structural search. The [`RuleBindings`] map tells
//! the chrysalis interpreter how to convert a [`prism_schema::Match`]
//! into an environment the reactum/guard/rate can read.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use indexmap::IndexMap;

use prism_schema::reaction::{GuardFn, RateFn, ReactumFn};
use prism_schema::{Bindings, Key, Pattern, ReactionRule, StateMap, Value};

use crate::ast::{Expr, Name};
use crate::eval::Evaluator;

/// Monotonic source of fresh state keys for ions a multiset reaction
/// produces (the soup is keyed, but anonymous-parallel reactants/products
/// have no inherent key).
static FRESH_NODE: AtomicU64 = AtomicU64::new(0);

fn fresh_id() -> String {
    format!("g{}", FRESH_NODE.fetch_add(1, Ordering::Relaxed))
}

/// `Value::Foreign` type tag for a chrysalis [`Rule`] carrier. Lets a
/// reaction flow through state as a first-class value (constructed by a
/// `reaction` definer call, deposited into a `BRS[rules: […]]`).
pub const FOREIGN_RULE: &str = "ChrysalisRule";

/// How to look up a chrysalis variable's value from a [`Bindings`]
/// produced by a successful match.
#[derive(Clone, Debug)]
pub enum BindingSource {
    /// The variable names the *outer* state key the redex matched
    /// — look it up in `bindings.key_map[redex_key]`. Yields a
    /// `Value::String`.
    OuterKey(Key),
    /// The variable names a captured subtree — look it up in
    /// `bindings.sites[prism_key]`. Yields the captured value
    /// (or a `{state_key: value}` map in surplus mode).
    Site(Key),
    /// The variable names a captured wire path — look it up in
    /// `bindings.edges[prism_key]`. Yields a `Value::List` of path
    /// segments.
    Edge(Key),
}

/// `chrysalis_name → BindingSource` — the mapping built by the
/// pattern lowering pass. Pass an `&Bindings` plus this map into
/// [`bind_environment`] to translate match results into a chrysalis
/// environment.
pub type RuleBindings = IndexMap<Name, BindingSource>;

/// How a reaction produces its reactum — the one thing prism's
/// structural `ReactionRule` can't express on its own, so chrysalis
/// classifies it here and `to_prism_rule` maps each case onto prism.
#[derive(Clone, Debug)]
pub enum Reactum {
    /// A structural Pattern→Pattern rewrite (MAPK-style: in-place link
    /// bonds via `~name`, `!` absent ports, rest-carry). Fired natively
    /// by prism's `instantiate`/`fire_rule_at`. `instantiation` maps
    /// reactum site keys to redex site keys; empty means prism's
    /// identity-by-name default (the common case).
    Structural {
        reactum: Pattern,
        instantiation: IndexMap<Key, Key>,
    },
    /// A reactum *computed* from the match (e.g. `?cell.divide(?cid)`):
    /// the expression is evaluated at fire time to a delta value. Mapped
    /// onto prism's `reactum_fn` closure hook.
    Computed(Expr),
}

/// A chrysalis-typed reaction rule — the reaction-as-value **data
/// carrier**. It holds the redex pattern, the reactum (structural or
/// computed), and the guard/rate expressions; [`to_prism_rule`] adapts it
/// to a `prism_schema::ReactionRule` that prism's BRS fires. chrysalis
/// owns the data and the binding convention; prism owns matching + firing.
#[derive(Clone, Debug)]
pub struct Rule {
    pub label: String,
    pub redex: Pattern,
    pub reactum: Reactum,
    pub guard: Option<Expr>,
    pub rate: Option<Expr>,
    /// Variables captured by the redex, with instructions for how to
    /// extract their values from a Match's Bindings.
    pub bindings: RuleBindings,
    /// Lexical environment captured at definition site — typically
    /// the parameters of the enclosing `reaction` definer.
    pub closure: Arc<IndexMap<Name, Value>>,
}

/// Convert a successful [`Bindings`] into a chrysalis environment
/// against [`RuleBindings`]. The `base` environment supplies any
/// closure-captured parameters; match bindings shadow those when
/// names collide.
pub fn bind_environment(
    bindings: &Bindings,
    rule_bindings: &RuleBindings,
    base: &IndexMap<Name, prism_schema::Value>,
) -> IndexMap<Name, prism_schema::Value> {
    let mut env = base.clone();
    for (chrysalis_name, source) in rule_bindings {
        let value = match source {
            BindingSource::OuterKey(redex_key) => bindings
                .key_map
                .get(redex_key)
                .map(|k| prism_schema::Value::String(k.to_string()))
                .unwrap_or(prism_schema::Value::None),
            BindingSource::Site(prism_key) => bindings
                .sites
                .get(prism_key)
                .cloned()
                .unwrap_or(prism_schema::Value::None),
            BindingSource::Edge(prism_key) => bindings
                .edges
                .get(prism_key)
                .cloned()
                .unwrap_or(prism_schema::Value::None),
        };
        env.insert(chrysalis_name.clone(), value);
    }
    env
}

/// Decode the `rules` list from a BRS config `Value` into chrysalis
/// [`Rule`]s. Each rode in as a `Value::Foreign(FOREIGN_RULE, Rule)`,
/// produced by a `reaction` definer call.
pub fn extract_rules(config: &Value) -> Vec<Rule> {
    let mut rules = Vec::new();
    let Some(list) = config
        .as_map()
        .and_then(|m| m.get("rules"))
        .and_then(|v| v.as_list())
    else {
        return rules;
    };
    for item in list {
        if let Value::Foreign(f) = item {
            if f.type_name == FOREIGN_RULE {
                if let Some(rule) = f.downcast_ref::<Rule>() {
                    rules.push(rule.clone());
                }
            }
        }
    }
    rules
}

/// Adapt a chrysalis [`Rule`] (the reaction-as-value data carrier) into a
/// `prism_schema::ReactionRule` that prism's `BigraphicalReactiveSystem`
/// fires. chrysalis owns only the *data* — the redex pattern, the
/// reactum/guard/rate expressions, and how a match's bindings map to
/// chrysalis variables; **prism owns all matching, firing, and diffing**.
/// The expressions become closures over the evaluator, run against each
/// match's bindings at fire time.
pub fn to_prism_rule(rule: &Rule, evaluator: Arc<Evaluator>) -> ReactionRule {
    // The structural redex is always matched by prism. The reactum is
    // either a STRUCTURAL pattern (fired by prism's native `instantiate` —
    // MAPK-style link/rest rewrites) or COMPUTED (an expression evaluated
    // against the match bindings to a delta value — e.g. `?cell.divide(?cid)`).
    let mut pr = match &rule.reactum {
        Reactum::Structural {
            reactum,
            instantiation,
        } => {
            let mut pr = ReactionRule::new(rule.redex.clone(), reactum.clone())
                .with_label(rule.label.clone());
            // Empty → prism resolves identity-by-name (carries shared sites
            // like `bystanders`/`name` through unchanged).
            pr.instantiation = instantiation.clone();
            pr
        }
        Reactum::Computed(_) => {
            ReactionRule::new(rule.redex.clone(), Pattern::Site).with_label(rule.label.clone())
        }
    };

    if let Some(guard_expr) = rule.guard.clone() {
        let ev = Arc::clone(&evaluator);
        let bindings = rule.bindings.clone();
        let closure = Arc::clone(&rule.closure);
        let g: GuardFn = Arc::new(move |b: &Bindings| {
            let env = bind_environment(b, &bindings, &closure);
            matches!(ev.eval_value(&guard_expr, &env), Ok(Value::Bool(true)))
        });
        pr = pr.with_guard(g);
    }

    if let Reactum::Computed(reactum_expr) = &rule.reactum {
        let ev = Arc::clone(&evaluator);
        let bindings = rule.bindings.clone();
        let closure = Arc::clone(&rule.closure);
        let reactum_expr = reactum_expr.clone();
        // An anonymous-parallel (List) redex is a multiset reaction: it
        // consumes the matched ions and produces fresh ones, vs a keyed
        // (Map) redex that removes its OuterKey binding.
        let is_list = matches!(rule.redex, Pattern::List(_));
        let rf: ReactumFn = Arc::new(move |b: &Bindings| {
            let env = bind_environment(b, &bindings, &closure);
            let reactum_val = ev.eval_value(&reactum_expr, &env).unwrap_or(Value::None);
            reaction_delta(is_list, b, &bindings, reactum_val)
        });
        pr = pr.with_reactum_fn(rf);
    }

    if let Some(rate_expr) = rule.rate.clone() {
        let ev = Arc::clone(&evaluator);
        let bindings = rule.bindings.clone();
        let closure = Arc::clone(&rule.closure);
        let rt: RateFn = Arc::new(move |b: &Bindings| {
            let env = bind_environment(b, &bindings, &closure);
            ev.eval_value(&rate_expr, &env)
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0)
        });
        pr = pr.with_rate_fn(rt);
    }

    pr
}

/// Build the localized delta for a computed reactum: `_remove` the state
/// key(s) the redex matched at its OUTER binding(s), `_add` the reactum value
/// — or pass an explicit `{_add, _remove}` map the reactum produced straight
/// through. This is the chrysalis firing convention (which key the match
/// consumes); prism's `fire_rule_at` then emits this delta at the match path.
fn reaction_delta(
    is_list: bool,
    bindings: &Bindings,
    rule_bindings: &RuleBindings,
    reactum_val: Value,
) -> Value {
    if is_list {
        // Multiset reaction (`?f::F | ?b::B => …`): consume EVERY matched
        // child (all of key_map), and add the reactum's ion(s) under fresh
        // keys. The reactum value is a List for parallel products (`F | Phi`)
        // or a single ion (`F`).
        let removed: Vec<Value> = bindings
            .key_map
            .values()
            .map(|k| Value::String(k.to_string()))
            .collect();
        let items = match reactum_val {
            Value::List(xs) => xs,
            Value::None => vec![],
            other => vec![other],
        };
        let mut add: StateMap = StateMap::new();
        for item in items {
            add.insert(Key::from(fresh_id().as_str()), item);
        }
        let mut delta: StateMap = StateMap::new();
        if !removed.is_empty() {
            delta.insert(Key::from("_remove"), Value::List(removed));
        }
        delta.insert(Key::from("_add"), Value::Map(add));
        return Value::Map(delta);
    }

    let matched_keys: Vec<Value> = rule_bindings
        .values()
        .filter_map(|src| match src {
            BindingSource::OuterKey(redex_key) => bindings
                .key_map
                .get(redex_key)
                .map(|k| Value::String(k.to_string())),
            _ => None,
        })
        .collect();

    let mut delta: StateMap = StateMap::new();
    if !matched_keys.is_empty() {
        delta.insert(Key::from("_remove"), Value::List(matched_keys));
    }
    match reactum_val {
        // Explicit delta from the reactum — pass its sentinels through.
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
        Value::Map(m) => {
            delta.insert(Key::from("_add"), Value::Map(m));
        }
        other => {
            delta.insert(Key::from("_add"), other);
        }
    }
    Value::Map(delta)
}
