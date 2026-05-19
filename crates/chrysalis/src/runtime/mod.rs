//! Chrysalis runtime artifacts — the prism-side Process / Step
//! implementations that interpret chrysalis ASTs at runtime.
//!
//! These types are produced by the compiler and registered in a
//! [`prism_bigraph::ProcessRegistry`]; the prism engine instantiates
//! them by `_type` / `address` discovery, the same as any other
//! Process.
//!
//! Layout:
//! - [`rule`] — `chrysalis::Rule`, the chrysalis-typed reaction. Carries
//!   a static [`prism_schema::Pattern`] redex plus chrysalis [`Expr`]s
//!   for reactum, guard, and rate.
//! - [`brs`] — `ChrysalisBrs`, a [`prism_bigraph::Process`] that fires
//!   chrysalis [`Rule`]s. Uses prism's `find_matches` for matching, then
//!   interprets the rule's reactum/guard/rate against match bindings.
//! - [`expr_process`] — `ExprProcess`, a `Process` whose `update` body
//!   is a chrysalis `Expr`.
//! - [`expr_step`] — `ExprStep`, a `Step` whose `update` body is a
//!   chrysalis `Expr`.

pub mod brs;
pub mod expr_process;
pub mod expr_step;
pub mod rule;

pub use brs::{BrsConfig, BrsMode, ChrysalisBrs, FOREIGN_RULE};
pub use expr_process::ExprProcess;
pub use expr_step::ExprStep;
pub use rule::{BindingSource, Rule, RuleBindings};
