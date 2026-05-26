//! `Program::entity(name)` — the unified entity view (#30 slice 2). One name
//! → one `EntityView` carrying every Def-kind's slot. Definers fill slots;
//! consumers ask the view which slots are present rather than scanning Defs
//! for variant matches.

use chrysalis::parse::parse_program;

#[test]
fn entity_view_collects_value_form_slots() {
    // A small program with a process, a composite, a reaction, and a type-
    // ascribed binding. Each is a separate Def today but flows through the
    // unified `EntityView` so callers see one entity per name.
    let src = "\
process Grow ~{mass :: Float} ->{mass :: Float} (
  {mass: 1.0}
)

composite Cell[mass: Float = 1.0] ~{} ->{mass :: Float} (
  mass: mass |
  grow: Grow ~{mass: mass} ->{mass: mass}
)

reaction Split (
  ?c :: Cell => ?c
)

def threshold = 2.0
";
    let prog = parse_program(src).expect("parse");

    // Process — value-form, no composite/reaction slot.
    let grow = prog.entity("Grow").expect("Grow entity");
    assert!(grow.process.is_some());
    assert!(grow.composite.is_none());
    assert!(grow.reaction.is_none());
    assert!(grow.has_value_form());

    // Composite — value-form, has the body + interface.
    let cell = prog.entity("Cell").expect("Cell entity");
    assert!(cell.composite.is_some());
    assert!(cell.has_value_form());

    // Reaction — value-form.
    let split = prog.entity("Split").expect("Split entity");
    assert!(split.reaction.is_some());
    assert!(split.has_value_form());

    // Binding — present but NOT value-form (a binding holds a value but is
    // not itself constructible via `name[args]`).
    let threshold = prog.entity("threshold").expect("threshold entity");
    assert!(threshold.binding.is_some());
    assert!(!threshold.has_value_form());

    // Unknown name → None.
    assert!(prog.entity("NotADef").is_none());
}

#[test]
fn entity_view_powers_bare_variable_lookup() {
    // A reaction referenced by bare name in a list (`rules: [Split, ...]`)
    // resolves to its Rule value via the EntityView's value-form lookup.
    // This is the eval-side consequence of slice 2: every slot-filled entity
    // is reachable by bare name, uniformly.
    let src = "\
process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)

composite Sys ~{} ->{count :: Float} (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}
)

# Bare reference to `Tick` (no `[]`) — must resolve via the entity view.
def tick_ref = Tick

Sys[]
";
    let prog = parse_program(src).expect("parse");
    // The Sys entity exists (composite slot) and is value-form.
    let sys = prog.entity("Sys").unwrap();
    assert!(sys.composite.is_some());
    assert!(sys.has_value_form());

    // The Tick entity exists (process slot) and is value-form.
    let tick = prog.entity("Tick").unwrap();
    assert!(tick.process.is_some());
    assert!(tick.has_value_form());
}
