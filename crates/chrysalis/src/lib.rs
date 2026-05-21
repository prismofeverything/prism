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
//! - [`units`] — dimensional check + unit lowering ("check once, erase,
//!   run raw"): resolves `unit`/`context` decls and verifies expression
//!   bodies are dimensionally consistent before they run.
//!
//! The substrate stays prism — chrysalis is the abstraction layer that
//! turns the framework into a language. See `docs/chrysalis-design.md`.

pub mod ast;
pub mod check;
pub mod compile;
pub mod eval;
pub mod fixtures;
pub mod parse;
pub mod unparse;
pub mod runtime;
pub mod schema;
pub mod units;
