//! Connector routes — CRUD, sync, upload, exports.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tracing::{info, warn};

use crate::state::AppState;
use mindsage_connectors::*;
use mindsage_store::AddDocumentOptions;

// ---------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // CRUD
        .route("/connectors", get(list_connectors).post(create_connector))
        .route(
            "/connectors/{id}",
            put(update_connector).delete(delete_connector),
        )
        // Sync & Status
        .route("/connectors/{id}/sync", post(sync_connector))
        .route("/connectors/{id}/status", get(get_status))
        .route("/connectors/{id}/stop", post(stop_sync))
        // Upload
        .route("/connectors/{id}/upload", post(upload_file))
        // Exports
        .route("/connectors/{id}/exports", get(list_exports))
        .route(
            "/connectors/{id}/exports/{filename}",
            get(get_export_file),
        )
        // Pending media
        .route(
            "/connectors/{id}/pending-media",
            get(get_pending_media),
        )
        .route("/pending-media", get(get_all_pending_media))
}

// ---------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------

async fn list_connectors(State(state): State<Arc<AppState>>) -> Json<Vec<ConnectorConfig>> {
    Json(state.connector_manager.list())
}

async fn create_connector(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateConnectorRequest>,
) -> Json<ConnectorConfig> {
    let connector = state.connector_manager.create(req);
    Json(connector)
}

async fn update_connector(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(updates): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    match state.connector_manager.update(&id, updates) {
        Some(connector) => Json(serde_json::to_value(connector).unwrap_or_default()),
        None => Json(serde_json::json!({ "error": "Connector not found" })),
    }
}

async fn delete_connector(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if state.connector_manager.delete(&id) {
        Json(serde_json::json!({ "success": true }))
    } else {
        Json(serde_json::json!({ "error": "Connector not found" }))
    }
}

async fn sync_connector(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let connector = match state.connector_manager.get(&id) {
        Some(c) => c,
        None => return Json(serde_json::json!({ "error": "Connector not found" })),
    };

    info!("Sync requested for connector: {} ({})", connector.name, id);

    // For custom/file connectors, sync is triggered by upload
    // For API connectors (Notion), we'd need the API token
    Json(serde_json::json!({
        "success": true,
        "message": format!("Sync started for {}", connector.name)
    }))
}

async fn get_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<RunStatus> {
    Json(state.connector_manager.get_run_status(&id))
}

async fn stop_sync(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    info!("Stop sync requested for connector: {}", id);
    Json(serde_json::json!({ "success": true }))
}

async fn upload_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Json<serde_json::Value> {
    let connector = match state.connector_manager.get(&id) {
        Some(c) => c,
        None => return Json(serde_json::json!({ "error": "Connector not found" })),
    };

    if body.is_empty() {
        return Json(serde_json::json!({ "error": "No file data received" }));
    }

    // Determine import type from connector config
    let script = connector
        .config
        .get("script")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    let exports_dir = state.connector_manager.exports_dir_for(&id);

    // Save the uploaded ZIP to a temp file
    let temp_zip = exports_dir.join("_upload.zip");
    if let Err(e) = std::fs::write(&temp_zip, &body) {
        return Json(serde_json::json!({
            "error": format!("Failed to save upload: {}", e)
        }));
    }

    let result = match script {
        "chatgpt-import" => chatgpt::process_chatgpt_export(&temp_zip, &exports_dir),
        "facebook-import" => facebook::process_facebook_export(&temp_zip, &exports_dir),
        _ => ImportResult {
            success: false,
            item_count: 0,
            error: Some(format!("Unknown import type: {}", script)),
            details: None,
        },
    };

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_zip);

    if result.success {
        // Update connector status
        state
            .connector_manager
            .mark_import_complete(&id, result.item_count);

        // Auto-index exported files to vector store
        let indexed = auto_index_exports(&state, &id, &exports_dir);

        Json(serde_json::json!({
            "success": true,
            "itemCount": result.item_count,
            "indexed": indexed,
            "details": result.details,
        }))
    } else {
        state
            .connector_manager
            .mark_error(&id, result.error.as_deref().unwrap_or("Unknown error"));

        Json(serde_json::json!({
            "success": false,
            "error": result.error,
        }))
    }
}

/// Auto-index connector exports into the vector store.
fn auto_index_exports(state: &AppState, connector_id: &str, exports_dir: &std::path::Path) -> usize {
    let documents = chatgpt::build_index_documents(exports_dir);
    let mut indexed = 0;

    for (text, metadata) in documents {
        let mut meta = metadata;
        meta.as_object_mut().map(|m| {
            m.insert(
                "connectorId".to_string(),
                serde_json::Value::String(connector_id.to_string()),
            )
        });

        match state.store.add_document(
            &text,
            AddDocumentOptions {
                metadata: Some(meta),
                ..Default::default()
            },
        ) {
            Ok(_) => indexed += 1,
            Err(e) => {
                warn!("Failed to index connector document: {}", e);
            }
        }
    }

    if indexed > 0 {
        info!(
            "Auto-indexed {} documents from connector {}",
            indexed, connector_id
        );
    }

    indexed
}

async fn list_exports(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Vec<String>> {
    Json(state.connector_manager.list_exports(&id))
}

async fn get_export_file(
    State(state): State<Arc<AppState>>,
    Path((id, filename)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    match state.connector_manager.read_export(&id, &filename) {
        Some(data) => Json(data),
        None => Json(serde_json::json!({ "error": "Export file not found" })),
    }
}

async fn get_pending_media(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.connector_manager.get_pending_media(&id) {
        Some(registry) => Json(serde_json::to_value(registry).unwrap_or_default()),
        None => Json(serde_json::json!({
            "files": [],
            "totalSize": 0,
            "counts": { "photos": 0, "videos": 0, "audio": 0 }
        })),
    }
}

async fn get_all_pending_media(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let connectors = state.connector_manager.list();
    let mut all_files = Vec::new();
    let mut total_size = 0u64;

    for connector in &connectors {
        if let Some(registry) = state.connector_manager.get_pending_media(&connector.id) {
            total_size += registry.total_size;
            all_files.extend(registry.files);
        }
    }

    Json(serde_json::json!({
        "files": all_files.len(),
        "totalSize": total_size,
        "connectors": connectors.len(),
    }))
}
