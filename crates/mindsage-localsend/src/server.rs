//! LocalSend server — session management, discovery, file handling.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::types::*;

/// Maximum session age before auto-cleanup.
const SESSION_TTL: Duration = Duration::from_secs(3600);

/// LocalSend server managing sessions, discovery, and file reception.
pub struct LocalSendServer {
    pub device_info: DeviceInfo,
    uploads_dir: PathBuf,
    sessions: RwLock<HashMap<String, TransferSession>>,
    discovered_devices: RwLock<HashMap<String, String>>,
    running: RwLock<bool>,
}

impl LocalSendServer {
    /// Create a new LocalSend server.
    pub fn new(uploads_dir: &Path, device_name: &str) -> Self {
        let fingerprint = generate_fingerprint(device_name);
        let device_info = DeviceInfo {
            alias: device_name.to_string(),
            version: PROTOCOL_VERSION.to_string(),
            device_model: Some("MindSage".to_string()),
            device_type: "desktop".to_string(),
            fingerprint,
            port: LOCALSEND_PORT,
            protocol: "http".to_string(),
            download: false,
            announce: true,
            address: None,
        };

        Self {
            device_info,
            uploads_dir: uploads_dir.to_path_buf(),
            sessions: RwLock::new(HashMap::new()),
            discovered_devices: RwLock::new(HashMap::new()),
            running: RwLock::new(false),
        }
    }

    /// Mark server as running.
    pub fn start(&self) {
        *self.running.write() = true;
        info!(
            "LocalSend server started (fingerprint: {})",
            self.device_info.fingerprint
        );
    }

    /// Mark server as stopped.
    pub fn stop(&self) {
        *self.running.write() = false;
        info!("LocalSend server stopped");
    }

    /// Check if server is running.
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Get server status.
    pub fn get_status(&self) -> LocalSendStatus {
        LocalSendStatus {
            running: self.is_running(),
            port: LOCALSEND_PORT,
            device_name: self.device_info.alias.clone(),
            fingerprint: self.device_info.fingerprint.clone(),
            discovered_devices: self.discovered_devices.read().len(),
            active_sessions: self.sessions.read().len(),
        }
    }

    /// Get device info (for /api/localsend/v2/info and / endpoints).
    pub fn get_device_info(&self) -> &DeviceInfo {
        &self.device_info
    }

    /// Handle device registration (POST /api/localsend/v2/register).
    pub fn register_device(&self, info: &DeviceInfo) {
        if let Some(addr) = &info.address {
            self.discovered_devices
                .write()
                .insert(info.fingerprint.clone(), addr.clone());
        }
    }

    // ---------------------------------------------------------------
    // Session Management
    // ---------------------------------------------------------------

    /// Prepare a new upload session. Returns session ID and file tokens.
    pub fn prepare_upload(&self, req: PrepareUploadRequest) -> PrepareUploadResponse {
        // Cleanup stale sessions
        self.cleanup_stale_sessions();

        let session_id = uuid::Uuid::new_v4().to_string();
        let mut file_tokens = HashMap::new();

        for (file_id, _file_info) in &req.files {
            let token = uuid::Uuid::new_v4().to_string();
            file_tokens.insert(file_id.clone(), token);
        }

        let session = TransferSession {
            id: session_id.clone(),
            sender_info: req.info,
            files: req.files,
            file_tokens: file_tokens.clone(),
            received_files: std::collections::HashSet::new(),
            saved_filenames: Vec::new(),
            created_at: std::time::Instant::now(),
        };

        self.sessions.write().insert(session_id.clone(), session);

        info!(
            "Transfer session created: {} ({} files)",
            session_id,
            file_tokens.len()
        );

        PrepareUploadResponse {
            session_id,
            files: file_tokens,
        }
    }

    /// Validate upload parameters. Returns Ok(file_name) or Err(error_msg).
    pub fn validate_upload(
        &self,
        session_id: &str,
        file_id: &str,
        token: &str,
    ) -> Result<String, (u16, String)> {
        let sessions = self.sessions.read();
        let session = sessions
            .get(session_id)
            .ok_or((404, "Session not found".to_string()))?;

        let expected_token = session
            .file_tokens
            .get(file_id)
            .ok_or((404, "File not found in session".to_string()))?;

        if expected_token != token {
            return Err((403, "Invalid token".to_string()));
        }

        let file_info = session
            .files
            .get(file_id)
            .ok_or((404, "File info not found".to_string()))?;

        Ok(file_info.file_name.clone())
    }

    /// Record a completed file upload.
    pub fn record_upload(&self, session_id: &str, file_id: &str, saved_filename: &str) {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(session_id) {
            session.received_files.insert(file_id.to_string());
            session
                .saved_filenames
                .push(saved_filename.to_string());
        }
    }

    /// Resolve a unique filename in the uploads directory.
    pub fn resolve_filename(&self, original_name: &str) -> PathBuf {
        let path = self.uploads_dir.join(original_name);
        if !path.exists() {
            return path;
        }

        // Add timestamp to avoid collision
        let stem = Path::new(original_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = Path::new(original_name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        if ext.is_empty() {
            self.uploads_dir.join(format!("{}-{}", stem, ts))
        } else {
            self.uploads_dir.join(format!("{}-{}.{}", stem, ts, ext))
        }
    }

    /// Finish a session, returning saved filenames for auto-import.
    pub fn finish_session(&self, session_id: &str) -> Option<Vec<String>> {
        let mut sessions = self.sessions.write();
        let session = sessions.remove(session_id)?;
        info!(
            "Session {} finished: {} files received",
            session_id,
            session.saved_filenames.len()
        );
        Some(session.saved_filenames)
    }

    /// Cancel a session.
    pub fn cancel_session(&self, session_id: &str) -> bool {
        let removed = self.sessions.write().remove(session_id).is_some();
        if removed {
            info!("Session {} cancelled", session_id);
        }
        removed
    }

    /// Get uploads directory path.
    pub fn uploads_dir(&self) -> &Path {
        &self.uploads_dir
    }

    // ---------------------------------------------------------------
    // Discovery
    // ---------------------------------------------------------------

    /// Build the multicast announcement payload.
    pub fn announcement_payload(&self) -> serde_json::Value {
        serde_json::to_value(&self.device_info).unwrap_or_default()
    }

    /// Record a discovered device.
    pub fn record_discovered_device(&self, fingerprint: &str, address: &str) {
        self.discovered_devices
            .write()
            .insert(fingerprint.to_string(), address.to_string());
    }

    // ---------------------------------------------------------------
    // Internal
    // ---------------------------------------------------------------

    fn cleanup_stale_sessions(&self) {
        let mut sessions = self.sessions.write();
        let stale: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.created_at.elapsed() > SESSION_TTL)
            .map(|(id, _)| id.clone())
            .collect();

        for id in &stale {
            warn!("Cleaning up stale session: {}", id);
            sessions.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_server() -> (LocalSendServer, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let uploads_dir = dir.path().join("uploads");
        std::fs::create_dir_all(&uploads_dir).unwrap();
        let server = LocalSendServer::new(&uploads_dir, "TestDevice");
        (server, dir)
    }

    #[test]
    fn test_device_info() {
        let (server, _dir) = test_server();
        let info = server.get_device_info();
        assert_eq!(info.alias, "TestDevice");
        assert_eq!(info.version, PROTOCOL_VERSION);
        assert_eq!(info.port, LOCALSEND_PORT);
        assert!(!info.fingerprint.is_empty());
    }

    #[test]
    fn test_start_stop() {
        let (server, _dir) = test_server();
        assert!(!server.is_running());

        server.start();
        assert!(server.is_running());

        server.stop();
        assert!(!server.is_running());
    }

    #[test]
    fn test_status() {
        let (server, _dir) = test_server();
        let status = server.get_status();
        assert!(!status.running);
        assert_eq!(status.port, LOCALSEND_PORT);
        assert_eq!(status.device_name, "TestDevice");
        assert_eq!(status.active_sessions, 0);
    }

    #[test]
    fn test_prepare_and_finish_session() {
        let (server, _dir) = test_server();

        let mut files = HashMap::new();
        files.insert(
            "file-1".to_string(),
            FileInfo {
                id: "file-1".to_string(),
                file_name: "test.txt".to_string(),
                size: 100,
                file_type: "text/plain".to_string(),
                sha256: None,
                preview: None,
            },
        );

        let req = PrepareUploadRequest {
            info: SenderInfo {
                alias: "Phone".to_string(),
                version: "2.0".to_string(),
                device_model: None,
                device_type: "mobile".to_string(),
                fingerprint: "abc123".to_string(),
            },
            files,
        };

        let resp = server.prepare_upload(req);
        assert!(!resp.session_id.is_empty());
        assert_eq!(resp.files.len(), 1);
        assert!(resp.files.contains_key("file-1"));

        // Verify session exists
        assert_eq!(server.get_status().active_sessions, 1);

        // Record upload and finish
        server.record_upload(&resp.session_id, "file-1", "test.txt");
        let saved = server.finish_session(&resp.session_id).unwrap();
        assert_eq!(saved, vec!["test.txt"]);

        // Session removed
        assert_eq!(server.get_status().active_sessions, 0);
    }

    #[test]
    fn test_validate_upload() {
        let (server, _dir) = test_server();

        let mut files = HashMap::new();
        files.insert(
            "f1".to_string(),
            FileInfo {
                id: "f1".to_string(),
                file_name: "doc.pdf".to_string(),
                size: 5000,
                file_type: "application/pdf".to_string(),
                sha256: None,
                preview: None,
            },
        );

        let resp = server.prepare_upload(PrepareUploadRequest {
            info: SenderInfo {
                alias: "Sender".to_string(),
                version: "2.0".to_string(),
                device_model: None,
                device_type: "mobile".to_string(),
                fingerprint: "xyz".to_string(),
            },
            files,
        });

        let token = resp.files.get("f1").unwrap();

        // Valid token
        let name = server
            .validate_upload(&resp.session_id, "f1", token)
            .unwrap();
        assert_eq!(name, "doc.pdf");

        // Wrong token
        let err = server
            .validate_upload(&resp.session_id, "f1", "bad-token")
            .unwrap_err();
        assert_eq!(err.0, 403);

        // Wrong file ID
        let err = server
            .validate_upload(&resp.session_id, "f999", token)
            .unwrap_err();
        assert_eq!(err.0, 404);

        // Wrong session
        let err = server
            .validate_upload("no-session", "f1", token)
            .unwrap_err();
        assert_eq!(err.0, 404);
    }

    #[test]
    fn test_cancel_session() {
        let (server, _dir) = test_server();

        let resp = server.prepare_upload(PrepareUploadRequest {
            info: SenderInfo {
                alias: "S".to_string(),
                version: "2.0".to_string(),
                device_model: None,
                device_type: "mobile".to_string(),
                fingerprint: "f".to_string(),
            },
            files: HashMap::new(),
        });

        assert!(server.cancel_session(&resp.session_id));
        assert!(!server.cancel_session(&resp.session_id)); // already cancelled
    }

    #[test]
    fn test_register_device() {
        let (server, _dir) = test_server();

        let info = DeviceInfo {
            alias: "Phone".to_string(),
            version: "2.0".to_string(),
            device_model: None,
            device_type: "mobile".to_string(),
            fingerprint: "phone-fp".to_string(),
            port: 53317,
            protocol: "http".to_string(),
            download: false,
            announce: true,
            address: Some("192.168.1.50".to_string()),
        };

        server.register_device(&info);
        assert_eq!(server.get_status().discovered_devices, 1);
    }

    #[test]
    fn test_resolve_filename() {
        let (server, _dir) = test_server();

        let path1 = server.resolve_filename("test.txt");
        assert!(path1.to_string_lossy().ends_with("test.txt"));

        // Create the file so next resolve gets a unique name
        std::fs::write(&path1, "data").unwrap();
        let path2 = server.resolve_filename("test.txt");
        assert_ne!(path1, path2);
        assert!(path2.to_string_lossy().contains("test-"));
    }

    #[test]
    fn test_fingerprint_consistency() {
        let fp1 = generate_fingerprint("Device");
        let fp2 = generate_fingerprint("Device");
        assert_eq!(fp1, fp2);

        let fp3 = generate_fingerprint("OtherDevice");
        assert_ne!(fp1, fp3);
    }
}

/// Generate a consistent device fingerprint.
fn generate_fingerprint(device_name: &str) -> String {
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "mindsage".to_string());

    let mut hasher = Sha256::new();
    hasher.update(device_name.as_bytes());
    hasher.update(hostname.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..16])
}
