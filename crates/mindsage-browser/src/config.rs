//! Browser connector configuration persistence.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::SiteAuthConfig;

/// Persisted browser connector configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConnectorConfig {
    #[serde(default = "default_false")]
    pub auto_start: bool,
    #[serde(default = "default_url")]
    pub default_url: String,
    #[serde(default = "default_false")]
    pub headed: bool,
    #[serde(default = "default_vnc_port")]
    pub vnc_port: u16,
    #[serde(default = "default_memory_limit")]
    pub memory_limit: usize,
    #[serde(default)]
    pub sites: HashMap<String, SiteAuthConfig>,
    #[serde(default = "default_false")]
    pub auto_sync_enabled: bool,
    #[serde(default = "default_interval")]
    pub auto_sync_interval_hours: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_result: Option<crate::types::SyncResult>,
    /// Path to config file (not serialized).
    #[serde(skip)]
    pub config_path: PathBuf,
}

fn default_false() -> bool {
    false
}
fn default_url() -> String {
    "https://chatgpt.com".into()
}
fn default_vnc_port() -> u16 {
    5900
}
fn default_memory_limit() -> usize {
    512
}
fn default_interval() -> f64 {
    6.0
}

impl Default for BrowserConnectorConfig {
    fn default() -> Self {
        Self {
            auto_start: false,
            default_url: "https://chatgpt.com".into(),
            headed: false,
            vnc_port: 5900,
            memory_limit: 512,
            sites: HashMap::new(),
            auto_sync_enabled: false,
            auto_sync_interval_hours: 6.0,
            last_sync_at: None,
            last_sync_result: None,
            config_path: PathBuf::new(),
        }
    }
}

impl BrowserConnectorConfig {
    /// Load config from a JSON file, or return defaults.
    pub fn load(config_dir: &Path) -> Self {
        let config_path = config_dir.join("config.json");
        let mut config: BrowserConnectorConfig = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        config.config_path = config_path;
        config
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<(), std::io::Error> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&self.config_path, json)
    }

    /// Get auth config for a site, creating an entry if missing.
    pub fn get_site_auth(&self, site: &str) -> SiteAuthConfig {
        self.sites.get(site).cloned().unwrap_or_default()
    }

    /// Update auth config for a site.
    pub fn set_site_auth(&mut self, site: &str, auth: SiteAuthConfig) {
        self.sites.insert(site.to_string(), auth);
    }
}
