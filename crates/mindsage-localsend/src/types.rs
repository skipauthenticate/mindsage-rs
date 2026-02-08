//! LocalSend v2 protocol types.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Protocol version.
pub const PROTOCOL_VERSION: &str = "2.0";
/// Standard LocalSend port.
pub const LOCALSEND_PORT: u16 = 53317;
/// Multicast group address.
pub const MULTICAST_GROUP: &str = "224.0.0.167";

/// Device information for discovery and identification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub alias: String,
    pub version: String,
    #[serde(rename = "deviceModel", skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(rename = "deviceType")]
    pub device_type: String,
    pub fingerprint: String,
    pub port: u16,
    pub protocol: String,
    pub download: bool,
    pub announce: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

/// File metadata from sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub size: u64,
    #[serde(rename = "fileType")]
    pub file_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// Sender info in prepare-upload request.
#[derive(Debug, Clone, Deserialize)]
pub struct SenderInfo {
    pub alias: String,
    pub version: String,
    #[serde(rename = "deviceModel")]
    pub device_model: Option<String>,
    #[serde(rename = "deviceType")]
    pub device_type: String,
    pub fingerprint: String,
}

/// Prepare-upload request body.
#[derive(Debug, Clone, Deserialize)]
pub struct PrepareUploadRequest {
    pub info: SenderInfo,
    pub files: HashMap<String, FileInfo>,
}

/// Prepare-upload response.
#[derive(Debug, Clone, Serialize)]
pub struct PrepareUploadResponse {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub files: HashMap<String, String>,
}

/// Active transfer session.
pub struct TransferSession {
    pub id: String,
    pub sender_info: SenderInfo,
    pub files: HashMap<String, FileInfo>,
    pub file_tokens: HashMap<String, String>,
    pub received_files: HashSet<String>,
    pub saved_filenames: Vec<String>,
    pub created_at: std::time::Instant,
}

/// Upload query parameters.
#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "fileId")]
    pub file_id: String,
    pub token: String,
}

/// Cancel/finish query parameters.
#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// LocalSend server status.
#[derive(Debug, Clone, Serialize)]
pub struct LocalSendStatus {
    pub running: bool,
    pub port: u16,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    pub fingerprint: String,
    #[serde(rename = "discoveredDevices")]
    pub discovered_devices: usize,
    #[serde(rename = "activeSessions")]
    pub active_sessions: usize,
}
