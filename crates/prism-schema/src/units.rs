//! Units, dimensions, and cross-dimension contexts — the runtime side
//! of chrysalis's quantity system.
//!
//! This is the *resolved* (normalized) form that the chrysalis check
//! phase produces and then **erases**: dimensional checking and
//! conversion-factor resolution happen once, here, off the hot path; the
//! result is a [`Conversion`] whose [`Conversion::apply`] is a single
//! `f64` arithmetic op. There is no per-operation validation and no
//! boxed quantity at runtime — magnitudes stay bare `f64`. See
//! `docs/chrysalis-design.md`, "Units and quantities" →
//! "Check once, erase, run raw".
//!
//! Layers:
//!   - [`Dimension`] — rational-exponent vector over base dimensions;
//!     the *compatibility* layer (two quantities wire iff dimensions are
//!     equal).
//!   - [`Unit`] — a scale (+ affine offset) within one dimension; the
//!     *conversion* layer (m/ft, pg/kg, molecule/mol).
//!   - [`Context`] — bridges *different* dimensions when a physical
//!     relation justifies it (amount↔concentration via volume).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ── Rational exponents ───────────────────────────────────────────────

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// A rational exponent (always stored reduced, with positive denominator)
/// so `√Hz`-style fractional dimensions are representable and equality is
/// structural.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Ratio {
    pub num: i64,
    pub den: i64,
}

impl Ratio {
    pub fn new(num: i64, den: i64) -> Self {
        assert!(den != 0, "zero denominator");
        let s = if den < 0 { -1 } else { 1 };
        let (num, den) = (num * s, den * s);
        let g = gcd(num, den);
        Self {
            num: num / g,
            den: den / g,
        }
    }
    pub fn int(n: i64) -> Self {
        Self { num: n, den: 1 }
    }
    fn add(self, o: Ratio) -> Ratio {
        Ratio::new(self.num * o.den + o.num * self.den, self.den * o.den)
    }
    fn mul_int(self, k: i64) -> Ratio {
        Ratio::new(self.num * k, self.den)
    }
    fn is_zero(self) -> bool {
        self.num == 0
    }
}

// ── Dimensions ───────────────────────────────────────────────────────

/// A dimension as base-name → rational exponent, kept free of
/// zero-exponent entries so `PartialEq` is exact. Empty = dimensionless.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Dimension(BTreeMap<String, Ratio>);

impl Dimension {
    pub fn dimensionless() -> Self {
        Self::default()
    }

    pub fn base(name: &str) -> Self {
        let mut m = BTreeMap::new();
        m.insert(name.to_string(), Ratio::int(1));
        Self(m)
    }

    pub fn is_dimensionless(&self) -> bool {
        self.0.is_empty()
    }

    /// Multiply dimensions = add exponents (dropping any that cancel).
    pub fn mul(&self, o: &Dimension) -> Dimension {
        let mut m = self.0.clone();
        for (k, v) in &o.0 {
            let cur = m.get(k).copied().unwrap_or(Ratio::int(0));
            let nv = cur.add(*v);
            if nv.is_zero() {
                m.remove(k);
            } else {
                m.insert(k.clone(), nv);
            }
        }
        Dimension(m)
    }

    pub fn inv(&self) -> Dimension {
        self.pow(-1)
    }

    pub fn div(&self, o: &Dimension) -> Dimension {
        self.mul(&o.inv())
    }

    /// Raise to an integer power = scale every exponent.
    pub fn pow(&self, k: i64) -> Dimension {
        let mut m = BTreeMap::new();
        for (key, r) in &self.0 {
            let nr = r.mul_int(k);
            if !nr.is_zero() {
                m.insert(key.clone(), nr);
            }
        }
        Dimension(m)
    }
}

// ── Units ────────────────────────────────────────────────────────────

/// A unit: a scale to its dimension's canonical unit, plus an affine
/// `offset` (0 for purely multiplicative units; non-zero for offset
/// units like °C). `canonical = magnitude * scale + offset`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub dimension: Dimension,
    pub scale: f64,
    pub offset: f64,
}

impl Unit {
    pub fn multiplicative(dimension: Dimension, scale: f64) -> Self {
        Self {
            dimension,
            scale,
            offset: 0.0,
        }
    }
    pub fn affine(dimension: Dimension, scale: f64, offset: f64) -> Self {
        Self {
            dimension,
            scale,
            offset,
        }
    }
}

// ── Contexts (cross-dimension bridges) ───────────────────────────────

/// A normalized context transform. The chrysalis compiler lowers a
/// context rule's transform expression (`value / volume`) into one of
/// these recognized shapes; a constant factor is baked in, a parameter
/// factor is supplied from state at the (single) conversion site.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Bridge {
    /// `value * k` for a compile-time constant `k`.
    ScaleConst(f64),
    /// `value / <param>` — param resolved to a runtime state value.
    DivByParam(String),
    /// `value * <param>`.
    MulByParam(String),
}

impl Bridge {
    fn to_conversion(&self, reversed: bool) -> Conversion {
        match (self, reversed) {
            (Bridge::ScaleConst(k), false) => Conversion::Scale(*k),
            (Bridge::ScaleConst(k), true) => Conversion::Scale(1.0 / k),
            (Bridge::DivByParam(p), false) | (Bridge::MulByParam(p), true) => {
                Conversion::ByState { op: StateOp::DivBy, param: p.clone() }
            }
            (Bridge::MulByParam(p), false) | (Bridge::DivByParam(p), true) => {
                Conversion::ByState { op: StateOp::MulBy, param: p.clone() }
            }
        }
    }
}

/// One rule: bridges `from` → `to` (and the reverse, if `bidirectional`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextRule {
    pub from: Dimension,
    pub to: Dimension,
    pub bidirectional: bool,
    pub bridge: Bridge,
}

/// A named, parameterized set of cross-dimension rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub name: String,
    pub rules: Vec<ContextRule>,
}

impl Context {
    /// Find a rule bridging `from` → `to`, honoring bidirectional rules.
    pub fn bridge(&self, from: &Dimension, to: &Dimension) -> Option<Conversion> {
        for r in &self.rules {
            if &r.from == from && &r.to == to {
                return Some(r.bridge.to_conversion(false));
            }
            if r.bidirectional && &r.to == from && &r.from == to {
                return Some(r.bridge.to_conversion(true));
            }
        }
        None
    }
}

// ── The erased conversion ────────────────────────────────────────────

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StateOp {
    DivBy,
    MulBy,
}

/// The result of resolving a conversion *once*, in the check phase. This
/// is what lowered code carries: a closed form, applied as a single op.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Conversion {
    /// Same unit — no-op.
    Identity,
    /// `m * k` — multiplicative same-dimension, or a constant context bridge.
    Scale(f64),
    /// `m * scale + offset` — affine (offset-unit) conversion.
    Affine { scale: f64, offset: f64 },
    /// `m {/, *} <param>` where the factor is supplied from state at the
    /// conversion site (e.g. the enclosing compartment's volume). The
    /// *choice* of op was resolved once; only the genuine arithmetic
    /// remains at runtime. `param` names the state field — prism's `apply`
    /// supplies its value as `state_factor`, while chrysalis bakes
    /// `value / <param>` into the (unit-erased) body.
    ByState { op: StateOp, param: String },
}

impl Conversion {
    /// Apply the erased conversion to a bare magnitude. `state_factor`
    /// supplies the runtime factor for a [`Conversion::ByState`]; it is
    /// ignored otherwise. One arithmetic op — this is the "run raw".
    pub fn apply(&self, magnitude: f64, state_factor: f64) -> f64 {
        match self {
            Conversion::Identity => magnitude,
            Conversion::Scale(k) => magnitude * k,
            Conversion::Affine { scale, offset } => magnitude * scale + offset,
            Conversion::ByState { op, .. } => match op {
                StateOp::DivBy => magnitude / state_factor,
                StateOp::MulBy => magnitude * state_factor,
            },
        }
    }
}

/// Resolve the conversion from `src` to `dst`, consulting in-scope
/// `contexts` for cross-dimension bridges. `None` means the dimensions
/// are unbridgeable — a check-phase error (illegal programs are
/// unrepresentable). This runs **once**; the returned [`Conversion`] is
/// what the lowered, unit-erased code bakes in.
pub fn resolve_conversion(src: &Unit, dst: &Unit, contexts: &[&Context]) -> Option<Conversion> {
    if src.dimension == dst.dimension {
        if src.offset == 0.0 && dst.offset == 0.0 {
            let k = src.scale / dst.scale;
            return Some(if (k - 1.0).abs() < f64::EPSILON {
                Conversion::Identity
            } else {
                Conversion::Scale(k)
            });
        }
        // Affine: canonical = m*src.scale + src.offset, then
        // m' = (canonical - dst.offset) / dst.scale.
        return Some(Conversion::Affine {
            scale: src.scale / dst.scale,
            offset: (src.offset - dst.offset) / dst.scale,
        });
    }
    // Cross-dimension: only legal through a context bridge. (A full impl
    // also composes any residual same-dimension unit scale; the shape is
    // what matters here.)
    for ctx in contexts {
        if let Some(conv) = ctx.bridge(&src.dimension, &dst.dimension) {
            return Some(conv);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length() -> Dimension {
        Dimension::base("length")
    }

    #[test]
    fn dimension_algebra_is_exact() {
        // [substance] / [length]^3 built two ways is equal.
        let a = Dimension::base("substance").div(&length().pow(3));
        let b = Dimension::base("substance").mul(&length().pow(-3));
        assert_eq!(a, b);
        assert!(!a.is_dimensionless());
        assert!(length().div(&length()).is_dimensionless());
    }

    #[test]
    fn units_vocabulary_round_trips_through_serde() {
        // #71 units-in-schema FOUNDATION: the units vocabulary is now plain
        // serializable DATA. This unblocks two consumers — the schema codec can
        // carry a `Dimension` (the next step of #71), and lang can reify a
        // `unit`/`context` DEFINITION (UnitDef/ContextDef wire through these
        // types). Law: `deserialize ∘ serialize = id` over the vocabulary; the
        // normalization invariants (reduced `Ratio`, zero-free `Dimension`)
        // survive because only already-normalized values are ever serialized.
        fn rt<T>(v: &T)
        where
            T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
        {
            let json = serde_json::to_string(v).unwrap();
            let back: T = serde_json::from_str(&json).unwrap();
            assert_eq!(*v, back, "round-trips: {json}");
        }

        // Ratio: a non-trivially-reduced input stays reduced through the trip.
        let r = Ratio::new(2, 4);
        assert_eq!((r.num, r.den), (1, 2), "Ratio is stored reduced");
        rt(&r);

        // Dimension: substance · length⁻³ (concentration) exercises the
        // BTreeMap<String, Ratio> codec; the empty (dimensionless) case too.
        let concentration = Dimension::base("substance").div(&length().pow(3));
        rt(&concentration);
        rt(&Dimension::dimensionless());

        // Unit: an affine (offset) unit — °C-style.
        rt(&Unit::affine(Dimension::base("temperature"), 1.0, 273.15));

        // Context: a cross-dimension bridge (amount↔concentration via a runtime
        // `volume` factor) — the full Context → ContextRule → Bridge nesting.
        let ctx = Context {
            name: "compartment".to_string(),
            rules: vec![ContextRule {
                from: Dimension::base("substance"),
                to: concentration.clone(),
                bidirectional: true,
                bridge: Bridge::DivByParam("volume".to_string()),
            }],
        };
        rt(&ctx);

        // Conversion: the erased runtime form (both the stateful and affine arms).
        rt(&Conversion::ByState { op: StateOp::DivBy, param: "volume".to_string() });
        rt(&Conversion::Affine { scale: 2.0, offset: -1.5 });
    }

    #[test]
    fn same_dimension_scales() {
        let m = Unit::multiplicative(length(), 1.0);
        let ft = Unit::multiplicative(length(), 0.3048);
        let c = resolve_conversion(&m, &ft, &[]).unwrap();
        assert!((c.apply(1.0, 0.0) - 3.280_84).abs() < 1e-4); // 1 m ≈ 3.28 ft
    }

    #[test]
    fn cross_dimension_without_context_is_an_error() {
        let mass = Unit::multiplicative(Dimension::base("mass"), 1.0);
        let time = Unit::multiplicative(Dimension::base("time"), 1.0);
        assert!(resolve_conversion(&mass, &time, &[]).is_none());
    }

    #[test]
    fn affine_offset_units() {
        let celsius = Unit::affine(Dimension::base("temperature"), 1.0, 273.15);
        let kelvin = Unit::multiplicative(Dimension::base("temperature"), 1.0);
        let conv = resolve_conversion(&celsius, &kelvin, &[]).unwrap();
        assert!((conv.apply(0.0, 0.0) - 273.15).abs() < 1e-9); // 0 °C = 273.15 K
    }

    #[test]
    fn context_bridges_amount_and_concentration() {
        let substance = Dimension::base("substance");
        let conc_dim = substance.div(&length().pow(3));
        let amount = Unit::multiplicative(substance.clone(), 1.0);
        let conc = Unit::multiplicative(conc_dim.clone(), 1.0);
        let ctx = Context {
            name: "concentration".into(),
            rules: vec![ContextRule {
                from: substance,
                to: conc_dim,
                bidirectional: true,
                bridge: Bridge::DivByParam("volume".into()),
            }],
        };
        // amount → concentration: divide by the volume supplied at the site.
        let fwd = resolve_conversion(&amount, &conc, &[&ctx]).unwrap();
        assert_eq!(fwd.apply(100.0, 10.0), 10.0); // 100 molecules in 10 fL
        // concentration → amount: the reverse multiplies by volume.
        let rev = resolve_conversion(&conc, &amount, &[&ctx]).unwrap();
        assert_eq!(rev.apply(10.0, 10.0), 100.0);
    }
}
