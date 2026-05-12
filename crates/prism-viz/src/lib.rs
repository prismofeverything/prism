//! Visualization of process bigraphs and reaction-rule patterns.
//!
//! Three families of renderers:
//!
//! - [`render_dot`] — process-bigraph documents → Graphviz DOT (used
//!   by the spatio-flux report). Shell out to `dot` to get SVG/PNG.
//! - [`render_pattern_dot`] / [`render_rule_pair_dot`] — reaction-rule
//!   [`Pattern`](prism_schema::reaction::Pattern)s → Graphviz DOT. Same
//!   shell-out pipeline. Gives the cleanest layout when `dot` is
//!   available.
//! - [`render_pattern_svg`] / [`render_rule_pair_svg`] — patterns →
//!   self-contained SVG strings. Pure-Rust, no external dep, useful
//!   when Graphviz isn't installed or when you just want an inline
//!   `<svg>` to embed in HTML.

mod dot;
pub mod pattern;
pub mod pattern_dot;

pub use dot::{
    render_dot, render_state_dot, render_state_dot_with_links, render_to_file,
    save_dot, DotOptions, LinkEdge,
};
pub use pattern::{
    linkvar_color, render_pattern_svg, render_rule_pair_svg, rule_color, RULE_COLORS,
};
pub use pattern_dot::{render_pattern_dot, render_rule_pair_dot};
