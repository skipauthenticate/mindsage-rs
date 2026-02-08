//! Document ingestion pipeline: file → text → chunk → store.

use std::path::Path;

use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::chunking::{calculate_chunk_size, should_chunk, HierarchicalChunker};
use crate::file;
use mindsage_core::{Error, Result};
use mindsage_store::{AddDocumentOptions, SqliteStore};

/// Handles document ingestion: text extraction, chunking, and storage.
pub struct Ingester<'a> {
    store: &'a SqliteStore,
}

impl<'a> Ingester<'a> {
    pub fn new(store: &'a SqliteStore) -> Self {
        Self { store }
    }

    /// Ingest a file: extract text, chunk, and store.
    /// Returns the document ID if successful.
    pub fn ingest_file(&self, path: &Path) -> Result<Option<i64>> {
        let text = match file::extract_text(path)? {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                debug!("No text extracted from {}", path.display());
                return Ok(None);
            }
        };

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Content hash for dedup
        let content_hash = content_hash(&text);

        // Check for duplicate
        if self.store.find_document_by_hash(&content_hash)?.is_some() {
            debug!("Duplicate content, skipping: {}", filename);
            return Err(Error::DuplicateContent(content_hash));
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e));
        let ext_ref = ext.as_deref();

        // Build metadata
        let metadata = serde_json::json!({
            "source": "file",
            "filename": filename,
            "file_extension": ext_ref.unwrap_or(""),
            "file_size": std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        });

        self.ingest_text(&text, &content_hash, &metadata, ext_ref)
    }

    /// Ingest raw text with metadata.
    pub fn ingest_text(
        &self,
        text: &str,
        content_hash: &str,
        metadata: &serde_json::Value,
        file_extension: Option<&str>,
    ) -> Result<Option<i64>> {
        // Check for duplicate
        if self.store.find_document_by_hash(content_hash)?.is_some() {
            return Err(Error::DuplicateContent(content_hash.to_string()));
        }

        // Store document
        let doc_id = self.store.add_document(
            text,
            AddDocumentOptions {
                metadata: Some(metadata.clone()),
                content_hash: Some(content_hash.to_string()),
                ..Default::default()
            },
        )?;

        // Chunk the document
        if should_chunk(text, file_extension) {
            let (chunk_size, chunk_overlap) = calculate_chunk_size(file_extension);
            let chunker = HierarchicalChunker::new(chunk_size, chunk_overlap);
            let chunks = chunker.chunk(text);

            let mut section_db_ids: std::collections::HashMap<usize, i64> =
                std::collections::HashMap::new();

            for chunk in &chunks {
                let parent_db_id = chunk
                    .parent_index
                    .and_then(|pi| section_db_ids.get(&pi).copied());

                let chunk_id = self.store.add_chunk(
                    doc_id,
                    &chunk.text,
                    chunk.chunk_index as i32,
                    chunk.level,
                    parent_db_id,
                    Some(chunk.char_start as i32),
                    Some(chunk.char_end as i32),
                    None, // enriched_text added later by extraction
                    None, // chunk metadata
                    None, // created_at
                )?;

                if chunk.level == 0 {
                    section_db_ids.insert(chunk.chunk_index, chunk_id);
                }
            }

            let para_count = chunks.iter().filter(|c| c.level == 1).count();
            info!(
                "Ingested document {} with {} chunks ({} paragraphs)",
                doc_id,
                chunks.len(),
                para_count
            );
        } else {
            // Small text — store as a single level=1 chunk
            self.store.add_chunk(
                doc_id,
                text,
                0,     // chunk_index
                1,     // level (paragraph, searchable)
                None,  // parent_chunk_id
                Some(0),
                Some(text.len() as i32),
                None, // enriched_text
                None, // metadata
                None, // created_at
            )?;
            info!("Ingested document {} as single chunk", doc_id);
        }

        Ok(Some(doc_id))
    }
}

/// Compute SHA-256 content hash.
pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}
