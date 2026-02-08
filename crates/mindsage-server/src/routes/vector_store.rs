//! Vector store routes — document CRUD, search, topics, graph.
//! Matches /api/vector-store/* endpoints from the Express server.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::state::AppState;
use mindsage_ingest::ingest::content_hash;
use mindsage_store::{AddDocumentOptions, SearchHit};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Health
        .route("/vector-store/status", get(get_status))
        .route("/vector-store/debug", get(get_debug))
        // Documents
        .route("/vector-store/documents", post(add_document).get(list_documents))
        .route("/vector-store/documents/batch", post(batch_add_documents))
        .route(
            "/vector-store/documents/{id}",
            get(get_document).delete(delete_document),
        )
        // Search
        .route("/vector-store/search", post(search))
        .route("/vector-store/search/enhanced", post(enhanced_search))
        .route("/vector-store/search/with-topic", post(search_with_topic))
        // Topics
        .route("/vector-store/topics", get(get_topics))
        .route("/vector-store/topics/{topic}/documents", get(get_documents_by_topic))
        .route(
            "/vector-store/documents/{id}/topics",
            get(get_document_topics).put(update_document_topics),
        )
        .route("/vector-store/documents/{id}/topics/generate", post(generate_topics))
        // Knowledge Graph
        .route("/vector-store/graph", post(get_graph))
        .route("/vector-store/graph/node/{node_id}", get(get_graph_node))
}

// ---------------------------------------------------------------
// Health
// ---------------------------------------------------------------

async fn get_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let stats = state.store.get_stats().ok();
    Json(serde_json::json!({
        "status": "healthy",
        "service": "mindsage-rs",
        "documents": stats.as_ref().map(|s| s.total_documents).unwrap_or(0),
        "chunks": stats.as_ref().map(|s| s.total_chunks).unwrap_or(0),
        "embeddings": stats.as_ref().map(|s| s.embeddings_stored).unwrap_or(0),
    }))
}

async fn get_debug(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let caps = mindsage_core::DeviceCapabilities::discover();
    let stats = state.store.get_stats().ok();

    Json(serde_json::json!({
        "device": {
            "tier": caps.tier,
            "totalRamBytes": caps.total_ram_bytes,
            "availableRamBytes": caps.available_ram_bytes,
            "cpuCores": caps.cpu_cores,
            "hasGpu": caps.has_gpu,
            "isJetson": caps.is_jetson,
        },
        "store": stats,
    }))
}

// ---------------------------------------------------------------
// Documents
// ---------------------------------------------------------------

#[derive(Deserialize)]
struct AddDocumentRequest {
    text: String,
    metadata: Option<serde_json::Value>,
    content_hash: Option<String>,
}

async fn add_document(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddDocumentRequest>,
) -> impl IntoResponse {
    let hash = req
        .content_hash
        .unwrap_or_else(|| content_hash(&req.text));

    match state.store.add_document(
        &req.text,
        AddDocumentOptions {
            metadata: req.metadata,
            content_hash: Some(hash.clone()),
            ..Default::default()
        },
    ) {
        Ok(doc_id) => {
            // Also create a single chunk for searchability
            // Chunk the document for searchability
            let _ = chunk_document(&state, doc_id, &req.text, None);

            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": doc_id,
                    "content_hash": hash,
                    "status": "added",
                })),
            )
        }
        Err(mindsage_core::Error::DuplicateContent(_)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "Duplicate content",
                "content_hash": hash,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Chunk a document and store chunks in the database.
fn chunk_document(
    state: &AppState,
    doc_id: i64,
    text: &str,
    file_extension: Option<&str>,
) -> Result<(), mindsage_core::Error> {
    use mindsage_ingest::chunking::{calculate_chunk_size, should_chunk, HierarchicalChunker};
    use std::collections::HashMap;

    if should_chunk(text, file_extension) {
        let (chunk_size, chunk_overlap) = calculate_chunk_size(file_extension);
        let chunker = HierarchicalChunker::new(chunk_size, chunk_overlap);
        let chunks = chunker.chunk(text);

        let mut section_db_ids: HashMap<usize, i64> = HashMap::new();

        for chunk in &chunks {
            let parent_db_id = chunk
                .parent_index
                .and_then(|pi| section_db_ids.get(&pi).copied());

            let chunk_id = state.store.add_chunk(
                doc_id,
                &chunk.text,
                chunk.chunk_index as i32,
                chunk.level,
                parent_db_id,
                Some(chunk.char_start as i32),
                Some(chunk.char_end as i32),
                None,
                None,
                None,
            )?;

            if chunk.level == 0 {
                section_db_ids.insert(chunk.chunk_index, chunk_id);
            }
        }
    } else {
        state.store.add_chunk(
            doc_id,
            text,
            0,
            1,
            None,
            Some(0),
            Some(text.len() as i32),
            None,
            None,
            None,
        )?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct BatchAddRequest {
    documents: Vec<AddDocumentRequest>,
}

async fn batch_add_documents(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchAddRequest>,
) -> Json<serde_json::Value> {
    let mut added = Vec::new();
    let mut errors = Vec::new();
    let mut duplicates = 0;

    for doc in req.documents {
        let hash = doc
            .content_hash
            .unwrap_or_else(|| content_hash(&doc.text));

        match state.store.add_document(
            &doc.text,
            AddDocumentOptions {
                metadata: doc.metadata,
                content_hash: Some(hash.clone()),
                ..Default::default()
            },
        ) {
            Ok(doc_id) => {
                let _ = chunk_document(&state, doc_id, &doc.text, None);
                added.push(serde_json::json!({ "id": doc_id, "content_hash": hash }));
            }
            Err(mindsage_core::Error::DuplicateContent(_)) => {
                duplicates += 1;
            }
            Err(e) => {
                errors.push(serde_json::json!({ "error": e.to_string(), "content_hash": hash }));
            }
        }
    }

    Json(serde_json::json!({
        "added": added.len(),
        "duplicates": duplicates,
        "errors": errors.len(),
        "results": added,
        "errorDetails": errors,
    }))
}

#[derive(Deserialize)]
struct ListDocumentsQuery {
    page: Option<usize>,
    page_size: Option<usize>,
    ascending: Option<bool>,
}

async fn list_documents(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListDocumentsQuery>,
) -> Json<serde_json::Value> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    let ascending = params.ascending.unwrap_or(false);

    match state
        .store
        .get_documents_paginated(page, page_size, ascending)
    {
        Ok((docs, total)) => Json(serde_json::json!({
            "documents": docs,
            "total": total,
            "page": page,
            "pageSize": page_size,
            "totalPages": (total as f64 / page_size as f64).ceil() as i64,
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn get_document(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match state.store.get_document(id) {
        Ok(Some(doc)) => {
            let chunks = state.store.get_chunks_for_document(id).unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "document": doc,
                    "chunks": chunks,
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Document not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match state.store.delete_document(id) {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({ "deleted": true, "id": id })),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Document not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

// ---------------------------------------------------------------
// Search
// ---------------------------------------------------------------

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_top_k")]
    top_k: usize,
}

fn default_top_k() -> usize {
    10
}

async fn search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> Json<serde_json::Value> {
    // Try hybrid search if embedder is available, else fall back to BM25
    let (results, search_type) = if state.embedder.is_available() {
        if let Some(emb_result) = state.embedder.embed(&req.query) {
            match state.store.hybrid_search(
                &req.query,
                &emb_result.embedding,
                1,
                req.top_k * 2,
                req.top_k * 2,
                60,
            ) {
                Ok(hits) => (hits, "hybrid"),
                Err(_) => match state.store.bm25_search(&req.query, 1, req.top_k * 2) {
                    Ok(hits) => (hits, "bm25"),
                    Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
                },
            }
        } else {
            match state.store.bm25_search(&req.query, 1, req.top_k * 2) {
                Ok(hits) => (hits, "bm25"),
                Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
            }
        }
    } else {
        match state.store.bm25_search(&req.query, 1, req.top_k * 2) {
            Ok(hits) => (hits, "bm25"),
            Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
        }
    };

    let boosted = apply_entity_boost(&results, &req.query);
    let deduped = dedup_by_document(boosted, req.top_k);

    let formatted: Vec<serde_json::Value> = deduped
        .iter()
        .map(|hit| {
            serde_json::json!({
                "chunk_id": hit.chunk_id,
                "doc_id": hit.doc_id,
                "text": hit.text,
                "score": hit.score,
                "metadata": hit.metadata,
            })
        })
        .collect();

    Json(serde_json::json!({
        "results": formatted,
        "total": formatted.len(),
        "query": req.query,
        "search_type": search_type,
    }))
}

#[derive(Deserialize)]
struct EnhancedSearchRequest {
    query: String,
    #[serde(default = "default_top_k")]
    top_k: usize,
    #[serde(default)]
    include_passages: Option<bool>,
}

async fn enhanced_search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnhancedSearchRequest>,
) -> Json<serde_json::Value> {
    let include_passages = req.include_passages.unwrap_or(true);

    // Try hybrid search if embedder is available
    let (results, search_type) = if state.embedder.is_available() {
        if let Some(emb_result) = state.embedder.embed(&req.query) {
            match state.store.hybrid_search(
                &req.query,
                &emb_result.embedding,
                1,
                req.top_k * 2,
                req.top_k * 2,
                60,
            ) {
                Ok(hits) => (hits, "enhanced_hybrid"),
                Err(_) => match state.store.bm25_search(&req.query, 1, req.top_k * 2) {
                    Ok(hits) => (hits, "enhanced_bm25"),
                    Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
                },
            }
        } else {
            match state.store.bm25_search(&req.query, 1, req.top_k * 2) {
                Ok(hits) => (hits, "enhanced_bm25"),
                Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
            }
        }
    } else {
        match state.store.bm25_search(&req.query, 1, req.top_k * 2) {
            Ok(hits) => (hits, "enhanced_bm25"),
            Err(e) => return Json(serde_json::json!({ "error": e.to_string() })),
        }
    };

    let boosted = apply_entity_boost(&results, &req.query);
    let deduped = dedup_by_document(boosted, req.top_k);

    let formatted: Vec<serde_json::Value> = deduped
        .iter()
        .map(|hit| {
            let mut result = serde_json::json!({
                "chunk_id": hit.chunk_id,
                "doc_id": hit.doc_id,
                "text": hit.text,
                "score": hit.score,
                "metadata": hit.metadata,
            });

            if include_passages {
                let passage = extract_passage(&hit.text, &req.query);
                result["passage"] = serde_json::json!({
                    "text": passage,
                    "method": "heuristic",
                });
            }

            // Include enriched metadata if available
            if let Some(enriched) = &hit.enriched_text {
                result["enriched_text"] = serde_json::json!(enriched);
            }

            // Add parent context if available
            if let Some(parent_id) = hit.parent_chunk_id {
                if let Ok(Some(parent)) = state.store.get_chunk(parent_id) {
                    result["parent_context"] = serde_json::json!({
                        "text": parent.text,
                        "chunk_id": parent.id,
                    });
                }
            }

            result
        })
        .collect();

    Json(serde_json::json!({
        "results": formatted,
        "total": formatted.len(),
        "query": req.query,
        "search_type": search_type,
    }))
}

/// Apply entity boost to search results: +0.15 if query entities match enriched_text.
fn apply_entity_boost(results: &[SearchHit], query: &str) -> Vec<SearchHit> {
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    results
        .iter()
        .map(|hit| {
            let mut boosted = hit.clone();
            if let Some(enriched) = &hit.enriched_text {
                let enriched_lower = enriched.to_lowercase();
                // Check if any query term appears in the enriched entities/topics
                let has_entity_match = query_terms
                    .iter()
                    .any(|term| term.len() > 2 && enriched_lower.contains(term));
                if has_entity_match {
                    boosted.score += 0.15;
                }
            }
            boosted
        })
        .collect()
}

/// Deduplicate search results by document: keep only the best-scoring chunk per parent document.
fn dedup_by_document(mut results: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit> {
    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let mut seen_docs: HashMap<i64, bool> = HashMap::new();
    let mut deduped = Vec::new();

    for hit in results {
        if seen_docs.contains_key(&hit.doc_id) {
            continue;
        }
        seen_docs.insert(hit.doc_id, true);
        deduped.push(hit);
        if deduped.len() >= top_k {
            break;
        }
    }

    deduped
}

/// Heuristic passage extraction: find a window around query term matches.
fn extract_passage(text: &str, query: &str) -> String {
    let lower_text = text.to_lowercase();
    let terms: Vec<&str> = query.split_whitespace().collect();

    // Find first matching term position
    let mut best_pos = None;
    for term in &terms {
        if let Some(pos) = lower_text.find(&term.to_lowercase()) {
            best_pos = Some(pos);
            break;
        }
    }

    let pos = best_pos.unwrap_or(0);
    let window = 200; // chars on each side
    let start = pos.saturating_sub(window);
    let end = (pos + window).min(text.len());

    // Expand to word boundaries
    let start = text[..start]
        .rfind(' ')
        .map(|p| p + 1)
        .unwrap_or(start);
    let end = text[end..]
        .find(' ')
        .map(|p| end + p)
        .unwrap_or(end);

    let mut passage = text[start..end].to_string();
    if start > 0 {
        passage = format!("...{}", passage);
    }
    if end < text.len() {
        passage = format!("{}...", passage);
    }
    passage
}

// ---------------------------------------------------------------
// Topics (Phase 1 stubs — full implementation in Phase 2/3)
// ---------------------------------------------------------------

async fn get_topics(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // Scan all document metadata for topics
    let docs = state.store.get_all_documents(false).unwrap_or_default();
    let mut topic_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for doc in &docs {
        if let Some(metadata) = &doc.metadata {
            if let Some(topics) = metadata.get("topics").and_then(|t| t.as_array()) {
                for topic in topics {
                    if let Some(t) = topic.as_str() {
                        *topic_counts.entry(t.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let topics: Vec<serde_json::Value> = topic_counts
        .into_iter()
        .map(|(topic, count)| serde_json::json!({ "topic": topic, "count": count }))
        .collect();

    Json(serde_json::json!({ "topics": topics }))
}

async fn get_documents_by_topic(
    State(state): State<Arc<AppState>>,
    Path(topic): Path<String>,
) -> Json<serde_json::Value> {
    let docs = state.store.get_all_documents(false).unwrap_or_default();
    let filtered: Vec<&mindsage_store::Document> = docs
        .iter()
        .filter(|doc| {
            doc.metadata
                .as_ref()
                .and_then(|m| m.get("topics"))
                .and_then(|t| t.as_array())
                .map(|topics| {
                    topics
                        .iter()
                        .any(|t| t.as_str() == Some(topic.as_str()))
                })
                .unwrap_or(false)
        })
        .collect();

    Json(serde_json::json!({
        "topic": topic,
        "documents": filtered,
        "total": filtered.len(),
    }))
}

async fn get_document_topics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match state.store.get_document(id) {
        Ok(Some(doc)) => {
            let topics = doc
                .metadata
                .as_ref()
                .and_then(|m| m.get("topics"))
                .cloned()
                .unwrap_or(serde_json::json!([]));
            (StatusCode::OK, Json(serde_json::json!({ "topics": topics, "doc_id": id })))
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Document not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct UpdateTopicsRequest {
    topics: Vec<String>,
}

async fn update_document_topics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateTopicsRequest>,
) -> impl IntoResponse {
    let updates = serde_json::json!({ "topics": req.topics });
    match state.store.update_document_metadata(id, &updates) {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({ "updated": true, "topics": req.topics })),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Document not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

async fn generate_topics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    // Use heuristic extraction to generate topics
    let doc = match state.store.get_document(id) {
        Ok(Some(doc)) => doc,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Document not found" })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    let source = doc
        .metadata
        .as_ref()
        .and_then(|m| m.get("source"))
        .and_then(|s| s.as_str());
    let filename = doc
        .metadata
        .as_ref()
        .and_then(|m| m.get("filename"))
        .and_then(|s| s.as_str());

    let result = mindsage_ingest::extract_all(&doc.text, source, filename);

    // Update document metadata with topics
    let updates = serde_json::json!({
        "topics": result.topics,
        "primary_topic": result.primary_topic,
        "extraction_method": "heuristic",
    });
    let _ = state.store.update_document_metadata(id, &updates);

    // Also enrich chunks
    if let Ok(chunks) = state.store.get_chunks_for_document(id) {
        for chunk in &chunks {
            if chunk.enriched_text.is_some() {
                continue;
            }
            let chunk_result = mindsage_ingest::extract_all(&chunk.text, source, filename);
            let enriched = mindsage_ingest::build_enriched_text(&chunk_result);
            if !enriched.is_empty() {
                let _ = state.store.update_chunk_enriched_text(chunk.id, &enriched);
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "doc_id": id,
            "topics": result.topics,
            "primary_topic": result.primary_topic,
            "method": "heuristic",
        })),
    )
}

#[derive(Deserialize)]
struct SearchWithTopicRequest {
    query: String,
    topic: String,
    #[serde(default = "default_top_k")]
    top_k: usize,
}

async fn search_with_topic(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchWithTopicRequest>,
) -> Json<serde_json::Value> {
    // Hybrid or BM25 search, then filter by topic
    let search_results = if state.embedder.is_available() {
        if let Some(emb_result) = state.embedder.embed(&req.query) {
            state.store.hybrid_search(
                &req.query,
                &emb_result.embedding,
                1,
                req.top_k * 3,
                req.top_k * 3,
                60,
            )
        } else {
            state.store.bm25_search(&req.query, 1, req.top_k * 3)
        }
    } else {
        state.store.bm25_search(&req.query, 1, req.top_k * 3)
    };
    match search_results {
        Ok(results) => {
            let filtered: Vec<&mindsage_store::SearchHit> = results
                .iter()
                .filter(|hit| {
                    hit.metadata
                        .as_ref()
                        .and_then(|m| m.get("topics"))
                        .and_then(|t| t.as_array())
                        .map(|topics| {
                            topics.iter().any(|t| t.as_str() == Some(req.topic.as_str()))
                        })
                        .unwrap_or(false)
                })
                .take(req.top_k)
                .collect();

            Json(serde_json::json!({
                "results": filtered,
                "total": filtered.len(),
                "query": req.query,
                "topic": req.topic,
            }))
        }
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

// ---------------------------------------------------------------
// Knowledge Graph (Phase 1 stubs)
// ---------------------------------------------------------------

async fn get_graph(State(_state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "nodes": [],
        "edges": [],
        "stats": {
            "nodeCount": 0,
            "edgeCount": 0,
        },
    }))
}

async fn get_graph_node(
    State(_state): State<Arc<AppState>>,
    Path(_node_id): Path<String>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "Graph not yet implemented" })),
    )
}
