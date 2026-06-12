//! The `Complex` sort — one ℂ shared across quantum, manifold, and synth
//! (#68, `docs/categorical-core.md` §6b). *One representation, per-domain methods*:
//! quantum reads it as an amplitude, manifold as phase+amplitude, synth as an
//! analytic signal — the data is the same, the method-set varies.
//!
//! **Representation: `array[[2]]` = `[re, im]`** (per §6b), so ℂ addition rides
//! the `Array` schema's element-wise additive apply for free — no per-type
//! summation code. The arithmetic that ISN'T additive (`mul`/`conj`/`abs`/…) lives
//! here as plain Rust functions operating on `Value`s.
//!
//! **Float coercion (the migration hinge):** a bare `Float`/`Int` is read as
//! `re + 0i`. This is what lets `Qubits` move `map[float] → map[Complex]` WITHOUT
//! breaking the existing real-amplitude demos — a real amplitude is just a complex
//! one with zero imaginary part, and [`of`] re-collapses an exactly-real result
//! back to a bare float so real circuits stay byte-identical.

use prism_schema::registry::TypeRegistry;
use prism_schema::{Schema, Value};

/// The name under which `Complex` is registered in the [`TypeRegistry`].
pub const TYPE_NAME: &str = "Complex";

/// `(re, im)` of any value: a `[re, im]` array, OR a bare `Float`/`Int` read as
/// `re + 0i` (the backward-compat coercion), OR `(0, 0)` for anything else.
pub fn parts(v: &Value) -> (f64, f64) {
    match v.as_list() {
        Some(xs) => (
            xs.first().and_then(Value::as_f64).unwrap_or(0.0),
            xs.get(1).and_then(Value::as_f64).unwrap_or(0.0),
        ),
        None => (v.as_f64().unwrap_or(0.0), 0.0),
    }
}

/// Build a complex `Value` from `(re, im)`. An exactly-real value collapses to a
/// bare `Float` so real-amplitude circuits keep producing `float` (not `[re, 0]`)
/// outputs — the property that keeps the pre-complex demos byte-identical.
pub fn of(re: f64, im: f64) -> Value {
    if im == 0.0 {
        Value::float(re)
    } else {
        Value::List(vec![Value::float(re), Value::float(im)])
    }
}

/// The same as [`of`] but ALWAYS the `[re, im]` array form (never the float
/// collapse) — for the `Complex` type's canonical representation / `default`.
pub fn array(re: f64, im: f64) -> Value {
    Value::List(vec![Value::float(re), Value::float(im)])
}

/// `0 + 0i`.
pub fn zero() -> Value {
    of(0.0, 0.0)
}
/// `1 + 0i`.
pub fn one() -> Value {
    of(1.0, 0.0)
}
/// The imaginary unit `i = 0 + 1i`.
pub fn i() -> Value {
    of(0.0, 1.0)
}

/// ℂ addition. (Also the `Array` schema's element-wise additive apply — provided
/// here for symmetry with the non-additive ops.)
pub fn add(a: &Value, b: &Value) -> Value {
    let (ar, ai) = parts(a);
    let (br, bi) = parts(b);
    of(ar + br, ai + bi)
}

/// ℂ subtraction.
pub fn sub(a: &Value, b: &Value) -> Value {
    let (ar, ai) = parts(a);
    let (br, bi) = parts(b);
    of(ar - br, ai - bi)
}

/// ℂ multiplication: `(ar + ai·i)(br + bi·i)`.
pub fn mul(a: &Value, b: &Value) -> Value {
    let (ar, ai) = parts(a);
    let (br, bi) = parts(b);
    of(ar * br - ai * bi, ar * bi + ai * br)
}

/// Real-scalar scaling `s·z`.
pub fn scale(z: &Value, s: f64) -> Value {
    let (re, im) = parts(z);
    of(re * s, im * s)
}

/// ℂ division `a / b = a·conj(b) / |b|²`. Returns `0` if `b == 0` (used by the
/// rank-1 phase recovery in `factorize`, where the denominator is pre-checked
/// nonzero).
pub fn div(a: &Value, b: &Value) -> Value {
    let d = abs2(b);
    if d == 0.0 {
        return zero();
    }
    scale(&mul(a, &conj(b)), 1.0 / d)
}

/// Negation `-z`.
pub fn neg(z: &Value) -> Value {
    let (re, im) = parts(z);
    of(-re, -im)
}

/// Complex conjugate `re − im·i`.
pub fn conj(z: &Value) -> Value {
    let (re, im) = parts(z);
    of(re, -im)
}

/// Real part.
pub fn re(z: &Value) -> f64 {
    parts(z).0
}
/// Imaginary part.
pub fn im(z: &Value) -> f64 {
    parts(z).1
}

/// Squared modulus `|z|² = re² + im²` — the Born-rule weight, the one that never
/// needs a `sqrt`.
pub fn abs2(z: &Value) -> f64 {
    let (re, im) = parts(z);
    re * re + im * im
}

/// Modulus `|z| = √(re² + im²)`.
pub fn abs(z: &Value) -> f64 {
    abs2(z).sqrt()
}

/// Argument (phase) `atan2(im, re)` in radians.
pub fn arg(z: &Value) -> f64 {
    let (re, im) = parts(z);
    im.atan2(re)
}

/// The unit complex number at angle `θ`: `e^{iθ} = cos θ + i·sin θ`.
pub fn from_phase(theta: f64) -> Value {
    of(theta.cos(), theta.sin())
}

/// Rotate `z` by `θ` (multiply by `e^{iθ}`): the global/relative phase op gates
/// like `S`/`T`/`phase` apply.
pub fn rotate(z: &Value, theta: f64) -> Value {
    mul(z, &from_phase(theta))
}

/// Register the `Complex` named type in a [`TypeRegistry`]: representation
/// `array[[2]]`, `default = 0 + 0i`. No custom [`TypeMethods`] — the `Array`
/// schema's element-wise additive apply already IS ℂ addition (§6b), and the
/// non-additive ops are the Rust functions above (the surface value-method form
/// is deferred to the #68 catalog work; method dispatch keys off a `_type` tag /
/// `Foreign` name, which the bare `array[[2]]` representation does not carry).
pub fn register_complex_type(types: &mut TypeRegistry) {
    types.register_full(
        TYPE_NAME,
        Schema::Array {
            shape: vec![2],
            element: Box::new(Schema::float()),
        },
        Some(array(0.0, 0.0)),
        None,
        Vec::new(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn float_coerces_to_real_complex() {
        // A bare float is re + 0i — the migration hinge.
        assert_eq!(parts(&Value::float(0.7071)), (0.7071, 0.0));
        assert_eq!(parts(&Value::Int(3)), (3.0, 0.0));
        // And an exactly-real result collapses back to a bare float (green-keeper).
        assert_eq!(of(0.5, 0.0), Value::float(0.5));
        assert!(matches!(of(0.0, 1.0), Value::List(_)));
    }

    #[test]
    fn i_squared_is_minus_one() {
        let neg_one = mul(&i(), &i());
        assert_eq!(parts(&neg_one), (-1.0, 0.0));
    }

    #[test]
    fn modulus_of_three_four_i_is_five() {
        let z = array(3.0, 4.0);
        assert!(close(abs(&z), 5.0));
        assert!(close(abs2(&z), 25.0));
    }

    #[test]
    fn mul_add_conj() {
        // (1 + 2i)(3 + 4i) = 3 + 4i + 6i + 8i² = -5 + 10i
        let z = mul(&array(1.0, 2.0), &array(3.0, 4.0));
        assert_eq!(parts(&z), (-5.0, 10.0));
        // conj(a)·a = |a|²  (real)
        let a = array(1.0, 2.0);
        let nn = mul(&conj(&a), &a);
        assert_eq!(parts(&nn), (5.0, 0.0));
        // addition
        assert_eq!(parts(&add(&array(1.0, 2.0), &array(3.0, -5.0))), (4.0, -3.0));
    }

    #[test]
    fn from_phase_is_on_the_unit_circle() {
        let q = from_phase(std::f64::consts::FRAC_PI_2); // e^{iπ/2} = i
        assert!(close(re(&q), 0.0));
        assert!(close(im(&q), 1.0));
        assert!(close(abs(&from_phase(1.234)), 1.0));
        // rotate by π/2 turns 1 into i
        assert!(close(im(&rotate(&one(), std::f64::consts::FRAC_PI_2)), 1.0));
    }

    #[test]
    fn arg_of_i_is_half_pi() {
        assert!(close(arg(&i()), std::f64::consts::FRAC_PI_2));
        assert!(close(arg(&one()), 0.0));
    }
}
