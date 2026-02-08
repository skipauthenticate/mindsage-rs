//! Background indexing queue — processes files asynchronously.
//! Also runs heuristic extraction on newly indexed chunks.

use std::path::Path;
use std::sync::Arc;

use tracing::{debug, error, info};

use crate::state::{AppState, IndexingStatus};
use mindsage_ingest::Ingester;

/// Start the background indexing worker task.
pub fn start_indexing_worker(state: Arc<AppState>) {
    let mut rx = match state.take_indexing_rx() {
        Some(rx) => rx,
        None => {
            error!("Indexing worker already started");
            return;
        }
    };

    // Run embedding + extraction on any unprocessed chunks from prior sessions
    let catchup_state = state.clone();
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            embed_pending_chunks(&catchup_state);
            run_pending_extractions(&catchup_state);
        })
        .await
        .ok();
    });

    tokio::spawn(async move {
        info!("Background indexing worker started");
        while let Some(request) = rx.recv().await {
            process_indexing_job(&state, &request.job_id, &request.file_path, &request.filename);
        }
    });
}

fn process_indexing_job(state: &AppState, job_id: &str, file_path: &str, filename: &str) {
    let now = now_millis();

    // Update job status to processing
    {
        let mut jobs = state.indexing_jobs.write();
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = IndexingStatus::Processing;
            job.started_at = Some(now);
        }
    }

    info!("Processing indexing job {}: {}", job_id, filename);

    let path = Path::new(file_path);
    let ingester = Ingester::new(&state.store);

    match ingester.ingest_file(path) {
        Ok(Some(doc_id)) => {
            let completed_at = now_millis();
            {
                let mut jobs = state.indexing_jobs.write();
                if let Some(job) = jobs.get_mut(job_id) {
                    job.status = IndexingStatus::Completed;
                    job.document_id = Some(doc_id);
                    job.completed_at = Some(completed_at);
                }
            }
            state.mark_file_indexed(file_path, Some(doc_id));
            info!("Indexed {} → document {}", filename, doc_id);

            // Embed level=1 chunks if embedder is available
            embed_document_chunks(state, doc_id);

            // Run heuristic extraction on the new document's chunks
            run_extraction_for_document(state, doc_id);
        }
        Ok(None) => {
            let completed_at = now_millis();
            {
                let mut jobs = state.indexing_jobs.write();
                if let Some(job) = jobs.get_mut(job_id) {
                    job.status = IndexingStatus::Completed;
                    job.completed_at = Some(completed_at);
                    job.error = Some("No text extracted".to_string());
                }
            }
            info!("No text extracted from {}", filename);
        }
        Err(e) => {
            let completed_at = now_millis();
            let err_msg = e.to_string();
            {
                let mut jobs = state.indexing_jobs.write();
                if let Some(job) = jobs.get_mut(job_id) {
                    if err_msg.contains("Duplicate content") {
                        job.status = IndexingStatus::Completed;
                        job.error = Some("Duplicate content".to_string());
                    } else {
                        job.status = IndexingStatus::Failed;
                        job.error = Some(err_msg.clone());
                    }
                    job.completed_at = Some(completed_at);
                }
            }
            if err_msg.contains("Duplicate content") {
                info!("Skipped duplicate: {}", filename);
            } else {
                error!("Failed to index {}: {}", filename, err_msg);
            }
        }
    }

    // Cleanup old completed jobs (keep last 100)
    cleanup_old_jobs(state);
}

fn cleanup_old_jobs(state: &AppState) {
    let mut jobs = state.indexing_jobs.write();
    let completed: Vec<String> = jobs
        .iter()
        .filter(|(_, j)| {
            j.status == IndexingStatus::Completed || j.status == IndexingStatus::Failed
        })
        .map(|(id, _)| id.clone())
        .collect();

    if completed.len() > 100 {
        let mut to_remove: Vec<(String, i64)> = completed
            .iter()
            .filter_map(|id| {
                jobs.get(id)
                    .and_then(|j| j.completed_at)
                    .map(|t| (id.clone(), t))
            })
            .collect();
        to_remove.sort_by_key(|(_, t)| *t);
        let remove_count = to_remove.len() - 100;
        for (id, _) in to_remove.into_iter().take(remove_count) {
            jobs.remove(&id);
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ---------------------------------------------------------------
// Embedding
// ---------------------------------------------------------------

/// Embed all level=1 (paragraph) chunks for a document.
fn embed_document_chunks(state: &AppState, doc_id: i64) {
    if !state.embedder.is_available() {
        return;
    }

    let chunks = match state.store.get_chunks_for_document(doc_id) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to get chunks for embedding (doc {}): {}", doc_id, e);
            return;
        }
    };

    let paragraph_chunks: Vec<_> = chunks.iter().filter(|c| c.level == 1).collect();
    if paragraph_chunks.is_empty() {
        return;
    }

    let texts: Vec<&str> = paragraph_chunks.iter().map(|c| c.text.as_str()).collect();
    let embeddings = state.embedder.embed_batch(&texts);

    let mut embedded_count = 0;
    for (chunk, emb_result) in paragraph_chunks.iter().zip(embeddings.iter()) {
        if let Some(result) = emb_result {
            if let Err(e) = state.store.add_chunk_embedding(chunk.id, &result.embedding) {
                error!("Failed to store embedding for chunk {}: {}", chunk.id, e);
                continue;
            }
            // Also update in-memory matrix for fast vector search
            if let Err(e) = state.store.append_to_matrix(chunk.id, &result.embedding) {
                debug!("Matrix append deferred for chunk {}: {}", chunk.id, e);
            }
            embedded_count += 1;
        }
    }

    if embedded_count > 0 {
        debug!(
            "Embedded {} paragraph chunks for document {}",
            embedded_count, doc_id
        );
    }
}

/// Embed any level=1 chunks from prior sessions that don't have embeddings yet.
fn embed_pending_chunks(state: &AppState) {
    if !state.embedder.is_available() {
        return;
    }

    let batch_size = 50;
    let mut total = 0;

    loop {
        let chunks = match state.store.get_chunks_without_embedding(batch_size) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to get pending chunks for embedding: {}", e);
                break;
            }
        };

        if chunks.is_empty() {
            break;
        }

        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        let embeddings = state.embedder.embed_batch(&texts);

        for (chunk, emb_result) in chunks.iter().zip(embeddings.iter()) {
            if let Some(result) = emb_result {
                if let Err(e) = state.store.add_chunk_embedding(chunk.id, &result.embedding) {
                    error!("Failed to store embedding for chunk {}: {}", chunk.id, e);
                    continue;
                }
                let _ = state.store.append_to_matrix(chunk.id, &result.embedding);
                total += 1;
            }
        }
    }

    if total > 0 {
        info!("Embedded {} pending chunks from prior sessions", total);
    }
}

// ---------------------------------------------------------------
// Heuristic Extraction
// ---------------------------------------------------------------

/// Run heuristic extraction on all chunks of a newly indexed document.
fn run_extraction_for_document(state: &AppState, doc_id: i64) {
    let chunks = match state.store.get_chunks_for_document(doc_id) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to get chunks for extraction (doc {}): {}", doc_id, e);
            return;
        }
    };

    // Get document metadata for source info
    let doc = state.store.get_document(doc_id).ok().flatten();
    let source = doc
        .as_ref()
        .and_then(|d| d.metadata.as_ref())
        .and_then(|m| m.get("source"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let filename = doc
        .as_ref()
        .and_then(|d| d.metadata.as_ref())
        .and_then(|m| m.get("filename"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    let mut extracted_count = 0;
    let mut doc_topics: Vec<String> = Vec::new();

    for chunk in &chunks {
        if chunk.enriched_text.is_some() {
            continue; // Already extracted
        }

        let result = mindsage_ingest::extract_all(
            &chunk.text,
            source.as_deref(),
            filename.as_deref(),
        );

        let enriched = mindsage_ingest::build_enriched_text(&result);
        if !enriched.is_empty() {
            if let Err(e) = state.store.update_chunk_enriched_text(chunk.id, &enriched) {
                error!("Failed to update enriched_text for chunk {}: {}", chunk.id, e);
                continue;
            }
        }

        // Collect topics from all chunks
        for topic in &result.topics {
            if !doc_topics.contains(topic) {
                doc_topics.push(topic.clone());
            }
        }
        extracted_count += 1;
    }

    // Update document-level metadata with extracted topics and filters
    if !doc_topics.is_empty() {
        let updates = serde_json::json!({
            "topics": doc_topics,
            "extraction_method": "heuristic",
            "extracted_at": now_millis(),
        });
        let _ = state.store.update_document_metadata(doc_id, &updates);
    }

    if extracted_count > 0 {
        debug!(
            "Extracted metadata for {} chunks of document {}",
            extracted_count, doc_id
        );
    }
}

/// Process any chunks that don't have enriched_text yet (from prior sessions).
fn run_pending_extractions(state: &AppState) {
    let batch_size = 50;
    let mut total = 0;

    loop {
        let chunks = match state.store.get_chunks_without_enrichment(batch_size) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to get pending chunks for extraction: {}", e);
                break;
            }
        };

        if chunks.is_empty() {
            break;
        }

        for chunk in &chunks {
            let result = mindsage_ingest::extract_all(&chunk.text, None, None);
            let enriched = mindsage_ingest::build_enriched_text(&result);
            if !enriched.is_empty() {
                let _ = state.store.update_chunk_enriched_text(chunk.id, &enriched);
            }
            total += 1;
        }
    }

    if total > 0 {
        info!("Completed pending extraction for {} chunks", total);
    }
}
