//! Concrete [`crate::protocol::Protocol`] implementations.
//!
//! Each submodule is one protocol. Add them to an [`crate::protocol::ProtocolRegistry`]
//! via `register(Arc::new(ParallelProtocol::default()))` etc.

pub mod mesh;
pub mod mesh_agent;
pub mod parallel;
pub mod swim;
pub mod rest;
pub mod rest_server;

pub use mesh::{mesh_links, MeshProtocol, MeshReplica};
pub use mesh_agent::{GossipHandle, MeshAgent};
pub use swim::Membership;
pub use parallel::{ParallelProcess, ParallelProtocol};
pub use rest::{RestProcess, RestProtocol};
pub use rest_server::RestProcessServer;
