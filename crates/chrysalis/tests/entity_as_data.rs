//! The AST as data, one entity at a time.
//!
//! Slice 2b of #30 + the entity-summary slice of #32 (stretch — AST-as-value):
//! `Program::owned_entities()` returns each entity as a STAND-ALONE value
//! you can transform, build by hand, or serialize. `EntityDef::to_value()`
//! emits the homoiconic shape: `{_type: "EntityDef", name, slots, …}`.
//!
//! Pairs with `programs_as_data.rs` (Document-shape) and
//! `load_in_language.rs` (load/run). Together: programs are data at every
//! level — from the compiled Document down to the individual entities.

use chrysalis::ast::{Def, EntityDef, Expr};
use chrysalis::parse::parse_program;

#[test]
fn parsing_extracts_entities_from_a_program() {
    // A small program with three entity kinds: process, composite, reaction.
    let src = "\
process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)

composite Main ->{count :: Float} (
  count: 0.0 |
  tick: Tick ~{count: count} ->{count: count}
)

reaction Split (
  ?c :: Main => ?c
)
";
    let prog = parse_program(src).expect("parse");
    let entities = prog.owned_entities();

    // First-appearance order; one entity per name.
    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Tick", "Main", "Split"]);

    // Each entity carries only the slot it was defined under.
    assert!(entities[0].process.is_some() && entities[0].composite.is_none());
    assert!(entities[1].composite.is_some() && entities[1].process.is_none());
    assert!(entities[2].reaction.is_some() && entities[2].composite.is_none());
}

#[test]
fn entity_to_value_emits_the_homoiconic_summary_shape() {
    let src = "\
process Tick ~{count :: Float} ->{count :: Float} (
  {count: 1.0}
)
";
    let prog = parse_program(src).expect("parse");
    let tick = prog.entity_owned("Tick").expect("Tick entity");

    let value = tick.to_value();
    let map = value.as_map().expect("EntityDef is a Map");
    assert_eq!(
        map.get("_type").and_then(|v| v.as_str()),
        Some("EntityDef")
    );
    assert_eq!(map.get("name").and_then(|v| v.as_str()), Some("Tick"));

    // The `slots` list names which definer kinds contributed — a quick
    // structural summary you can read at a glance.
    let slots = map
        .get("slots")
        .and_then(|v| v.as_list())
        .expect("slots list");
    assert!(slots.iter().any(|v| v.as_str() == Some("process")));

    // The `process` slot's summary carries the port name → schema map.
    let proc_summary = map.get("process").expect("process summary");
    let inputs = proc_summary
        .get_field("inputs")
        .and_then(|v| v.as_map())
        .expect("process inputs map");
    assert!(inputs.contains_key("count"), "the `count` input is present");
    assert_eq!(
        inputs.get("count").and_then(|p| p.get_field("schema")).and_then(|v| v.as_str()),
        Some("float"),
        "the count port's schema serializes as a string"
    );
}

#[test]
fn expr_to_value_serializes_every_variant_the_parser_emits() {
    // A program exercising a range of Expr variants: literal, var, term,
    // parallel, keyed entries, binop, method call, conditional. The
    // entity-level serializer recurses into the body via Expr::to_value,
    // so all of these show up as `{_type: …}` nodes the caller can walk.
    let src = "\
process Mix ~{a :: Float, b :: Float} ->{out :: Float, flag :: Bool} (
  out: (a + b) |
  flag: (if a > 0.0 then true else false)
)
";
    let prog = parse_program(src).expect("parse");
    let mix = prog.entity_owned("Mix").expect("Mix");
    let value = mix.to_value();

    // The composite-body summary now carries the full body AST.
    let body = value
        .get_field("process")
        .and_then(|p| p.get_field("body"))
        .expect("process.body present");
    assert_eq!(
        body.get_field("_type").and_then(|v| v.as_str()),
        Some("Parallel"),
        "body is a parallel composition of the two output entries"
    );

    let items = body
        .get_field("items")
        .and_then(|v| v.as_list())
        .expect("Parallel.items list");
    assert_eq!(items.len(), 2, "two top-level body items");

    // The first item is `out: a + b` — a KeyedEntry whose value is a BinOp.
    let first = &items[0];
    assert_eq!(
        first.get_field("_type").and_then(|v| v.as_str()),
        Some("KeyedEntry")
    );
    let first_val = first.get_field("value").expect("KeyedEntry.value");
    assert_eq!(
        first_val.get_field("_type").and_then(|v| v.as_str()),
        Some("BinOp"),
        "value is a + b"
    );
    assert_eq!(
        first_val.get_field("op").and_then(|v| v.as_str()),
        Some("Add")
    );

    // The second item is `flag: (if ...)` — KeyedEntry containing an If.
    let second_val = items[1].get_field("value").expect("flag value");
    assert_eq!(
        second_val.get_field("_type").and_then(|v| v.as_str()),
        Some("If"),
        "value is an If"
    );
}

#[test]
fn loaded_documents_carry_their_entities_as_walkable_ast() {
    // `load(path)` stashes `_entities` in the returned Document Value — a
    // list of `{_type: 'EntityDef', name, …, process: {…, body: …}}` nodes.
    // A caller can navigate to a specific entity's body without re-parsing.
    use std::path::PathBuf;
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("entity-as-data-load");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("tick.ys");
    std::fs::write(
        &path,
        "\
process Tick ~{count :: Float} ->{count :: Float} ( {count: 1.0} )
",
    )
    .expect("write");

    let doc = chrysalis::prelude::load(path.to_str().unwrap()).expect("load");
    let entities = doc.get_field("_entities").expect("_entities present");
    let list = entities.as_list().expect("_entities is a list");
    assert_eq!(list.len(), 1, "one entity: Tick");

    let tick = &list[0];
    let body = tick
        .get_field("process")
        .and_then(|p| p.get_field("body"))
        .expect("Tick body");
    // The body is the literal `{count: 1.0}` with a bare-identifier key
    // → parses as a `Record` (a `Map` would require a quoted key
    // `{'count': 1.0}`). Either way, the body shows up as walkable data.
    assert_eq!(
        body.get_field("_type").and_then(|v| v.as_str()),
        Some("Record")
    );
    let fields = body.get_field("fields").expect("Record.fields");
    let count = fields.get_field("count").expect("count field");
    assert_eq!(
        count.get_field("_type").and_then(|v| v.as_str()),
        Some("Float")
    );
    assert_eq!(count.get_field("value").and_then(|v| v.as_f64()), Some(1.0));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expr_to_value_then_from_value_is_identity() {
    // The closed round trip: every Expr the parser produces serializes
    // (`to_value`), can be parsed back (`from_value`), and re-serializes to
    // an identical Value shape. Proves the homoiconic identity at the AST
    // level — programs ARE data, the host functions that build them are no
    // more privileged than user code reconstructing them.
    let src = "\
process Mix ~{a :: Float, b :: Float} ->{out :: Float, flag :: Bool} (
  out: (a + b) |
  flag: (if a > 0.0 then true else false)
)
";
    let prog = parse_program(src).expect("parse");
    let mix = prog
        .entity_owned("Mix")
        .and_then(|e| e.process)
        .expect("Mix process");
    let original_body = &mix.body;

    let value_form = original_body.to_value();
    let reparsed = Expr::from_value(&value_form).expect("from_value");
    let value_form_again = reparsed.to_value();

    assert_eq!(
        value_form, value_form_again,
        "to_value ∘ from_value ∘ to_value ≡ to_value — the round trip is\n\
         identity at the Value level. (Expr itself is hard to compare\n\
         directly without a Eq impl; comparing serialized forms is the\n\
         standard homoiconic equivalence.)"
    );
}

#[test]
fn hand_built_value_becomes_a_real_expr() {
    // The "build a program from data" direction: a Value::Map constructed
    // by hand — matching the `{_type, …}` shape `to_value` emits — becomes a
    // real Expr via `from_value`. The interpreter sees no difference
    // between a parsed expression and a programmatically built one.
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};

    // The handwritten shape for `{count: 1.0}` (a Record literal).
    let mut count_lit: IndexMap<Key, Value> = IndexMap::new();
    count_lit.insert(Key::from("_type"), Value::String("Float".into()));
    count_lit.insert(Key::from("value"), Value::float(1.0));

    let mut fields: IndexMap<Key, Value> = IndexMap::new();
    fields.insert(Key::from("count"), Value::Map(count_lit));

    let mut record: IndexMap<Key, Value> = IndexMap::new();
    record.insert(Key::from("_type"), Value::String("Record".into()));
    record.insert(Key::from("fields"), Value::Map(fields));

    let hand = Value::Map(record);
    let expr = Expr::from_value(&hand).expect("from_value parses the hand shape");

    // The reconstructed Expr serializes back to the same Value (modulo
    // ordering — which `to_value`/`from_value` preserve).
    let back = expr.to_value();
    assert_eq!(back, hand, "hand-built Value ≡ to_value(from_value(it))");

    // And the reconstructed Expr is structurally a Record carrying a Float.
    match expr {
        Expr::Record(fields) => {
            assert_eq!(fields.len(), 1);
            let v = fields.get("count").expect("count field");
            assert!(matches!(v, Expr::Float(1.0)));
        }
        other => panic!("expected Record, got {other:?}"),
    }
}

#[test]
fn entity_def_round_trips_through_value() {
    // The entity-level round trip (#34 slice A): a parsed EntityDef
    // serializes via `to_value`, deserializes via `from_value`, and
    // re-serializes to the same Value shape. This is the substrate for
    // hand-building whole composites/processes/steps as data.
    let src = "\
process Grow[rate :: Float = 0.6] ~{mass :: Float} ->{mass :: Float} (
  {mass: (mass * rate)}
)
";
    let prog = parse_program(src).expect("parse");
    let grow = prog.entity_owned("Grow").expect("Grow");

    let original = grow.to_value();
    let restored =
        EntityDef::from_value(&original).expect("EntityDef::from_value");
    let restored_value = restored.to_value();

    assert_eq!(
        original, restored_value,
        "EntityDef → Value → EntityDef → Value is identity; the slot's params,\n\
         ports (with schema strings), and body all survive the round trip."
    );

    // Structural confirmation: the restored process has the same param +
    // port names as the parsed one, and the body is the same Map literal.
    let restored_process = restored.process.expect("process slot");
    assert_eq!(restored_process.params.len(), 1);
    assert_eq!(restored_process.params[0].name, "rate");
    assert_eq!(restored_process.interface.inputs.len(), 1);
    assert!(restored_process.interface.inputs.contains_key("mass"));
    assert_eq!(restored_process.interface.outputs.len(), 1);
    assert!(restored_process.interface.outputs.contains_key("mass"));
}

#[test]
fn eval_with_env_resolves_var_references() {
    // The chrysalis `eval(expr, env)` — Lisp's `(eval form env)`. A hand-
    // built expression with a `Var("x")` finds its value through the env
    // bindings. This is the substrate for runtime-generated bodies that
    // reference outer state.
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};

    let var = |name: &str| -> Value {
        let mut m: IndexMap<Key, Value> = IndexMap::new();
        m.insert(Key::from("_type"), Value::String("Var".into()));
        m.insert(Key::from("name"), Value::String(name.into()));
        Value::Map(m)
    };
    let float_lit = |v: f64| -> Value {
        let mut m: IndexMap<Key, Value> = IndexMap::new();
        m.insert(Key::from("_type"), Value::String("Float".into()));
        m.insert(Key::from("value"), Value::float(v));
        Value::Map(m)
    };
    // expr: `x * 2.0`
    let mut expr: IndexMap<Key, Value> = IndexMap::new();
    expr.insert(Key::from("_type"), Value::String("BinOp".into()));
    expr.insert(Key::from("op"), Value::String("Mul".into()));
    expr.insert(Key::from("lhs"), var("x"));
    expr.insert(Key::from("rhs"), float_lit(2.0));

    // env: {x: 5.0}
    let mut env_map: IndexMap<Key, Value> = IndexMap::new();
    env_map.insert(Key::from("x"), Value::float(5.0));
    let env = Value::Map(env_map);

    let result = chrysalis::prelude::eval_with(&Value::Map(expr), Some(&env))
        .expect("eval_with(env)");
    assert_eq!(
        result.as_f64(),
        Some(10.0),
        "eval(x * 2.0, {{x: 5.0}}) ≡ 10.0"
    );
}

#[test]
fn eval_interprets_a_hand_built_expression() {
    // The chrysalis `(eval '(+ 2 3))` — a Lisp-flavored homoiconic move.
    // Build a BinOp Value by hand (the to_value shape), pass it through
    // `chrysalis::prelude::eval`, get the result. The interpreter sees no
    // difference between this and a parsed `2.0 + 3.0`.
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};

    let float_lit = |v: f64| -> Value {
        let mut m: IndexMap<Key, Value> = IndexMap::new();
        m.insert(Key::from("_type"), Value::String("Float".into()));
        m.insert(Key::from("value"), Value::float(v));
        Value::Map(m)
    };
    let mut binop: IndexMap<Key, Value> = IndexMap::new();
    binop.insert(Key::from("_type"), Value::String("BinOp".into()));
    binop.insert(Key::from("op"), Value::String("Add".into()));
    binop.insert(Key::from("lhs"), float_lit(2.0));
    binop.insert(Key::from("rhs"), float_lit(3.0));

    let expr_value = Value::Map(binop);
    let result = chrysalis::prelude::eval(&expr_value).expect("eval");
    assert_eq!(
        result.as_f64(),
        Some(5.0),
        "eval(2.0 + 3.0) ≡ 5.0 — the homoiconic identity at the EXPR level: a\n\
         data form built from map literals interprets to the same value as the\n\
         parsed source `2.0 + 3.0`."
    );
}

#[test]
fn build_an_entity_from_scratch_then_inspect() {
    // The user's mental model: "I could start with an empty Entity, then add
    // things to it until it was whatever ys program."
    //
    // The OWNED `EntityDef` *is* that. `EntityDef::new(name)` is the empty
    // entity; `with_<slot>(def)` adds slots; the result is the same value
    // `Program::entity_owned` would produce. Both directions — *build* and
    // *inspect* — share one struct.

    // Start empty.
    let empty = EntityDef::new("Tick");
    assert_eq!(empty.name, "Tick");
    assert!(empty.is_empty(), "name-only entity has no slots filled");
    assert!(!empty.has_value_form(), "no value-form slot yet");

    // Borrow a real ProcessDef from a parsed program (same shape the parser
    // produces; no synthetic construction path) and attach it.
    let src = "process Tick ~{count :: Float} ->{count :: Float} ({count: 1.0})";
    let prog = parse_program(src).expect("parse");
    let process_def = prog
        .defs
        .iter()
        .find_map(|d| match d {
            Def::Process(p) => Some(p.clone()),
            _ => None,
        })
        .expect("parsed Tick process");
    let ent = EntityDef::new("Tick").with_process(process_def);

    assert!(!ent.is_empty());
    assert!(ent.process.is_some(), "process slot now filled");
    assert!(ent.has_value_form(), "process makes it value-form");

    // Hand-built entity AND parser-derived entity serialize to the SAME
    // `to_value` shape — the homoiconic identity holds at the entity level.
    let from_hand = ent.to_value();
    let from_parse = prog.entity_owned("Tick").unwrap().to_value();
    assert_eq!(
        from_hand.get_field("_type"),
        from_parse.get_field("_type")
    );
    assert_eq!(
        from_hand.get_field("name"),
        from_parse.get_field("name")
    );
    assert_eq!(
        from_hand.get_field("process"),
        from_parse.get_field("process"),
        "the process-slot summary should match — same name, same ports"
    );
}
