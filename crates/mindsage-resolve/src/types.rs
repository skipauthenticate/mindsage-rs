//! Resolver types.

use serde::{Deserialize, Serialize};

/// Available resolver strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolverKind {
    /// BM25 keyword search only.
    Keyword,
    /// Vector similarity search only.
    Vector,
    /// BM25 + vector with RRF fusion.
    Hybrid,
    /// Entity-focused search.
    Entity,
    /// Timeline-aware search.
    Timeline,
    /// LLM-based answer generation.
    Answer,
}

/// A resolve query with strategy selection.
#[derive(Debug, Clone, Deserialize)]
pub struct ResolveQuery {
    pub query: String,
    #[serde(default)]
    pub resolver: Option<ResolverKind>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub filters: Option<ResolveFilters>,
}

fn default_limit() -> usize {
    10
}

/// Optional filters for resolve queries.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResolveFilters {
    pub source: Option<String>,
    pub topic: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

/// A resolved result item.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedItem {
    pub id: i64,
    pub text: String,
    pub score: f64,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passage: Option<String>,
}

/// Result of a resolve operation.
#[derive(Debug, Clone, Serialize)]
pub struct ResolveResult {
    pub items: Vec<ResolvedItem>,
    pub resolver_used: ResolverKind,
    pub total_found: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}
