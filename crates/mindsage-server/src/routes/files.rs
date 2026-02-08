//! File management routes — upload, list, delete, import.
//! Matches /api/files/* endpoints from Express.

use std::sync::Arc;

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::state::{AppState, IndexingJob, IndexingRequest, IndexingStatus};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/files", get(list_files))
        .route("/files/upload", post(upload_files))
        .route("/files/{filename}", delete(delete_file))
        .route("/files/{filename}/import", post(import_file))
}

/// GET /api/files — list uploaded files.
async fn list_files(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let uploads_dir = &state.config.data_paths.uploads;
    let imports_dir = &state.config.data_paths.imports;

    let mut files = Vec::new();

    // List files from both uploads and imports
    for (dir, location) in [(uploads_dir, "uploads"), (imports_dir, "imports")] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        let filename = entry.file_name().to_string_lossy().to_string();
                        let file_path = entry.path().to_string_lossy().to_string();
                        let indexed = state.is_file_indexed(&file_path);

                        files.push(serde_json::json!({
                            "filename": filename,
                            "path": file_path,
                            "size": meta.len(),
                            "modified": meta.modified()
                                .ok()
                                .map(|m| chrono::DateTime::<chrono::Utc>::from(m).to_rfc3339())
                                .unwrap_or_default(),
                            "location": location,
                            "indexed": indexed,
                        }));
                    }
                }
            }
        }
    }

    // Sort by modified time, newest first
    files.sort_by(|a, b| {
        let a_time = a.get("modified").and_then(|v| v.as_str()).unwrap_or("");
        let b_time = b.get("modified").and_then(|v| v.as_str()).unwrap_or("");
        b_time.cmp(a_time)
    });

    Json(serde_json::json!({
        "files": files,
        "total": files.len(),
    }))
}

/// POST /api/files/upload — upload files (multipart).
async fn upload_files(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut uploaded = Vec::new();
    let mut errors = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = match field.file_name() {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Sanitize filename
        let safe_filename = sanitize_filename(&filename);
        let upload_path = state.config.data_paths.uploads.join(&safe_filename);

        match field.bytes().await {
            Ok(bytes) => {
                // Handle duplicate filenames
                let final_path = if upload_path.exists() {
                    let stem = std::path::Path::new(&safe_filename)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file");
                    let ext = std::path::Path::new(&safe_filename)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
                    let new_name = if ext.is_empty() {
                        format!("{}_{}", stem, ts)
                    } else {
                        format!("{}_{}.{}", stem, ts, ext)
                    };
                    state.config.data_paths.uploads.join(new_name)
                } else {
                    upload_path
                };

                match std::fs::write(&final_path, &bytes) {
                    Ok(()) => {
                        let final_filename = final_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();

                        // Auto-import: move to imports and queue indexing
                        let import_path = state.config.data_paths.imports.join(&final_filename);
                        if let Err(e) = std::fs::rename(&final_path, &import_path) {
                            // If rename fails (cross-device), copy+delete
                            if std::fs::copy(&final_path, &import_path).is_ok() {
                                let _ = std::fs::remove_file(&final_path);
                            } else {
                                errors.push(serde_json::json!({
                                    "filename": final_filename,
                                    "error": format!("Failed to move to imports: {}", e),
                                }));
                                continue;
                            }
                        }

                        // Queue for indexing
                        let job_id = uuid::Uuid::new_v4().to_string();
                        let import_path_str = import_path.to_string_lossy().to_string();

                        let job = IndexingJob {
                            id: job_id.clone(),
                            filename: final_filename.clone(),
                            file_path: import_path_str.clone(),
                            status: IndexingStatus::Queued,
                            document_id: None,
                            error: None,
                            queued_at: now_millis(),
                            started_at: None,
                            completed_at: None,
                        };
                        state.indexing_jobs.write().insert(job_id.clone(), job);

                        let _ = state.indexing_tx.send(IndexingRequest {
                            job_id: job_id.clone(),
                            file_path: import_path_str,
                            filename: final_filename.clone(),
                        });

                        uploaded.push(serde_json::json!({
                            "filename": final_filename,
                            "size": bytes.len(),
                            "jobId": job_id,
                        }));
                    }
                    Err(e) => {
                        errors.push(serde_json::json!({
                            "filename": safe_filename,
                            "error": format!("Write failed: {}", e),
                        }));
                    }
                }
            }
            Err(e) => {
                errors.push(serde_json::json!({
                    "filename": safe_filename,
                    "error": format!("Read failed: {}", e),
                }));
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "uploaded": uploaded.len(),
            "errors": errors.len(),
            "files": uploaded,
            "errorDetails": errors,
        })),
    )
}

/// DELETE /api/files/:filename — delete a file.
async fn delete_file(
    State(state): State<Arc<AppState>>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    let safe_filename = sanitize_filename(&filename);

    // Try both directories
    for dir in [&state.config.data_paths.uploads, &state.config.data_paths.imports] {
        let file_path = dir.join(&safe_filename);
        if file_path.exists() {
            // Security: ensure path is within the directory
            if let (Ok(canonical), Ok(dir_canonical)) =
                (file_path.canonicalize(), dir.canonicalize())
            {
                if !canonical.starts_with(&dir_canonical) {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({ "error": "Path traversal not allowed" })),
                    );
                }
            }

            match std::fs::remove_file(&file_path) {
                Ok(()) => {
                    return (
                        StatusCode::OK,
                        Json(serde_json::json!({ "deleted": true, "filename": safe_filename })),
                    );
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e.to_string() })),
                    );
                }
            }
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "File not found" })),
    )
}

/// POST /api/files/:filename/import — queue a file for indexing.
async fn import_file(
    State(state): State<Arc<AppState>>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    let safe_filename = sanitize_filename(&filename);

    // Find the file
    let file_path = if state.config.data_paths.imports.join(&safe_filename).exists() {
        state.config.data_paths.imports.join(&safe_filename)
    } else if state.config.data_paths.uploads.join(&safe_filename).exists() {
        state.config.data_paths.uploads.join(&safe_filename)
    } else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "File not found" })),
        );
    };

    let file_path_str = file_path.to_string_lossy().to_string();
    let job_id = uuid::Uuid::new_v4().to_string();

    let job = IndexingJob {
        id: job_id.clone(),
        filename: safe_filename.clone(),
        file_path: file_path_str.clone(),
        status: IndexingStatus::Queued,
        document_id: None,
        error: None,
        queued_at: now_millis(),
        started_at: None,
        completed_at: None,
    };
    state.indexing_jobs.write().insert(job_id.clone(), job);

    let _ = state.indexing_tx.send(IndexingRequest {
        job_id: job_id.clone(),
        file_path: file_path_str,
        filename: safe_filename,
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "queued",
            "jobId": job_id,
        })),
    )
}

/// Sanitize a filename to prevent path traversal.
fn sanitize_filename(name: &str) -> String {
    // Remove directory components
    let name = name
        .replace('/', "")
        .replace('\\', "")
        .replace("..", "");

    // Take just the filename part
    std::path::Path::new(&name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
