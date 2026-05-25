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
        // Map ⊓ Tree where the Map's value is a process-node `Link`: a
        // **dynamic collection** (cells that grow/divide). The per-key `Tree` is
        // only a snapshot of the entries present *right now*, NOT a fixed struct,
        // so KEEP the `Map` — every entry, including future daughters, is typed
        // by its uniform `Link` value. Without this, `from_state`'s
        // `resolve(infer(state), declared)` collapses a declared
        // `cells: Map[Cell]` to `Tree{c0:…}` at init: the `_divide` sentinel
        // (a Map op) stops applying, and a daughter `c0_0` — absent from the
        // snapshot's branches — falls through to `Schema::Any`. The declared
        // Link value is authoritative for a process node (inference can't improve
        // it), so we keep it as-is. (cells-and-division.md §6.2.)
        (Map { value }, Tree { .. }) | (Tree { .. }, Map { value }) if value.is_link_kind() => {
            Map { value: value.clone() }
        }
        // Map ⊓ Tree (data map): the per-key `Tree` is the **more specific**
        // type — `Tree{k:V}` refines `Map[V]` (a fixed struct IS a dict, not
        // vice versa), so the meet keeps the Tree. Push the Map's uniform
        // element type onto each branch so declared element semantics (e.g.
        // additive `Array`, `Delta`) survive there. The argument order keeps the
        // **second** (update/declared) side authoritative over the inferred
        // snapshot (so a declared per-key `Array` isn't dropped for the `List`
        // inference saw).
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

// ───────────────────────────────────────────────────────────────────────
// promote — the *local* resolve (sparse projection of `library` over `sparse`)
// ───────────────────────────────────────────────────────────────────────
//
// `resolve(library, sparse)` walks every branch of `library` (the full known
// schema — typically the whole Composite/state schema), even branches `sparse`
// never touched. For the per-tick apply path that is wasted work: the update
// only lands on a few paths, and we need only the typed nodes along them.
//
// `promote` walks only `sparse`'s structure. At each node:
//   - both typed leaves (non-Tree) → `resolve(library, sparse)` (so e.g.
//     `resolve(Float, overwrite[float]) = overwrite[float]` composes the
//     port's wrapper over the slot's type — the diffusion / overwrite-port fix)
//   - both `Tree` → recurse over **sparse's** branches only, substituting
//     `library`'s typed branch where it exists; keep `sparse`'s where library
//     is missing
//   - library missing (`Any`) → keep `sparse`
//   - sparse missing (`Any`) → keep `library`
//
// Faithful port of `bigraph_schema/methods/resolve.py::promote`. Law #5:
// `promote(lib, sparse)` agrees with `resolve(lib, sparse)` on every path
// `sparse` touches, and leaves the rest of `lib` untouched.
pub fn promote(library: &Schema, sparse: &Schema) -> Schema {
    use Schema::*;

    // No library type here → keep sparse; nothing in sparse → keep library.
    if is_any(library) {
        return sparse.clone();
    }
    if is_any(sparse) {
        return library.clone();
    }

    match (library, sparse) {
        // Both per-key trees: walk only sparse's branches.
        (Tree { branches: lib }, Tree { branches: sp }) => {
            let mut result: IndexMap<Key, Schema> = IndexMap::new();
            for (k, sv) in sp {
                let promoted = match lib.get(k) {
                    Some(lv) => promote(lv, sv),
                    None => sv.clone(),
                };
                result.insert(k.clone(), promoted);
            }
            Tree { branches: result }
        }
        // Anything else (typed Node leaves, Map/Array/List/wrappers, …):
        // fall through to the global join. resolve already restricts itself
        // to the two schemas it's given, so on a leaf pair promote ≡ resolve.
        _ => resolve(library, sparse),
    }
}

// ───────────────────────────────────────────────────────────────────────
// generalize — the schema **meet** (greatest sort both refine)
// ───────────────────────────────────────────────────────────────────────
//
// Dual to `resolve` (the join). Where the two differ: `generalize` forgets
// update-*modifiers* (`Maybe`/`Overwrite`/`Const`/`Quote`) — the meet of a
// wrapped and an unwrapped type is the underlying data type — and a numeric
// pair generalizes to the wider `Float`. Tree/Map/List/etc. union like
// `resolve`. Faithful port of `bigraph_schema/methods/generalize.py` for the
// sorts prism has.
//
// Laws (meet-semilattice): idempotent `generalize(s,s) ≡ s`, commutative up
// to default, associative; `generalize(Any, s) ≡ s` (`Any` is the identity).
pub fn generalize(current: &Schema, update: &Schema) -> Schema {
    use Schema::*;

    if is_any(current) {
        return update.clone();
    }
    if is_any(update) {
        return current.clone();
    }
    // Forget update-modifiers — the meet drops the wrapper, generalizing to
    // the underlying data type.
    if let Some(inner) = strip_modifier(current) {
        return generalize(inner, update);
    }
    if let Some(inner) = strip_modifier(update) {
        return generalize(current, inner);
    }

    match (current, update) {
        // Numeric meet → Float (the wider type), keeping a present default.
        (Integer { default: ci }, Float { default: uf })
        | (Float { default: uf }, Integer { default: ci }) => Float {
            default: uf.or_else(|| ci.map(|i| i as f64)),
        },
        (Float { default: c }, Float { default: u }) => Float { default: u.or(*c) },
        (Integer { default: c }, Integer { default: u }) => Integer { default: u.or(*c) },
        (Delta { default: c }, Delta { default: u }) => Delta { default: u.or(*c) },
        (Delta { default: c }, Float { default: u }) | (Float { default: u }, Delta { default: c }) => {
            Delta { default: u.or(*c) }
        }
        (Bool { default: c }, Bool { default: u }) => Bool { default: u.or(*c) },
        (String { default: c }, String { default: u }) => {
            String { default: u.clone().or_else(|| c.clone()) }
        }
        (Enum { values: cv, default: cd }, Enum { values: uv, default: ud }) => {
            let mut values = cv.clone();
            for v in uv {
                if !values.contains(v) {
                    values.push(v.clone());
                }
            }
            Enum { values, default: ud.clone().or_else(|| cd.clone()) }
        }

        // Containers: union / recurse like resolve.
        (List { element: a }, List { element: b }) => List { element: Box::new(generalize(a, b)) },
        (Array { shape: cs, element: ce }, Array { shape: us, element: ue }) => Array {
            shape: max_shape(cs, us),
            element: Box::new(generalize(ce, ue)),
        },
        (Map { value: a }, Map { value: b }) => Map { value: Box::new(generalize(a, b)) },
        (Tree { branches: a }, Tree { branches: b }) => Tree { branches: union_generalize(a, b) },
        (Tuple { elements: a }, Tuple { elements: b }) => Tuple { elements: zip_generalize(a, b) },
        (RecursiveTree { leaf: a }, RecursiveTree { leaf: b }) => {
            RecursiveTree { leaf: Box::new(generalize(a, b)) }
        }

        (a, b) if a == b => a.clone(),
        // Incompatible: the update side wins (matches resolve's total fallback).
        (_, b) => b.clone(),
    }
}

// ───────────────────────────────────────────────────────────────────────
// refines — the lattice **order**, derived from the join
// ───────────────────────────────────────────────────────────────────────

/// `refines(specific, general)` — the partial order induced by the join:
/// `specific ⊒ general  ⟺  resolve(specific, general) == specific`.
///
/// This is **derived from [`resolve`]**, not a new operation — it is
/// `resolve` plus `==`, so it stays inside the algebra (no new primitive,
/// no law to port). It is the predicate behind process-contract
/// substitutability — a fulfiller's contract must `refine` the demanded one
/// (`docs/process-contracts.md`) — and, generally, "is `specific` an
/// admissible refinement of `general`". `refines(s, Any)` is always true
/// (`Any` is ⊥); `refines` is reflexive and transitive (it is a lattice
/// order). The companion meet-form `general` ⊒-relation is
/// `generalize(s, g) == g`.
pub fn refines(specific: &Schema, general: &Schema) -> bool {
    resolve(specific, general) == *specific
}

/// The inner schema of an update-modifier wrapper, if any. `generalize`
/// forgets these; `resolve` keeps them.
fn strip_modifier(s: &Schema) -> Option<&Schema> {
    match s {
        Schema::Maybe { inner }
        | Schema::Overwrite { inner }
        | Schema::Const { inner }
        | Schema::Quote { inner } => Some(inner),
        _ => None,
    }
}

/// Union two branch maps (current order first), generalizing shared keys.
fn union_generalize(a: &IndexMap<Key, Schema>, b: &IndexMap<Key, Schema>) -> IndexMap<Key, Schema> {
    let mut out = a.clone();
    for (k, bv) in b {
        match out.get(k) {
            Some(av) => {
                let r = generalize(av, bv);
                out.insert(k.clone(), r);
            }
            None => {
                out.insert(k.clone(), bv.clone());
            }
        }
    }
    out
}

/// Generalize two sequences element-wise, keeping the longer tail.
fn zip_generalize(a: &[Schema], b: &[Schema]) -> Vec<Schema> {
    let mut out: Vec<Schema> = a.iter().zip(b).map(|(x, y)| generalize(x, y)).collect();
    if a.len() > b.len() {
        out.extend_from_slice(&a[b.len()..]);
    } else if b.len() > a.len() {
        out.extend_from_slice(&b[a.len()..]);
    }
    out
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
        // resolved with declared Map(Array) → Tree{glucose: Array, acetate:
        // Array}. The per-key Tree is the more-specific type (it wins the
        // meet), and the Map's additive `Array` element is pushed onto each
        // branch so per-cell updates stay additive (mass-conserving), not
        // replacing the inferred `List`.
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

    // ── promote ──────────────────────────────────────────────────────

    #[test]
    fn promote_leaf_pair_is_resolve() {
        // The diffusion fix: a loose port (Map[Any]) promoted onto an
        // additive slot (Map[Array]) keeps the Array — apply stays additive.
        let library = Schema::map(Schema::Array { shape: vec![3, 3], element: Box::new(Schema::float()) });
        let sparse = Schema::map(Schema::Any);
        let Schema::Map { value } = promote(&library, &sparse) else { panic!("expected Map") };
        assert!(matches!(*value, Schema::Array { .. }), "Array survived the loose port");
    }

    #[test]
    fn promote_overwrite_port_wins_over_additive_slot() {
        // A port declaring overwrite[float] over an additive Float slot:
        // promote composes the wrapper so apply uses replace semantics.
        let library = Schema::float();
        let sparse = Schema::overwrite(Schema::float());
        assert!(matches!(promote(&library, &sparse), Schema::Overwrite { .. }));
    }

    #[test]
    fn promote_walks_only_sparse_branches() {
        // library has {a, b}; sparse touches only {a}. The result mirrors
        // sparse (only `a`), with library's typed `a` substituted.
        let library = Schema::tree([
            ("a", Schema::Array { shape: vec![2], element: Box::new(Schema::float()) }),
            ("b", Schema::float()),
        ]);
        let sparse = Schema::tree([("a", Schema::Any)]);
        let Schema::Tree { branches } = promote(&library, &sparse) else { panic!() };
        assert_eq!(branches.len(), 1, "only sparse's touched branch is present");
        assert!(matches!(branches.get("a"), Some(Schema::Array { .. })), "library's Array substituted");
    }

    #[test]
    fn promote_keeps_sparse_where_library_missing() {
        let library = Schema::tree([("a", Schema::float())]);
        let sparse = Schema::tree([("z", Schema::overwrite(Schema::float()))]);
        let Schema::Tree { branches } = promote(&library, &sparse) else { panic!() };
        assert!(matches!(branches.get("z"), Some(Schema::Overwrite { .. })));
    }

    // ── generalize (meet) ────────────────────────────────────────────

    #[test]
    fn generalize_forgets_modifiers() {
        // The meet of overwrite[float] and float is the underlying float.
        assert_eq!(generalize(&Schema::overwrite(Schema::float()), &Schema::float()), Schema::float());
        assert_eq!(generalize(&Schema::float(), &Schema::overwrite(Schema::float())), Schema::float());
    }

    #[test]
    fn generalize_numeric_widens_to_float() {
        assert!(matches!(generalize(&Schema::integer(), &Schema::float()), Schema::Float { .. }));
        assert!(matches!(generalize(&Schema::float(), &Schema::integer()), Schema::Float { .. }));
    }

    #[test]
    fn generalize_any_is_identity() {
        assert_eq!(generalize(&Schema::Any, &Schema::float()), Schema::float());
        assert_eq!(generalize(&Schema::float(), &Schema::Any), Schema::float());
    }

    // ── refines (the join order) ──────────────────────────────────────

    #[test]
    fn refines_is_the_join_order() {
        // `Any` is ⊥: everything refines it; it refines nothing concrete.
        assert!(refines(&Schema::float(), &Schema::Any));
        assert!(!refines(&Schema::Any, &Schema::float()));
        // Reflexive.
        assert!(refines(&Schema::float(), &Schema::float()));
        // Array ⊒ List (the more specific sequence refines the looser one).
        let arr = Schema::Array { shape: vec![3], element: Box::new(Schema::float()) };
        let list = Schema::List { element: Box::new(Schema::Any) };
        assert!(refines(&arr, &list));
        assert!(!refines(&list, &arr));
    }
}
