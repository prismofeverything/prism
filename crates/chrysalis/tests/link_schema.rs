//! **#61a — a `link`'s declared schema is first-class on its pool slot.**
//!
//! `link name :: T = default` declares a value-bearing hyperedge; the engine
//! shares it as ONE slot `name` (resolved by `resolve_link` up the place graph).
//! The MERGE on that slot — multiple attached ports reconciling their writes —
//! must be driven by `T`, in the schema algebra, not by whatever the runtime
//! value happens to be (the [[feedback_schema_algebra]] discipline: link
//! behavior = the slot's schema reconcile, never a side-channel).
//!
//! Before the fix, `collect_branches` skipped `Expr::LinkDecl`, so the slot got
//! NO declared schema and was typed only by `infer` over the default value — a
//! `map[Reaction]` pool seeded `{}` inferred to an empty/`Any` map, losing the
//! element type, so an `_add` rode the sentinel structurally instead of merging
//! per-element through `Reaction`'s methods. This pins the declared `T` onto the
//! slot. (The end-to-end AlChemy link tests — `shared_link_alchemy`,
//! `outer_link_alchemy`, `distributed_alchemy` — are the behavioral guards.)

use chrysalis::ast::Def;
use chrysalis::schema::composite_inner_schema;
use prism_schema::Schema;

/// Build the composite's inner-state schema (the `Tree` whose branches are its
/// slots), as the engine derives it for a subengine.
fn inner_branches(src: &str, composite: &str) -> indexmap::IndexMap<prism_schema::Key, Schema> {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let def = program
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Composite(c) if c.name == composite => Some(c),
            _ => None,
        })
        .expect("the composite def");
    match composite_inner_schema(def, &program) {
        Schema::Tree { branches } => branches,
        other => panic!("a composite inner schema is a Tree, got {other:?}"),
    }
}

const SRC: &str = r#"
process Tick ~{n :: Float} ->{n :: Float} ( { n: 1.0 } )

composite Lab ->{ n :: Float @ n } (
  n: 0.0 |
  link reactions :: map[Reaction] = {} |
  link counter = 0.0 |
  tick: Tick ~{n: n} ->{n: n}
)
"#;

#[test]
fn a_typed_link_slot_carries_its_declared_element_type() {
    let branches = inner_branches(SRC, "Lab");

    // The `link reactions :: map[Reaction]` slot is typed by its declared `T`:
    // a `Map` whose ELEMENT is the dispatchable `Custom("Reaction")` — NOT an
    // inferred `Any`/empty map. This is what makes the pool merge schema-driven
    // (an `_add` realizes each element through `Reaction`'s methods).
    let reactions = branches
        .get(&prism_schema::Key::from("reactions"))
        .unwrap_or_else(|| panic!("the link slot `reactions` must be a declared branch: {branches:?}"));
    match reactions {
        Schema::Map { value } => match value.as_ref() {
            Schema::Custom { name, .. } => {
                assert_eq!(name, "Reaction", "the map element is the declared type");
            }
            other => panic!("expected `Map{{Custom(Reaction)}}`, the element was {other:?}"),
        },
        other => panic!("expected the `reactions` slot to be a typed `Map`, got {other:?}"),
    }
}

#[test]
fn an_untyped_link_is_left_to_infer() {
    let branches = inner_branches(SRC, "Lab");

    // `link counter = 0.0` (no `:: T`) contributes NO declared schema — it falls
    // through to `infer`, which types the slot from the default value at engine
    // init (exactly like a pure-data `KeyedEntry`). So it is absent from the
    // DECLARED branches here. (Pinning the declared type is opt-in via `:: T`;
    // a bare link is not silently stamped `Any`.)
    assert!(
        !branches.contains_key(&prism_schema::Key::from("counter")),
        "an untyped link must not appear as a declared branch (infer types it): {branches:?}"
    );
}
