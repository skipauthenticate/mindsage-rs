//! HTTP route handlers — matches the existing Express API surface.

pub mod browser;
pub mod chat;
pub mod connectors;
pub mod files;
pub mod indexing;
pub mod localsend;
pub mod privacy;
pub mod stats;
pub mod vector_store;

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;

use crate::state::AppState;

/// Build the main Axum router with all routes.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .nest("/api", api_routes())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .merge(stats::routes())
        .merge(vector_store::routes())
        .merge(files::routes())
        .merge(indexing::routes())
        .merge(chat::routes())
        .merge(browser::routes())
        .merge(localsend::routes())
        .merge(connectors::routes())
        .merge(privacy::routes())
}
