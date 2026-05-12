//! MAPK bigraph signature — the set of sort labels (controls).

/// The MAPK signature `K`: every node in state carries one of these
/// strings as its `_type` tag. The matcher uses string equality on
/// `_type` to enforce control matching.
pub const MAPK_SORTS: &[&str] = &[
    "Cell",
    "Compartment",
    "MEK",
    "ERK",
    "pERK",
    "Cytoplasm",
    "Nucleus",
    "ERLumen",
];
