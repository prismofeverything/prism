//! Consolidated integration-test binary for the `prism-mapk` crate.
//!
//! Every `tests/*.rs` is included here as a module, so the crate links ONE test
//! binary instead of N — cutting link time + disk (debug symbols × N binaries).
//! Auto-generated; `autotests = false` + the `[[test]] name = "suite"` target in
//! Cargo.toml route all integration tests through this file.
//!
//! Run one file's tests with a module-name filter:
//!     cargo test -p prism-mapk --test suite <module_name>
//! See coord/ROSTER.md § Build coordination.
#![allow(dead_code, unused_imports, unused_macros)]

#[path = "end_to_end.rs"]
mod end_to_end;
#[path = "report_workflow.rs"]
mod report_workflow;
