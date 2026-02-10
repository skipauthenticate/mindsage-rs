//! PII detection, anonymization, and consent session routes.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::state::AppState;
use mindsage_protocol::consent::*;
use mindsage_protocol::pii::*;

// ---------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // PII
        .route("/pii/detect", post(detect_pii))
        .route("/pii/anonymize", post(anonymize_text))
        .route("/pii/deanonymize", post(deanonymize_text))
        .route("/pii/status", get(pii_status))
        // Consent
        .route("/consent/session", post(create_consent_session))
        .route("/consent/sessions", get(list_consent_sessions))
        .route(
            "/consent/session/{id}",
            get(get_consent_session).delete(revoke_consent_session),
        )
        .route("/consent/session/{id}/check", post(check_consent))
        .route(
            "/consent/session/{id}/categories",
            post(update_consent_categories),
        )
        // Status & presets (used by frontend)
        .route("/consent/status", get(consent_status))
        .route("/consent/presets", get(consent_presets))
}

// ---------------------------------------------------------------
// Request/Response types
// ---------------------------------------------------------------

#[derive(serde::Deserialize)]
struct TextInput {
    text: String,
}

#[derive(serde::Deserialize)]
struct CheckConsentBody {
    category: DataCategory,
}

#[derive(serde::Deserialize)]
struct UpdateCategoriesBody {
    categories: Vec<DataCategory>,
}

// ---------------------------------------------------------------
// PII Handlers
// ---------------------------------------------------------------

async fn detect_pii(
    State(state): State<Arc<AppState>>,
    Json(input): Json<TextInput>,
) -> Json<serde_json::Value> {
    let entities = state.pii_detector.detect(&input.text);
    Json(serde_json::json!({
        "entities": entities,
        "count": entities.len(),
    }))
}

async fn anonymize_text(
    State(state): State<Arc<AppState>>,
    Json(input): Json<TextInput>,
) -> Json<AnonymizationResult> {
    Json(state.pii_detector.anonymize(&input.text))
}

async fn deanonymize_text(
    State(state): State<Arc<AppState>>,
    Json(input): Json<TextInput>,
) -> Json<serde_json::Value> {
    let restored = state.pii_detector.deanonymize(&input.text);
    Json(serde_json::json!({ "text": restored }))
}

async fn pii_status(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let counts = state.pii_detector.get_status();
    Json(serde_json::json!({
        "active": true,
        "tokenCounts": counts,
    }))
}

// ---------------------------------------------------------------
// Consent Handlers
// ---------------------------------------------------------------

async fn create_consent_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateConsentRequest>,
) -> Json<ConsentSession> {
    Json(state.consent_manager.create_session(req))
}

async fn list_consent_sessions(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let sessions = state.consent_manager.list_sessions();
    Json(serde_json::json!({
        "sessions": sessions,
        "count": sessions.len(),
    }))
}

async fn get_consent_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.consent_manager.get_session(&id) {
        Some(session) => Json(serde_json::to_value(session).unwrap_or_default()),
        None => Json(serde_json::json!({ "error": "Session not found" })),
    }
}

async fn revoke_consent_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if state.consent_manager.revoke_session(&id) {
        Json(serde_json::json!({ "success": true }))
    } else {
        Json(serde_json::json!({ "error": "Session not found" }))
    }
}

async fn check_consent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<CheckConsentBody>,
) -> Json<serde_json::Value> {
    let allowed = state.consent_manager.check_category(&id, &body.category);
    Json(serde_json::json!({
        "allowed": allowed,
        "category": body.category,
    }))
}

async fn update_consent_categories(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateCategoriesBody>,
) -> Json<serde_json::Value> {
    match state.consent_manager.update_session(&id, body.categories) {
        Some(session) => Json(serde_json::to_value(session).unwrap_or_default()),
        None => Json(serde_json::json!({ "error": "Session not found" })),
    }
}

async fn consent_status(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let sessions = state.consent_manager.list_sessions();
    Json(serde_json::json!({
        "available": true,
        "active_sessions": sessions.len(),
        "presets_available": ["full_access", "minimal", "anonymized"],
        "categories_available": [
            "personal_info", "financial", "health", "location",
            "communications", "browsing_history", "preferences"
        ]
    }))
}

async fn consent_presets() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "presets": [
            {
                "name": "full_access",
                "description": "Allow access to all data categories",
                "allowed_categories": [
                    "personal_info", "financial", "health", "location",
                    "communications", "browsing_history", "preferences"
                ],
                "blocked_categories": [],
                "exposed_pii_types": []
            },
            {
                "name": "minimal",
                "description": "Minimal access — only preferences and browsing history",
                "allowed_categories": ["preferences", "browsing_history"],
                "blocked_categories": [
                    "personal_info", "financial", "health", "location", "communications"
                ],
                "exposed_pii_types": []
            },
            {
                "name": "anonymized",
                "description": "Access all categories but anonymize PII",
                "allowed_categories": [
                    "personal_info", "financial", "health", "location",
                    "communications", "browsing_history", "preferences"
                ],
                "blocked_categories": [],
                "exposed_pii_types": ["name", "email", "phone", "address", "ssn"]
            }
        ]
    }))
}
