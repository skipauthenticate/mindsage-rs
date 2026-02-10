//! Browser connector routes — Chrome lifecycle, capture, auth, sync, cookies.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::state::AppState;
use mindsage_browser::*;
use mindsage_store::AddDocumentOptions;

// ---------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Status & Control
        .route("/browser-connector/status", get(get_status))
        .route("/browser-connector/launch", post(launch_browser))
        .route("/browser-connector/close", post(close_browser))
        .route("/browser-connector/navigate", post(navigate))
        // Capture & Conversations
        .route("/browser-connector/capture", post(capture))
        .route("/browser-connector/conversations", get(list_conversations))
        .route(
            "/browser-connector/conversations/{id}",
            get(get_conversation).delete(delete_conversation),
        )
        // Indexing & Stats
        .route("/browser-connector/reindex", post(reindex))
        .route("/browser-connector/stats", get(get_stats))
        // Config
        .route(
            "/browser-connector/config",
            get(get_config).put(update_config),
        )
        // VNC
        .route("/browser-connector/vnc/status", get(vnc_status))
        .route("/browser-connector/vnc/check", get(vnc_check))
        // Auth
        .route("/browser-connector/auth-status", get(auth_status))
        .route("/browser-connector/report-auth", post(report_auth))
        .route("/browser-connector/auth", delete(clear_auth))
        // Sites
        .route("/browser-connector/sites", get(get_sites))
        // Sync
        .route("/browser-connector/sync", post(start_sync))
        .route(
            "/browser-connector/navigate-to-site",
            post(navigate_to_site),
        )
        .route("/browser-connector/sync-complete", post(sync_complete))
        // Auto-sync
        .route("/browser-connector/auto-sync", get(auto_sync_status))
        .route("/browser-connector/auto-sync/start", post(auto_sync_start))
        .route("/browser-connector/auto-sync/stop", post(auto_sync_stop))
        .route(
            "/browser-connector/auto-sync/interval",
            put(auto_sync_interval),
        )
        // Cookies
        .route(
            "/browser-connector/import-cookies",
            post(import_cookies),
        )
        .route(
            "/browser-connector/pending-cookies",
            get(pending_cookies),
        )
        // Debug
        .route("/browser-connector/debug", post(debug_endpoint))
}

// ---------------------------------------------------------------
// Query / Body types
// ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ConversationQuery {
    site: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SiteQuery {
    site: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NavigateBody {
    url: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LaunchBody {
    headed: Option<bool>,
    #[serde(rename = "startUrl")]
    start_url: Option<String>,
    vnc: Option<bool>,
    #[serde(rename = "vncPort")]
    vnc_port: Option<u16>,
    #[serde(rename = "wsPort")]
    ws_port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct ReportAuthBody {
    site: String,
    authenticated: bool,
}

#[derive(Debug, Deserialize)]
struct SyncBody {
    site: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NavigateToSiteBody {
    site: String,
    #[serde(rename = "forSync")]
    for_sync: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AutoSyncStartBody {
    #[serde(rename = "intervalHours")]
    interval_hours: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct AutoSyncIntervalBody {
    hours: f64,
}

// ---------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------

#[derive(Serialize)]
struct SuccessResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl SuccessResponse {
    fn ok() -> Self {
        Self {
            success: true,
            message: None,
        }
    }
    fn with_message(msg: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(msg.into()),
        }
    }
}

#[derive(Serialize)]
struct ConversationListResponse {
    conversations: Vec<ConversationSummary>,
    total: usize,
    page: usize,
    #[serde(rename = "pageSize")]
    page_size: usize,
}

#[derive(Serialize)]
struct ConversationSummary {
    id: String,
    site: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    url: String,
    #[serde(rename = "messageCount")]
    message_count: usize,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    indexed: bool,
}

#[derive(Serialize)]
struct VncCheckResponse {
    available: Vec<String>,
    missing: Vec<String>,
    #[serde(rename = "installCommand")]
    install_command: Option<String>,
}

// ---------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------

async fn get_status(State(state): State<Arc<AppState>>) -> Json<BrowserStatus> {
    Json(state.browser_manager.get_status())
}

async fn launch_browser(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LaunchBody>,
) -> Json<serde_json::Value> {
    // Apply launch options to config
    if let Some(headed) = body.headed {
        let mut config = state.browser_manager.config.write();
        config.headed = headed;
    }
    if let Some(url) = &body.start_url {
        let mut config = state.browser_manager.config.write();
        config.default_url = url.clone();
    }

    // Note: actual Chrome process spawning will be implemented in Phase 4
    // when we add tokio::process::Command for Chrome lifecycle
    info!("Browser launch requested (stub — Chrome process management pending)");
    Json(serde_json::json!({
        "success": true,
        "message": "Browser launch queued (Chrome process management pending)"
    }))
}

async fn close_browser(State(state): State<Arc<AppState>>) -> Json<SuccessResponse> {
    if !state.browser_manager.is_running() {
        return Json(SuccessResponse::with_message("Browser is not running"));
    }
    // Stub: actual Chrome kill will be implemented in Phase 4
    info!("Browser close requested (stub)");
    Json(SuccessResponse::with_message("Browser close requested"))
}

async fn navigate(
    State(state): State<Arc<AppState>>,
    Json(body): Json<NavigateBody>,
) -> Json<serde_json::Value> {
    if !state.browser_manager.is_running() {
        return Json(serde_json::json!({ "error": "Browser is not running" }));
    }
    info!("Navigate to: {}", body.url);
    // Stub: actual CDP navigation in Phase 4
    Json(serde_json::json!({ "success": true, "url": body.url }))
}

async fn capture(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CapturePayload>,
) -> Json<serde_json::Value> {
    // Validate site
    if SupportedSite::from_name(&payload.site).is_none() {
        return Json(serde_json::json!({
            "error": format!("Unsupported site: {}", payload.site)
        }));
    }

    let new_messages = state.browser_manager.process_capture(payload);
    Json(serde_json::json!({
        "success": true,
        "newMessages": new_messages
    }))
}

async fn list_conversations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ConversationQuery>,
) -> Json<ConversationListResponse> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    let page = (offset / limit) + 1;

    let (conversations, total) =
        state
            .browser_manager
            .get_conversations(page, limit, query.site.as_deref());

    let summaries: Vec<ConversationSummary> = conversations
        .into_iter()
        .map(|c| ConversationSummary {
            id: c.id,
            site: c.site,
            title: c.title,
            url: c.url,
            message_count: c.message_count,
            created_at: c.created_at,
            updated_at: c.updated_at,
            indexed: c.indexed,
        })
        .collect();

    Json(ConversationListResponse {
        conversations: summaries,
        total,
        page,
        page_size: limit,
    })
}

async fn get_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.browser_manager.get_conversation(&id) {
        Some(conv) => Json(serde_json::to_value(conv).unwrap_or_default()),
        None => Json(serde_json::json!({ "error": "Conversation not found" })),
    }
}

async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<SuccessResponse> {
    if state.browser_manager.delete_conversation(&id) {
        Json(SuccessResponse::ok())
    } else {
        Json(SuccessResponse::with_message("Conversation not found"))
    }
}

async fn reindex(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let (conversations, total) = state.browser_manager.get_conversations(1, 10000, None);
    info!("Reindex requested for {} conversations", total);

    // Queue each conversation for indexing into the vector store
    let mut indexed = 0;
    for conv in &conversations {
        // Build document content from messages
        let content = conv
            .messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        if content.is_empty() {
            continue;
        }

        let title = conv
            .title
            .as_deref()
            .unwrap_or("Untitled conversation");

        let metadata = serde_json::json!({
            "title": title,
            "source": format!("browser-connector-{}", conv.site),
            "url": conv.url,
            "conversationId": conv.id,
        });

        match state.store.add_document(
            &content,
            AddDocumentOptions {
                metadata: Some(metadata),
                ..Default::default()
            },
        ) {
            Ok(_doc_id) => {
                indexed += 1;
            }
            Err(e) => {
                warn!("Failed to index conversation {}: {}", conv.id, e);
            }
        }
    }

    Json(serde_json::json!({
        "success": true,
        "total": total,
        "indexed": indexed
    }))
}

async fn get_stats(State(state): State<Arc<AppState>>) -> Json<CaptureStats> {
    Json(state.browser_manager.get_capture_stats())
}

async fn get_config(State(state): State<Arc<AppState>>) -> Json<BrowserConnectorConfig> {
    Json(state.browser_manager.get_config())
}

async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(updates): Json<serde_json::Value>,
) -> Json<SuccessResponse> {
    state.browser_manager.update_config(updates);
    Json(SuccessResponse::ok())
}

async fn vnc_status(State(state): State<Arc<AppState>>) -> Json<VncInfo> {
    let status = state.browser_manager.get_status();
    Json(status.vnc)
}

async fn vnc_check() -> Json<VncCheckResponse> {
    // Check for VNC dependencies (Xvfb, x11vnc, websockify)
    let deps = ["Xvfb", "x11vnc", "websockify"];
    let mut available = Vec::new();
    let mut missing = Vec::new();

    for dep in &deps {
        if which_exists(dep) {
            available.push(dep.to_string());
        } else {
            missing.push(dep.to_string());
        }
    }

    let install_command = if !missing.is_empty() {
        Some(format!(
            "sudo apt-get install -y {}",
            missing
                .iter()
                .map(|d| d.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        ))
    } else {
        None
    };

    Json(VncCheckResponse {
        available,
        missing,
        install_command,
    })
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn auth_status(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SiteQuery>,
) -> Json<AuthStatus> {
    Json(state.browser_manager.get_auth_status(query.site.as_deref()))
}

async fn report_auth(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ReportAuthBody>,
) -> Json<SuccessResponse> {
    if body.authenticated {
        state.browser_manager.set_authenticated(&body.site);
    }
    Json(SuccessResponse::ok())
}

async fn clear_auth(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SiteQuery>,
) -> Json<SuccessResponse> {
    state.browser_manager.clear_auth(query.site.as_deref());
    Json(SuccessResponse::ok())
}

async fn get_sites(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let sites = state.browser_manager.get_sites_info();
    let sites_with_id: Vec<serde_json::Value> = sites
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.name,
                "name": s.name,
                "url": s.url,
                "authenticated": s.authenticated,
            })
        })
        .collect();
    Json(serde_json::json!({ "sites": sites_with_id }))
}

async fn start_sync(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SyncBody>,
) -> Json<serde_json::Value> {
    let site = body.site.as_deref().unwrap_or("chatgpt");

    // Check auth
    let auth = state.browser_manager.get_auth_status(Some(site));
    if !auth.authenticated {
        return Json(serde_json::json!({
            "error": format!("Not authenticated for {}. Please authenticate first.", site),
            "status": 401
        }));
    }

    // Stub: actual sync (CDP navigation + extension interaction) in Phase 4
    info!("Sync requested for {} (stub)", site);
    Json(serde_json::json!({
        "success": true,
        "message": format!("Sync started for {} (headless sync pending)", site)
    }))
}

async fn navigate_to_site(
    State(state): State<Arc<AppState>>,
    Json(body): Json<NavigateToSiteBody>,
) -> Json<serde_json::Value> {
    if !state.browser_manager.is_running() {
        return Json(serde_json::json!({ "error": "Browser is not running" }));
    }

    let site = match SupportedSite::from_name(&body.site) {
        Some(s) => s,
        None => {
            return Json(serde_json::json!({
                "error": format!("Unknown site: {}", body.site)
            }))
        }
    };

    info!("Navigate to site: {} (url: {})", site, site.base_url());
    // Stub: actual navigation via CDP in Phase 4
    Json(serde_json::json!({
        "success": true,
        "url": site.base_url()
    }))
}

async fn sync_complete(
    State(state): State<Arc<AppState>>,
    Json(result): Json<SyncResult>,
) -> Json<SuccessResponse> {
    info!("Sync complete: success={}", result.success);
    // Store sync result in config
    let mut config = state.browser_manager.config.write();
    config.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
    config.last_sync_result = Some(result);
    let _ = config.save();
    Json(SuccessResponse::ok())
}

async fn auto_sync_status(State(state): State<Arc<AppState>>) -> Json<AutoSyncStatus> {
    Json(state.browser_manager.get_auto_sync_status())
}

async fn auto_sync_start(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AutoSyncStartBody>,
) -> Json<serde_json::Value> {
    // Check if at least one site is authenticated
    let auth = state.browser_manager.get_auth_status(None);
    if !auth.authenticated {
        return Json(serde_json::json!({
            "error": "Not authenticated for any site",
            "status": 401
        }));
    }

    if let Some(hours) = body.interval_hours {
        state.browser_manager.set_auto_sync_interval(hours);
    }

    state.browser_manager.start_auto_sync();
    Json(serde_json::json!({
        "success": true,
        "message": "Auto-sync enabled"
    }))
}

async fn auto_sync_stop(State(state): State<Arc<AppState>>) -> Json<SuccessResponse> {
    state.browser_manager.stop_auto_sync();
    Json(SuccessResponse::with_message("Auto-sync disabled"))
}

async fn auto_sync_interval(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AutoSyncIntervalBody>,
) -> Json<serde_json::Value> {
    if body.hours < 0.5 || body.hours > 24.0 {
        return Json(serde_json::json!({
            "error": "Interval must be between 0.5 and 24 hours"
        }));
    }
    state.browser_manager.set_auto_sync_interval(body.hours);
    Json(serde_json::json!({
        "success": true,
        "intervalHours": body.hours
    }))
}

async fn import_cookies(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CookieImportPayload>,
) -> Json<serde_json::Value> {
    // Validate site
    let site = match SupportedSite::from_name(&payload.site) {
        Some(s) => s,
        None => {
            return Json(serde_json::json!({
                "error": format!("Unknown site: {}", payload.site)
            }))
        }
    };

    if payload.cookies.is_empty() {
        return Json(serde_json::json!({ "error": "No cookies provided" }));
    }

    // Filter cookies to allowed domains
    let allowed_domains = site.cookie_domains();
    let filtered: Vec<ImportedCookie> = payload
        .cookies
        .into_iter()
        .filter(|c| {
            allowed_domains
                .iter()
                .any(|d| c.domain.ends_with(d.trim_start_matches('.')))
        })
        .collect();

    let count = filtered.len();
    info!(
        "Importing {} cookies for {} (filtered from request)",
        count,
        site.name()
    );

    state
        .browser_manager
        .store_pending_cookies(site.name(), filtered);

    // TODO: trigger headless sync asynchronously (Phase 4)

    Json(serde_json::json!({
        "success": true,
        "imported": count,
        "site": site.name()
    }))
}

async fn pending_cookies(
    State(state): State<Arc<AppState>>,
) -> Json<std::collections::HashMap<String, usize>> {
    Json(state.browser_manager.get_pending_cookies_counts())
}

async fn debug_endpoint(Json(body): Json<serde_json::Value>) -> Json<SuccessResponse> {
    info!("Browser debug: {:?}", body);
    Json(SuccessResponse::ok())
}
