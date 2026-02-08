//! Browser connector — Chrome lifecycle, CDP cookie injection, extension relay.
//!
//! Manages a Chromium instance for capturing AI conversations from
//! ChatGPT, Claude, and Gemini via a companion Chrome extension.

pub mod config;
pub mod manager;
pub mod types;

pub use config::BrowserConnectorConfig;
pub use manager::BrowserManager;
pub use types::*;
