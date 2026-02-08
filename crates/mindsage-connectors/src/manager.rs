//! Connector manager — CRUD, persistence, sync orchestration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use tracing::{info, warn};

use crate::types::*;

/// Manages connector configurations and sync state.
pub struct ConnectorManager {
    connectors_file: PathBuf,
    exports_dir: PathBuf,
    connectors: RwLock<Vec<ConnectorConfig>>,
    run_statuses: RwLock<HashMap<String, RunStatus>>,
}

impl ConnectorManager {
    /// Create a new connector manager.
    pub fn new(connectors_file: &Path, exports_dir: &Path) -> Self {
        let connectors = load_connectors(connectors_file);
        info!("ConnectorManager: {} connectors loaded", connectors.len());

        Self {
            connectors_file: connectors_file.to_path_buf(),
            exports_dir: exports_dir.to_path_buf(),
            connectors: RwLock::new(connectors),
            run_statuses: RwLock::new(HashMap::new()),
        }
    }

    // ---------------------------------------------------------------
    // CRUD
    // ---------------------------------------------------------------

    /// List all connectors.
    pub fn list(&self) -> Vec<ConnectorConfig> {
        self.connectors.read().clone()
    }

    /// Get a connector by ID.
    pub fn get(&self, id: &str) -> Option<ConnectorConfig> {
        self.connectors.read().iter().find(|c| c.id == id).cloned()
    }

    /// Create a new connector.
    pub fn create(&self, req: CreateConnectorRequest) -> ConnectorConfig {
        let connector = ConnectorConfig {
            id: chrono::Utc::now().timestamp_millis().to_string(),
            name: req.name,
            connector_type: req.connector_type,
            config: req.config,
            status: ConnectorStatus::Connected,
            last_sync: None,
            item_count: 0,
        };

        let mut connectors = self.connectors.write();
        connectors.push(connector.clone());
        drop(connectors);
        self.save();

        connector
    }

    /// Update a connector. Returns the updated connector or None if not found.
    pub fn update(&self, id: &str, updates: serde_json::Value) -> Option<ConnectorConfig> {
        let mut connectors = self.connectors.write();
        let connector = connectors.iter_mut().find(|c| c.id == id)?;

        if let Some(name) = updates.get("name").and_then(|v| v.as_str()) {
            connector.name = name.to_string();
        }
        if let Some(config) = updates.get("config") {
            connector.config = config.clone();
        }
        if let Some(status) = updates.get("status").and_then(|v| v.as_str()) {
            connector.status = match status {
                "syncing" => ConnectorStatus::Syncing,
                "error" => ConnectorStatus::Error,
                "paused" => ConnectorStatus::Paused,
                _ => ConnectorStatus::Connected,
            };
        }

        let updated = connector.clone();
        drop(connectors);
        self.save();
        Some(updated)
    }

    /// Delete a connector. Returns true if found and deleted.
    pub fn delete(&self, id: &str) -> bool {
        let mut connectors = self.connectors.write();
        let len_before = connectors.len();
        connectors.retain(|c| c.id != id);
        let deleted = connectors.len() < len_before;
        drop(connectors);

        if deleted {
            self.save();
        }
        deleted
    }

    // ---------------------------------------------------------------
    // Sync Status
    // ---------------------------------------------------------------

    /// Get run status for a connector.
    pub fn get_run_status(&self, id: &str) -> RunStatus {
        self.run_statuses
            .read()
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    /// Update connector after a successful import.
    pub fn mark_import_complete(&self, id: &str, item_count: usize) {
        let mut connectors = self.connectors.write();
        if let Some(connector) = connectors.iter_mut().find(|c| c.id == id) {
            connector.status = ConnectorStatus::Connected;
            connector.last_sync = Some(chrono::Utc::now().to_rfc3339());
            connector.item_count = item_count;
        }
        drop(connectors);
        self.save();
    }

    /// Mark a connector as errored.
    pub fn mark_error(&self, id: &str, error: &str) {
        let mut connectors = self.connectors.write();
        if let Some(connector) = connectors.iter_mut().find(|c| c.id == id) {
            connector.status = ConnectorStatus::Error;
        }
        drop(connectors);
        self.save();

        self.run_statuses.write().insert(
            id.to_string(),
            RunStatus {
                running: false,
                output: vec![format!("Error: {}", error)],
                last_run: Some(chrono::Utc::now().to_rfc3339()),
                exit_code: Some(1),
                connector_id: Some(id.to_string()),
            },
        );
    }

    // ---------------------------------------------------------------
    // Exports
    // ---------------------------------------------------------------

    /// Get the exports directory for a connector.
    pub fn exports_dir_for(&self, id: &str) -> PathBuf {
        let dir = self.exports_dir.join(id);
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    /// List export files for a connector.
    pub fn list_exports(&self, id: &str) -> Vec<String> {
        let dir = self.exports_dir.join(id);
        match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".json") && !name.starts_with('.') {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Read an export file.
    pub fn read_export(&self, id: &str, filename: &str) -> Option<serde_json::Value> {
        let path = self.exports_dir.join(id).join(filename);
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Load pending media registry for a connector.
    pub fn get_pending_media(&self, id: &str) -> Option<crate::types::PendingMediaRegistry> {
        let exports_dir = self.exports_dir.join(id);
        crate::facebook::load_media_registry(&exports_dir)
    }

    // ---------------------------------------------------------------
    // Persistence
    // ---------------------------------------------------------------

    fn save(&self) {
        let connectors = self.connectors.read();
        if let Ok(data) = serde_json::to_string_pretty(&*connectors) {
            if let Err(e) = std::fs::write(&self.connectors_file, data) {
                warn!("Failed to save connectors: {}", e);
            }
        }
    }
}

fn load_connectors(path: &Path) -> Vec<ConnectorConfig> {
    match std::fs::read_to_string(path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager(dir: &Path) -> ConnectorManager {
        let connectors_file = dir.join("connectors.json");
        let exports_dir = dir.join("exports");
        std::fs::create_dir_all(&exports_dir).unwrap();
        ConnectorManager::new(&connectors_file, &exports_dir)
    }

    #[test]
    fn test_create_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = test_manager(dir.path());

        let conn = mgr.create(CreateConnectorRequest {
            name: "My ChatGPT".into(),
            connector_type: ConnectorType::File,
            config: serde_json::json!({}),
        });

        assert_eq!(conn.name, "My ChatGPT");
        assert_eq!(conn.connector_type, ConnectorType::File);
        assert_eq!(conn.status, ConnectorStatus::Connected);

        let list = mgr.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, conn.id);
    }

    #[test]
    fn test_get_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = test_manager(dir.path());

        let conn = mgr.create(CreateConnectorRequest {
            name: "Test".into(),
            connector_type: ConnectorType::Api,
            config: serde_json::json!({}),
        });

        assert!(mgr.get(&conn.id).is_some());
        assert!(mgr.delete(&conn.id));
        assert!(mgr.get(&conn.id).is_none());
        assert!(!mgr.delete(&conn.id)); // already deleted
    }

    #[test]
    fn test_update() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = test_manager(dir.path());

        let conn = mgr.create(CreateConnectorRequest {
            name: "Original".into(),
            connector_type: ConnectorType::Api,
            config: serde_json::json!({}),
        });

        let updated = mgr
            .update(&conn.id, serde_json::json!({ "name": "Renamed" }))
            .unwrap();
        assert_eq!(updated.name, "Renamed");
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let connectors_file = dir.path().join("connectors.json");
        let exports_dir = dir.path().join("exports");
        std::fs::create_dir_all(&exports_dir).unwrap();

        // Create with first manager
        {
            let mgr = ConnectorManager::new(&connectors_file, &exports_dir);
            mgr.create(CreateConnectorRequest {
                name: "Persisted".into(),
                connector_type: ConnectorType::File,
                config: serde_json::json!({}),
            });
        }

        // Load with second manager
        let mgr2 = ConnectorManager::new(&connectors_file, &exports_dir);
        let list = mgr2.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Persisted");
    }

    #[test]
    fn test_mark_import_complete() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = test_manager(dir.path());

        let conn = mgr.create(CreateConnectorRequest {
            name: "Test".into(),
            connector_type: ConnectorType::File,
            config: serde_json::json!({}),
        });

        mgr.mark_import_complete(&conn.id, 42);
        let updated = mgr.get(&conn.id).unwrap();
        assert_eq!(updated.item_count, 42);
        assert!(updated.last_sync.is_some());
    }

    #[test]
    fn test_mark_error() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = test_manager(dir.path());

        let conn = mgr.create(CreateConnectorRequest {
            name: "Test".into(),
            connector_type: ConnectorType::File,
            config: serde_json::json!({}),
        });

        mgr.mark_error(&conn.id, "connection failed");
        let updated = mgr.get(&conn.id).unwrap();
        assert_eq!(updated.status, ConnectorStatus::Error);

        let status = mgr.get_run_status(&conn.id);
        assert!(!status.running);
        assert_eq!(status.exit_code, Some(1));
    }
}
