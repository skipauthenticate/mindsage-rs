//! Browser manager — Chrome lifecycle, capture state, cookie storage, sync orchestration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use tracing::{info, warn};

use crate::config::BrowserConnectorConfig;
use crate::types::*;

/// Central browser connector manager.
pub struct BrowserManager {
    pub config: RwLock<BrowserConnectorConfig>,
    data_dir: PathBuf,
    /// Chrome process PID if running.
    chrome_pid: RwLock<Option<u32>>,
    /// Session capture stats.
    capture_stats: RwLock<CaptureStats>,
    /// Captured conversations by ID.
    conversations: RwLock<HashMap<String, CapturedConversation>>,
    /// Pending cookies per site (from companion extension).
    pending_cookies: RwLock<HashMap<String, Vec<ImportedCookie>>>,
    /// Auto-sync interval handle (None if disabled).
    auto_sync_active: RwLock<bool>,
    /// When browser was launched.
    launched_at: RwLock<Option<String>>,
}

impl BrowserManager {
    /// Create a new browser manager with the given data directory.
    pub fn new(data_dir: &Path) -> Self {
        let config = BrowserConnectorConfig::load(data_dir);
        let conversations = Self::load_conversations(data_dir);

        info!(
            "BrowserManager initialized: {} conversations loaded",
            conversations.len()
        );

        Self {
            config: RwLock::new(config),
            data_dir: data_dir.to_path_buf(),
            chrome_pid: RwLock::new(None),
            capture_stats: RwLock::new(CaptureStats::default()),
            conversations: RwLock::new(conversations),
            pending_cookies: RwLock::new(HashMap::new()),
            auto_sync_active: RwLock::new(false),
            launched_at: RwLock::new(None),
        }
    }

    // ---------------------------------------------------------------
    // Status
    // ---------------------------------------------------------------

    /// Get current browser status.
    pub fn get_status(&self) -> BrowserStatus {
        let config = self.config.read();
        let pid = *self.chrome_pid.read();
        let stats = self.capture_stats.read().clone();
        let conversations = self.conversations.read();
        let launched_at = self.launched_at.read().clone();

        let connected_sites: Vec<String> = SupportedSite::all()
            .iter()
            .filter(|s| {
                config
                    .get_site_auth(s.name())
                    .authenticated_at
                    .is_some()
            })
            .map(|s| s.name().to_string())
            .collect();

        let mut stats_out = stats;
        stats_out.conversations_tracked = conversations.len();

        BrowserStatus {
            running: pid.is_some(),
            pid,
            active_url: None,
            connected_sites,
            launched_at,
            memory_usage_mb: None,
            capture_stats: stats_out,
            vnc: VncInfo {
                enabled: false,
                ws_port: None,
                vnc_port: None,
                display: None,
            },
        }
    }

    /// Check if browser is running.
    pub fn is_running(&self) -> bool {
        self.chrome_pid.read().is_some()
    }

    // ---------------------------------------------------------------
    // Authentication
    // ---------------------------------------------------------------

    /// Get authentication status, optionally for a specific site.
    pub fn get_auth_status(&self, site: Option<&str>) -> AuthStatus {
        let config = self.config.read();
        if let Some(site_name) = site {
            let auth = config.get_site_auth(site_name);
            AuthStatus {
                authenticated: auth.authenticated_at.is_some(),
                authenticated_at: auth.authenticated_at,
                site: Some(site_name.to_string()),
            }
        } else {
            // Check if any site is authenticated
            let any_auth = SupportedSite::all()
                .iter()
                .any(|s| config.get_site_auth(s.name()).authenticated_at.is_some());
            AuthStatus {
                authenticated: any_auth,
                authenticated_at: None,
                site: None,
            }
        }
    }

    /// Record authentication for a site.
    pub fn set_authenticated(&self, site: &str) {
        let mut config = self.config.write();
        let mut auth = config.get_site_auth(site);
        auth.authenticated_at = Some(chrono::Utc::now().to_rfc3339());
        config.set_site_auth(site, auth);
        let _ = config.save();
        info!("Authenticated: {}", site);
    }

    /// Clear authentication for a site (or all sites).
    pub fn clear_auth(&self, site: Option<&str>) {
        let mut config = self.config.write();
        if let Some(site_name) = site {
            config.sites.remove(site_name);
        } else {
            config.sites.clear();
        }
        let _ = config.save();
    }

    /// Get list of supported sites with their auth status.
    pub fn get_sites_info(&self) -> Vec<SiteInfo> {
        let config = self.config.read();
        SupportedSite::all()
            .iter()
            .map(|site| {
                let auth = config.get_site_auth(site.name());
                SiteInfo {
                    name: site.name().to_string(),
                    url: site.base_url().to_string(),
                    authenticated: auth.authenticated_at.is_some(),
                    authenticated_at: auth.authenticated_at.clone(),
                    last_sync_at: auth.last_sync_at.clone(),
                }
            })
            .collect()
    }

    // ---------------------------------------------------------------
    // Capture Management
    // ---------------------------------------------------------------

    /// Process a capture payload from the extension.
    pub fn process_capture(&self, payload: CapturePayload) -> usize {
        let now = chrono::Utc::now().to_rfc3339();
        let mut conversations = self.conversations.write();

        let entry = conversations
            .entry(payload.conversation_id.clone())
            .or_insert_with(|| CapturedConversation {
                id: payload.conversation_id.clone(),
                site: payload.site.clone(),
                title: payload.title.clone(),
                url: payload.conversation_url.clone(),
                messages: Vec::new(),
                created_at: now.clone(),
                updated_at: now.clone(),
                indexed: false,
                message_count: 0,
            });

        // Update title if provided
        if payload.title.is_some() {
            entry.title = payload.title;
        }

        // Add new messages (dedup by ID)
        let existing_ids: std::collections::HashSet<String> =
            entry.messages.iter().map(|m| m.id.clone()).collect();

        let mut new_count = 0;
        for msg in payload.messages {
            if !existing_ids.contains(&msg.id) {
                entry.messages.push(msg);
                new_count += 1;
            }
        }

        entry.message_count = entry.messages.len();
        entry.updated_at = chrono::Utc::now().to_rfc3339();

        drop(conversations);

        // Update stats
        {
            let mut stats = self.capture_stats.write();
            stats.total_captures += 1;
        }

        // Persist conversations
        self.save_conversations();

        new_count
    }

    /// Get all conversations (paginated).
    pub fn get_conversations(
        &self,
        page: usize,
        page_size: usize,
        site: Option<&str>,
    ) -> (Vec<CapturedConversation>, usize) {
        let conversations = self.conversations.read();
        let mut filtered: Vec<&CapturedConversation> = conversations
            .values()
            .filter(|c| site.map_or(true, |s| c.site == s))
            .collect();

        filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let total = filtered.len();
        let start = (page.saturating_sub(1)) * page_size;
        let paged: Vec<CapturedConversation> = filtered
            .into_iter()
            .skip(start)
            .take(page_size)
            .cloned()
            .collect();

        (paged, total)
    }

    /// Get a single conversation by ID.
    pub fn get_conversation(&self, id: &str) -> Option<CapturedConversation> {
        self.conversations.read().get(id).cloned()
    }

    /// Delete a conversation.
    pub fn delete_conversation(&self, id: &str) -> bool {
        let removed = self.conversations.write().remove(id).is_some();
        if removed {
            self.save_conversations();
        }
        removed
    }

    /// Get capture statistics.
    pub fn get_capture_stats(&self) -> CaptureStats {
        let stats = self.capture_stats.read();
        let conversations = self.conversations.read();
        CaptureStats {
            total_captures: stats.total_captures,
            conversations_tracked: conversations.len(),
        }
    }

    // ---------------------------------------------------------------
    // Cookie Management (Companion Extension)
    // ---------------------------------------------------------------

    /// Store pending cookies from the companion extension.
    pub fn store_pending_cookies(&self, site: &str, cookies: Vec<ImportedCookie>) {
        let count = cookies.len();
        self.pending_cookies
            .write()
            .insert(site.to_string(), cookies);
        info!("Stored {} pending cookies for {}", count, site);

        // Mark site as authenticated
        self.set_authenticated(site);
    }

    /// Get and clear pending cookies for a site.
    pub fn take_pending_cookies(&self, site: &str) -> Option<Vec<ImportedCookie>> {
        self.pending_cookies.write().remove(site)
    }

    /// Get pending cookie counts per site (without exposing values).
    pub fn get_pending_cookies_counts(&self) -> HashMap<String, usize> {
        self.pending_cookies
            .read()
            .iter()
            .map(|(site, cookies)| (site.clone(), cookies.len()))
            .collect()
    }

    // ---------------------------------------------------------------
    // Auto-Sync
    // ---------------------------------------------------------------

    /// Get auto-sync status.
    pub fn get_auto_sync_status(&self) -> AutoSyncStatus {
        let config = self.config.read();
        let active = *self.auto_sync_active.read();
        AutoSyncStatus {
            enabled: active,
            interval_hours: config.auto_sync_interval_hours,
            last_sync_at: config.last_sync_at.clone(),
            last_sync_result: config.last_sync_result.clone(),
            next_sync_at: None, // Would calculate from last_sync_at + interval
        }
    }

    /// Enable auto-sync.
    pub fn start_auto_sync(&self) {
        *self.auto_sync_active.write() = true;
        let mut config = self.config.write();
        config.auto_sync_enabled = true;
        let _ = config.save();
        info!("Auto-sync enabled");
    }

    /// Disable auto-sync.
    pub fn stop_auto_sync(&self) {
        *self.auto_sync_active.write() = false;
        let mut config = self.config.write();
        config.auto_sync_enabled = false;
        let _ = config.save();
        info!("Auto-sync disabled");
    }

    /// Update auto-sync interval.
    pub fn set_auto_sync_interval(&self, hours: f64) {
        let mut config = self.config.write();
        config.auto_sync_interval_hours = hours.max(0.5).min(24.0);
        let _ = config.save();
    }

    // ---------------------------------------------------------------
    // Configuration
    // ---------------------------------------------------------------

    /// Get a copy of the current config.
    pub fn get_config(&self) -> BrowserConnectorConfig {
        self.config.read().clone()
    }

    /// Update configuration with partial values.
    pub fn update_config(&self, updates: serde_json::Value) {
        let mut config = self.config.write();
        if let Some(auto_start) = updates.get("autoStart").and_then(|v| v.as_bool()) {
            config.auto_start = auto_start;
        }
        if let Some(url) = updates.get("defaultUrl").and_then(|v| v.as_str()) {
            config.default_url = url.to_string();
        }
        if let Some(headed) = updates.get("headed").and_then(|v| v.as_bool()) {
            config.headed = headed;
        }
        let _ = config.save();
    }

    // ---------------------------------------------------------------
    // Persistence
    // ---------------------------------------------------------------

    fn conversations_path(&self) -> PathBuf {
        self.data_dir.join("conversations.json")
    }

    fn load_conversations(data_dir: &Path) -> HashMap<String, CapturedConversation> {
        let path = data_dir.join("conversations.json");
        match std::fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn save_conversations(&self) {
        let conversations = self.conversations.read();
        if let Ok(data) = serde_json::to_string_pretty(&*conversations) {
            if let Err(e) = std::fs::write(self.conversations_path(), data) {
                warn!("Failed to save conversations: {}", e);
            }
        }
    }
}
