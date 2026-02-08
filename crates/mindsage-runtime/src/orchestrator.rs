//! Orchestrator — coordinates SDK verbs with resource budgets.

use std::sync::Arc;

use mindsage_consolidate::ConsolidationPipeline;
use mindsage_core::{CapabilityTier, DeviceCapabilities};
use mindsage_infer::EmbedderBackend;
use mindsage_ingest::Ingester;
use mindsage_resolve::HybridResolver;
use mindsage_store::SqliteStore;
use tracing::{debug, error, info};

use crate::types::*;

/// Top-level orchestrator that coordinates all SDK verbs.
pub struct Orchestrator {
    tier: CapabilityTier,
    budget: ResourceBudget,
}

impl Orchestrator {
    /// Create a new orchestrator, detecting device capabilities.
    pub fn new() -> Self {
        let caps = DeviceCapabilities::discover();
        let tier = caps.tier;
        let budget = ResourceBudget::for_tier(tier);

        info!(
            "Orchestrator initialized: tier={:?}, memory_budget={}MB",
            tier, budget.max_memory_mb
        );

        Self { tier, budget }
    }

    /// Create with explicit tier (for testing).
    pub fn with_tier(tier: CapabilityTier) -> Self {
        let budget = ResourceBudget::for_tier(tier);
        Self { tier, budget }
    }

    /// Get current capability tier.
    pub fn tier(&self) -> CapabilityTier {
        self.tier
    }

    /// Get resource budget.
    pub fn budget(&self) -> &ResourceBudget {
        &self.budget
    }

    /// SDK verb: ingest — text → chunk → embed → store.
    ///
    /// Returns the document ID if successful.
    pub fn ingest(
        &self,
        store: &SqliteStore,
        embedder: &Arc<dyn EmbedderBackend>,
        text: &str,
        content_hash: &str,
        metadata: &serde_json::Value,
        file_extension: Option<&str>,
    ) -> mindsage_core::Result<Option<i64>> {
        let ingester = Ingester::new(store);
        let doc_id = ingester.ingest_text(text, content_hash, metadata, file_extension)?;

        // Embed level=1 chunks
        if let Some(doc_id) = doc_id {
            if embedder.is_available() {
                let chunks = store.get_chunks_for_document(doc_id)?;
                let paragraphs: Vec<_> = chunks.iter().filter(|c| c.level == 1).collect();
                if !paragraphs.is_empty() {
                    let texts: Vec<&str> = paragraphs.iter().map(|c| c.text.as_str()).collect();
                    let embeddings = embedder.embed_batch(&texts);
                    let mut count = 0;
                    for (chunk, emb) in paragraphs.iter().zip(embeddings.iter()) {
                        if let Some(result) = emb {
                            let _ = store.add_chunk_embedding(chunk.id, &result.embedding);
                            let _ = store.append_to_matrix(chunk.id, &result.embedding);
                            count += 1;
                        }
                    }
                    debug!("Embedded {} chunks for document {}", count, doc_id);
                }
            }

            // Run heuristic extraction
            let chunks = store.get_chunks_for_document(doc_id)?;
            let mut doc_topics: Vec<String> = Vec::new();
            for chunk in &chunks {
                if chunk.enriched_text.is_some() {
                    continue;
                }
                let source = metadata.get("source").and_then(|s| s.as_str());
                let filename = metadata.get("filename").and_then(|s| s.as_str());
                let result = mindsage_ingest::extract_all(&chunk.text, source, filename);
                let enriched = mindsage_ingest::build_enriched_text(&result);
                if !enriched.is_empty() {
                    let _ = store.update_chunk_enriched_text(chunk.id, &enriched);
                }
                for topic in &result.topics {
                    if !doc_topics.contains(topic) {
                        doc_topics.push(topic.clone());
                    }
                }
            }
            if !doc_topics.is_empty() {
                let updates = serde_json::json!({
                    "topics": doc_topics,
                    "extraction_method": "heuristic",
                });
                let _ = store.update_document_metadata(doc_id, &updates);
            }
        }

        Ok(doc_id)
    }

    /// SDK verb: distill — run extraction on all pending chunks.
    ///
    /// Processes chunks that haven't been enriched or embedded yet.
    /// Returns (enriched_count, embedded_count).
    pub fn distill(
        &self,
        store: &SqliteStore,
        embedder: &Arc<dyn EmbedderBackend>,
    ) -> (usize, usize) {
        let batch_size = 50;
        let mut enriched_total = 0;
        let mut embedded_total = 0;

        // Embed unembedded chunks
        if embedder.is_available() {
            loop {
                let chunks = match store.get_chunks_without_embedding(batch_size) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to get chunks for embedding: {}", e);
                        break;
                    }
                };
                if chunks.is_empty() {
                    break;
                }
                let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
                let embeddings = embedder.embed_batch(&texts);
                for (chunk, emb) in chunks.iter().zip(embeddings.iter()) {
                    if let Some(result) = emb {
                        let _ = store.add_chunk_embedding(chunk.id, &result.embedding);
                        let _ = store.append_to_matrix(chunk.id, &result.embedding);
                        embedded_total += 1;
                    }
                }
            }
        }

        // Enrich unenriched chunks
        loop {
            let chunks = match store.get_chunks_without_enrichment(batch_size) {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to get chunks for extraction: {}", e);
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
                    let _ = store.update_chunk_enriched_text(chunk.id, &enriched);
                }
                enriched_total += 1;
            }
        }

        if enriched_total > 0 || embedded_total > 0 {
            info!(
                "Distill complete: {} enriched, {} embedded",
                enriched_total, embedded_total
            );
        }

        (enriched_total, embedded_total)
    }

    /// SDK verb: recall — query with tier-aware resolver selection.
    pub fn recall(
        &self,
        store: &SqliteStore,
        query: mindsage_resolve::ResolveQuery,
    ) -> mindsage_resolve::ResolveResult {
        HybridResolver::resolve(store, &query, self.tier)
    }

    /// SDK verb: consolidate — run maintenance pipeline.
    pub fn consolidate(
        &self,
        store: &SqliteStore,
    ) -> mindsage_consolidate::ConsolidationReport {
        ConsolidationPipeline::run(store, self.tier)
    }

    /// Get runtime status.
    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            tier: self.tier,
            budget: self.budget.clone(),
            active_verbs: Vec::new(),
            pending_distill: 0,
        }
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mindsage_store::AddDocumentOptions;

    fn test_store() -> (SqliteStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path(), 384).unwrap();
        (store, dir)
    }

    #[test]
    fn test_with_tier() {
        let orch = Orchestrator::with_tier(CapabilityTier::Enhanced);
        assert_eq!(orch.tier(), CapabilityTier::Enhanced);
        assert_eq!(orch.budget().max_memory_mb, 512);
    }

    #[test]
    fn test_resource_budgets() {
        let base = ResourceBudget::for_tier(CapabilityTier::Base);
        assert_eq!(base.max_memory_mb, 256);
        assert_eq!(base.max_concurrency, 1);

        let full = ResourceBudget::for_tier(CapabilityTier::Full);
        assert_eq!(full.max_memory_mb, 2048);
        assert_eq!(full.max_concurrency, 8);
    }

    #[test]
    fn test_recall() {
        let (store, _dir) = test_store();
        let text = "Tokio is an async runtime for Rust";
        let doc_id = store
            .add_document(text, AddDocumentOptions::default())
            .unwrap();
        store
            .add_chunk(doc_id, text, 0, 1, None, Some(0), Some(text.len() as i32), None, None, None)
            .unwrap();

        let orch = Orchestrator::with_tier(CapabilityTier::Base);
        let result = orch.recall(
            &store,
            mindsage_resolve::ResolveQuery {
                query: "Tokio async".into(),
                resolver: None,
                limit: 5,
                filters: None,
            },
        );
        assert!(result.total_found > 0);
        assert!(result.items[0].text.contains("Tokio"));
    }

    #[test]
    fn test_consolidate() {
        let (store, _dir) = test_store();
        let orch = Orchestrator::with_tier(CapabilityTier::Base);
        let report = orch.consolidate(&store);
        assert_eq!(report.orphans_pruned, 0);
        assert_eq!(report.duplicates_removed, 0);
    }

    #[test]
    fn test_status() {
        let orch = Orchestrator::with_tier(CapabilityTier::Advanced);
        let status = orch.status();
        assert_eq!(status.tier, CapabilityTier::Advanced);
        assert_eq!(status.budget.max_memory_mb, 1024);
        assert!(status.active_verbs.is_empty());
    }

    #[test]
    fn test_ingest() {
        let (store, _dir) = test_store();
        let orch = Orchestrator::with_tier(CapabilityTier::Base);
        let embedder: Arc<dyn EmbedderBackend> =
            Arc::new(mindsage_infer::NoopEmbedder::new(384));

        let text = "Machine learning is transforming how we build software applications.";
        let hash = "abc123";
        let metadata = serde_json::json!({"source": "test"});

        let doc_id = orch
            .ingest(&store, &embedder, text, hash, &metadata, None)
            .unwrap()
            .unwrap();
        assert!(doc_id > 0);

        // Document should exist
        let doc = store.get_document(doc_id).unwrap().unwrap();
        assert!(doc.text.contains("Machine learning"));

        // Chunks should exist
        let chunks = store.get_chunks_for_document(doc_id).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_ingest_duplicate() {
        let (store, _dir) = test_store();
        let orch = Orchestrator::with_tier(CapabilityTier::Base);
        let embedder: Arc<dyn EmbedderBackend> =
            Arc::new(mindsage_infer::NoopEmbedder::new(384));

        let text = "Duplicate content test";
        let hash = "dupe_hash";
        let metadata = serde_json::json!({});

        // First ingest succeeds
        let result = orch.ingest(&store, &embedder, text, hash, &metadata, None);
        assert!(result.is_ok());

        // Second ingest with same hash fails
        let result = orch.ingest(&store, &embedder, text, hash, &metadata, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_distill() {
        let (store, _dir) = test_store();
        let orch = Orchestrator::with_tier(CapabilityTier::Base);
        let embedder: Arc<dyn EmbedderBackend> =
            Arc::new(mindsage_infer::NoopEmbedder::new(384));

        // Add a document with an unenriched chunk
        let doc_id = store
            .add_document("Test doc", AddDocumentOptions::default())
            .unwrap();
        store
            .add_chunk(doc_id, "Python is a programming language used for data science and machine learning", 0, 1, None, Some(0), Some(77), None, None, None)
            .unwrap();

        // Distill should enrich the chunk
        let (enriched, embedded) = orch.distill(&store, &embedder);
        assert!(enriched > 0);
        assert_eq!(embedded, 0); // NoopEmbedder returns None
    }
}
