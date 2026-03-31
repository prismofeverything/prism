//! Field array utilities for handling both 1D flat and 2D nested arrays.
//!
//! IMPORTANT: We preserve 2D structure throughout. flatten_field is only
//! for internal computation. rebuild_field restores the original shape.

use prism_schema::Value;

/// Flatten a field value into a 1D Vec<f64> for computation.
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

/// Detect if a field value is 2D (list of lists).
pub fn is_2d(value: &Value) -> bool {
    matches!(value, Value::List(items) if items.first().is_some_and(|v| matches!(v, Value::List(_))))
}

/// Get the row width of a 2D field (number of columns).
pub fn field_width(value: &Value) -> Option<usize> {
    if let Value::List(rows) = value {
        if let Some(Value::List(first_row)) = rows.first() {
            return Some(first_row.len());
        }
    }
    None
}

/// Rebuild a field Value from flat data, preserving the original shape.
/// If the original was 2D, rebuilds as 2D. If 1D or scalar, returns flat.
pub fn rebuild_field(data: &[f64], original: &Value) -> Value {
    if let Some(nx) = field_width(original) {
        // Rebuild as 2D
        let ny = data.len() / nx.max(1);
        let mut rows = Vec::with_capacity(ny);
        for y in 0..ny {
            let start = y * nx;
            let end = (start + nx).min(data.len());
            let row: Vec<Value> = data[start..end]
                .iter()
                .map(|&v| Value::float(v))
                .collect();
            rows.push(Value::List(row));
        }
        Value::List(rows)
    } else if data.len() == 1 {
        // Scalar
        Value::float(data[0])
    } else {
        // Flat 1D
        Value::List(data.iter().map(|&v| Value::float(v)).collect())
    }
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

    #[test]
    fn test_rebuild_2d() {
        let original = Value::List(vec![
            Value::List(vec![Value::float(0.0), Value::float(0.0)]),
            Value::List(vec![Value::float(0.0), Value::float(0.0)]),
        ]);
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let rebuilt = rebuild_field(&data, &original);
        assert_eq!(
            rebuilt,
            Value::List(vec![
                Value::List(vec![Value::float(1.0), Value::float(2.0)]),
                Value::List(vec![Value::float(3.0), Value::float(4.0)]),
            ])
        );
    }

    #[test]
    fn test_rebuild_1d() {
        let original = Value::List(vec![Value::float(0.0), Value::float(0.0)]);
        let data = vec![1.0, 2.0];
        let rebuilt = rebuild_field(&data, &original);
        assert_eq!(rebuilt, Value::List(vec![Value::float(1.0), Value::float(2.0)]));
    }
}
