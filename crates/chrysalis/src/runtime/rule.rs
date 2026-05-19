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

use prism_schema::{Bindings, Key, Pattern};

use crate::ast::{Expr, Name};

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

/// A chrysalis-typed reaction rule.
#[derive(Clone, Debug)]
pub struct Rule {
    pub label: String,
    pub redex: Pattern,
    pub reactum: Expr,
    pub guard: Option<Expr>,
    pub rate: Option<Expr>,
    /// `prism_schema::ReactionRule::instantiation` analogue. Maps
    /// reactum site keys back to redex site keys for the "rest"
    /// capture semantics.
    pub instantiation: IndexMap<Key, Key>,
    /// Variables captured by the redex, with instructions for how to
    /// extract their values from a Match's Bindings.
    pub bindings: RuleBindings,
    /// Lexical environment captured at definition site — typically
    /// the parameters of the enclosing `reaction` definer.
    pub closure: Arc<IndexMap<Name, prism_schema::Value>>,
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
