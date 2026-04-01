//! Tests ported from Python bigraph-schema/tests.py
//!
//! These verify that prism-schema behaves consistently with the Python
//! bigraph-schema reference implementation. Tests are organized by
//! operation (check, default, apply, serialize, realize).
//!
//! Tests marked `#[ignore]` require features not yet implemented
//! (e.g., Schema::Link, full dispatch system).

use indexmap::IndexMap;
use prism_schema::{Schema, Value, parse_type_expression};

// ═══════════════════════════════════════════════════════════
// Schema Parsing (access)
// ═══════════════════════════════════════════════════════════

#[test]
fn test_parse_float() {
    let schema = parse_type_expression("float");
    assert!(matches!(schema, Schema::Float { .. }));
}

#[test]
fn test_parse_string() {
    let schema = parse_type_expression("string");
    assert!(matches!(schema, Schema::String { .. }));
}

#[test]
fn test_parse_integer() {
    let schema = parse_type_expression("integer");
    assert!(matches!(schema, Schema::Integer { .. }));
}

#[test]
fn test_parse_boolean() {
    let schema = parse_type_expression("boolean");
    assert!(matches!(schema, Schema::Bool { .. }));
}

#[test]
fn test_parse_map() {
    let schema = parse_type_expression("map[float]");
    match &schema {
        Schema::Map { value } => {
            assert!(matches!(**value, Schema::Float { .. }));
        }
        _ => panic!("expected Map, got {:?}", schema),
    }
}

#[test]
fn test_parse_list() {
    let schema = parse_type_expression("list[string]");
    match &schema {
        Schema::List { element } => {
            assert!(matches!(**element, Schema::String { .. }));
        }
        _ => panic!("expected List, got {:?}", schema),
    }
}

#[test]
fn test_parse_maybe() {
    let schema = parse_type_expression("maybe[float]");
    match &schema {
        Schema::Maybe { inner } => {
            assert!(matches!(**inner, Schema::Float { .. }));
        }
        _ => panic!("expected Maybe, got {:?}", schema),
    }
}

#[test]
fn test_parse_overwrite() {
    let schema = parse_type_expression("overwrite[float]");
    match &schema {
        Schema::Overwrite { inner } => {
            assert!(matches!(**inner, Schema::Float { .. }));
        }
        _ => panic!("expected Overwrite, got {:?}", schema),
    }
}

#[test]
fn test_parse_delta() {
    let schema = parse_type_expression("delta");
    assert!(matches!(schema, Schema::Delta { .. }));
}

#[test]
fn test_parse_enum() {
    let schema = parse_type_expression("enum[a,b,c]");
    match &schema {
        Schema::Enum { values, .. } => {
            assert_eq!(values, &["a", "b", "c"]);
        }
        _ => panic!("expected Enum, got {:?}", schema),
    }
}

#[test]
fn test_parse_tuple() {
    let schema = parse_type_expression("tuple[float,string]");
    match &schema {
        Schema::Tuple { elements } => {
            assert_eq!(elements.len(), 2);
            assert!(matches!(elements[0], Schema::Float { .. }));
            assert!(matches!(elements[1], Schema::String { .. }));
        }
        _ => panic!("expected Tuple, got {:?}", schema),
    }
}

#[test]
fn test_parse_tree_expression() {
    // Python: {'a': 'float', 'b': 'string'} → Tree with named branches
    let schema = parse_type_expression("a:float|b:string");
    match &schema {
        Schema::Tree { branches } => {
            assert!(branches.contains_key("a"));
            assert!(branches.contains_key("b"));
            assert!(matches!(branches["a"], Schema::Float { .. }));
            assert!(matches!(branches["b"], Schema::String { .. }));
        }
        _ => panic!("expected Tree, got {:?}", schema),
    }
}

#[test]
fn test_parse_nested_map() {
    // Python: {'_type': 'map', '_value': {'a': 'float', 'b': 'string'}}
    let schema = parse_type_expression("map[a:float|b:string]");
    match &schema {
        Schema::Map { value } => {
            match &**value {
                Schema::Tree { branches } => {
                    assert!(branches.contains_key("a"));
                    assert!(branches.contains_key("b"));
                }
                _ => panic!("expected inner Tree, got {:?}", value),
            }
        }
        _ => panic!("expected Map, got {:?}", schema),
    }
}

#[test]
#[ignore] // Requires Schema::Link
fn test_parse_link() {
    // Python: 'link[x:integer,y:string]'
    let _schema = parse_type_expression("link[x:integer,y:string]");
    // Should parse into Schema::Link with input/output ports
}

#[test]
#[ignore] // Requires Schema::Link
fn test_parse_process() {
    // Python: 'process[level:float,level:float]'
    let _schema = parse_type_expression("process[level:float,level:float]");
}

#[test]
#[ignore] // Requires Schema::Link
fn test_parse_step() {
    // Python: 'step[a:float|b:float,c:float]'
    let _schema = parse_type_expression("step[a:float|b:float,c:float]");
}

// ═══════════════════════════════════════════════════════════
// Default (generate default state from schema)
// ═══════════════════════════════════════════════════════════

/// Python test_default: default float is 0.0, default string is ""
#[test]
fn test_default_float() {
    let schema = Schema::float();
    let val = schema.default_value();
    assert_eq!(val, Value::float(0.0));
}

#[test]
fn test_default_float_with_default() {
    let schema = Schema::float_default(11.111);
    let val = schema.default_value();
    assert_eq!(val, Value::float(11.111));
}

#[test]
fn test_default_string() {
    let schema = Schema::string();
    let val = schema.default_value();
    assert_eq!(val, Value::String(String::new()));
}

#[test]
fn test_default_integer() {
    let schema = Schema::integer();
    let val = schema.default_value();
    assert_eq!(val, Value::Int(0));
}

#[test]
fn test_default_bool() {
    let schema = Schema::bool();
    let val = schema.default_value();
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn test_default_map() {
    let schema = Schema::map(Schema::float());
    let val = schema.default_value();
    assert_eq!(val, Value::Map(IndexMap::new()));
}

#[test]
fn test_default_enum() {
    // Python: default enum is the first value
    let schema = Schema::Enum {
        values: vec!["one".into(), "two".into(), "three".into()],
        default: None,
    };
    let val = schema.default_value();
    assert_eq!(val, Value::String("one".to_string()));
}

#[test]
fn test_default_enum_with_default() {
    let schema = Schema::Enum {
        values: vec!["x".into(), "y".into(), "z".into()],
        default: Some("y".into()),
    };
    let val = schema.default_value();
    assert_eq!(val, Value::String("y".to_string()));
}

// ═══════════════════════════════════════════════════════════
// Check (validate state against schema)
// ═══════════════════════════════════════════════════════════

/// Python test_check: tree[float] accepts nested float maps
#[test]
fn test_check_tree_float() {
    let schema = parse_type_expression("tree[float]");
    let tree_a = Value::tree([
        ("a", Value::tree([
            ("b", Value::float(5.5)),
        ])),
        ("c", Value::float(3.3)),
    ]);
    assert!(schema.check(&tree_a));
}

#[test]
fn test_check_tree_rejects_string() {
    let schema = parse_type_expression("tree[float]");
    assert!(!schema.check(&Value::String("not a tree".into())));
}

#[test]
fn test_check_float() {
    let schema = Schema::float();
    assert!(schema.check(&Value::float(5.5)));
    assert!(!schema.check(&Value::String("nope".into())));
}

#[test]
fn test_check_map() {
    let schema = Schema::map(Schema::float());
    let good = Value::Map(IndexMap::from([
        ("a".into(), Value::float(1.0)),
        ("b".into(), Value::float(2.0)),
    ]));
    let bad = Value::Map(IndexMap::from([
        ("a".into(), Value::String("nope".into())),
    ]));
    assert!(schema.check(&good));
    assert!(!schema.check(&bad));
}

// ═══════════════════════════════════════════════════════════
// Apply (update state using schema-guided semantics)
// ═══════════════════════════════════════════════════════════

/// Float updates are additive (delta semantics)
#[test]
fn test_apply_float_additive() {
    let schema = Schema::float();
    let current = Value::float(10.0);
    let update = Value::float(5.0);
    let result = schema.apply_update(&current, &update);
    assert_eq!(result, Value::float(15.0));
}

/// Integer updates are additive
#[test]
fn test_apply_integer_additive() {
    let schema = Schema::integer();
    let current = Value::Int(10);
    let update = Value::Int(3);
    let result = schema.apply_update(&current, &update);
    assert_eq!(result, Value::Int(13));
}

/// Overwrite replaces the value
#[test]
fn test_apply_overwrite() {
    let schema = Schema::Overwrite { inner: Box::new(Schema::float()) };
    let current = Value::float(10.0);
    let update = Value::float(99.9);
    let result = schema.apply_update(&current, &update);
    assert_eq!(result, Value::float(99.9));
}

/// Map apply merges keys, applying inner schema to matching keys
#[test]
fn test_apply_map_merge() {
    let schema = Schema::map(Schema::float());
    let current = Value::Map(IndexMap::from([
        ("a".into(), Value::float(1.0)),
        ("b".into(), Value::float(2.0)),
    ]));
    let update = Value::Map(IndexMap::from([
        ("a".into(), Value::float(0.5)),
        ("c".into(), Value::float(3.0)),
    ]));
    let result = schema.apply_update(&current, &update);
    let map = result.as_map().unwrap();
    // a: 1.0 + 0.5 = 1.5 (additive)
    assert_eq!(map.get("a").unwrap().as_f64().unwrap(), 1.5);
    // b: unchanged
    assert_eq!(map.get("b").unwrap().as_f64().unwrap(), 2.0);
    // c: new key, 0.0 + 3.0 = 3.0
    assert_eq!(map.get("c").unwrap().as_f64().unwrap(), 3.0);
}

/// Map _add inserts new entries
#[test]
fn test_apply_map_add() {
    let schema = Schema::map(Schema::float());
    let current = Value::Map(IndexMap::from([
        ("a".into(), Value::float(1.0)),
    ]));
    let update = Value::Map(IndexMap::from([
        ("_add".into(), Value::Map(IndexMap::from([
            ("b".into(), Value::float(2.0)),
            ("c".into(), Value::float(3.0)),
        ]))),
    ]));
    let result = schema.apply_update(&current, &update);
    let map = result.as_map().unwrap();
    assert_eq!(map.len(), 3);
    assert_eq!(map.get("b").unwrap().as_f64().unwrap(), 2.0);
}

/// Map _remove deletes keys
#[test]
fn test_apply_map_remove() {
    let schema = Schema::map(Schema::float());
    let current = Value::Map(IndexMap::from([
        ("a".into(), Value::float(1.0)),
        ("b".into(), Value::float(2.0)),
        ("c".into(), Value::float(3.0)),
    ]));
    let update = Value::Map(IndexMap::from([
        ("_remove".into(), Value::List(vec![
            Value::String("b".into()),
        ])),
    ]));
    let result = schema.apply_update(&current, &update);
    let map = result.as_map().unwrap();
    assert_eq!(map.len(), 2);
    assert!(!map.contains_key("b"));
}

/// Tree apply merges branches using per-branch schemas
#[test]
fn test_apply_tree() {
    let schema = Schema::Tree {
        branches: IndexMap::from([
            ("mass".into(), Schema::float()),
            ("name".into(), Schema::Overwrite { inner: Box::new(Schema::string()) }),
        ]),
    };
    let current = Value::tree([
        ("mass", Value::float(1.0)),
        ("name", Value::String("cell".into())),
    ]);
    let update = Value::tree([
        ("mass", Value::float(0.5)),
        ("name", Value::String("updated".into())),
    ]);
    let result = schema.apply_update(&current, &update);
    let map = result.as_map().unwrap();
    // mass: 1.0 + 0.5 = 1.5 (float is additive)
    assert_eq!(map.get("mass").unwrap().as_f64().unwrap(), 1.5);
    // name: "updated" (overwrite replaces)
    assert_eq!(map.get("name").unwrap().as_str().unwrap(), "updated");
}

/// Tuple apply is element-wise
#[test]
fn test_apply_tuple() {
    let schema = Schema::Tuple {
        elements: vec![Schema::float(), Schema::float()],
    };
    let current = Value::List(vec![Value::float(1.0), Value::float(2.0)]);
    let update = Value::List(vec![Value::float(0.5), Value::float(0.3)]);
    let result = schema.apply_update(&current, &update);
    let list = result.as_list().unwrap();
    assert_eq!(list[0].as_f64().unwrap(), 1.5);
    assert_eq!(list[1].as_f64().unwrap(), 2.3);
}

/// List replaces when schema is List (not element-wise)
#[test]
fn test_apply_list_replace() {
    let schema = Schema::List { element: Box::new(Schema::float()) };
    let current = Value::List(vec![Value::float(1.0), Value::float(2.0)]);
    let update = Value::List(vec![Value::float(10.0)]);
    let result = schema.apply_update(&current, &update);
    // Lists replace entirely (different from Tuple which is element-wise)
    let list = result.as_list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].as_f64().unwrap(), 10.0);
}

// ═══════════════════════════════════════════════════════════
// Merge (Python test_merge)
// ═══════════════════════════════════════════════════════════

/// Python test_merge: tree merge combines nested structures
#[test]
fn test_merge_trees() {
    let schema = parse_type_expression("tree[float]");
    let tree_a = Value::tree([
        ("a", Value::tree([
            ("b", Value::float(5.5)),
            ("y", Value::float(555.55)),
            ("x", Value::tree([
                ("further", Value::tree([
                    ("down", Value::float(111111.111)),
                ])),
            ])),
        ])),
        ("c", Value::float(3.3)),
    ]);

    let tree_b = Value::tree([
        ("a", Value::tree([
            ("b", Value::float(0.111)),
            ("z", Value::float(999999.4444)),
            ("x", Value::float(444.444)),
        ])),
        ("d", Value::float(11.11)),
    ]);

    // Merging tree_b into tree_a: tree_a values take precedence for
    // existing keys, tree_b contributes new keys
    let result = schema.apply_update(&tree_b, &tree_a);
    let map = result.as_map().unwrap();
    let a = map.get("a").unwrap().as_map().unwrap();
    // 'x' in tree_a is a subtree, in tree_b it's a float.
    // tree_a's version should win (it's the update).
    let x = a.get("x").unwrap().as_map().unwrap();
    assert!(x.contains_key("further"));
}

// ═══════════════════════════════════════════════════════════
// Traverse (Python test_traverse)
// ═══════════════════════════════════════════════════════════

/// Python test_traverse: navigate into nested tree state
#[test]
fn test_traverse_tree() {
    let tree = Value::tree([
        ("a", Value::tree([
            ("b", Value::float(5.5)),
            ("x", Value::tree([
                ("further", Value::tree([
                    ("down", Value::float(111111.111)),
                ])),
            ])),
        ])),
        ("c", Value::float(3.3)),
    ]);

    // Navigate to a.x.further.down
    let result = tree
        .get_path(&["a".into(), "x".into(), "further".into(), "down".into()])
        .unwrap();
    assert_eq!(result.as_f64().unwrap(), 111111.111);

    // Navigate to a.x.further
    let result = tree
        .get_path(&["a".into(), "x".into(), "further".into()])
        .unwrap();
    assert!(result.as_map().is_some());
    assert_eq!(
        result.as_map().unwrap().get("down").unwrap().as_f64().unwrap(),
        111111.111
    );
}

// ═══════════════════════════════════════════════════════════
// Link / Process / Step (require Schema::Link)
// ═══════════════════════════════════════════════════════════

#[test]
#[ignore] // Requires Schema::Link
fn test_link_schema_default() {
    // Python: default link has address='local:edge', default wiring
    // let schema = parse_type_expression("link[mass:float,mass:delta]");
    // let default = schema.default_value();
    // assert address, inputs, outputs present
}

#[test]
#[ignore] // Requires Schema::Link
fn test_link_realize() {
    // Python test_realize: decode encoded link state
    // Should parse address, instantiate edge, resolve wiring
}

#[test]
#[ignore] // Requires Schema::Link
fn test_link_check() {
    // Python test_check: link state must have address, inputs, outputs
    // Raw dict without realized instance should fail check
    // Realized instance should pass check
}

#[test]
#[ignore] // Requires Schema::Link
fn test_link_serialize() {
    // Python test_serialize: encode link to JSON
    // Should produce address string, _inputs/_outputs schema strings
}

#[test]
#[ignore] // Requires Schema::Link and realize
fn test_generate_with_link() {
    // Python test_generate: realize a schema+state with embedded links
    // Links should be instantiated, default values filled from port schemas
}

#[test]
#[ignore] // Requires Schema::Link
fn test_resolve_conflict() {
    // Python test_resolve_conflict: two links wiring to same path
    // with incompatible types should raise an error
}

// ═══════════════════════════════════════════════════════════
// Round-trip: serialize → realize → check
// ═══════════════════════════════════════════════════════════

#[test]
#[ignore] // Requires serialize/realize
fn test_round_trip_float() {
    // schema = 'float'
    // state = 55.55
    // serialized = serialize(schema, state)
    // realized = realize(schema, serialized)
    // assert check(schema, realized)
}

#[test]
#[ignore] // Requires serialize/realize
fn test_round_trip_tree() {
    // schema = {a: float, b: string}
    // state = {a: 5.5, b: "hello"}
    // serialized → realized → check
}
