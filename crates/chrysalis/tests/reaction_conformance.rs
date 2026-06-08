//! Conformance / invariant tests for the reaction firing system.
//!
//! The reaction pipeline has ONE matcher (`prism_schema::find_matches`) but the
//! REACTUM → delta step branches: a STRUCTURAL reactum (a pure template) fires
//! via prism's native `instantiate`; a COMPUTED reactum (containing a method /
//! `if` / arithmetic) is an Expr evaluated against the match bindings
//! (`reactum_fn` → `reaction_delta`). These tests PIN the invariant that the two
//! paths agree — a reaction's effect is its MEANING, not an artifact of which
//! path `reactum_is_structural` routed it through.
//!
//! Invariant under test here: **anything the redex BINDS is reachable in the
//! reactum, identically, in both paths.** A bound link `~e` resolved in a
//! structural reactum (via `instantiate`) but ERRORED in a computed one
//! (`eval_value` rejected a `LinkVar`) — fixed by recording the edge binding at
//! redex-lowering and resolving it in `eval_value`, symmetric with sites `?x`.

use prism_bigraph::Engine;
use prism_schema::Value;

fn run(src: &str, time: f64) -> Value {
    let program = chrysalis::parse::parse_program(src).expect("parse");
    let result = chrysalis::compile::compile(&program).expect("compile");
    let mut engine = Engine::from_state(
        result.topology.state_schema.clone(),
        result.initial_state.clone(),
        result.core.clone(),
    )
    .expect("engine init");
    engine.discover_all_processes();
    engine.run(time);
    engine.state().clone()
}

/// Shared cell + two reactions that are MEANT to be identical — tag the matched
/// cell's slot with an `Edge` recording the bound link `~e`. `TagStruct`'s
/// reactum is a pure template (STRUCTURAL); `TagComp`'s wraps the same `Edge` in
/// an `if true then … else …` so `reactum_is_structural` classifies it COMPUTED.
/// Both reference the bound link `~e`.
const DEFS: &str = r#"
composite Cell[] ~{edge :: any} ->{x :: Float @ x} ( x: 1.0 )

reaction TagStruct (
  ?a :: Cell ~{edge: ~e} => { tag: Edge[link: ~e] }
)

reaction TagComp (
  ?a :: Cell ~{edge: ~e} => { tag: if true then Edge[link: ~e] else Edge[] }
)

composite EnvStruct ->{cells :: map[Cell] @ cells} (
  link e :: any = 'e0' |
  cells: { 'a': Cell[] ~{edge: ~e} ->{x: %.x} } |
  rxn: BRS[rules: [TagStruct]] ~{state: cells} ->{state: cells}
)

composite EnvComp ->{cells :: map[Cell] @ cells} (
  link e :: any = 'e0' |
  cells: { 'a': Cell[] ~{edge: ~e} ->{x: %.x} } |
  rxn: BRS[rules: [TagComp]] ~{state: cells} ->{state: cells}
)
"#;

fn cells_after(entry: &str) -> Value {
    let src = format!("{DEFS}\n{entry}[]\n");
    run(&src, 2.0)
        .get_field("cells")
        .cloned()
        .unwrap_or(Value::None)
}

#[test]
fn bound_link_is_reachable_in_a_computed_reactum() {
    // The fix: `~e` resolves in a COMPUTED reactum (it always did in a structural
    // one). The tag carries the bound link, not an error / None.
    let cells = cells_after("EnvComp");
    let tag = cells
        .get_field("tag")
        .unwrap_or_else(|| panic!("computed reactum produced a `tag`; cells={cells:?}"));
    assert_eq!(tag.get_field("_type").and_then(|v| v.as_str()), Some("Edge"));
    assert!(
        tag.get_field("link").is_some_and(|v| !matches!(v, Value::None)),
        "the bound link `~e` resolved to a value in the computed reactum: {tag:?}"
    );
}

#[test]
fn structural_and_computed_reactums_agree() {
    // THE conformance invariant: two reactums that mean the same thing produce
    // the same end-state, regardless of the structural-vs-computed path.
    let struct_cells = cells_after("EnvStruct");
    let comp_cells = cells_after("EnvComp");
    assert_eq!(
        struct_cells, comp_cells,
        "structural and computed firings of an equivalent reaction must agree\n\
         structural={struct_cells:?}\ncomputed={comp_cells:?}"
    );
}

// ── Multiset (List redex) — the case the keying unification fixed ───────────

/// A multiset reaction `F | B => G | H` written two ways. `MultiStruct`'s
/// reactum is a pure template (STRUCTURAL — it used to return None, a silent
/// no-op); `MultiComp` wraps the product list in `if true then … else …`
/// (COMPUTED). Both must consume F, B and produce G, H — products keyed FRESHLY
/// by the ONE shared `localize_fire`.
const MULTISET: &str = r#"
reaction MultiStruct ( ?f :: F | ?b :: B => G[] | H[] )

reaction MultiComp ( ?f :: F | ?b :: B => if true then [G[], H[]] else [] )

composite EnvMultiStruct ->{soup :: map[any] @ soup} (
  soup: { 'f': F[], 'b': B[] } |
  rxn: BRS[rules: [MultiStruct]] ~{state: soup} ->{state: soup}
)

composite EnvMultiComp ->{soup :: map[any] @ soup} (
  soup: { 'f': F[], 'b': B[] } |
  rxn: BRS[rules: [MultiComp]] ~{state: soup} ->{state: soup}
)
"#;

/// The multiset of `_type` labels in the soup after running, sorted — fresh keys
/// differ between paths, so the conformance comparison is modulo keys.
fn soup_types(entry: &str) -> Vec<String> {
    let src = format!("{MULTISET}\n{entry}[]\n");
    let mut ts: Vec<String> = run(&src, 2.0)
        .get_field("soup")
        .and_then(|v| v.as_map().cloned())
        .map(|m| {
            m.values()
                .filter_map(|v| v.get_field("_type").and_then(|t| t.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    ts.sort();
    ts
}

#[test]
fn structural_and_computed_multiset_reactions_agree() {
    // A structural LIST reactum now FIRES (was a silent no-op) with the same
    // products as the computed one — the keying is one implementation.
    let s = soup_types("EnvMultiStruct");
    let c = soup_types("EnvMultiComp");
    assert_eq!(s, vec!["G".to_string(), "H".to_string()], "structural fired F|B => G|H: {s:?}");
    assert_eq!(
        s, c,
        "structural and computed multiset reactions agree (modulo fresh keys): struct={s:?} comp={c:?}"
    );
}
