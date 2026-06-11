//! Consolidated integration-test binary for the `prism-bigraph` crate.
//!
//! Every `tests/*.rs` is included here as a module, so the crate links ONE test
//! binary instead of N — cutting link time + disk (debug symbols × N binaries).
//! Auto-generated; `autotests = false` + the `[[test]] name = "suite"` target in
//! Cargo.toml route all integration tests through this file.
//!
//! Run one file's tests with a module-name filter:
//!     cargo test -p prism-bigraph --test suite <module_name>
//! See coord/ROSTER.md § Build coordination.
#![allow(dead_code, unused_imports, unused_macros)]

#[path = "cells_division.rs"]
mod cells_division;
#[path = "closure_guard.rs"]
mod closure_guard;
#[path = "composite_step_cache.rs"]
mod composite_step_cache;
#[path = "core_merge.rs"]
mod core_merge;
#[path = "core_threading_conformance.rs"]
mod core_threading_conformance;
#[path = "cross_composite_reactor.rs"]
mod cross_composite_reactor;
#[path = "execution_invariants.rs"]
mod execution_invariants;
#[path = "fold_unfurl_consumer.rs"]
mod fold_unfurl_consumer;
#[path = "from_config_composite.rs"]
mod from_config_composite;
#[path = "gillespie.rs"]
mod gillespie;
#[path = "grow_divide_over_rest.rs"]
mod grow_divide_over_rest;
#[path = "growth_division.rs"]
mod growth_division;
#[path = "incremental_steps.rs"]
mod incremental_steps;
#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "mesh_continuous.rs"]
mod mesh_continuous;
#[path = "mesh_live_bridge.rs"]
mod mesh_live_bridge;
#[path = "mesh_live_stream.rs"]
mod mesh_live_stream;
#[path = "mesh_npeer.rs"]
mod mesh_npeer;
#[path = "mesh_protocol.rs"]
mod mesh_protocol;
#[path = "mesh_vector_carrier.rs"]
mod mesh_vector_carrier;
#[path = "missing_references.rs"]
mod missing_references;
#[path = "parallel_engine.rs"]
mod parallel_engine;
#[path = "parallel_protocol.rs"]
mod parallel_protocol;
#[path = "peer_shared_link.rs"]
mod peer_shared_link;
#[path = "process_bigraph_compat.rs"]
mod process_bigraph_compat;
#[path = "protocol_types.rs"]
mod protocol_types;
#[path = "reaction_creates_process.rs"]
mod reaction_creates_process;
#[path = "reaction_data_as_state.rs"]
mod reaction_data_as_state;
#[path = "rest_engine.rs"]
mod rest_engine;
#[path = "rest_protocol.rs"]
mod rest_protocol;
#[path = "rest_schema.rs"]
mod rest_schema;
#[path = "rest_server.rs"]
mod rest_server;
#[path = "rest_sidecar.rs"]
mod rest_sidecar;
#[path = "rules_as_state.rs"]
mod rules_as_state;
#[path = "swim_membership.rs"]
mod swim_membership;
