//! Graphviz DOT visualization of process bigraph compositions.
//!
//! Generates DOT format digraphs from prism Topologies and Documents,
//! showing processes (boxes), state nodes (circles), and wiring (dashed edges).
//! Port of bigraph-viz from Python.

mod dot;

pub use dot::{render_dot, render_to_file, save_dot, DotOptions};
