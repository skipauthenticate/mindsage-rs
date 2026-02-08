//! Heuristic metadata extraction — replaces TinyLlama LLM.
//!
//! Extracts topics, entities, passages, and document filters using
//! keyword matching, stemming, regex patterns, and scoring heuristics.
//! This eliminates the need for a 2GB LLM on GPU, freeing memory for
//! embeddings and reranker.

pub mod entities;
pub mod filters;
pub mod passages;
pub mod stemmer;
pub mod topics;

use serde::{Deserialize, Serialize};

/// Combined extraction result for a document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Topic classifications (e.g., ["programming", "work"]).
    pub topics: Vec<String>,
    /// Primary topic.
    pub primary_topic: String,
    /// Key entities found in text.
    pub key_entities: Vec<String>,
    /// Key passage sentences.
    pub key_passages: Vec<String>,
    /// Structured metadata (persons, orgs, dates, tech, etc.).
    pub structured_metadata: StructuredMetadata,
    /// Document content type and domain filters.
    pub document_filters: DocumentFilters,
}

/// Structured metadata extracted from text.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StructuredMetadata {
    pub persons: Vec<String>,
    pub organizations: Vec<String>,
    pub locations: Vec<String>,
    pub dates: Vec<String>,
    pub times: Vec<String>,
    pub temporal_refs: Vec<String>,
    pub quantities: Vec<String>,
    pub activities: Vec<String>,
    pub technologies: Vec<String>,
}

/// Document-level content classification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentFilters {
    pub content_type: String,
    pub domain: String,
}

/// Run all heuristic extractions on a text.
pub fn extract_all(
    text: &str,
    source: Option<&str>,
    filename: Option<&str>,
) -> ExtractionResult {
    let topic_result = topics::classify_by_keywords(text);
    let key_passages = passages::extract_key_sentences(text, 3);
    let key_entities = entities::extract_entities(text, 10);
    let structured = entities::extract_structured_metadata(text, 5);
    let doc_filters = filters::generate_filters(text, source, filename);

    ExtractionResult {
        topics: topic_result.topics,
        primary_topic: topic_result.primary_topic,
        key_entities,
        key_passages,
        structured_metadata: structured,
        document_filters: doc_filters,
    }
}

/// Build enriched_text string for FTS indexing from extraction results.
///
/// Format: `"topics: a b | entities: x y | passages: ... | persons: ... | technologies: ..."`
pub fn build_enriched_text(result: &ExtractionResult) -> String {
    let mut parts = Vec::new();

    if !result.topics.is_empty() {
        parts.push(format!("topics: {}", result.topics.join(" ")));
    }
    if !result.key_entities.is_empty() {
        parts.push(format!("entities: {}", result.key_entities.join(" ")));
    }
    if !result.key_passages.is_empty() {
        let joined: String = result.key_passages.join(" ");
        let truncated = if joined.len() > 500 { &joined[..500] } else { &joined };
        parts.push(format!("passages: {}", truncated));
    }

    let sm = &result.structured_metadata;
    if !sm.persons.is_empty() {
        parts.push(format!("persons: {}", sm.persons.join(" ")));
    }
    if !sm.organizations.is_empty() {
        parts.push(format!("organizations: {}", sm.organizations.join(" ")));
    }
    if !sm.locations.is_empty() {
        parts.push(format!("locations: {}", sm.locations.join(" ")));
    }
    if !sm.technologies.is_empty() {
        parts.push(format!("technologies: {}", sm.technologies.join(" ")));
    }

    parts.join(" | ")
}
