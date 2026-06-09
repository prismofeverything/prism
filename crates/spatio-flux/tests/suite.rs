//! Consolidated integration-test binary for the `spatio-flux` crate.
//!
//! Every `tests/*.rs` is included here as a module, so the crate links ONE test
//! binary instead of N — cutting link time + disk (debug symbols × N binaries).
//! Auto-generated; `autotests = false` + the `[[test]] name = "suite"` target in
//! Cargo.toml route all integration tests through this file.
//!
//! Run one file's tests with a module-name filter:
//!     cargo test -p spatio-flux --test suite <module_name>
//! See coord/ROSTER.md § Build coordination.
#![allow(dead_code, unused_imports, unused_macros)]

#[path = "chrysalis_port.rs"]
mod chrysalis_port;
#[path = "composite_process.rs"]
mod composite_process;
#[path = "load_fixtures.rs"]
mod load_fixtures;
#[path = "parse_import.rs"]
mod parse_import;
#[path = "test_suite.rs"]
mod test_suite;
#[path = "vivarium_compat.rs"]
mod vivarium_compat;
