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
