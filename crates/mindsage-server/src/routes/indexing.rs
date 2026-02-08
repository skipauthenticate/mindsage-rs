//! Indexing status routes.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

use crate::state::{AppState, IndexingStatus};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/indexing/status", get(get_indexing_status))
        .route("/indexing/jobs", get(get_indexing_jobs))
        .route("/indexing/jobs/{job_id}", get(get_indexing_job))
}

/// GET /api/indexing/status — summary of indexing state.
async fn get_indexing_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let jobs = state.indexing_jobs.read();
    let queued = jobs
        .values()
        .filter(|j| j.status == IndexingStatus::Queued)
        .count();
    let processing = jobs
        .values()
        .filter(|j| j.status == IndexingStatus::Processing)
        .count();
    let completed = jobs
        .values()
        .filter(|j| j.status == IndexingStatus::Completed)
        .count();
    let failed = jobs
        .values()
        .filter(|j| j.status == IndexingStatus::Failed)
        .count();

    Json(serde_json::json!({
        "queued": queued,
        "processing": processing,
        "completed": completed,
        "failed": failed,
        "total": jobs.len(),
    }))
}

/// GET /api/indexing/jobs — list all jobs.
async fn get_indexing_jobs(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let jobs = state.indexing_jobs.read();
    let mut all_jobs: Vec<&crate::state::IndexingJob> = jobs.values().collect();
    all_jobs.sort_by(|a, b| b.queued_at.cmp(&a.queued_at));

    Json(serde_json::json!({
        "jobs": all_jobs,
        "total": all_jobs.len(),
    }))
}

/// GET /api/indexing/jobs/:jobId — get a single job.
async fn get_indexing_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let jobs = state.indexing_jobs.read();
    match jobs.get(&job_id) {
        Some(job) => (StatusCode::OK, Json(serde_json::json!(job))),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Job not found" })),
        ),
    }
}
