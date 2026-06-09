//! Homoiconic round-trip totality — the build-failing one-door guard.
//!
//! `simplify` / Stage 3 of `docs/homoiconic-unification.md` (invariant #2: *total
//! quote/reify*). Companion to — not a replacement for — `lang`'s Stage-1a feature
//! test: this is the ARCHITECTURAL INVARIANT that keeps the round-trip total as the
//! `Expr` grammar grows.
//!
//! ## Why a guard at all
//! `quote` (`Expr::to_value`, `ast.rs:1638`) is compiler-exhaustive — it matches
//! `&Expr`, so a new variant cannot be forgotten. `reify` (`Expr::from_value`,
//! `ast.rs:1973`) is NOT: it dispatches on a string `_type` tag with an
//! `other => Err(..)` wildcard, so a new variant *silently* stops round-tripping with
//! zero build feedback. "Partial homoiconicity isn't homoiconicity"
//! (`categorical-core.md` §7). The existing `expr_to_value_then_from_value_is_identity`
//! test is sample-based — it would never notice a missing variant.
//!
//! ## The mechanism (the "one door", per `generative-core.md` §3)
//! The `roundtrip_suite!` macro is the SINGLE SOURCE OF TRUTH: each entry supplies both
//! (a) a no-wildcard match arm for the `variant_tag` witness and (b) a round-trip
//! sample. A new `Expr` variant fails to compile in `variant_tag` until the suite
//! covers it — and that same entry forces a sample to exist. "Every variant
//! round-trips" can no longer rot silently.
//!
//! ## The handshake with `lang` (Stage 1a closed)
//! `lang` landed the impl that makes reify total (Stage 1a — the `Comprehension` arm,
//! `ast.rs:2209`), so `PENDING_REIFY` is now EMPTY and EVERY variant must reify to an
//! identical value. The `PENDING_REIFY` mechanism is retained for the future: a variant
//! added before its reify arm is listed there as a logged, lenient, shrinking exception
//! (per `generative-core.md`'s "log the exceptions") so the guard never red-windows an
//! in-flight implementer — then the arm is closed and the list re-emptied.

use chrysalis::ast::{
    BinOp, Block, Expr, PathRoot, PlacePath, PortBindings, StringLit, StringSeg, TermArg, UnaryOp,
};

fn b(e: Expr) -> Box<Expr> {
    Box::new(e)
}

// Multi-field samples kept out of the macro invocation for readability.
fn term_sample() -> Expr {
    Expr::Term {
        control: "Foo".into(),
        args: vec![
            TermArg::Named { name: "k".into(), value: Expr::Int(1) },
            TermArg::Positional(Expr::Float(2.0)),
        ],
        ports: PortBindings::default(),
        body: Some(b(Expr::Int(0))),
    }
}
fn record_sample() -> Expr {
    let mut m: indexmap::IndexMap<String, Expr> = indexmap::IndexMap::new();
    m.insert("f".into(), Expr::Int(1));
    m.insert("g".into(), Expr::Bool(true));
    Expr::Record(m)
}
fn comprehension_sample() -> Expr {
    // Exercise EVERY optional (key_var, filter, key) so reify must cover the full arm.
    Expr::Comprehension {
        key_var: Some("i".into()),
        var: "v".into(),
        source: b(Expr::Var("xs".into())),
        filter: Some(b(Expr::Bool(true))),
        body: b(Expr::Var("v".into())),
        key: Some(b(Expr::Str(StringLit::plain("k")))),
    }
}

/// One entry per `Expr` variant → both the exhaustiveness witness and a sample.
macro_rules! roundtrip_suite {
    ( $( $tag:literal => $pat:pat , $sample:expr ; )+ ) => {
        /// Build-failing exhaustiveness WITNESS — no wildcard. A new `Expr` variant
        /// fails to compile here until the suite covers it (which also supplies its
        /// round-trip sample). This is the one-door teeth.
        fn variant_tag(e: &Expr) -> &'static str {
            match e { $( $pat => $tag , )+ }
        }
        fn samples() -> Vec<(&'static str, Expr)> {
            vec![ $( ($tag, $sample) ),+ ]
        }
    };
}

roundtrip_suite! {
    "Unit"          => Expr::Unit,                Expr::Unit;
    "Bool"          => Expr::Bool(_),             Expr::Bool(true);
    "Int"           => Expr::Int(_),              Expr::Int(7);
    "Float"         => Expr::Float(_),            Expr::Float(1.5);
    "Str"           => Expr::Str(_),              Expr::Str(StringLit::plain("hi"));
    "Var"           => Expr::Var(_),              Expr::Var("x".into());
    "Path"          => Expr::Path(_),             Expr::Path(PlacePath { root: PathRoot::Local("a".into()), segments: vec!["b".into(), "c".into()] });
    "Term"          => Expr::Term { .. },         term_sample();
    "Parallel"      => Expr::Parallel(_),         Expr::Parallel(vec![Expr::Int(1), Expr::Int(2)]);
    "KeyedEntry"    => Expr::KeyedEntry { .. },   Expr::KeyedEntry { key: StringLit::plain("k"), value: b(Expr::Int(1)) };
    "Map"           => Expr::Map(_),              Expr::Map(vec![(StringLit::plain("k"), Expr::Int(1))]);
    "Record"        => Expr::Record(_),           record_sample();
    "List"          => Expr::List(_),             Expr::List(vec![Expr::Int(1)]);
    "Site"          => Expr::Site { .. },         Expr::Site { name: "c".into(), sort: Some(b(Expr::Var("Cell".into()))) };
    "Unbound"       => Expr::Unbound,             Expr::Unbound;
    "LinkVar"       => Expr::LinkVar(_),          Expr::LinkVar("e".into());
    "LinkDecl"      => Expr::LinkDecl { .. },     Expr::LinkDecl { name: "pool".into(), schema: None, mesh: true, default: b(Expr::Float(0.0)) };
    "Rule"          => Expr::Rule { .. },         Expr::Rule { redex: b(Expr::Site { name: "c".into(), sort: None }), reactum: b(Expr::Site { name: "c".into(), sort: None }) };
    "Let"           => Expr::Let { .. },          Expr::Let { bindings: vec![("x".into(), Expr::Int(1))], body: b(Expr::Var("x".into())) };
    "Block"         => Expr::Block(_),            Expr::Block(Block::from_parts(vec![("x".into(), Expr::Int(1))], Expr::Var("x".into())));
    "If"            => Expr::If { .. },           Expr::If { cond: b(Expr::Bool(true)), then_: b(Expr::Int(1)), else_: Some(b(Expr::Int(2))) };
    "BinOp"         => Expr::BinOp { .. },        Expr::BinOp { op: BinOp::Add, lhs: b(Expr::Int(1)), rhs: b(Expr::Int(2)) };
    "UnaryOp"       => Expr::UnaryOp { .. },      Expr::UnaryOp { op: UnaryOp::Neg, operand: b(Expr::Int(1)) };
    "Method"        => Expr::Method { .. },       Expr::Method { receiver: b(Expr::Var("x".into())), method: "m".into(), args: vec![Expr::Int(1)] };
    "Field"         => Expr::Field { .. },        Expr::Field { base: b(Expr::Var("x".into())), name: "f".into() };
    "Call"          => Expr::Call { .. },         Expr::Call { func: b(Expr::Var("f".into())), args: vec![Expr::Int(1)] };
    "Comprehension" => Expr::Comprehension { .. }, comprehension_sample();
    "ReplaceWith"   => Expr::ReplaceWith { .. },  Expr::ReplaceWith { id: b(Expr::Var("x".into())), with: b(Expr::Int(1)) };
    "Where"         => Expr::Where { .. },        Expr::Where { inner: b(Expr::Site { name: "c".into(), sort: None }), predicate: b(Expr::Bool(true)) };
}

/// Variants `Expr::from_value` does not yet reify. `lang`'s Stage 1a landed the
/// `Comprehension` arm (`ast.rs:2209`), so this is now EMPTY → the guard enforces TOTAL
/// quote/reify with no exceptions (`docs/homoiconic-unification.md` invariant #2).
/// The mechanism is RETAINED: if a future `Expr` variant lands before its reify arm,
/// list it here (a logged, lenient, shrinking exception per `generative-core.md`'s
/// "log the exceptions") rather than weakening the guard — then close the arm and
/// re-empty. So the guard tracks the grammar's growth without ever red-windowing an
/// in-flight implementer.
const PENDING_REIFY: &[&str] = &[];

/// Extra samples that hit DISTINCT `from_value` branches the canonical one-per-variant
/// samples don't reach: the OTHER side of each optional (`If` no-else, `Site` no-sort,
/// `Comprehension` no-optionals), template strings, and `Here`-paths. This is COVERAGE,
/// not exhaustiveness — exhaustiveness is the witness's job. (Absorbs the alternate
/// shapes from `lang`'s `from_value_is_total_over_every_expr_variant` corpus so this one
/// guard subsumes it — see the #46 consolidation note in `coord/simplify.ys`.)
fn alternate_shapes() -> Vec<(&'static str, Expr)> {
    vec![
        (
            "Str(template)",
            Expr::Str(StringLit::template(vec![
                StringSeg::Lit("id-".into()),
                StringSeg::Expr(Expr::Var("x".into())),
            ])),
        ),
        ("Path(Here)", Expr::Path(PlacePath { root: PathRoot::Here, segments: vec![] })),
        ("If(no-else)", Expr::If { cond: b(Expr::Bool(true)), then_: b(Expr::Int(1)), else_: None }),
        ("Site(plain)", Expr::Site { name: "c".into(), sort: None }),
        (
            "Comprehension(minimal)",
            Expr::Comprehension {
                key_var: None,
                var: "v".into(),
                source: b(Expr::Var("xs".into())),
                filter: None,
                body: b(Expr::Var("v".into())),
                key: None,
            },
        ),
    ]
}

fn assert_round_trips(label: &str, e: &Expr) {
    // reify succeeded → the round trip must be identity at the Value level (the standard
    // homoiconic equivalence; `Expr` has no `Eq`). A PENDING variant is lenient (it may
    // error OR reify) so the guard never red-windows an in-flight reify implementer;
    // PENDING_REIFY is empty today, so every variant is enforced.
    let pending = PENDING_REIFY.contains(&variant_tag(e));
    let quoted = e.to_value(); // quote : Expr → Value
    match Expr::from_value(&quoted) {
        Ok(back) if !pending => assert_eq!(
            quoted,
            back.to_value(),
            "quote/reify is NOT identity for `{label}`:\n  quote       = {quoted:?}\n  quote∘reify = {:?}",
            back.to_value()
        ),
        Ok(_) => {}
        Err(err) => assert!(
            pending,
            "`from_value` FAILED for `{label}` (NOT in PENDING_REIFY) — the quote/reify \
             round trip regressed, or a new variant is unhandled by reify: {err}"
        ),
    }
}

#[test]
fn quote_reify_round_trip_is_total_over_every_expr_variant() {
    // Canonical: exactly one sample per variant. Exhaustiveness is compiler-enforced by
    // the no-wildcard `variant_tag` witness — a new variant can't compile until the
    // suite covers it (which forces a sample). The label IS the variant tag here.
    for (tag, e) in samples() {
        assert_eq!(
            variant_tag(&e),
            tag,
            "suite mislabeled `{tag}` — the sample is a different variant"
        );
        assert_round_trips(tag, &e);
    }
    // Plus the alternate shapes (branch coverage; labels are descriptive, not tags).
    for (label, e) in alternate_shapes() {
        assert_round_trips(label, &e);
    }
}

#[test]
fn pending_reify_names_only_real_variants() {
    // Guard the guard: a PENDING_REIFY entry must be a real variant tag (catches a typo
    // or a stale entry). Coverage of EVERY variant is enforced structurally by the
    // no-wildcard `variant_tag` witness, so this only needs to validate the exception list.
    let known: std::collections::BTreeSet<&str> = samples().iter().map(|(t, _)| *t).collect();
    for tag in PENDING_REIFY {
        assert!(
            known.contains(tag),
            "PENDING_REIFY names `{tag}`, which is not a real Expr variant tag"
        );
    }
}
