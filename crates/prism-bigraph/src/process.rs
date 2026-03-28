//! Process and Step traits — the core computational units.
//!
//! A `Process` is a temporal computation: given state at its input ports
//! and a time interval, it produces an update at its output ports.
//!
//! A `Step` is a dependency-triggered computation: it fires when its
//! input state changes, with no notion of time.

use std::any::Any;
use std::fmt::Debug;

use prism_schema::{Schema, Value};

use crate::ports::PortSchema;
use crate::update::Update;

/// A temporal process that advances state over time.
///
/// This is the Rust equivalent of process-bigraph's `Process` class.
/// Processes declare typed input/output ports, receive sliced state
/// at those ports, and return updates to apply.
pub trait Process: Send + Sync + Debug {
    /// Schema for this process's configuration.
    fn config_schema() -> Schema
    where
        Self: Sized,
    {
        Schema::Any
    }

    /// Declare input ports: what state this process reads.
    fn inputs(&self) -> PortSchema;

    /// Declare output ports: what state this process writes.
    fn outputs(&self) -> PortSchema;

    /// The time interval between invocations (in seconds).
    fn interval(&self) -> f64 {
        1.0
    }

    /// Compute an update given the current state at input ports
    /// and the elapsed time interval.
    ///
    /// The returned `Value` should be a map keyed by output port names,
    /// containing the updates to apply to the connected state.
    fn update(&self, state: &Value, interval: f64) -> Update;

    /// Optional: provide initial state for ports this process manages.
    fn initial_state(&self) -> Value {
        Value::None
    }

    /// Downcast support for trait objects.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A dependency-triggered step (no time dimension).
///
/// Steps fire when their input state is updated by another process or step.
/// They enable reactive dataflow within the composition.
pub trait Step: Send + Sync + Debug {
    /// Schema for this step's configuration.
    fn config_schema() -> Schema
    where
        Self: Sized,
    {
        Schema::Any
    }

    /// Declare input ports.
    fn inputs(&self) -> PortSchema;

    /// Declare output ports.
    fn outputs(&self) -> PortSchema;

    /// Priority for ordering when multiple steps are triggered simultaneously.
    /// Higher priority steps run first.
    fn priority(&self) -> f64 {
        0.0
    }

    /// Compute an update given the current state at input ports.
    fn update(&self, state: &Value) -> Update;

    /// Optional: provide initial state.
    fn initial_state(&self) -> Value {
        Value::None
    }

    /// Downcast support.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A node in the composition that is either a Process or a Step.
#[derive(Debug)]
pub enum ProcessNode {
    Process(Box<dyn Process>),
    Step(Box<dyn Step>),
}

impl ProcessNode {
    pub fn inputs(&self) -> PortSchema {
        match self {
            Self::Process(p) => p.inputs(),
            Self::Step(s) => s.inputs(),
        }
    }

    pub fn outputs(&self) -> PortSchema {
        match self {
            Self::Process(p) => p.outputs(),
            Self::Step(s) => s.outputs(),
        }
    }

    pub fn initial_state(&self) -> Value {
        match self {
            Self::Process(p) => p.initial_state(),
            Self::Step(s) => s.initial_state(),
        }
    }

    pub fn is_process(&self) -> bool {
        matches!(self, Self::Process(_))
    }

    pub fn is_step(&self) -> bool {
        matches!(self, Self::Step(_))
    }
}
