//! Concrete [`crate::protocol::Protocol`] implementations.
//!
//! Each submodule is one protocol. Add them to an [`crate::protocol::ProtocolRegistry`]
//! via `register(Arc::new(ParallelProtocol::default()))` etc.

pub mod http;
pub mod mesh;
pub mod mesh_agent;
pub mod parallel;
pub mod registry;
pub mod swim;
pub mod rest;
pub mod rest_server;
pub mod web;

pub use http::HttpServer;
pub use mesh::{mesh_links, MeshProtocol, MeshReplica};
pub use mesh_agent::{GossipHandle, LiveField, MeshAgent};
pub use registry::RegistryServer;
pub use web::{serve_web, serve_web_default, WebServer};
pub use swim::Membership;
pub use parallel::{ParallelProcess, ParallelProtocol};
pub use rest::{RestProcess, RestProtocol};
pub use rest_server::RestProcessServer;
