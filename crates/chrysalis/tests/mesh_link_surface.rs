//! #62 / grand-synthesis M1 — the `link :: T mesh` chrysalis surface + its
//! compile-time CRDT gate.
//!
//! `link name :: T mesh = default` declares a REPLICATED (CRDT) link in `.ys`. The
//! `mesh_safety` closure invariant is enforced at COMPILE time (`validate_connections`,
//! run in `compile.rs`): an unsafe replicated link — additive scalar, last-writer-wins,
//! a missing type — is a COMPILE ERROR. Illegal distributed states are unrepresentable
//! in the surface, not a runtime divergence (the M1 checkpoint: "the algebra refuses an
//! unsafe merge at compile time").

use chrysalis::check::validate_connections;
use chrysalis::parse::parse_program;
use chrysalis::unparse::unparse;

const SAFE: &str = "\
composite SafePool ->{ values :: map[float] @ shared } (
  link shared :: map[float] mesh = {}
)
";

const UNSAFE_ADDITIVE: &str = "\
composite BadPool ->{ total :: Float @ shared } (
  link shared :: Float mesh = 0.0
)
";

const NO_SCHEMA: &str = "\
composite NoSchema ->{ values :: map[float] @ shared } (
  link shared mesh = {}
)
";

#[test]
fn a_mesh_safe_link_compiles_clean() {
    // A per-source `map[float]` pool is the canonical mesh-safe link.
    let p = parse_program(SAFE).expect("parse");
    let errs = validate_connections(&p);
    assert!(errs.is_empty(), "expected clean, got {errs:?}");
}

#[test]
fn an_additive_mesh_link_is_a_compile_error() {
    // A bare additive scalar double-counts on re-delivery → refused at compile.
    let p = parse_program(UNSAFE_ADDITIVE).expect("parse");
    let errs = validate_connections(&p);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("not CRDT-safe") && e.message.contains("additive")),
        "expected a CRDT-safety compile error naming the footgun, got {errs:?}"
    );
}

#[test]
fn a_mesh_link_must_declare_its_schema() {
    // Without a declared type, the CRDT-safety of the merge can't be checked.
    let p = parse_program(NO_SCHEMA).expect("parse");
    let errs = validate_connections(&p);
    assert!(
        errs.iter().any(|e| e.message.contains("must declare its value-schema")),
        "expected a missing-schema compile error, got {errs:?}"
    );
}

#[test]
fn the_mesh_modifier_round_trips_through_unparse() {
    let p = parse_program(SAFE).expect("parse");
    let src = unparse(&p);
    assert!(src.contains("mesh"), "unparse preserves the `mesh` modifier:\n{src}");
    // Re-parse the unparsed source: both the modifier and the clean gate survive.
    let p2 = parse_program(&src).expect("reparse");
    assert!(validate_connections(&p2).is_empty(), "round-trip stays clean");
    assert!(unparse(&p2).contains("mesh"), "the modifier is stable under round-trip");
}
