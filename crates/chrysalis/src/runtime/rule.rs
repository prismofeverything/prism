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

use indexmap::IndexMap;

use prism_schema::reaction::{GuardFn, RateFn, ReactumFn};
use prism_schema::registry::{TypeMethods, TypeRegistry};
use prism_schema::value::Foreign;
use prism_schema::{
    localize_fire, Bindings, DivideContext, FOREIGN_REACTION, Key, Pattern, ReactionRule, Schema,
    StateMap, Value,
};

use crate::ast::{Expr, Name};
use crate::eval::Evaluator;


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
    /// The variable names the matched NODE itself — a top-level `?c :: Cell`
    /// redex. It binds the whole matched node as a VALUE (so `?c.divide()` /
    /// `?c.mass` work, from `bindings.sites[prism_key]`) AND marks the entry
    /// CONSUMED: on fire the matched key (`bindings.key_map[prism_key]`) is
    /// removed and replaced by the reactum's products. The unification of the
    /// two homoiconic-division needs — "name the node" + "the container owns
    /// the key" — into one binding (vs the old key-var/value-var split).
    Node(Key),
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
            BindingSource::Site(prism_key) | BindingSource::Node(prism_key) => bindings
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

/// Build the localized delta for a COMPUTED reactum. This computes only the
/// chrysalis-specific HEAD — which keys the reaction CONSUMES — then defers the
/// keying of products to `prism_schema::localize_fire`, the ONE convention the
/// STRUCTURAL path (`fire_rule_at`) also uses. So a structural and a computed
/// reaction with the same meaning produce the same delta (the consistency
/// invariant pinned by `chrysalis/tests/reaction_conformance.rs`).
///
/// The consumed set:
///   - a multiset redex (`?f | ?b => …`) consumes EVERY matched child;
///   - a keyed / top-level `?c :: Cell` redex consumes its `Node` / `OuterKey`
///     entries (the entry the reaction REPLACES).
/// The `_divide` directive (`?c.divide()`) is routed through the late-bound
/// `_divide` apply unchanged (it is not a plain product).
fn reaction_delta(
    is_list: bool,
    bindings: &Bindings,
    rule_bindings: &RuleBindings,
    reactum_val: Value,
) -> Value {
    if is_list {
        // Multiset reaction (`?f::F | ?b::B => …`): consume EVERY matched child;
        // `localize_fire` fresh-keys the products. Normalize a single ion (or a
        // bare value) to a list so it is fresh-keyed, not added as a named map.
        let removed: Vec<Value> = bindings
            .key_map
            .values()
            .map(|k| Value::String(k.to_string()))
            .collect();
        let products = match reactum_val {
            Value::List(_) => reactum_val,
            Value::None => Value::List(vec![]),
            other => Value::List(vec![other]),
        };
        return localize_fire(removed, products);
    }

    // The consumed (removed-and-replaced) key(s): a top-level `?c :: Cell` binds
    // the matched NODE (`Node`), and the classic key-var form binds the key
    // (`OuterKey`). Both name the entry the reaction REPLACES.
    let matched_keys: Vec<Value> = rule_bindings
        .values()
        .filter_map(|src| match src {
            BindingSource::OuterKey(redex_key) | BindingSource::Node(redex_key) => bindings
                .key_map
                .get(redex_key)
                .map(|k| Value::String(k.to_string())),
            _ => None,
        })
        .collect();

    // A DIVIDE directive — `?c.divide()` returns `{_divide: [<override per
    // daughter>]}`. Route it through the LATE-BOUND `_divide` apply (the SAME
    // path the Form-3 Divider uses): the apply splits the LIVE node — INCLUDING
    // this tick's growth — via `divide_by_schema(CompositeLink)`, so growth in
    // the dividing tick is CONSERVED (an early snapshot-split would drop it). The
    // container owns the keys: each daughter is named `<mother>_i`. This unifies
    // reaction-driven and step-driven (Divider) division onto ONE mechanism.
    if let Some(overrides) = reactum_val
        .as_map()
        .and_then(|m| m.get("_divide"))
        .and_then(|v| v.as_list())
    {
        if let Some(mother) = matched_keys.first().and_then(|v| v.as_str()) {
            let mut daughters: StateMap = StateMap::new();
            for (i, ov) in overrides.iter().enumerate() {
                daughters.insert(Key::from(format!("{mother}_{i}").as_str()), ov.clone());
            }
            let div = Value::Map(IndexMap::from([
                (Key::from("mother"), Value::String(mother.to_string())),
                (Key::from("daughters"), Value::Map(daughters)),
            ]));
            return Value::Map(IndexMap::from([(Key::from("_divide"), div)]));
        }
    }

    // The keying — what's removed, how products are keyed — is the SHARED
    // convention (one implementation across structural + computed paths).
    localize_fire(matched_keys, reactum_val)
}

// ════════════════════════════════════════════════════════════════════
// Closure-free conversion — chrysalis Rule → prism ReactionRule (#42)
// ════════════════════════════════════════════════════════════════════

/// Convert a chrysalis [`Rule`] into a `prism_schema::ReactionRule`
/// WITHOUT an evaluator. Succeeds only for **structural** reactions
/// with no chrysalis-[`Expr`] guard or rate — the closed-form case
/// where the rule's behaviour is fully captured by its redex /
/// reactum patterns + label + instantiation map.
///
/// Returns `None` for reactions whose behaviour depends on chrysalis
/// `Expr` evaluation at fire time (`Reactum::Computed`, or a guard /
/// rate `Expr` requiring an evaluator). For those, use the full
/// [`to_prism_rule`] with an evaluator.
///
/// This is the closure-free converter that lets a chrysalis reaction
/// VALUE travel across a `:: bigraph` port (#42 / merge-protocol
/// slice 5): once Foreign-wrapped, the prism `ReactionRule` carries
/// itself + fires at the receiver — no evaluator needed at apply
/// time.
pub fn to_structural_rule(rule: &Rule) -> Option<ReactionRule> {
    let Reactum::Structural {
        reactum,
        instantiation,
    } = &rule.reactum
    else {
        return None;
    };
    if rule.guard.is_some() || rule.rate.is_some() {
        return None;
    }
    let mut pr =
        ReactionRule::new(rule.redex.clone(), reactum.clone()).with_label(rule.label.clone());
    pr.instantiation = instantiation.clone();
    Some(pr)
}

/// Wrap a chrysalis [`Rule`] as the [`FOREIGN_REACTION`]-tagged Value
/// the `:: bigraph` port type's `apply` accepts (#42). Returns `None`
/// for reactions that can't be made closure-free (see
/// [`to_structural_rule`]).
///
/// With this in hand, a chrysalis-built reaction flows across a
/// `:: bigraph` input port as a typed update; the receiver's
/// `algebra::apply_with(Custom{"bigraph"}, current, value)` dispatches
/// to `BigraphTypeMethods::apply`, fires the reaction against `current`,
/// and returns the post-fire state — locally identical to a `stream:`
/// or `rest:` transport (merge-protocol slice 5).
pub fn to_bigraph_value(rule: &Rule) -> Option<Value> {
    to_structural_rule(rule).map(|pr| Value::Foreign(Foreign::new(FOREIGN_REACTION, pr)))
}

/// The `reaction` TYPE — a reaction *at rest*, as transmittable / storable DATA.
///
/// Its `realize` REIFIES a chrysalis [`Rule`] value (`FOREIGN_RULE`) into the
/// closure-free `Foreign(FOREIGN_REACTION, ReactionRule)` form (the #42 wire
/// form). So a `:: reaction` / `:: map[reaction]` slot — a rules pool, a shared
/// `link` — AUTO-converts on store: the `.ys` author writes `=> Grow`, no
/// reification verb. That SAME form is what `BigraphicalReactiveSystem`'s
/// `collect_state_rules` reads (rules-as-state) AND what fires across a
/// `:: bigraph` bridge (#42) — ONE value for store / link-share / send, which is
/// why AlChemy is the BATWD demo: reactions are ordinary transmittable state.
///
/// Unlike the `bigraph` type (whose `apply` FIRES the reaction), `reaction`'s
/// `apply` STORES (overwrite by the reified value). Only STRUCTURAL reactions
/// reify here (closure-free, no evaluator needed); a computed reaction (guard /
/// rate / computed reactum) passes through unconverted — a host that wants it as
/// a live rule must reify with an evaluator (`to_prism_rule`).
#[derive(Debug)]
pub struct ReactionType;

impl TypeMethods for ReactionType {
    fn default(&self, _r: &TypeRegistry, _s: &Schema) -> Value {
        Value::None
    }

    fn apply(&self, r: &TypeRegistry, s: &Schema, _state: &Value, update: &Value) -> Value {
        // A reaction slot is overwrite-by-the-REIFIED-update: STORE the reaction
        // (don't fire it — that's `bigraph`). Realizing the update is the reify.
        self.realize(r, s, update)
    }

    fn divide(&self, _r: &TypeRegistry, _s: &Schema, state: &Value, ctx: &DivideContext) -> Vec<Value> {
        // A reaction is intensive — each daughter inherits a copy of the rule.
        vec![state.clone(); ctx.n_daughters.max(2)]
    }

    fn serialize(&self, _r: &TypeRegistry, _s: &Schema, state: &Value) -> Value {
        // The transmittable form of a reaction is the closure-free STRUCTURAL
        // `ReactionRule` rendered to data (`ReactionRule::to_data_value`) — plain
        // maps/strings/scalars, so a `:: reaction` slot crosses a `rest:`/
        // `stream:` bridge as JSON (a runnable `Foreign` would `value_to_json` to
        // `null`). A stored reaction is `Foreign(FOREIGN_REACTION, ReactionRule)`;
        // a chrysalis `Rule` (`FOREIGN_RULE`) reifies to structural first. A
        // COMPUTED rule (guard / computed reactum / rate closure) has no wire form
        // (`to_data_value` → `None`) — pass it through unchanged (in-process use
        // only; the honest boundary, not a lossy encode). Inverse: `realize`.
        match state {
            Value::Foreign(f) if f.type_name == FOREIGN_REACTION => f
                .downcast_ref::<ReactionRule>()
                .and_then(|rr| rr.to_data_value())
                .unwrap_or_else(|| state.clone()),
            Value::Foreign(f) if f.type_name == FOREIGN_RULE => f
                .downcast_ref::<Rule>()
                .and_then(to_structural_rule)
                .and_then(|rr| rr.to_data_value())
                .unwrap_or_else(|| state.clone()),
            _ => state.clone(),
        }
    }

    fn realize(&self, _r: &TypeRegistry, _s: &Schema, encoded: &Value) -> Value {
        match encoded {
            // The reify: chrysalis Rule → closure-free transmittable form.
            Value::Foreign(f) if f.type_name == FOREIGN_RULE => f
                .downcast_ref::<Rule>()
                .and_then(to_bigraph_value)
                .unwrap_or_else(|| encoded.clone()),
            // A wire-arrived reaction in DATA form (`{_pat: "Rule", …}` — off a
            // `rest:`/`stream:` bridge, a deserialized link, or any serde
            // boundary): reconstruct the runnable `Foreign(FOREIGN_REACTION, …)`
            // so the BRS reads it as a rule (rules-as-state). Inverse of
            // `serialize`. A non-rule map is left untouched.
            Value::Map(m) if m.get("_pat").and_then(|t| t.as_str()) == Some("Rule") => {
                ReactionRule::from_data_value(encoded)
                    .map(|rr| Value::Foreign(Foreign::new(FOREIGN_REACTION, rr)))
                    .unwrap_or_else(|_| encoded.clone())
            }
            // Already transmittable, or not a reaction value — pass through.
            _ => encoded.clone(),
        }
    }

    fn check(&self, _r: &TypeRegistry, _s: &Schema, state: &Value) -> bool {
        matches!(
            state,
            Value::Foreign(f) if f.type_name == FOREIGN_REACTION || f.type_name == FOREIGN_RULE
        )
    }
}
