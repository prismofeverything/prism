//! Field array utilities for handling both 1D flat and 2D nested arrays.

use prism_schema::Value;

/// Flatten a field value into a 1D Vec<f64>.
///
/// Handles both formats:
/// - Flat: `[1.0, 2.0, 3.0, ...]`
/// - 2D nested: `[[1.0, 2.0], [3.0, 4.0], ...]`
/// - Scalar: `5.0` (returned as single-element vec)
pub fn flatten_field(value: &Value) -> Vec<f64> {
    match value {
        Value::Float(f) => vec![f.0],
        Value::Int(i) => vec![*i as f64],
        Value::List(items) => {
            let mut result = Vec::new();
            for item in items {
                match item {
                    Value::Float(f) => result.push(f.0),
                    Value::Int(i) => result.push(*i as f64),
                    Value::List(row) => {
                        // 2D: flatten the row
                        for cell in row {
                            if let Some(v) = cell.as_f64() {
                                result.push(v);
                            }
                        }
                    }
                    _ => result.push(0.0),
                }
            }
            result
        }
        _ => vec![],
    }
}

/// Convert a flat Vec<f64> back to a Value.
/// Returns as a flat list (our canonical format).
pub fn unflatten_field(data: &[f64]) -> Value {
    Value::List(data.iter().map(|&v| Value::float(v)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flatten_2d() {
        let field_2d = Value::List(vec![
            Value::List(vec![Value::float(1.0), Value::float(2.0)]),
            Value::List(vec![Value::float(3.0), Value::float(4.0)]),
        ]);
        assert_eq!(flatten_field(&field_2d), vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_flatten_1d() {
        let field_1d = Value::List(vec![
            Value::float(1.0),
            Value::float(2.0),
            Value::float(3.0),
        ]);
        assert_eq!(flatten_field(&field_1d), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_flatten_scalar() {
        assert_eq!(flatten_field(&Value::float(5.0)), vec![5.0]);
    }
}
