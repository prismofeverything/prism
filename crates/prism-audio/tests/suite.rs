//! Consolidated integration-test binary for the `prism-audio` crate.
//!
//! Every `tests/*.rs` is included here as a module, so the crate links ONE test
//! binary instead of N — cutting link time + disk (debug symbols × N binaries).
//! Auto-generated; `autotests = false` + the `[[test]] name = "suite"` target in
//! Cargo.toml route all integration tests through this file.
//!
//! Run one file's tests with a module-name filter:
//!     cargo test -p prism-audio --test suite <module_name>
//! See coord/ROSTER.md § Build coordination.
#![allow(dead_code, unused_imports, unused_macros)]

#[path = "device.rs"]
mod device;
#[path = "factory.rs"]
mod factory;
#[path = "instrument.rs"]
mod instrument;
#[path = "mix_bus.rs"]
mod mix_bus;
#[path = "modules.rs"]
mod modules;
#[path = "offline_render.rs"]
mod offline_render;
#[path = "patch.rs"]
mod patch;
#[path = "reactions.rs"]
mod reactions;
