//! Consolidated integration-test binary for the `chrysalis` crate.
//!
//! Every `tests/*.rs` is included here as a module, so the crate links ONE test
//! binary instead of N — cutting link time + disk (debug symbols × N binaries).
//! Auto-generated; `autotests = false` + the `[[test]] name = "suite"` target in
//! Cargo.toml route all integration tests through this file.
//!
//! Run one file's tests with a module-name filter:
//!     cargo test -p chrysalis --test suite <module_name>
//! See coord/ROSTER.md § Build coordination.
#![allow(dead_code, unused_imports, unused_macros)]

#[path = "agreement_engines.rs"]
mod agreement_engines;
#[path = "alchemy_pool.rs"]
mod alchemy_pool;
#[path = "alchemy.rs"]
mod alchemy;
#[path = "bench_run.rs"]
mod bench_run;
#[path = "bigraph_rule_through_algebra.rs"]
mod bigraph_rule_through_algebra;
#[path = "canonical_run_core.rs"]
mod canonical_run_core;
#[path = "compile_imports.rs"]
mod compile_imports;
#[path = "composite_bridge.rs"]
mod composite_bridge;
#[path = "composite_constructor.rs"]
mod composite_constructor;
#[path = "composite_interval.rs"]
mod composite_interval;
#[path = "contract_enforcement.rs"]
mod contract_enforcement;
#[path = "contract_query.rs"]
mod contract_query;
#[path = "coord_heartbeats_parse.rs"]
mod coord_heartbeats_parse;
#[path = "coord_set.rs"]
mod coord_set;
#[path = "cross_composite_firing.rs"]
mod cross_composite_firing;
#[path = "cross_composite_link_redex.rs"]
mod cross_composite_link_redex;
#[path = "cross_composite_redex_via_map_literal.rs"]
mod cross_composite_redex_via_map_literal;
#[path = "distributed_alchemy.rs"]
mod distributed_alchemy;
#[path = "distributed_cross_composite.rs"]
mod distributed_cross_composite;
#[path = "effects_first_slice.rs"]
mod effects_first_slice;
#[path = "engine_driver_one_door_guard.rs"]
mod engine_driver_one_door_guard;
#[path = "entity_as_data.rs"]
mod entity_as_data;
#[path = "entity_roundtrip_complete.rs"]
mod entity_roundtrip_complete;
#[path = "entity_view.rs"]
mod entity_view;
#[path = "export_import.rs"]
mod export_import;
#[path = "external_fulfillers.rs"]
mod external_fulfillers;
#[path = "file_entry.rs"]
mod file_entry;
#[path = "fixture_sync.rs"]
mod fixture_sync;
#[path = "functions.rs"]
mod functions;
#[path = "gillespie.rs"]
mod gillespie;
#[path = "graph_type.rs"]
mod graph_type;
#[path = "grow_divide_glucose.rs"]
mod grow_divide_glucose;
#[path = "grow_divide_homoiconic.rs"]
mod grow_divide_homoiconic;
#[path = "grow_divide_stream.rs"]
mod grow_divide_stream;
#[path = "hand_built_program.rs"]
mod hand_built_program;
#[path = "homoiconic_roundtrip_guard.rs"]
mod homoiconic_roundtrip_guard;
#[path = "instantiate_one_door_guard.rs"]
mod instantiate_one_door_guard;
#[path = "integrator_comparison.rs"]
mod integrator_comparison;
#[path = "invoke.rs"]
mod invoke;
#[path = "invoke_trace.rs"]
mod invoke_trace;
#[path = "kuramoto_mesh_distributed.rs"]
mod kuramoto_mesh_distributed;
#[path = "kuramoto_mesh.rs"]
mod kuramoto_mesh;
#[path = "kuramoto_plastic.rs"]
mod kuramoto_plastic;
#[path = "kuramoto_sync.rs"]
mod kuramoto_sync;
#[path = "link_pool.rs"]
mod link_pool;
#[path = "link_schema.rs"]
mod link_schema;
#[path = "live_cross_composite.rs"]
mod live_cross_composite;
#[path = "load_sibling_imports.rs"]
mod load_sibling_imports;
#[path = "load_in_language.rs"]
mod load_in_language;
#[path = "load_in_language_ys.rs"]
mod load_in_language_ys;
#[path = "mapk.rs"]
mod mapk;
#[path = "mesh_link_distributed.rs"]
mod mesh_link_distributed;
#[path = "mesh_link_runtime.rs"]
mod mesh_link_runtime;
#[path = "mesh_link_surface.rs"]
mod mesh_link_surface;
#[path = "metacircular_node_rung.rs"]
mod metacircular_node_rung;
#[path = "mr.rs"]
mod mr;
#[path = "nested_composite.rs"]
mod nested_composite;
#[path = "one_shot_steps.rs"]
mod one_shot_steps;
#[path = "outer_link_alchemy.rs"]
mod outer_link_alchemy;
#[path = "package_lockfile.rs"]
mod package_lockfile;
#[path = "package_path_dep.rs"]
mod package_path_dep;
#[path = "package_transitive.rs"]
mod package_transitive;
#[path = "parse_auto_key.rs"]
mod parse_auto_key;
#[path = "parse_composite.rs"]
mod parse_composite;
#[path = "parse_contract.rs"]
mod parse_contract;
#[path = "parse_graph.rs"]
mod parse_graph;
#[path = "parse_grow_divide.rs"]
mod parse_grow_divide;
#[path = "parse_nuclear_shuttle.rs"]
mod parse_nuclear_shuttle;
#[path = "parse_star_wire.rs"]
mod parse_star_wire;
#[path = "parse_survey.rs"]
mod parse_survey;
#[path = "parse_units.rs"]
mod parse_units;
#[path = "parse_use_import.rs"]
mod parse_use_import;
#[path = "pattern_def.rs"]
mod pattern_def;
#[path = "primer_doctest.rs"]
mod primer_doctest;
#[path = "programs_as_data.rs"]
mod programs_as_data;
#[path = "protocol_surface.rs"]
mod protocol_surface;
#[path = "quantum_auto_split.rs"]
mod quantum_auto_split;
#[path = "quantum_factorize.rs"]
mod quantum_factorize;
#[path = "quantum_lifecycle.rs"]
mod quantum_lifecycle;
#[path = "quantum_lifecycle_stream.rs"]
mod quantum_lifecycle_stream;
#[path = "quantum_locc.rs"]
mod quantum_locc;
#[path = "quantum_self_decohere.rs"]
mod quantum_self_decohere;
#[path = "quantum_teleportation.rs"]
mod quantum_teleportation;
#[path = "quantum_tensor.rs"]
mod quantum_tensor;
#[path = "qubits_tensor_through_algebra.rs"]
mod qubits_tensor_through_algebra;
#[path = "reaction_as_data.rs"]
mod reaction_as_data;
#[path = "reaction_conformance.rs"]
mod reaction_conformance;
#[path = "reaction_divide.rs"]
mod reaction_divide;
#[path = "reaction_rate.rs"]
mod reaction_rate;
#[path = "reaction_rest.rs"]
mod reaction_rest;
#[path = "reaction_transport.rs"]
mod reaction_transport;
#[path = "reaction_type.rs"]
mod reaction_type;
#[path = "reflective_reaction.rs"]
mod reflective_reaction;
#[path = "repl_coherence.rs"]
mod repl_coherence;
#[path = "rule_carrier_one_door_guard.rs"]
mod rule_carrier_one_door_guard;
#[path = "run_core_one_door_guard.rs"]
mod run_core_one_door_guard;
#[path = "scaffold.rs"]
mod scaffold;
#[path = "schlogl.rs"]
mod schlogl;
#[path = "server.rs"]
mod server;
#[path = "shared_link_alchemy.rs"]
mod shared_link_alchemy;
#[path = "stream_protocol.rs"]
mod stream_protocol;
#[path = "stream.rs"]
mod stream;
#[path = "structural_reaction.rs"]
mod structural_reaction;
#[path = "units_engine.rs"]
mod units_engine;
#[path = "units_in_schema.rs"]
mod units_in_schema;
#[path = "unparse_roundtrip.rs"]
mod unparse_roundtrip;
#[path = "ys_files_roundtrip.rs"]
mod ys_files_roundtrip;
#[path = "ys_files_run.rs"]
mod ys_files_run;
