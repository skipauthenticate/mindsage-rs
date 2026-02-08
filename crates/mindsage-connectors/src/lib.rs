//! Data connectors: Notion API, Facebook ZIP, ChatGPT import.
//!
//! Manages data source connections, file-based imports (ChatGPT ZIP,
//! Facebook ZIP), and API-based syncs (Notion). Persists connector
//! configuration to `data/connectors.json`.

pub mod chatgpt;
pub mod facebook;
pub mod manager;
pub mod types;

pub use manager::ConnectorManager;
pub use types::*;
