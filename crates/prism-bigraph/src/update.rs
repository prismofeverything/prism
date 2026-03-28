//! Update types for process outputs.
//!
//! Updates can be synchronous (immediate value) or represent
//! different merge semantics (delta, replace, etc.)

use prism_schema::Value;

/// An update produced by a process or step.
#[derive(Clone, Debug)]
pub enum Update {
    /// A synchronous update with values at output ports.
    /// Values are merged into state according to their schema
    /// (delta types are additive, others replace).
    Value(Value),

    /// No update — process chose to do nothing this tick.
    Noop,
}

impl Update {
    pub fn value(v: Value) -> Self {
        Self::Value(v)
    }

    pub fn is_noop(&self) -> bool {
        matches!(self, Self::Noop)
    }

    pub fn into_value(self) -> Option<Value> {
        match self {
            Self::Value(v) => Some(v),
            Self::Noop => None,
        }
    }
}

impl From<Value> for Update {
    fn from(v: Value) -> Self {
        Self::Value(v)
    }
}
