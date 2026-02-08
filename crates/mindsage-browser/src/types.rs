//! Browser connector types — matching the TypeScript API surface.

use serde::{Deserialize, Serialize};

/// Supported AI chat sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportedSite {
    #[serde(rename = "chatgpt")]
    ChatGPT,
    Claude,
    Gemini,
}

impl SupportedSite {
    pub fn all() -> &'static [SupportedSite] {
        &[Self::ChatGPT, Self::Claude, Self::Gemini]
    }

    pub fn base_url(&self) -> &'static str {
        match self {
            Self::ChatGPT => "https://chatgpt.com",
            Self::Claude => "https://claude.ai",
            Self::Gemini => "https://gemini.google.com",
        }
    }

    pub fn cookie_domains(&self) -> &'static [&'static str] {
        match self {
            Self::ChatGPT => &[".chatgpt.com", ".openai.com"],
            Self::Claude => &[".claude.ai", ".anthropic.com"],
            Self::Gemini => &[".google.com", ".gemini.google.com"],
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::ChatGPT => "chatgpt",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "chatgpt" => Some(Self::ChatGPT),
            "claude" => Some(Self::Claude),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }
}

impl std::fmt::Display for SupportedSite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Browser runtime status.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserStatus {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "activeUrl")]
    pub active_url: Option<String>,
    #[serde(rename = "connectedSites")]
    pub connected_sites: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "launchedAt")]
    pub launched_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "memoryUsageMB")]
    pub memory_usage_mb: Option<f64>,
    #[serde(rename = "captureStats")]
    pub capture_stats: CaptureStats,
    pub vnc: VncInfo,
}

/// Capture statistics for the current session.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CaptureStats {
    #[serde(rename = "totalCaptures")]
    pub total_captures: u64,
    #[serde(rename = "conversationsTracked")]
    pub conversations_tracked: usize,
}

/// VNC connection info.
#[derive(Debug, Clone, Serialize)]
pub struct VncInfo {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "wsPort")]
    pub ws_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "vncPort")]
    pub vnc_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

/// Authentication status for a site.
#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "authenticatedAt")]
    pub authenticated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

/// Captured AI conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedConversation {
    pub id: String,
    pub site: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub url: String,
    pub messages: Vec<CapturedMessage>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub indexed: bool,
    #[serde(rename = "messageCount")]
    pub message_count: usize,
}

/// A single message in a captured conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedMessage {
    pub id: String,
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
    pub site: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Capture payload from the extension.
#[derive(Debug, Clone, Deserialize)]
pub struct CapturePayload {
    pub site: String,
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    #[serde(rename = "conversationUrl")]
    pub conversation_url: String,
    pub title: Option<String>,
    pub messages: Vec<CapturedMessage>,
    #[serde(rename = "fullConversation")]
    pub full_conversation: Option<bool>,
}

/// Cookie from the companion extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    #[serde(rename = "httpOnly")]
    pub http_only: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sameSite")]
    pub same_site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "expirationDate")]
    pub expiration_date: Option<f64>,
}

/// Cookie import payload from companion extension.
#[derive(Debug, Clone, Deserialize)]
pub struct CookieImportPayload {
    pub site: String,
    pub cookies: Vec<ImportedCookie>,
}

/// Sync result from headless sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Auto-sync schedule status.
#[derive(Debug, Clone, Serialize)]
pub struct AutoSyncStatus {
    pub enabled: bool,
    #[serde(rename = "intervalHours")]
    pub interval_hours: f64,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSyncAt")]
    pub last_sync_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSyncResult")]
    pub last_sync_result: Option<SyncResult>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "nextSyncAt")]
    pub next_sync_at: Option<String>,
}

/// Per-site auth configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SiteAuthConfig {
    #[serde(skip_serializing_if = "Option::is_none", rename = "authenticatedAt")]
    pub authenticated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSyncAt")]
    pub last_sync_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSyncResult")]
    pub last_sync_result: Option<SyncResult>,
}

/// Site info response.
#[derive(Debug, Clone, Serialize)]
pub struct SiteInfo {
    pub name: String,
    pub url: String,
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "authenticatedAt")]
    pub authenticated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSyncAt")]
    pub last_sync_at: Option<String>,
}
