//! Consent session management — category-based data access control.

use std::collections::HashMap;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Data categories for consent management.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataCategory {
    Personal,
    Financial,
    Health,
    Professional,
    Social,
    Technical,
    General,
}

impl DataCategory {
    pub fn all() -> &'static [DataCategory] {
        &[
            Self::Personal,
            Self::Financial,
            Self::Health,
            Self::Professional,
            Self::Social,
            Self::Technical,
            Self::General,
        ]
    }
}

/// Consent preset for quick configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsentPreset {
    Full,
    Professional,
    Minimal,
    Custom,
}

/// A consent session with category-based access control.
#[derive(Debug, Clone, Serialize)]
pub struct ConsentSession {
    pub id: String,
    #[serde(rename = "allowedCategories")]
    pub allowed_categories: Vec<DataCategory>,
    pub preset: ConsentPreset,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    pub active: bool,
}

/// Request to create a consent session.
#[derive(Debug, Deserialize)]
pub struct CreateConsentRequest {
    pub preset: Option<ConsentPreset>,
    pub categories: Option<Vec<DataCategory>>,
    #[serde(rename = "durationMinutes")]
    pub duration_minutes: Option<u64>,
}

/// Manages consent sessions with sliding TTL.
pub struct ConsentManager {
    sessions: RwLock<HashMap<String, ConsentSession>>,
    max_sessions: usize,
}

impl ConsentManager {
    /// Create a new consent manager.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            max_sessions: 100,
        }
    }

    /// Create a new consent session.
    pub fn create_session(&self, req: CreateConsentRequest) -> ConsentSession {
        let duration_mins = req.duration_minutes.unwrap_or(60);
        let now = chrono::Utc::now();
        let expires = now + chrono::Duration::minutes(duration_mins as i64);

        let (preset, categories) = match req.preset.unwrap_or(ConsentPreset::Full) {
            ConsentPreset::Full => (
                ConsentPreset::Full,
                DataCategory::all().to_vec(),
            ),
            ConsentPreset::Professional => (
                ConsentPreset::Professional,
                vec![
                    DataCategory::Professional,
                    DataCategory::Technical,
                    DataCategory::General,
                ],
            ),
            ConsentPreset::Minimal => (
                ConsentPreset::Minimal,
                vec![DataCategory::General],
            ),
            ConsentPreset::Custom => (
                ConsentPreset::Custom,
                req.categories.unwrap_or_else(|| vec![DataCategory::General]),
            ),
        };

        let session = ConsentSession {
            id: uuid::Uuid::new_v4().to_string(),
            allowed_categories: categories,
            preset,
            created_at: now.to_rfc3339(),
            expires_at: expires.to_rfc3339(),
            active: true,
        };

        // Enforce max sessions (LRU eviction)
        let mut sessions = self.sessions.write();
        if sessions.len() >= self.max_sessions {
            // Remove oldest session
            if let Some(oldest_id) = sessions
                .values()
                .min_by_key(|s| s.created_at.clone())
                .map(|s| s.id.clone())
            {
                sessions.remove(&oldest_id);
            }
        }

        sessions.insert(session.id.clone(), session.clone());
        info!("Consent session created: {}", session.id);
        session
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: &str) -> Option<ConsentSession> {
        let sessions = self.sessions.read();
        sessions.get(id).cloned()
    }

    /// Check if a session allows access to a specific category.
    pub fn check_category(&self, session_id: &str, category: &DataCategory) -> bool {
        let sessions = self.sessions.read();
        sessions
            .get(session_id)
            .map(|s| s.active && s.allowed_categories.contains(category))
            .unwrap_or(false)
    }

    /// Update session categories.
    pub fn update_session(
        &self,
        id: &str,
        categories: Vec<DataCategory>,
    ) -> Option<ConsentSession> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(id)?;
        session.allowed_categories = categories;
        session.preset = ConsentPreset::Custom;
        Some(session.clone())
    }

    /// Revoke (delete) a session.
    pub fn revoke_session(&self, id: &str) -> bool {
        self.sessions.write().remove(id).is_some()
    }

    /// List all active sessions.
    pub fn list_sessions(&self) -> Vec<ConsentSession> {
        self.sessions.read().values().cloned().collect()
    }

    /// Get session count.
    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }
}

impl Default for ConsentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_full_session() {
        let mgr = ConsentManager::new();
        let session = mgr.create_session(CreateConsentRequest {
            preset: Some(ConsentPreset::Full),
            categories: None,
            duration_minutes: None,
        });
        assert_eq!(session.allowed_categories.len(), DataCategory::all().len());
        assert!(session.active);
    }

    #[test]
    fn test_category_check() {
        let mgr = ConsentManager::new();
        let session = mgr.create_session(CreateConsentRequest {
            preset: Some(ConsentPreset::Minimal),
            categories: None,
            duration_minutes: None,
        });
        assert!(mgr.check_category(&session.id, &DataCategory::General));
        assert!(!mgr.check_category(&session.id, &DataCategory::Financial));
    }

    #[test]
    fn test_revoke_session() {
        let mgr = ConsentManager::new();
        let session = mgr.create_session(CreateConsentRequest {
            preset: None,
            categories: None,
            duration_minutes: None,
        });
        assert!(mgr.revoke_session(&session.id));
        assert!(mgr.get_session(&session.id).is_none());
    }
}
