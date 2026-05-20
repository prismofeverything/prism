//! Bigraphical algebraic assembly — interfaces, composition, tensor,
//! and the elementary bigraphs.
//!
//! Reference: Milner, *Space and Motion of Communicating Agents*
//! (2008), Defs. 2.5 (composition), 2.7 (tensor), and 3.1–3.5
//! (elementary bigraphs). Python upstream:
//! `bigraph_schema.assembly` (the algebra half).
//!
//! ## What this module is for
//!
//! Phase A of the Milner port: it turns schemas-with-holes into
//! arrows in a category. The objects are **interfaces**
//! `I = ⟨m, X⟩` (a place width `m` and a set of names `X`) and the
//! arrows are patterns `f : I → J` carrying sites for the inner face
//! and roots for the outer face. Two operations let you build up
//! bigraphs algebraically:
//!
//! - [`compose`] (`G ∘ F`) substitutes `F`'s roots into `G`'s sites.
//! - [`tensor`] (`F ⊗ G`) places two disjoint bigraphs side by side.
//!
//! Everything else in Milner's Ch. 3 is then constructible from a
//! handful of elementary bigraphs: [`barren`], [`merge`], [`ion`],
//! and (with first-class Links — deferred) substitution and closure.
//!
//! ## What we DON'T do here (yet)
//!
//! - Link-graph composition. Wiring `outputs: {port: [path]}` lives
//!   inside the `Value::Map` already, but lifting the rules of port
//!   matching during composition into a first-class operation needs
//!   a stronger `Link` schema than we currently have. Deferred to a
//!   future milestone — for now [`compose`] handles place-graph
//!   substitution only, which is what's needed for MAPK and most
//!   bigraphical-modelling examples.

use indexmap::IndexMap;

use crate::reaction::Pattern;
use crate::value::{Key, Path, StateMap, Value};

// ── Interfaces ──────────────────────────────────────────────────────

/// The inner face of a pattern `f : ⟨m, X⟩ → ⟨n, Y⟩`. Records the
/// positions of the `m` sites (the holes that composition fills).
///
/// `X` (the inner names) would describe link-graph endpoints awaiting
/// connection from outside. We don't model those yet — see the module
/// docs for the deferral.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InnerFace {
    /// Positions of each [`Pattern::Site`], in tree-walk order.
    pub sites: Vec<Path>,
}

/// The outer face of a pattern: the regions (top-level dict keys)
/// it exposes upward.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OuterFace {
    /// Top-level keys of the pattern. For Milner's "merge" this is
    /// a single root; for a tensor it's the concatenated keys.
    pub roots: Vec<Key>,
}

impl InnerFace {
    pub fn width(&self) -> usize {
        self.sites.len()
    }

    /// True for the trivial inner face `ε = ⟨0, ∅⟩`.
    pub fn is_epsilon(&self) -> bool {
        self.sites.is_empty()
    }
}

impl OuterFace {
    pub fn width(&self) -> usize {
        self.roots.len()
    }

    pub fn is_epsilon(&self) -> bool {
        self.roots.is_empty()
    }
}

/// Derive the inner and outer faces of a pattern.
///
/// Walks the pattern tree, recording every [`Pattern::Site`] position
/// as an inner-face site and every top-level dict key as an outer-face
/// root. A pattern with no sites is **ground** — its inner face is
/// `ε`, and [`is_ground`] returns true.
pub fn interfaces(pattern: &Pattern) -> (InnerFace, OuterFace) {
    let mut sites = Vec::new();
    let mut roots = Vec::new();

    match pattern {
        Pattern::Map(map) => {
            for (k, child) in map {
                if k.starts_with('_') {
                    continue;
                }
                roots.push(k.clone());
                let mut path = vec![k.clone()];
                walk_sites(child, &mut path, &mut sites);
            }
        }
        // Single non-map at the top is one "anonymous" root.
        _ => {
            let mut path = Vec::new();
            walk_sites(pattern, &mut path, &mut sites);
        }
    }

    (InnerFace { sites }, OuterFace { roots })
}

fn walk_sites(pattern: &Pattern, path: &mut Vec<Key>, sites: &mut Vec<Path>) {
    match pattern {
        Pattern::Site => sites.push(path.clone()),
        Pattern::Map(map) => {
            for (k, child) in map {
                if k.starts_with('_') {
                    continue;
                }
                path.push(k.clone());
                walk_sites(child, path, sites);
                path.pop();
            }
        }
        Pattern::List(items) => {
            for (i, child) in items.iter().enumerate() {
                path.push(Key::from(i.to_string()));
                walk_sites(child, path, sites);
                path.pop();
            }
        }
        _ => {}
    }
}

/// True if `pattern` is **ground** — no [`Pattern::Site`] holes.
/// Equivalent to the inner face being `ε`. Every concrete simulation
/// state corresponds to a ground pattern (no holes left to fill).
pub fn is_ground(pattern: &Pattern) -> bool {
    interfaces(pattern).0.is_epsilon()
}

/// The trivial inner face `ε = ⟨0, ∅⟩`. Unit of [`tensor`], the
/// domain of every ground bigraph.
pub fn epsilon() -> InnerFace {
    InnerFace::default()
}

// ── Composition ─────────────────────────────────────────────────────

/// Compose two patterns: `outer ∘ inner` substitutes `inner`'s roots
/// (its outer face) into `outer`'s sites (its inner face), one for
/// one in walk order.
///
/// Milner Def. 2.5 (p. 17). `compose` requires
/// `outer.inner_face.width() == inner.outer_face.width()`. The
/// composite has `inner`'s inner face and `outer`'s outer face — the
/// mediating interface disappears.
///
/// In our model: each [`Pattern::Site`] in `outer` is replaced by the
/// child rooted at the corresponding key in `inner`.
///
/// # Errors
///
/// Returns `Err(...)` if face widths don't match or if `inner` is
/// missing one of the named roots.
pub fn compose(outer: &Pattern, inner: &Pattern) -> Result<Pattern, String> {
    let (outer_inner, _) = interfaces(outer);
    let (_, inner_outer) = interfaces(inner);

    if outer_inner.width() != inner_outer.width() {
        return Err(format!(
            "compose: outer has {} sites but inner has {} roots — \
             face widths must match",
            outer_inner.width(),
            inner_outer.width(),
        ));
    }

    // Build the inner→filler map: for each root key, pull out the
    // pattern that root holds.
    let inner_map = match inner {
        Pattern::Map(m) => m,
        _ => {
            return Err("compose: inner must be a Pattern::Map".into());
        }
    };

    let mut result = outer.clone();
    for (site_path, root_key) in outer_inner.sites.iter().zip(inner_outer.roots.iter()) {
        let filler = inner_map
            .get(root_key)
            .ok_or_else(|| format!("compose: inner missing root {root_key:?}"))?
            .clone();
        set_pattern_at(&mut result, site_path, filler)?;
    }
    Ok(result)
}

/// Substitute `value` at `path` inside `pattern`. Walks down through
/// nested [`Pattern::Map`]s following the path, replacing whatever
/// is at the leaf.
fn set_pattern_at(pattern: &mut Pattern, path: &[Key], value: Pattern) -> Result<(), String> {
    if path.is_empty() {
        *pattern = value;
        return Ok(());
    }
    let mut current = pattern;
    for (i, step) in path.iter().enumerate() {
        match current {
            Pattern::Map(map) => {
                if i + 1 == path.len() {
                    map.insert(step.clone(), value);
                    return Ok(());
                }
                current = map
                    .get_mut(step)
                    .ok_or_else(|| format!("set_pattern_at: missing key {step:?}"))?;
            }
            _ => return Err(format!("set_pattern_at: non-map at step {step:?}")),
        }
    }
    Ok(())
}

// ── Tensor product ──────────────────────────────────────────────────

/// Tensor product `left ⊗ right`: places two patterns side by side.
///
/// Milner Def. 2.7 (p. 18). Both patterns must have disjoint top-level
/// (region) keys; the result's outer face concatenates the two and
/// its inner face concatenates the two sites lists. Inner names would
/// also concatenate but we don't model link-graph names yet.
///
/// # Errors
///
/// Returns `Err(...)` if the two patterns share any region key —
/// Milner requires disjoint supports.
pub fn tensor(left: &Pattern, right: &Pattern) -> Result<Pattern, String> {
    let lm = match left {
        Pattern::Map(m) => m.clone(),
        _ => return Err("tensor: left must be a Pattern::Map".into()),
    };
    let rm = match right {
        Pattern::Map(m) => m.clone(),
        _ => return Err("tensor: right must be a Pattern::Map".into()),
    };

    let overlap: Vec<&Key> = lm.keys()
        .filter(|k| !k.starts_with('_') && rm.contains_key(*k))
        .collect();
    if !overlap.is_empty() {
        let names: Vec<String> = overlap.iter().map(|k| k.to_string()).collect();
        return Err(format!(
            "tensor: schemas must have disjoint region keys but both \
             contain: {}",
            names.join(", "),
        ));
    }

    let mut merged = lm;
    for (k, v) in rm {
        merged.insert(k, v);
    }
    Ok(Pattern::Map(merged))
}

// ── Elementary bigraphs (Milner Defs. 3.1–3.5) ──────────────────────

/// The barren root `1 : 0 → 1`: one empty region, no sites, no nodes.
/// (Milner Def. 3.1, p. 28.)
///
/// In our model: a single root pointing at an empty Map.
pub fn barren(key: &str) -> Pattern {
    let mut m: IndexMap<Key, Pattern> = IndexMap::new();
    m.insert(Key::from(key), Pattern::Map(IndexMap::new()));
    Pattern::Map(m)
}

/// `merge_n : n → 1` — one root containing `n` sites. All `n` sites
/// are placed under a single root, so composition with `merge_n`
/// collapses `n` separate roots into one region. (Milner Def. 3.1.)
///
/// `merge_0 == barren(...)`.
pub fn merge(n: usize, root_key: &str) -> Pattern {
    if n == 0 {
        return barren(root_key);
    }
    let mut sites: IndexMap<Key, Pattern> = IndexMap::new();
    for i in 0..n {
        sites.insert(Key::from(format!("site{i}")), Pattern::Site);
    }
    let mut root: IndexMap<Key, Pattern> = IndexMap::new();
    root.insert(Key::from(root_key), Pattern::Map(sites));
    Pattern::Map(root)
}

/// A discrete ion `K_⃗x : 1 → ⟨1, {⃗x}⟩` — a single node with control
/// `control` and one site inside. (Milner Def. 3.4, p. 29.)
///
/// In our (link-graph-deferred) model an ion is just a sorted node
/// with a `Site`. The `names` vector is recorded as a `ports` list so
/// downstream tools that DO model link-graphs can find the open
/// names; today the list is informational only.
pub fn ion(control: &str, names: &[&str], site_key: &str) -> Pattern {
    let mut node: IndexMap<Key, Pattern> = IndexMap::new();
    node.insert(
        Key::from("_type"),
        Pattern::Atom(Value::String(control.to_string())),
    );
    node.insert(Key::from(site_key), Pattern::Site);
    if !names.is_empty() {
        let ports = names
            .iter()
            .map(|n| Pattern::Atom(Value::String((*n).to_string())))
            .collect::<Vec<_>>();
        node.insert(Key::from("ports"), Pattern::List(ports));
    }
    let mut root: IndexMap<Key, Pattern> = IndexMap::new();
    root.insert(Key::from(control), Pattern::Map(node));
    Pattern::Map(root)
}

// ── Concrete ground rendering ───────────────────────────────────────

/// Convert a ground pattern (no [`Pattern::Site`]s) to a concrete
/// [`Value`]. Useful when you've finished assembling a state via
/// [`compose`] / [`tensor`] and want to hand it off to the engine.
///
/// # Errors
///
/// Returns `Err(...)` if the pattern still has open holes (sites or
/// link variables).
pub fn to_value(pattern: &Pattern) -> Result<Value, String> {
    match pattern {
        Pattern::Site => Err("to_value: pattern still has open sites".into()),
        Pattern::LinkVar(name) => {
            Err(format!("to_value: pattern still has open link variable {name:?}"))
        }
        Pattern::Absent => {
            Err("to_value: pattern has Absent markers (only valid in redex)".into())
        }
        Pattern::Bind { .. } => {
            Err("to_value: pattern has Bind markers (only valid in redex)".into())
        }
        Pattern::Atom(v) => Ok(v.clone()),
        Pattern::Map(m) => {
            let mut out = StateMap::new();
            for (k, v) in m {
                out.insert(k.clone(), to_value(v)?);
            }
            Ok(Value::Map(out))
        }
        Pattern::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for p in items {
                out.push(to_value(p)?);
            }
            Ok(Value::List(out))
        }
    }
}

/// Lift a concrete `Value` into a ground pattern. Inverse of
/// [`to_value`] for values that don't contain pattern-only markers.
pub fn from_value(value: &Value) -> Pattern {
    match value {
        Value::Map(m) => {
            let mut out: IndexMap<Key, Pattern> = IndexMap::new();
            for (k, v) in m {
                out.insert(k.clone(), from_value(v));
            }
            Pattern::Map(out)
        }
        Value::List(items) => {
            Pattern::List(items.iter().map(from_value).collect())
        }
        other => Pattern::Atom(other.clone()),
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reaction::Pattern;

    fn val_str(s: &str) -> Value {
        Value::String(s.to_string())
    }

    #[test]
    fn ground_pattern_has_epsilon_inner_face() {
        let p = Pattern::map([(
            "x",
            Pattern::sort("Box", Vec::<(&str, Pattern)>::new()),
        )]);
        let (inner, outer) = interfaces(&p);
        assert!(inner.is_epsilon());
        assert_eq!(outer.roots, vec![Key::from("x")]);
        assert!(is_ground(&p));
    }

    #[test]
    fn site_shows_up_in_inner_face() {
        let p = Pattern::map([(
            "container",
            Pattern::sort("Box", [("hole", Pattern::site())]),
        )]);
        let (inner, outer) = interfaces(&p);
        assert_eq!(inner.width(), 1);
        assert_eq!(
            inner.sites[0],
            vec![Key::from("container"), Key::from("hole")]
        );
        assert_eq!(outer.roots, vec![Key::from("container")]);
        assert!(!is_ground(&p));
    }

    #[test]
    fn barren_is_ground_with_one_root() {
        let b = barren("region0");
        let (inner, outer) = interfaces(&b);
        assert!(inner.is_epsilon());
        assert_eq!(outer.roots, vec![Key::from("region0")]);
    }

    #[test]
    fn merge_n_has_n_sites_under_one_root() {
        let m = merge(3, "root");
        let (inner, outer) = interfaces(&m);
        assert_eq!(inner.width(), 3);
        assert_eq!(outer.roots, vec![Key::from("root")]);
    }

    #[test]
    fn ion_has_one_site_and_one_root() {
        let i = ion("K", &["x", "y"], "site0");
        let (inner, outer) = interfaces(&i);
        assert_eq!(inner.width(), 1);
        assert_eq!(outer.roots, vec![Key::from("K")]);
    }

    #[test]
    fn compose_substitutes_inner_root_into_outer_site() {
        // outer: { container: Box { body: Site } }     [1 site, 1 root]
        // inner: { body: Atom { value: 42 } }          [0 sites, 1 root]
        let outer = Pattern::map([(
            "container",
            Pattern::sort("Box", [("body", Pattern::site())]),
        )]);
        let inner = Pattern::map([(
            "body",
            Pattern::sort("Atom", [("value", Pattern::atom(Value::Int(42)))]),
        )]);
        let composed = compose(&outer, &inner).unwrap();

        // Composed should be: { container: Box { body: Atom { value: 42 } } }
        // — and ground (no sites left).
        assert!(is_ground(&composed));
        let v = to_value(&composed).unwrap();
        let int = v
            .get_path(&[
                Key::from("container"),
                Key::from("body"),
                Key::from("value"),
            ])
            .unwrap();
        assert_eq!(*int, Value::Int(42));
    }

    #[test]
    fn compose_rejects_mismatched_widths() {
        let outer = Pattern::map([("a", Pattern::sort("Box", [("h1", Pattern::site())]))]);
        let inner = Pattern::map([
            ("a", Pattern::atom(val_str("x"))),
            ("b", Pattern::atom(val_str("y"))),
        ]);
        let err = compose(&outer, &inner).unwrap_err();
        assert!(err.contains("face widths must match"));
    }

    #[test]
    fn tensor_concatenates_disjoint_regions() {
        let a = Pattern::map([("x", Pattern::sort("A", Vec::<(&str, Pattern)>::new()))]);
        let b = Pattern::map([("y", Pattern::sort("B", Vec::<(&str, Pattern)>::new()))]);
        let t = tensor(&a, &b).unwrap();
        let (_, outer) = interfaces(&t);
        assert_eq!(outer.width(), 2);
        assert!(outer.roots.contains(&Key::from("x")));
        assert!(outer.roots.contains(&Key::from("y")));
    }

    #[test]
    fn tensor_rejects_overlapping_keys() {
        let a = Pattern::map([("x", Pattern::sort("A", Vec::<(&str, Pattern)>::new()))]);
        let b = Pattern::map([("x", Pattern::sort("B", Vec::<(&str, Pattern)>::new()))]);
        let err = tensor(&a, &b).unwrap_err();
        assert!(err.contains("disjoint region keys"));
    }

    #[test]
    fn ion_composed_with_atom_yields_concrete_node() {
        // K_x ∘ atom → K with an atom inside, both ground.
        let k = ion("K", &["x"], "site0");
        let atom = Pattern::map([(
            "site0",
            Pattern::sort("Atom", [("value", Pattern::atom(Value::Int(7)))]),
        )]);
        let composed = compose(&k, &atom).unwrap();
        assert!(is_ground(&composed));
        let v = to_value(&composed).unwrap();
        // Result has the K root, with Atom inside under site0.
        let typ = v
            .get_path(&[Key::from("K"), Key::from("site0"), Key::from("_type")])
            .unwrap();
        assert_eq!(*typ, val_str("Atom"));
    }

    #[test]
    fn to_value_fails_on_open_pattern() {
        let p = Pattern::map([("x", Pattern::site())]);
        let err = to_value(&p).unwrap_err();
        assert!(err.contains("open sites"));
    }

    #[test]
    fn roundtrip_from_to_value() {
        let v = Value::tree([
            ("_type", val_str("Box")),
            ("inner", Value::Int(42)),
        ]);
        let p = from_value(&v);
        let v2 = to_value(&p).unwrap();
        assert_eq!(v, v2);
    }
}
