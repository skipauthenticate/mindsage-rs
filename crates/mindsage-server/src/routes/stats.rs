//! Stats and server info routes.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use crate::state::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/stats", get(get_stats))
        .route("/server-info", get(get_server_info))
}

/// GET /api/stats — storage statistics.
async fn get_stats(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let store_stats = state.store.get_stats().unwrap_or_else(|_| {
        mindsage_store::StoreStats {
            total_documents: 0,
            total_chunks: 0,
            paragraph_chunks: 0,
            section_chunks: 0,
            embeddings_stored: 0,
            embedding_dimension: state.config.embedding_dim,
            db_path: String::new(),
            db_size_mb: 0.0,
            matrix_loaded: false,
            matrix_rows: 0,
        }
    });

    // Count files in uploads/imports dirs
    let upload_count = count_files_in_dir(&state.config.data_paths.uploads);
    let import_count = count_files_in_dir(&state.config.data_paths.imports);

    let jobs = state.indexing_jobs.read();
    let queued = jobs.values().filter(|j| j.status == crate::state::IndexingStatus::Queued).count();
    let processing = jobs.values().filter(|j| j.status == crate::state::IndexingStatus::Processing).count();

    Json(serde_json::json!({
        "documents": store_stats.total_documents,
        "chunks": store_stats.total_chunks,
        "paragraphChunks": store_stats.paragraph_chunks,
        "sectionChunks": store_stats.section_chunks,
        "embeddings": store_stats.embeddings_stored,
        "embeddingDimension": store_stats.embedding_dimension,
        "dbSizeMb": store_stats.db_size_mb,
        "matrixLoaded": store_stats.matrix_loaded,
        "matrixRows": store_stats.matrix_rows,
        "uploads": upload_count,
        "imports": import_count,
        "indexingQueue": {
            "queued": queued,
            "processing": processing,
        },
    }))
}

/// GET /api/server-info — network info.
async fn get_server_info(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let hostname = hostname();
    let ip = local_ip();
    let port = state.config.port;

    Json(serde_json::json!({
        "hostname": hostname,
        "ip": ip,
        "port": port,
        "url": format!("http://{}:{}", ip, port),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    }))
}

fn count_files_in_dir(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn hostname() -> String {
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(not(unix))]
    {
        "unknown".to_string()
    }
}

fn local_ip() -> String {
    // Try to get the local IP by connecting to a public address
    use std::net::UdpSocket;
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}
