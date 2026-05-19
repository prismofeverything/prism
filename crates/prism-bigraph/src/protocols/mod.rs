//! Concrete [`crate::protocol::Protocol`] implementations.
//!
//! Each submodule is one protocol. Add them to an [`crate::protocol::ProtocolRegistry`]
//! via `register(Arc::new(ParallelProtocol::default()))` etc.

pub mod parallel;

pub use parallel::{ParallelProcess, ParallelProtocol};
