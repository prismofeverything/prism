//! MAPK signalling as a Bigraphical Reactive System.
//!
//! Port of `kaleidoscope.systems.mapk` (Python). The model is a
//! coarse-grained compartmentalised phosphorylation cycle:
//!
//!   - MEK in cytoplasm phosphorylates free ERK → MEK·pERK complex.
//!   - The complex dissociates → free MEK + free pERK.
//!   - pERK imports into nucleus (fast); occasionally leaks back.
//!   - Nuclear pERK is dephosphorylated back to ERK.
//!   - Unphosphorylated ERK diffuses between cytoplasm and child
//!     compartments (nucleus, ER lumen).
//!
//! The seven rules together produce a continuous-firing
//! steady-state where pERK accumulates in the nucleus — the actual
//! biological "signal" of the cycle.

pub mod rules;
pub mod state;
pub mod types;
pub mod workflow;

pub use rules::mapk_rules;
pub use state::initial_mapk_state;
pub use types::MAPK_SORTS;
