//! LocalSend v2: UDP multicast discovery, file transfer protocol.
//!
//! Implements the LocalSend v2 protocol for receiving files from mobile
//! devices on the local network. Supports multicast discovery and
//! HTTP-based file transfer with session management.

pub mod server;
pub mod types;

pub use server::LocalSendServer;
pub use types::*;
