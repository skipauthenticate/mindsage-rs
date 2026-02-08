//! Data types for documents, chunks, and search results.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A document row from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

impl Document {
    /// Parse metadata JSON into a map.
    pub fn metadata_map(&self) -> HashMap<String, serde_json::Value> {
        match &self.metadata {
            Some(v) => serde_json::from_value(v.clone()).unwrap_or_default(),
            None => HashMap::new(),
        }
    }
}

/// A chunk row from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: i64,
    pub doc_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_chunk_id: Option<i64>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enriched_text: Option<String>,
    pub chunk_index: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_start: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_end: Option<i32>,
    pub level: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub created_at: i64,
}

/// Intermediate search result before fusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub chunk_id: i64,
    pub doc_id: i64,
    pub text: String,
    pub score: f64,
    pub level: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enriched_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_chunk_id: Option<i64>,
    pub chunk_index: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_start: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_end: Option<i32>,
}

/// Store-level statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreStats {
    pub total_documents: i64,
    pub total_chunks: i64,
    pub paragraph_chunks: i64,
    pub section_chunks: i64,
    pub embeddings_stored: i64,
    pub embedding_dimension: usize,
    pub db_path: String,
    pub db_size_mb: f64,
    pub matrix_loaded: bool,
    pub matrix_rows: usize,
}

/// Options for adding a document.
#[derive(Debug, Clone, Default)]
pub struct AddDocumentOptions {
    pub metadata: Option<serde_json::Value>,
    pub content_hash: Option<String>,
    pub created_at: Option<i64>,
}
