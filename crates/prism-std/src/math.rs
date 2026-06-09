//! Axiomatic scalar + list methods shared across every domain (manifold,
//! spatial, synth, quantum).
//!
//! Deliberately MINIMAL — the surface language `+ - * /` covers arithmetic, but
//! lacks the transcendental/algebraic float ops and element access that any
//! oscillator / geometry / signal body needs. This is the axiomatic floor, not a
//! numeric library: richer array algebra (slicing `a[i:j]`, numpy-style vector
//! ops, broadcasting) belongs to the array/`Vec`/`Complex` type work, not here.
//! Methods ARE a type's algebra ([[type_methods_delta_model]]); these are the
//! ones shared by the structural sorts `Float`/`Int`/`List`.

use prism_schema::{MethodRegistry, Value};

/// Register the axiomatic shared math methods.
pub fn register_math(reg: &mut MethodRegistry) {
    // ── Float (and Int, structurally): transcendental + algebraic ──
    // One unary `f64 -> f64` op, registered on both numeric sorts so `2.sqrt()`
    // and `theta.cos()` both dispatch.
    for sort in ["Float", "Int"] {
        reg.register(sort, "sin", |r, _| Ok(Value::float(r.as_f64().unwrap_or(0.0).sin())));
        reg.register(sort, "cos", |r, _| Ok(Value::float(r.as_f64().unwrap_or(0.0).cos())));
        reg.register(sort, "sqrt", |r, _| Ok(Value::float(r.as_f64().unwrap_or(0.0).sqrt())));
    }

    // ── List: the axiomatic element accessor (the surface has no `xs[i]`) ──
    // `xs.at(i)` — i<0 counts from the end; out-of-range is `none`.
    reg.register("List", "at", |recv, args| {
        let xs = recv.as_list().unwrap_or(&[]);
        let i = args.first().and_then(Value::as_i64).unwrap_or(0);
        let idx = if i < 0 { xs.len() as i64 + i } else { i };
        Ok(usize::try_from(idx)
            .ok()
            .and_then(|u| xs.get(u))
            .cloned()
            .unwrap_or(Value::None))
    });

    // #69 sum: the axiomatic reduction — fold a collection's values additively
    // (scalar for numbers, element-wise for vector/array values). Empty -> none.
    // The mean-field / aggregate primitive Demo 2's mesh coupling needs (a
    // `map[id -> array[[2]]]` summed to one mean vector).
    reg.register("List", "sum", |recv, _| {
        Ok(fold_sum(recv.as_list().unwrap_or(&[]).iter()))
    });
    reg.register("Map", "sum", |recv, _| {
        Ok(match recv.as_map() {
            Some(m) => fold_sum(m.iter().filter(|(k, _)| !k.starts_with('_')).map(|(_, v)| v)),
            None => Value::None,
        })
    });
}

/// Element-wise additive fold over a collection's values (`sum`): scalar for
/// numbers, recursive element-wise for vectors. Empty -> `none`.
fn fold_sum<'a>(items: impl Iterator<Item = &'a Value>) -> Value {
    let mut acc: Option<Value> = None;
    for v in items {
        acc = Some(match acc {
            None => v.clone(),
            Some(a) => add_values(&a, v),
        });
    }
    acc.unwrap_or(Value::None)
}

/// Add two values: `Float`/`Int` numerically, `List` element-wise (recursively).
fn add_values(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::List(xs), Value::List(ys)) => {
            Value::List(xs.iter().zip(ys).map(|(x, y)| add_values(x, y)).collect())
        }
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => Value::float(x + y),
            _ => a.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_trig_and_sqrt() {
        let mut reg = MethodRegistry::new();
        register_math(&mut reg);
        assert!((reg.dispatch(&Value::float(0.0), "cos", &[]).unwrap().as_f64().unwrap() - 1.0).abs() < 1e-12);
        assert!((reg.dispatch(&Value::float(0.0), "sin", &[]).unwrap().as_f64().unwrap()).abs() < 1e-12);
        assert!((reg.dispatch(&Value::float(9.0), "sqrt", &[]).unwrap().as_f64().unwrap() - 3.0).abs() < 1e-12);
    }

    #[test]
    fn list_at_indexes_and_wraps() {
        let mut reg = MethodRegistry::new();
        register_math(&mut reg);
        let xs = Value::List(vec![Value::float(10.0), Value::float(20.0)]);
        assert_eq!(reg.dispatch(&xs, "at", &[Value::Int(0)]).unwrap().as_f64(), Some(10.0));
        assert_eq!(reg.dispatch(&xs, "at", &[Value::Int(1)]).unwrap().as_f64(), Some(20.0));
        assert_eq!(reg.dispatch(&xs, "at", &[Value::Int(-1)]).unwrap().as_f64(), Some(20.0));
        assert!(matches!(reg.dispatch(&xs, "at", &[Value::Int(5)]).unwrap(), Value::None));
    }
}
