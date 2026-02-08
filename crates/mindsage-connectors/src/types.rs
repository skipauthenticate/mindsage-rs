//! Connector types — matching the TypeScript API surface.

use serde::{Deserialize, Serialize};

/// Connector configuration persisted to connectors.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub connector_type: ConnectorType,
    #[serde(default)]
    pub config: serde_json::Value,
    pub status: ConnectorStatus,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSync")]
    pub last_sync: Option<String>,
    #[serde(rename = "itemCount", default)]
    pub item_count: usize,
}

/// Type of data connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorType {
    Api,
    Webhook,
    File,
    Custom,
}

/// Connector status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorStatus {
    Connected,
    Syncing,
    Error,
    Paused,
}

/// Request body for creating a connector.
#[derive(Debug, Deserialize)]
pub struct CreateConnectorRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub connector_type: ConnectorType,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// Sync run status.
#[derive(Debug, Clone, Serialize)]
pub struct RunStatus {
    pub running: bool,
    pub output: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastRun")]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "exitCode")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "connectorId")]
    pub connector_id: Option<String>,
}

impl Default for RunStatus {
    fn default() -> Self {
        Self {
            running: false,
            output: Vec::new(),
            last_run: None,
            exit_code: None,
            connector_id: None,
        }
    }
}

/// Result of processing an import (ChatGPT or Facebook).
#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub success: bool,
    #[serde(rename = "itemCount")]
    pub item_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Pending media file info (Facebook import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMediaFile {
    #[serde(rename = "originalPath")]
    pub original_path: String,
    pub filename: String,
    #[serde(rename = "type")]
    pub media_type: String,
    pub extension: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
    #[serde(rename = "storedAt")]
    pub stored_at: String,
    #[serde(rename = "storedPath")]
    pub stored_path: String,
}

/// Pending media registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingMediaRegistry {
    pub files: Vec<PendingMediaFile>,
    #[serde(rename = "lastUpdated")]
    pub last_updated: String,
    #[serde(rename = "totalSize")]
    pub total_size: u64,
    pub counts: MediaCounts,
}

/// Media type counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaCounts {
    pub photos: usize,
    pub videos: usize,
    pub audio: usize,
}
