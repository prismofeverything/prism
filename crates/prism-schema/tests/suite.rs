//! Consolidated integration-test binary for the `prism-schema` crate.
//!
//! Every `tests/*.rs` is included here as a module, so the crate links ONE test
//! binary instead of N — cutting link time + disk (debug symbols × N binaries).
//! Auto-generated; `autotests = false` + the `[[test]] name = "suite"` target in
//! Cargo.toml route all integration tests through this file.
//!
//! Run one file's tests with a module-name filter:
//!     cargo test -p prism-schema --test suite <module_name>
//! See coord/ROSTER.md § Build coordination.
#![allow(dead_code, unused_imports, unused_macros)]

#[path = "algebra_laws.rs"]
mod algebra_laws;
#[path = "bigraph_schema_compat.rs"]
mod bigraph_schema_compat;
#[path = "bigraph_type.rs"]
mod bigraph_type;
#[path = "contract_substitutability.rs"]
mod contract_substitutability;
#[path = "crdt_laws.rs"]
mod crdt_laws;
#[path = "dimension_in_schema.rs"]
mod dimension_in_schema;
#[path = "fire_across_composites.rs"]
mod fire_across_composites;
#[path = "fire_keying.rs"]
mod fire_keying;
#[path = "fold_unfurl.rs"]
mod fold_unfurl;
#[path = "functor_laws.rs"]
mod functor_laws;
#[path = "reaction_data.rs"]
mod reaction_data;
#[path = "tensor_by_schema.rs"]
mod tensor_by_schema;
