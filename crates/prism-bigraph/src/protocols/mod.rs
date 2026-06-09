//! Concrete [`crate::protocol::Protocol`] implementations.
//!
//! Each submodule is one protocol. Add them to an [`crate::protocol::ProtocolRegistry`]
//! via `register(Arc::new(ParallelProtocol::default()))` etc.

pub mod mesh;
pub mod parallel;
pub mod rest;
pub mod rest_server;

pub use mesh::{MeshProtocol, MeshReplica};
pub use parallel::{ParallelProcess, ParallelProtocol};
pub use rest::{RestProcess, RestProtocol};
pub use rest_server::RestProcessServer;
