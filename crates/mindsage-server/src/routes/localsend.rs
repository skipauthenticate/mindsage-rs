//! LocalSend routes — protocol endpoints + management routes.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use tracing::{info, warn};

use crate::state::AppState;
use mindsage_localsend::*;

// ---------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Management routes (port 3003)
        .route("/localsend/status", get(get_status))
        .route("/localsend/start", post(start_server))
        .route("/localsend/stop", post(stop_server))
        .route("/localsend/setup", post(setup))
        .route("/localsend/configure", post(configure))
        // Protocol v2 routes (also served on port 3003 for compat)
        .route("/localsend/v2/info", get(get_info))
        .route("/localsend/v2/register", post(register))
        .route("/localsend/v2/prepare-upload", post(prepare_upload))
        .route("/localsend/v2/upload", post(upload_file))
        .route("/localsend/v2/cancel", post(cancel))
        .route("/localsend/v2/finish", post(finish))
}

// ---------------------------------------------------------------
// Management handlers
// ---------------------------------------------------------------

async fn get_status(State(state): State<Arc<AppState>>) -> Json<LocalSendStatus> {
    Json(state.localsend_server.get_status())
}

async fn start_server(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    state.localsend_server.start();
    // Note: actual UDP multicast discovery is started by the runtime
    // (tokio task spawned at server startup). This endpoint just marks
    // the server as active.
    Json(serde_json::json!({
        "success": true,
        "message": "LocalSend server started"
    }))
}

async fn stop_server(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    state.localsend_server.stop();
    Json(serde_json::json!({
        "success": true,
        "message": "LocalSend server stopped"
    }))
}

async fn setup() -> Json<serde_json::Value> {
    // No-op — LocalSend is built-in
    Json(serde_json::json!({
        "success": true,
        "message": "LocalSend is built-in, no setup needed"
    }))
}

async fn configure() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "success": true,
        "message": "Configuration updated"
    }))
}

// ---------------------------------------------------------------
// Protocol v2 handlers
// ---------------------------------------------------------------

async fn get_info(State(state): State<Arc<AppState>>) -> Json<DeviceInfo> {
    Json(state.localsend_server.get_device_info().clone())
}

async fn register(
    State(state): State<Arc<AppState>>,
    Json(info): Json<DeviceInfo>,
) -> Json<DeviceInfo> {
    state.localsend_server.register_device(&info);
    Json(state.localsend_server.get_device_info().clone())
}

async fn prepare_upload(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PrepareUploadRequest>,
) -> Json<PrepareUploadResponse> {
    let response = state.localsend_server.prepare_upload(req);
    Json(response)
}

async fn upload_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Json<serde_json::Value> {
    // Validate session and token
    let file_name = match state
        .localsend_server
        .validate_upload(&query.session_id, &query.file_id, &query.token)
    {
        Ok(name) => name,
        Err((status, msg)) => {
            return Json(serde_json::json!({ "error": msg, "status": status }));
        }
    };

    if body.is_empty() {
        return Json(serde_json::json!({ "error": "No file data received" }));
    }

    // Resolve unique filename and save
    let dest = state.localsend_server.resolve_filename(&file_name);
    match tokio::fs::write(&dest, &body).await {
        Ok(_) => {
            let saved_name = dest
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&file_name)
                .to_string();

            info!(
                "File received: {} ({} bytes)",
                saved_name,
                body.len()
            );

            state
                .localsend_server
                .record_upload(&query.session_id, &query.file_id, &saved_name);

            Json(serde_json::json!({ "success": true }))
        }
        Err(e) => {
            warn!("Failed to save file {}: {}", file_name, e);
            Json(serde_json::json!({
                "error": format!("Failed to save file: {}", e),
                "status": 500
            }))
        }
    }
}

async fn cancel(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SessionQuery>,
) -> Json<serde_json::Value> {
    state.localsend_server.cancel_session(&query.session_id);
    Json(serde_json::json!({ "success": true }))
}

async fn finish(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SessionQuery>,
) -> Json<serde_json::Value> {
    match state.localsend_server.finish_session(&query.session_id) {
        Some(saved_files) => {
            // Queue received files for indexing
            for filename in &saved_files {
                let file_path = state
                    .localsend_server
                    .uploads_dir()
                    .join(filename)
                    .to_string_lossy()
                    .to_string();

                // Queue for indexing via the existing indexing pipeline
                let job_id = uuid::Uuid::new_v4().to_string();
                let _ = state.indexing_tx.send(crate::state::IndexingRequest {
                    job_id,
                    file_path,
                    filename: filename.clone(),
                });
            }

            Json(serde_json::json!({
                "success": true,
                "filesReceived": saved_files.len()
            }))
        }
        None => Json(serde_json::json!({
            "error": "Session not found",
            "status": 404
        })),
    }
}
