//! Parser for bigraph-schema type expressions.
//!
//! Parses type strings like "map[array[10|10,float]]", "tuple[set_float,set_float]",
//! "overwrite[mass]", etc. into Schema values.

use crate::schema::Schema;

/// Parse a bigraph-schema type expression string into a Schema.
///
/// Supported forms:
/// - `float`, `integer`, `string`, `bool` — atomic types
/// - `mass`, `concentration`, `count` — aliases for float (additive)
/// - `set_float` — alias for overwrite[float]
/// - `positive_float` — alias for float (clamped, but we treat as float)
/// - `array[ny|nx,element]` — multidimensional array
/// - `positive_array` — alias for array (clamped to ≥0)
/// - `map[value]` — map with typed values
/// - `list[element]` — list with typed elements
/// - `tuple[type1,type2,...]` — fixed-length typed tuple
/// - `overwrite[inner]` — replacement semantics
/// - `maybe[inner]` — optional
/// - `link[...]` — process port declaration (returns Any)
/// - `tree{...}` — tree with branches (not commonly used in fixtures)
pub fn parse_type_expression(expr: &str) -> Schema {
    let expr = expr.trim();
    if expr.is_empty() {
        return Schema::Any;
    }

    // Check for tree expression (a:float|b:string) before parameterized types.
    // Must contain ':' at top level and not start with a bracket.
    // Only trigger if it looks like "name:type|name:type"
    if !expr.starts_with('[') {
        let top_parts = split_top_level(expr, '|');
        if top_parts.len() >= 2 && top_parts.iter().all(|p| {
            let cp = split_top_level(p, ':');
            cp.len() >= 2 && !cp[0].trim().is_empty()
        }) {
            return parse_tree_expression(expr);
        }
    }

    // Check for parameterized types: name[params]
    if let Some(bracket_start) = expr.find('[') {
        let name = &expr[..bracket_start];
        let inner = &expr[bracket_start + 1..expr.len() - 1]; // strip [ and ]

        match name {
            "map" => {
                let value_schema = parse_type_expression(inner);
                Schema::map(value_schema)
            }
            "list" => {
                let element_schema = parse_type_expression(inner);
                Schema::list(element_schema)
            }
            "array" => parse_array_type(inner),
            "tuple" => {
                let elements: Vec<Schema> = split_top_level(inner, ',')
                    .iter()
                    .map(|s| parse_type_expression(s))
                    .collect();
                Schema::tuple(elements)
            }
            "overwrite" => {
                let inner_schema = parse_type_expression(inner);
                Schema::overwrite(inner_schema)
            }
            "maybe" => {
                let inner_schema = parse_type_expression(inner);
                Schema::Maybe { inner: Box::new(inner_schema) }
            }
            "enum" => {
                let values: Vec<String> = inner.split(',').map(|s| s.trim().to_string()).collect();
                Schema::Enum { values, default: None }
            }
            "tree" => {
                let leaf = parse_type_expression(inner);
                Schema::recursive_tree(leaf)
            }
            "link" | "process" | "step" => Schema::Any, // process declarations
            _ => Schema::Any, // unknown parameterized type
        }
    } else {
        // Simple type name
        match expr {
            "float" | "mass" | "concentration" | "count"
            | "positive_float" | "process_interval" | "number"
            | "nonnegative" => Schema::float(),
            "integer" => Schema::integer(),
            "string" => Schema::string(),
            "bool" | "boolean" | "xor" => Schema::bool(),
            "delta" => Schema::Delta { default: None },
            "set_float" => Schema::set_float(),
            "positive_array" => Schema::array(vec![], Schema::float()),
            "any" | "node" => Schema::Any,
            "link" => Schema::Any,
            _ => {
                // Try parsing as a tree expression (a:float|b:string)
                if expr.contains(':') && expr.contains('|') {
                    let tree = parse_tree_expression(expr);
                    if !matches!(tree, Schema::Any) {
                        return tree;
                    }
                }
                Schema::Any // truly unknown type
            }
        }
    }
}

/// Parse array type parameters: "ny|nx,element" or just "element"
fn parse_array_type(params: &str) -> Schema {
    // Split on comma at top level to separate shape from element type
    let parts = split_top_level(params, ',');
    if parts.len() >= 2 {
        // "10|10,float" → shape=[10,10], element=float
        let shape_str = parts[0].trim();
        let element_str = parts[1..].join(",");
        let shape: Vec<usize> = shape_str
            .split('|')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        let element = parse_type_expression(element_str.trim());
        Schema::array(shape, element)
    } else {
        // Just element type, no shape
        let element = parse_type_expression(params.trim());
        Schema::array(vec![], element)
    }
}

/// Parse a tree-style schema expression like "glucose:concentration|acetate:concentration|biomass:mass"
/// into a Tree schema with named branches.
pub fn parse_tree_expression(expr: &str) -> Schema {
    // Split on '|' at top level only (not inside brackets like array[6|5,float])
    let parts = split_top_level(expr, '|');
    let branches: indexmap::IndexMap<String, Schema> = parts
        .iter()
        .filter_map(|pair| {
            // Split on first ':' at top level for name:type
            let colon_parts = split_top_level(pair, ':');
            if colon_parts.len() < 2 {
                return None;
            }
            let name = colon_parts[0].trim().to_string();
            let type_str = colon_parts[1..].join(":"); // rejoin in case type has colons
            let type_str = type_str.trim();
            if name.is_empty() {
                return None;
            }
            Some((name, parse_type_expression(type_str)))
        })
        .collect();

    if branches.is_empty() {
        parse_type_expression(expr)
    } else {
        Schema::Tree { branches }
    }
}

/// Split a string by delimiter, but only at the top level (not inside brackets).
fn split_top_level(s: &str, delim: char) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            c if c == delim && depth == 0 => {
                result.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    result.push(&s[start..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_types() {
        assert_eq!(parse_type_expression("float"), Schema::float());
        assert_eq!(parse_type_expression("mass"), Schema::float());
        assert_eq!(parse_type_expression("concentration"), Schema::float());
        assert_eq!(parse_type_expression("set_float"), Schema::set_float());
        assert_eq!(parse_type_expression("string"), Schema::string());
    }

    #[test]
    fn test_array() {
        let s = parse_type_expression("array[10|10,float]");
        assert_eq!(s, Schema::array(vec![10, 10], Schema::float()));
    }

    #[test]
    fn test_map_array() {
        let s = parse_type_expression("map[array[10|10,float]]");
        assert_eq!(s, Schema::map(Schema::array(vec![10, 10], Schema::float())));
    }

    #[test]
    fn test_tuple() {
        let s = parse_type_expression("tuple[set_float,set_float]");
        assert_eq!(s, Schema::tuple(vec![Schema::set_float(), Schema::set_float()]));
    }

    #[test]
    fn test_overwrite() {
        let s = parse_type_expression("overwrite[mass]");
        assert_eq!(s, Schema::overwrite(Schema::float()));
    }

    #[test]
    fn test_tree_expression() {
        let s = parse_tree_expression("glucose:concentration|biomass:mass");
        match s {
            Schema::Tree { branches } => {
                assert_eq!(branches.len(), 2);
                assert_eq!(branches["glucose"], Schema::float());
                assert_eq!(branches["biomass"], Schema::float());
            }
            _ => panic!("Expected Tree"),
        }
    }

    #[test]
    fn test_tree_with_arrays() {
        // The | inside array[6|5,float] must not split the tree expression
        let s = parse_tree_expression("glucose:array[6|5,float]|acetate:array[6|5,float]");
        match s {
            Schema::Tree { branches } => {
                assert_eq!(branches.len(), 2);
                assert_eq!(branches["glucose"], Schema::array(vec![6, 5], Schema::float()));
                assert_eq!(branches["acetate"], Schema::array(vec![6, 5], Schema::float()));
            }
            _ => panic!("Expected Tree, got {s:?}"),
        }
    }
}
