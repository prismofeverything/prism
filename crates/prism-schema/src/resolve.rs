//! `resolve` — combine two schemas into the most-specific schema that
//! satisfies both. The keystone schema algebra.
//!
//! Faithful port of `bigraph_schema/methods/resolve.py` (plum
//! multi-dispatch on schema-type *pairs* → a Rust `match` over
//! `(current, update)` variant pairs). `resolve(current, update)` is how
//! two declarations of the same location reconcile: an inferred `List` and
//! a declared `Array` resolve to `Array`; two `Tree`s union their branches
//! and resolve each; the more-specific subtype wins; the more-informative
//! default is kept.
//!
//! This replaces prism's ad-hoc stand-ins — the 17-line `Schema::resolve`
//! stub, `Schema::infer_and_merge`, and chrysalis's `overlay_apply_types`
//! — which each re-invented a slice of this. (The path-aware `promote`
//! form used by the apply path is the next increment; see `resolve_path`.)

use indexmap::IndexMap;

use crate::schema::Schema;
use crate::value::Key;

/// Whether a schema's default slot is absent (upstream `is_empty` on the
/// default). Drives the Integer/Float "keep the informative default" rules.
fn empty_default(s: &Schema) -> bool {
    match s {
        Schema::Integer { default } => default.is_none(),
        Schema::Float { default } | Schema::Delta { default } => default.is_none(),
        Schema::Bool { default } => default.is_none(),
        Schema::String { default } => default.is_none(),
        Schema::Enum { default, .. } => default.is_none(),
        _ => true,
    }
}

/// "Emptiness" rank for the generic fallback: `Any` is the least-specific
/// (an unconstrained hole), so anything else wins over it.
fn is_any(s: &Schema) -> bool {
    matches!(s, Schema::Any)
}

/// Combine two schemas. Symmetric in spirit (the more-specific type wins
/// regardless of side); on a true tie `update` is preferred, mirroring the
/// upstream convention that the second argument is the refining schema.
pub fn resolve(current: &Schema, update: &Schema) -> Schema {
    use Schema::*;

    // ── Unconstrained side yields to the other (upstream Empty/Node()). ──
    if is_any(current) {
        return update.clone();
    }
    if is_any(update) {
        return current.clone();
    }

    match (current, update) {
        // ── Wrappers (upstream `Wrap`: Maybe / Overwrite / Const / Quote).
        // Same wrapper → wrap the resolved inner, preferring update's inner.
        // Mixed wrapper/non-wrapper → keep the wrapper, resolving inside.
        (Maybe { inner: a }, Maybe { inner: b }) => Maybe { inner: Box::new(resolve(a, b)) },
        (Overwrite { inner: a }, Overwrite { inner: b }) => {
            Overwrite { inner: Box::new(resolve(a, b)) }
        }
        (Const { inner: a }, Const { inner: b }) => Const { inner: Box::new(resolve(a, b)) },
        (Quote { inner: a }, Quote { inner: b }) => Quote { inner: Box::new(resolve(a, b)) },
        (Maybe { inner }, other) => Maybe { inner: Box::new(resolve(inner, other)) },
        (other, Maybe { inner }) => Maybe { inner: Box::new(resolve(other, inner)) },
        (Overwrite { inner }, other) => Overwrite { inner: Box::new(resolve(inner, other)) },
        (other, Overwrite { inner }) => Overwrite { inner: Box::new(resolve(other, inner)) },
        (Const { inner }, other) => Const { inner: Box::new(resolve(inner, other)) },
        (other, Const { inner }) => Const { inner: Box::new(resolve(other, inner)) },

        // ── Numeric: Integer ⊓ Float keeps Float (wider) but preserves a
        // present default; same-type prefers the side that carries a default.
        (Integer { default: ci }, Float { default: uf }) => Float {
            default: uf.or_else(|| ci.map(|i| i as f64)),
        },
        (Float { default: cf }, Integer { default: ui }) => Float {
            default: cf.or_else(|| ui.map(|i| i as f64)),
        },
        (Float { default: c }, Float { default: u }) => Float { default: u.or(*c) },
        (Integer { default: c }, Integer { default: u }) => Integer { default: u.or(*c) },
        (Delta { default: c }, Delta { default: u }) => Delta { default: u.or(*c) },
        // Delta vs Float: Delta is the additive refinement — keep it.
        (Delta { default: c }, Float { default: u }) | (Float { default: u }, Delta { default: c }) => {
            Delta { default: u.or(*c) }
        }
        (Bool { default: c }, Bool { default: u }) => Bool { default: u.or(*c) },
        (String { default: c }, String { default: u }) => String { default: u.clone().or_else(|| c.clone()) },
        (Enum { values: cv, default: cd }, Enum { values: uv, default: ud }) => {
            // Union the allowed values (current order first), keep a default.
            let mut values = cv.clone();
            for v in uv {
                if !values.contains(v) {
                    values.push(v.clone());
                }
            }
            Enum { values, default: ud.clone().or_else(|| cd.clone()) }
        }

        // ── Sequences. Array is the specific numeric form; it wins over a
        // List (which is often an inferred empty-default fallback).
        (Array { shape: cs, element: ce }, Array { shape: us, element: ue }) => Array {
            shape: max_shape(cs, us),
            element: Box::new(resolve(ce, ue)),
        },
        (List { .. }, Array { .. }) => update.clone(),
        (Array { .. }, List { .. }) => current.clone(),
        (List { element: a }, List { element: b }) => List { element: Box::new(resolve(a, b)) },
        (List { .. }, Tuple { .. }) => update.clone(),
        (Tuple { .. }, List { .. }) => current.clone(),
        (Tuple { elements: a }, Tuple { elements: b }) => Tuple { elements: zip_resolve(a, b) },

        // ── Maps / trees / recursive trees.
        // Tree{branches} ≈ upstream dict: union branches, resolve each.
        (Tree { branches: a }, Tree { branches: b }) => Tree { branches: union_resolve(a, b) },
        // Map{value} ≈ upstream Map: uniform element.
        (Map { value: a }, Map { value: b }) => Map { value: Box::new(resolve(a, b)) },
        // Map ⊓ Tree: the per-key Tree is more specific; push the Map's
        // uniform element type onto each branch so declared element types
        // (e.g. Array) survive.
        (Map { value }, Tree { branches }) => Tree {
            branches: branches.iter().map(|(k, v)| (k.clone(), resolve(value, v))).collect(),
        },
        (Tree { branches }, Map { value }) => Tree {
            branches: branches.iter().map(|(k, v)| (k.clone(), resolve(v, value))).collect(),
        },
        // RecursiveTree{leaf} ≈ upstream Tree[leaf].
        (RecursiveTree { leaf: a }, RecursiveTree { leaf: b }) => {
            RecursiveTree { leaf: Box::new(resolve(a, b)) }
        }
        (RecursiveTree { leaf }, Map { value }) | (Map { value }, RecursiveTree { leaf }) => {
            RecursiveTree { leaf: Box::new(resolve(leaf, value)) }
        }

        // ── Links: resolve the port maps and keep the most specific kind.
        // CompositeLink ⊐ ProcessLink ⊐ Link; StepLink is its own.
        (
            CompositeLink { inputs: ci, outputs: co, interval, inner_schema },
            other,
        ) if is_link_like(other) => {
            let (oi, oo) = link_ports(other);
            CompositeLink {
                inputs: union_resolve(ci, oi),
                outputs: union_resolve(co, oo),
                interval: *interval,
                inner_schema: inner_schema.clone(),
            }
        }
        (other, CompositeLink { inputs: ci, outputs: co, interval, inner_schema })
            if is_link_like(other) =>
        {
            let (oi, oo) = link_ports(other);
            CompositeLink {
                inputs: union_resolve(oi, ci),
                outputs: union_resolve(oo, co),
                interval: *interval,
                inner_schema: inner_schema.clone(),
            }
        }
        (ProcessLink { inputs: ai, outputs: ao, interval }, other) if is_link_like(other) => {
            let (bi, bo) = link_ports(other);
            ProcessLink {
                inputs: union_resolve(ai, bi),
                outputs: union_resolve(ao, bo),
                interval: *interval,
            }
        }
        (other, ProcessLink { inputs: ai, outputs: ao, interval }) if is_link_like(other) => {
            let (bi, bo) = link_ports(other);
            ProcessLink {
                inputs: union_resolve(bi, ai),
                outputs: union_resolve(bo, ao),
                interval: *interval,
            }
        }
        (StepLink { inputs: ai, outputs: ao, priority }, other) if is_link_like(other) => {
            let (bi, bo) = link_ports(other);
            StepLink {
                inputs: union_resolve(ai, bi),
                outputs: union_resolve(ao, bo),
                priority: *priority,
            }
        }
        (Link { inputs: ai, outputs: ao, temporal }, Link { inputs: bi, outputs: bo, temporal: bt }) => {
            Link {
                inputs: union_resolve(ai, bi),
                outputs: union_resolve(ao, bo),
                temporal: bt.or(*temporal),
            }
        }

        // ── Custom: same registered name → resolve parameters; otherwise
        // the named type is opaque, prefer the update.
        (Custom { name: cn, parameters: cp }, Custom { name: un, parameters: up }) if cn == un => {
            Custom { name: cn.clone(), parameters: union_resolve(cp, up) }
        }

        // ── Generic fallback: equal → current; else prefer the update
        // (the refining schema). Mirrors upstream's (Node, Node) default.
        (a, b) if a == b => a.clone(),
        (current, update) => {
            // Carry a present default across an otherwise-replacing refine.
            if empty_default(update) && !empty_default(current) {
                with_default_from(update, current)
            } else {
                update.clone()
            }
        }
    }
}

/// Element-wise max of two array shapes; the longer tail is kept.
fn max_shape(a: &[usize], b: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = a.iter().zip(b).map(|(x, y)| (*x).max(*y)).collect();
    if a.len() > b.len() {
        out.extend_from_slice(&a[b.len()..]);
    } else if b.len() > a.len() {
        out.extend_from_slice(&b[a.len()..]);
    }
    out
}

/// Resolve two sequences element-wise, keeping the longer tail.
fn zip_resolve(a: &[Schema], b: &[Schema]) -> Vec<Schema> {
    let mut out: Vec<Schema> = a.iter().zip(b).map(|(x, y)| resolve(x, y)).collect();
    if a.len() > b.len() {
        out.extend_from_slice(&a[b.len()..]);
    } else if b.len() > a.len() {
        out.extend_from_slice(&b[a.len()..]);
    }
    out
}

/// Union two branch maps (current order first), resolving shared keys.
fn union_resolve(a: &IndexMap<Key, Schema>, b: &IndexMap<Key, Schema>) -> IndexMap<Key, Schema> {
    let mut out = a.clone();
    for (k, bv) in b {
        match out.get(k) {
            Some(av) => {
                let r = resolve(av, bv);
                out.insert(k.clone(), r);
            }
            None => {
                out.insert(k.clone(), bv.clone());
            }
        }
    }
    out
}

fn is_link_like(s: &Schema) -> bool {
    matches!(
        s,
        Schema::Link { .. }
            | Schema::StepLink { .. }
            | Schema::ProcessLink { .. }
            | Schema::CompositeLink { .. }
    )
}

/// The (inputs, outputs) port maps of any link-like schema.
fn link_ports(s: &Schema) -> (&IndexMap<Key, Schema>, &IndexMap<Key, Schema>) {
    match s {
        Schema::Link { inputs, outputs, .. }
        | Schema::StepLink { inputs, outputs, .. }
        | Schema::ProcessLink { inputs, outputs, .. }
        | Schema::CompositeLink { inputs, outputs, .. } => (inputs, outputs),
        _ => unreachable!("link_ports called on non-link"),
    }
}

/// Clone `target` but adopt `source`'s default (for the refine fallback).
fn with_default_from(target: &Schema, source: &Schema) -> Schema {
    use Schema::*;
    match (target, source) {
        (Float { .. }, Float { default }) => Float { default: *default },
        (Integer { .. }, Integer { default }) => Integer { default: *default },
        (Delta { .. }, Delta { default }) => Delta { default: *default },
        (Bool { .. }, Bool { default }) => Bool { default: *default },
        (String { .. }, String { default }) => String { default: default.clone() },
        _ => target.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_yields_to_the_specific_side() {
        assert_eq!(resolve(&Schema::Any, &Schema::float()), Schema::float());
        assert_eq!(resolve(&Schema::float(), &Schema::Any), Schema::float());
    }

    #[test]
    fn array_wins_over_list_either_order() {
        let arr = Schema::Array { shape: vec![3, 3], element: Box::new(Schema::float()) };
        let list = Schema::List { element: Box::new(Schema::Any) };
        assert_eq!(resolve(&list, &arr), arr);
        assert_eq!(resolve(&arr, &list), arr);
    }

    #[test]
    fn map_pushes_element_onto_tree_branches() {
        // The diffusion case: inferred Tree{glucose: List, acetate: List}
        // resolved with declared Map(Array) → Tree{glucose: Array, ...}.
        let tree = Schema::Tree {
            branches: IndexMap::from([
                (Key::from("glucose"), Schema::List { element: Box::new(Schema::Any) }),
                (Key::from("acetate"), Schema::List { element: Box::new(Schema::Any) }),
            ]),
        };
        let map = Schema::Map {
            value: Box::new(Schema::Array { shape: vec![3, 3], element: Box::new(Schema::float()) }),
        };
        let r = resolve(&tree, &map);
        let Schema::Tree { branches } = r else { panic!("expected Tree") };
        assert!(matches!(branches.get("glucose"), Some(Schema::Array { .. })));
        assert!(matches!(branches.get("acetate"), Some(Schema::Array { .. })));
    }

    #[test]
    fn trees_union_branches_and_resolve_shared() {
        let a = Schema::Tree {
            branches: IndexMap::from([
                (Key::from("x"), Schema::float()),
                (Key::from("y"), Schema::Any),
            ]),
        };
        let b = Schema::Tree {
            branches: IndexMap::from([
                (Key::from("y"), Schema::integer()),
                (Key::from("z"), Schema::string()),
            ]),
        };
        let Schema::Tree { branches } = resolve(&a, &b) else { panic!() };
        assert_eq!(branches.len(), 3, "x, y, z unioned");
        assert!(matches!(branches.get("x"), Some(Schema::Float { .. })));
        assert!(matches!(branches.get("y"), Some(Schema::Integer { .. })), "Any⊓Integer = Integer");
        assert!(matches!(branches.get("z"), Some(Schema::String { .. })));
    }

    #[test]
    fn integer_float_keeps_float_and_default() {
        let r = resolve(&Schema::Integer { default: Some(3) }, &Schema::Float { default: None });
        assert_eq!(r, Schema::Float { default: Some(3.0) });
    }

    #[test]
    fn delta_refines_float() {
        assert!(matches!(
            resolve(&Schema::float(), &Schema::Delta { default: None }),
            Schema::Delta { .. }
        ));
    }
}
