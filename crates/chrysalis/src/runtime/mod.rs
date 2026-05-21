//! Chrysalis runtime artifacts — the prism-side Process / Step
//! implementations that interpret chrysalis ASTs at runtime.
//!
//! These types are produced by the compiler and registered in a
//! [`prism_bigraph::ProcessRegistry`]; the prism engine instantiates
//! them by `_type` / `address` discovery, the same as any other
//! Process.
//!
//! Layout:
//! - [`rule`] — `chrysalis::Rule`, the reaction-as-value data carrier
//!   (a [`prism_schema::Pattern`] redex plus chrysalis [`Expr`]s for
//!   reactum, guard, and rate) plus [`rule::to_prism_rule`], the adapter
//!   to a `prism_schema::ReactionRule`. There is **no** chrysalis BRS:
//!   reactions run on prism's [`prism_bigraph::BigraphicalReactiveSystem`]
//!   (the compiler registers a `Brs` factory; see `crate::compile`).
//! - [`expr_process`] — `ExprProcess`, a `Process` whose `update` body
//!   is a chrysalis `Expr`.
//! - [`expr_step`] — `ExprStep`, a `Step` whose `update` body is a
//!   chrysalis `Expr`.

pub mod expr_process;
pub mod expr_step;
pub mod rule;

pub use expr_process::ExprProcess;
pub use expr_step::ExprStep;
pub use rule::{BindingSource, Rule, RuleBindings, FOREIGN_RULE};
