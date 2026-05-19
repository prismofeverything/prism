//! Chrysalis — surface programming language compiling to the prism
//! process-bigraph runtime.
//!
//! Pipeline:
//!
//! ```text
//! source  ──[parse]──►  AST  ──[compile]──►  (Topology, ProcessRegistry, Value)
//!                                                          │
//!                                                          ▼
//!                                              prism_bigraph::Engine
//! ```
//!
//! Tier 1 entry points:
//! - [`ast`] — typed AST for definitions and expressions.
//! - [`eval`] — interpreter for expression bodies. Evaluates against an
//!   environment; calls into `prism_schema::method` for host operations.
//! - [`compile`] — lifts an AST [`ast::Program`] into a
//!   [`prism_bigraph::Topology`], a [`prism_bigraph::ProcessRegistry`],
//!   and the initial [`prism_schema::Value`] state the engine discovers.
//!
//! The substrate stays prism — chrysalis is the abstraction layer that
//! turns the framework into a language. See `docs/chrysalis-design.md`.

pub mod ast;
pub mod compile;
pub mod eval;
pub mod fixtures;
pub mod runtime;
