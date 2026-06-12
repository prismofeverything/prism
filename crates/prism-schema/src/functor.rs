//! `apply_functor` — the functor-application semantics (`categorical-core.md` §4,
//! `docs/functors.md`). The **core half** of the first-class `functor` definer: lang
//! builds a generator table from the surface (`functor Name : Source -> Target ( Gen
//! => construction )`); this module *applies* it — lifting a bigraph through `generator
//! |-> morphism` by **functoriality**.
//!
//! # A functor IS a BRS (the blessed semantics)
//!
//! The deep characterization (`lang`'s framing, blessed here as the §4 algebra home):
//! **a functor is an exhaustive, single-node [BRS]**, and *applying* it is running that
//! BRS to its **fixpoint**. Each `generator => construction` mapping is a rewrite rule
//! (redex = the generator node binding its fields; reactum = the construction over the
//! mapped children); the functor is that ruleset; rewriting every generator to its image
//! IS the functorial lift. This **reuses the reaction spine** (`reaction::find_matches`
//! / `fire_rule_at` / `apply_fire`) rather than adding a parallel engine — Felleisen-clean.
//!
//! It is a functor exactly when three conditions hold (the correctness contract):
//!
//! 1. **One rule per source generator** — so `F` is a *function* on generators
//!    (deterministic; no generator has two images).
//! 2. **Single-node redexes** — each rule rewrites ONE generator in its local context,
//!    carrying its children through as sites. Local, context-free rewriting is exactly
//!    `⊗`/`∘` preservation: `F(a ⊗ b) = F(a) ⊗ F(b)` (parallel siblings fire
//!    independently) and `F(g ∘ f) = F(g) ∘ F(f)` (the reactum's site is where the
//!    mapped child plugs in). **Functoriality = BRS locality.**
//! 3. **Target-disjoint constructions** — the image lives in the TARGET theory, whose
//!    generators are disjoint from the source's, so no rewrite creates a new redex ⟹ the
//!    BRS **terminates and is confluent in ONE pass** (each node rewritten once).
//!    ⚠️ An **endofunctor** (`target ⊇ source`, e.g. relabel-within-a-theory) violates
//!    (3): naive fixpoint re-fires its own output. It needs a *fire-once* discipline (a
//!    visited marker, or the single structural sweep below). render is safe — the markup
//!    prop is disjoint from any domain source.
//!
//! # Two implementations, one interface, one law suite
//!
//! [`Functor`] is the interface; [`Functor::apply`] is the whole-bigraph lift. There are
//! two impls, and **both satisfy the same laws** (`prism-schema/tests/functor_laws.rs`):
//!
//! - **the structural recursion** ([`apply_structural`], the [`Functor::apply`] *default*)
//!   — a postorder walk that is functorial **by construction** (the engine owns the
//!   `⊗`/`∘` recursion; the table's [`Functor::construct`] only maps each generator).
//!   The right impl for **native** functors (a render `View` written in Rust over
//!   `prism-viz` SVG constructors) and for endofunctors (the single sweep is fire-once).
//! - **the BRS-to-fixpoint** (a `RuleFunctor`, paired build with `lang`) — overrides
//!   [`Functor::apply`] to run the generator ruleset to convergence via the reaction
//!   spine. The right impl for **`.ys`-surface** functors (constructions are reaction
//!   reactums — data, or a *computed* reactum for a native construction). It belongs in
//!   prism as a NAMED op, NOT wired in chrysalis (a fixpoint in chrysalis is the
//!   `ChrysalisBrs` trap — `feedback_chrysalis_thin_layer`); lang builds the rules and
//!   *calls* it.
//!
//! The defining laws (`functor_laws.rs`):
//!
//! ```text
//!   F(id)     = id                       // identity preservation
//!   F(a ⊗ b)  = F(a) ⊗ F(b)              // tensor   preservation  (parallel siblings)
//!   F(g ∘ f)  = F(g) ∘ F(f)              // composition preservation (nesting; postorder)
//! ```
//!
//! The first consumer is **render** (`docs/functors.md` §2): the generic *structural*
//! render functor maps any bigraph to its own structure in the markup prop; domain *view*
//! functors override the generators they care about and **delegate to the structural
//! functor** for the rest.
//!
//! Sibling of [`crate::fold`] (the `fold`/`unfurl` BATWD maneuver) — one more
//! structure-preserving recursion over the bigraph, in the same closed-algebra spirit (a
//! named op with laws, not ad-hoc munging).
//!
//! [BRS]: crate::reaction

use crate::value::{StateMap, Value};

/// The keys, in priority order, under which a bigraph node names its **generator**
/// (its control / the atom a functor maps). `_type` is the brand (`#55`); `address`
/// is the process/protocol address; `control` is the explicit control name.
const CONTROL_KEYS: &[&str] = &["_type", "address", "control"];

/// Read a node's generator name (its control), best-effort — see [`CONTROL_KEYS`].
/// `None` for an anonymous node (a plain nested map with no control brand); a functor
/// table then falls back to its structural default.
pub fn control_of(node: &StateMap) -> Option<&str> {
    CONTROL_KEYS
        .iter()
        .find_map(|k| node.get(*k).and_then(Value::as_str))
}

/// A **functor** `F : Source -> Target` over bigraph values — a table mapping each
/// *generator* (a node's control) to its image (a *morphism* / construction in the
/// target). The whole-bigraph lift is [`apply`](Functor::apply).
///
/// For a **native** functor, implement [`construct`](Functor::construct) and use the
/// default [`apply`](Functor::apply) (the structural recursion); the engine has already
/// mapped the node's children (postorder), so a construction just assembles its target
/// box *around the mapped children* — that is what makes the result functorial. The
/// `construct_list` / `construct_leaf` defaults preserve `⊗` and act as identity on
/// leaves, so the bare-default impl is the **[`Identity`] functor** (`F(id) = id`); a
/// real functor overrides only the generators it transforms. A **rule-based** functor
/// (the `.ys` surface) instead overrides [`apply`](Functor::apply) with the
/// BRS-to-fixpoint (see the module docs).
pub trait Functor {
    /// The image of one node: its `control` (its generator name, if any), the original
    /// node map (so a construction may read config / links), and its already-mapped
    /// children (`mapped`, the recursion's results, name-aligned to the original).
    ///
    /// Default = **identity**: re-seal the mapped children as a map, unchanged.
    fn construct(&self, control: Option<&str>, node: &StateMap, mapped: StateMap) -> Value {
        let _ = (control, node);
        Value::Map(mapped)
    }

    /// The image of a parallel composite (a [`Value::List`]) from its mapped items.
    /// Default = **preserve `⊗`** (the tensor law) — override only for a target whose
    /// parallel form is not a list (e.g. an SVG `<g>` group).
    fn construct_list(&self, mapped: Vec<Value>) -> Value {
        Value::List(mapped)
    }

    /// The image of a leaf scalar. Default = **identity** (carry the value through).
    fn construct_leaf(&self, leaf: &Value) -> Value {
        leaf.clone()
    }

    /// Apply the functor to a whole bigraph — the lift through `generator |-> morphism`.
    ///
    /// Default = the **structural recursion** ([`apply_structural`]): functorial by
    /// construction, the right path for native functors. A rule-based (`.ys`) functor
    /// overrides this with the BRS-to-fixpoint over its generator ruleset; both impls
    /// satisfy the laws in `functor_laws.rs`.
    fn apply(&self, bigraph: &Value) -> Value {
        apply_structural(self, bigraph)
    }
}

/// The structural functorial lift — a **postorder** structure-preserving recursion.
/// Every child is mapped before its parent's [`Functor::construct`] is called, and the
/// parallel (`List`) / nested (`Map`) structure is walked by the engine itself — so the
/// result is functorial **by construction** (see the module docs + `functor_laws.rs`). A
/// `Struct` (the compiled form of a map) is normalized to its map projection first. This
/// is the default body of [`Functor::apply`]; native functors use it, rule functors
/// override `apply`.
pub fn apply_structural<F: Functor + ?Sized>(f: &F, v: &Value) -> Value {
    match v {
        // ⊗ — parallel siblings: map each, preserve the parallel composite.
        Value::List(items) => {
            let mapped = items.iter().map(|i| apply_structural(f, i)).collect();
            f.construct_list(mapped)
        }
        // ∘ + ⊗ — a node with named children: map the children FIRST (postorder), then
        // let the generator construct its image around them.
        Value::Map(node) => {
            let mapped: StateMap = node
                .iter()
                .map(|(k, child)| (k.clone(), apply_structural(f, child)))
                .collect();
            f.construct(control_of(node), node, mapped)
        }
        // Struct is the compiled (layout+values) form of a map — normalize, then recurse
        // the map arm so a functor sees one uniform place-graph shape.
        Value::Struct { .. } => {
            let mut as_map = v.clone();
            as_map.as_map_mut(); // promotes Struct -> Map in place
            apply_structural(f, &as_map)
        }
        // a leaf generator (scalar) — its image is the construction's choice.
        leaf => f.construct_leaf(leaf),
    }
}

/// Apply a functor to a bigraph value — the public entry, dispatching through
/// [`Functor::apply`] (the structural recursion by default; a rule-based functor's
/// BRS-to-fixpoint override). `apply_functor(&Identity, v) == v`.
pub fn apply_functor<F: Functor + ?Sized>(f: &F, v: &Value) -> Value {
    f.apply(v)
}

/// The **identity functor** `1 : A -> A` — every generator maps to itself, structure
/// untouched. `apply_functor(&Identity, v) == v` for every `Map`/`List`/scalar value
/// (the `F(id) = id` law, and the base case a domain *view* functor delegates to for
/// the generators it does not override).
pub struct Identity;

impl Functor for Identity {}
