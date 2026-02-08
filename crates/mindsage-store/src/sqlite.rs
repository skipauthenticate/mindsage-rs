//! SQLite-based hybrid search store with FTS5 + int8 vector search.
//!
//! Port of Python's `sqlite_store.py`. Same schema, same search algorithms.
//! Targets <200ms total search latency on Jetson Orin Nano.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ndarray::{Array1, Array2, Axis};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, info};

use crate::embedding::{dequantize_uint8, quantize_uint8};
use crate::schema::{FTS_SCHEMA_SQL, FTS_TRIGGERS_SQL, SCHEMA_SQL};
use crate::types::*;
use mindsage_core::{Error, Result};

/// SQLite store with FTS5 full-text search and int8 vector search.
pub struct SqliteStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    embedding_dim: usize,
    /// Pre-loaded normalized embedding matrix for vector search: (N, dim) float32.
    embedding_matrix: Mutex<EmbeddingMatrix>,
}

struct EmbeddingMatrix {
    /// Normalized embeddings, shape (N, dim).
    matrix: Array2<f32>,
    /// Chunk IDs corresponding to each row.
    chunk_ids: Vec<i64>,
    /// Whether the matrix needs reloading.
    dirty: bool,
}

impl SqliteStore {
    /// Open or create the SQLite store.
    ///
    /// `db_dir` is the directory (e.g., `data/vectordb/`). The file will be `db_dir/mindsage.db`.
    pub fn open(db_dir: impl AsRef<Path>, embedding_dim: usize) -> Result<Self> {
        let db_dir = db_dir.as_ref();
        std::fs::create_dir_all(db_dir).map_err(|e| Error::Storage(e.to_string()))?;
        let db_path = db_dir.join("mindsage.db");

        let conn = Self::create_connection(&db_path)?;
        Self::init_schema(&conn)?;

        let store = Self {
            conn: Mutex::new(conn),
            db_path,
            embedding_dim,
            embedding_matrix: Mutex::new(EmbeddingMatrix {
                matrix: Array2::zeros((0, embedding_dim)),
                chunk_ids: Vec::new(),
                dirty: true,
            }),
        };

        // Load embedding matrix
        store.load_embedding_matrix()?;

        let doc_count = store.count_documents()?;
        let chunk_count = store.count_chunks(None)?;
        info!(
            "SqliteStore initialized: {} documents, {} chunks, dim={}, path={}",
            doc_count,
            chunk_count,
            embedding_dim,
            store.db_path.display()
        );

        Ok(store)
    }

    fn create_connection(db_path: &Path) -> Result<Connection> {
        let conn = Connection::open(db_path)
            .map_err(|e| Error::Database(e.to_string()))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA cache_size = -65536;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(conn)
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        let full_schema = format!("{}\n{}\n{}", SCHEMA_SQL, FTS_SCHEMA_SQL, FTS_TRIGGERS_SQL);
        conn.execute_batch(&full_schema)
            .map_err(|e| Error::Database(format!("Schema init failed: {}", e)))?;
        Ok(())
    }

    // ---------------------------------------------------------------
    // Document CRUD
    // ---------------------------------------------------------------

    /// Insert a document. Returns the new document ID.
    pub fn add_document(&self, text: &str, opts: AddDocumentOptions) -> Result<i64> {
        let now = opts.created_at.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        });
        let meta_json = opts.metadata.as_ref().map(|m| serde_json::to_string(m).unwrap());

        let conn = self.conn.lock();
        let id = conn
            .prepare_cached(
                "INSERT INTO documents (text, metadata_json, content_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| Error::Database(e.to_string()))?
            .insert(params![text, meta_json, opts.content_hash, now])
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint") {
                    Error::DuplicateContent(opts.content_hash.unwrap_or_default())
                } else {
                    Error::Database(e.to_string())
                }
            })?;
        Ok(id)
    }

    /// Find a document by content hash.
    pub fn find_document_by_hash(&self, content_hash: &str) -> Result<Option<Document>> {
        let conn = self.conn.lock();
        let row = conn
            .prepare_cached("SELECT * FROM documents WHERE content_hash = ?1")
            .map_err(|e| Error::Database(e.to_string()))?
            .query_row(params![content_hash], |row| Ok(Self::row_to_document(row)))
            .optional()
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(row)
    }

    /// Get a document by ID.
    pub fn get_document(&self, doc_id: i64) -> Result<Option<Document>> {
        let conn = self.conn.lock();
        let row = conn
            .prepare_cached("SELECT * FROM documents WHERE id = ?1")
            .map_err(|e| Error::Database(e.to_string()))?
            .query_row(params![doc_id], |row| Ok(Self::row_to_document(row)))
            .optional()
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(row)
    }

    /// Delete a document and its chunks (cascade).
    pub fn delete_document(&self, doc_id: i64) -> Result<bool> {
        let conn = self.conn.lock();
        let count = conn
            .execute("DELETE FROM documents WHERE id = ?1", params![doc_id])
            .map_err(|e| Error::Database(e.to_string()))?;
        if count > 0 {
            drop(conn);
            self.embedding_matrix.lock().dirty = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Update (merge) metadata on a document.
    pub fn update_document_metadata(
        &self,
        doc_id: i64,
        updates: &serde_json::Value,
    ) -> Result<bool> {
        let conn = self.conn.lock();

        let existing_json: Option<String> = conn
            .prepare_cached("SELECT metadata_json FROM documents WHERE id = ?1")
            .map_err(|e| Error::Database(e.to_string()))?
            .query_row(params![doc_id], |row| row.get(0))
            .optional()
            .map_err(|e| Error::Database(e.to_string()))?
            .flatten();

        let mut existing: serde_json::Map<String, serde_json::Value> = existing_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        if let serde_json::Value::Object(map) = updates {
            for (k, v) in map {
                existing.insert(k.clone(), v.clone());
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        existing.insert(
            "metadata_updated_at".to_string(),
            serde_json::Value::Number(now.into()),
        );

        let new_json = serde_json::to_string(&existing).unwrap();
        let count = conn
            .execute(
                "UPDATE documents SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![new_json, now, doc_id],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(count > 0)
    }

    /// Count total documents.
    pub fn count_documents(&self) -> Result<i64> {
        let conn = self.conn.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(count)
    }

    /// Get documents with pagination. Returns (docs, total_count).
    pub fn get_documents_paginated(
        &self,
        page: usize,
        page_size: usize,
        ascending: bool,
    ) -> Result<(Vec<Document>, i64)> {
        let total = self.count_documents()?;
        let order = if ascending { "ASC" } else { "DESC" };
        let offset = (page.saturating_sub(1)) * page_size;

        let conn = self.conn.lock();
        let sql = format!(
            "SELECT * FROM documents ORDER BY created_at {} LIMIT ?1 OFFSET ?2",
            order
        );
        let mut stmt = conn.prepare_cached(&sql).map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![page_size as i64, offset as i64], |row| {
                Ok(Self::row_to_document(row))
            })
            .map_err(|e| Error::Database(e.to_string()))?;

        let docs: Vec<Document> = rows.filter_map(|r| r.ok()).collect();
        Ok((docs, total))
    }

    /// Get all documents.
    pub fn get_all_documents(&self, ascending: bool) -> Result<Vec<Document>> {
        let order = if ascending { "ASC" } else { "DESC" };
        let conn = self.conn.lock();
        let sql = format!("SELECT * FROM documents ORDER BY created_at {}", order);
        let mut stmt = conn.prepare(&sql).map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| Ok(Self::row_to_document(row)))
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------------------------------------------------------------
    // Chunk CRUD
    // ---------------------------------------------------------------

    /// Insert a chunk. Returns the new chunk ID.
    #[allow(clippy::too_many_arguments)]
    pub fn add_chunk(
        &self,
        doc_id: i64,
        text: &str,
        chunk_index: i32,
        level: i32,
        parent_chunk_id: Option<i64>,
        char_start: Option<i32>,
        char_end: Option<i32>,
        enriched_text: Option<&str>,
        metadata: Option<&serde_json::Value>,
        created_at: Option<i64>,
    ) -> Result<i64> {
        let now = created_at.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        });
        let meta_json = metadata.map(|m| serde_json::to_string(m).unwrap());

        let conn = self.conn.lock();
        let id = conn
            .prepare_cached(
                "INSERT INTO chunks (doc_id, parent_chunk_id, text, enriched_text, \
                 chunk_index, char_start, char_end, level, metadata_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )
            .map_err(|e| Error::Database(e.to_string()))?
            .insert(params![
                doc_id,
                parent_chunk_id,
                text,
                enriched_text,
                chunk_index,
                char_start,
                char_end,
                level,
                meta_json,
                now,
            ])
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(id)
    }

    /// Store a quantized embedding for a chunk.
    pub fn add_chunk_embedding(&self, chunk_id: i64, embedding: &Array1<f32>) -> Result<()> {
        let (q_bytes, scale, offset) = quantize_uint8(embedding);
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO chunk_embeddings (chunk_id, embedding, scale, offset_val) \
             VALUES (?1, ?2, ?3, ?4)",
            params![chunk_id, q_bytes, scale, offset],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
        drop(conn);
        self.embedding_matrix.lock().dirty = true;
        Ok(())
    }

    /// Append a single embedding to the in-memory matrix without full reload.
    pub fn append_to_matrix(&self, chunk_id: i64, embedding: &Array1<f32>) -> Result<()> {
        self.ensure_matrix_loaded()?;

        let norm = embedding.dot(embedding).sqrt();
        if norm < 1e-9 {
            return Ok(());
        }
        let normalized = embedding / norm;

        let mut mat = self.embedding_matrix.lock();
        if mat.matrix.nrows() == 0 {
            mat.matrix = normalized.insert_axis(ndarray::Axis(0)).to_owned();
        } else {
            mat.matrix
                .push(Axis(0), normalized.view())
                .map_err(|e| Error::Internal(format!("Matrix append failed: {}", e)))?;
        }
        mat.chunk_ids.push(chunk_id);
        mat.dirty = false;
        Ok(())
    }

    /// Get all chunks for a document.
    pub fn get_chunks_for_document(&self, doc_id: i64) -> Result<Vec<Chunk>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached("SELECT * FROM chunks WHERE doc_id = ?1 ORDER BY chunk_index")
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![doc_id], |row| Ok(Self::row_to_chunk(row)))
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get a chunk by ID.
    pub fn get_chunk(&self, chunk_id: i64) -> Result<Option<Chunk>> {
        let conn = self.conn.lock();
        let row = conn
            .prepare_cached("SELECT * FROM chunks WHERE id = ?1")
            .map_err(|e| Error::Database(e.to_string()))?
            .query_row(params![chunk_id], |row| Ok(Self::row_to_chunk(row)))
            .optional()
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(row)
    }

    /// Get the section-level parent of a paragraph chunk.
    pub fn get_parent_chunk(&self, chunk_id: i64) -> Result<Option<Chunk>> {
        let chunk = match self.get_chunk(chunk_id)? {
            Some(c) => c,
            None => return Ok(None),
        };
        match chunk.parent_chunk_id {
            Some(pid) => self.get_chunk(pid),
            None => Ok(None),
        }
    }

    /// Get sibling chunks (same parent) for context expansion.
    pub fn get_sibling_chunks(&self, chunk_id: i64) -> Result<Vec<Chunk>> {
        let chunk = match self.get_chunk(chunk_id)? {
            Some(c) => c,
            None => return Ok(Vec::new()),
        };
        let parent_id = match chunk.parent_chunk_id {
            Some(pid) => pid,
            None => return Ok(Vec::new()),
        };
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT * FROM chunks WHERE parent_chunk_id = ?1 ORDER BY chunk_index",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![parent_id], |row| Ok(Self::row_to_chunk(row)))
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Update enriched_text for a chunk (triggers FTS re-index via trigger).
    pub fn update_chunk_enriched_text(&self, chunk_id: i64, enriched_text: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let count = conn
            .execute(
                "UPDATE chunks SET enriched_text = ?1 WHERE id = ?2",
                params![enriched_text, chunk_id],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(count > 0)
    }

    /// Count chunks, optionally filtered by level.
    pub fn count_chunks(&self, level: Option<i32>) -> Result<i64> {
        let conn = self.conn.lock();
        let count: i64 = match level {
            Some(l) => conn
                .query_row(
                    "SELECT COUNT(*) FROM chunks WHERE level = ?1",
                    params![l],
                    |row| row.get(0),
                )
                .map_err(|e| Error::Database(e.to_string()))?,
            None => conn
                .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
                .map_err(|e| Error::Database(e.to_string()))?,
        };
        Ok(count)
    }

    /// Get chunks that haven't been enriched yet (for pending extraction).
    pub fn get_chunks_without_enrichment(&self, limit: usize) -> Result<Vec<Chunk>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT * FROM chunks WHERE enriched_text IS NULL AND level = 1 \
                 ORDER BY created_at ASC LIMIT ?1",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit as i64], |row| Ok(Self::row_to_chunk(row)))
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get level=1 chunks that have no embedding stored yet.
    pub fn get_chunks_without_embedding(&self, limit: usize) -> Result<Vec<Chunk>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT c.* FROM chunks c \
                 LEFT JOIN chunk_embeddings ce ON c.id = ce.chunk_id \
                 WHERE ce.chunk_id IS NULL AND c.level = 1 \
                 ORDER BY c.created_at ASC LIMIT ?1",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit as i64], |row| Ok(Self::row_to_chunk(row)))
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get surrounding paragraph chunks from the same document for context.
    pub fn get_surrounding_chunks(&self, chunk_id: i64, window: i32) -> Result<Vec<Chunk>> {
        let chunk = match self.get_chunk(chunk_id)? {
            Some(c) => c,
            None => return Ok(Vec::new()),
        };
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT * FROM chunks WHERE doc_id = ?1 AND level = ?2 \
                 AND chunk_index BETWEEN ?3 AND ?4 ORDER BY chunk_index",
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(
                params![
                    chunk.doc_id,
                    chunk.level,
                    chunk.chunk_index - window,
                    chunk.chunk_index + window
                ],
                |row| Ok(Self::row_to_chunk(row)),
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------------------------------------------------------------
    // BM25 Search (FTS5)
    // ---------------------------------------------------------------

    /// Full-text search using FTS5 BM25 ranking.
    pub fn bm25_search(&self, query: &str, level: i32, top_k: usize) -> Result<Vec<SearchHit>> {
        let fts_query = Self::sanitize_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock();
        let sql = "SELECT c.*, chunks_fts.rank AS bm25_score \
                   FROM chunks_fts \
                   JOIN chunks c ON c.id = chunks_fts.rowid \
                   WHERE chunks_fts MATCH ?1 \
                     AND c.level = ?2 \
                   ORDER BY chunks_fts.rank \
                   LIMIT ?3";

        let mut stmt = conn.prepare_cached(sql).map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![fts_query, level, top_k as i64], |row| {
                let bm25_score: f64 = row.get("bm25_score").unwrap_or(0.0);
                Ok(SearchHit {
                    chunk_id: row.get("id")?,
                    doc_id: row.get("doc_id")?,
                    text: row.get("text")?,
                    score: -bm25_score, // FTS5 rank is negative; negate for positive
                    level: row.get("level")?,
                    metadata: row
                        .get::<_, Option<String>>("metadata_json")?
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    enriched_text: row.get("enriched_text")?,
                    parent_chunk_id: row.get("parent_chunk_id")?,
                    chunk_index: row.get("chunk_index")?,
                    char_start: row.get("char_start")?,
                    char_end: row.get("char_end")?,
                })
            })
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Sanitize a user query for FTS5 MATCH syntax.
    /// Wraps each token in double quotes and joins with OR.
    fn sanitize_fts_query(query: &str) -> String {
        let tokens: Vec<String> = query
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect();
        if tokens.is_empty() {
            return String::new();
        }
        tokens.join(" OR ")
    }

    // ---------------------------------------------------------------
    // Vector Search
    // ---------------------------------------------------------------

    /// Load and normalize all chunk embeddings into a matrix for fast search.
    fn load_embedding_matrix(&self) -> Result<()> {
        let mut chunk_ids = Vec::new();
        let mut embeddings: Vec<Array1<f32>> = Vec::new();

        {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare(
                    "SELECT ce.chunk_id, ce.embedding, ce.scale, ce.offset_val \
                     FROM chunk_embeddings ce \
                     JOIN chunks c ON c.id = ce.chunk_id \
                     WHERE c.level = 1",
                )
                .map_err(|e| Error::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
                    let chunk_id: i64 = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let scale: f64 = row.get(2)?;
                    let offset: f64 = row.get(3)?;
                    Ok((chunk_id, blob, scale as f32, offset as f32))
                })
                .map_err(|e| Error::Database(e.to_string()))?;

            for row in rows {
                let (cid, blob, scale, offset) = row.map_err(|e| Error::Database(e.to_string()))?;
                let emb = dequantize_uint8(&blob, scale, offset);
                chunk_ids.push(cid);
                embeddings.push(emb);
            }
        } // conn and stmt dropped here

        let mut mat = self.embedding_matrix.lock();
        if embeddings.is_empty() {
            mat.matrix = Array2::zeros((0, self.embedding_dim));
            mat.chunk_ids = Vec::new();
            mat.dirty = false;
            return Ok(());
        }

        // Stack into matrix and normalize rows
        let n = embeddings.len();
        let dim = self.embedding_dim;
        let mut matrix = Array2::zeros((n, dim));
        for (i, emb) in embeddings.iter().enumerate() {
            matrix.row_mut(i).assign(emb);
        }

        // Normalize rows for cosine similarity via dot product
        for mut row in matrix.rows_mut() {
            let norm = row.dot(&row).sqrt();
            if norm > 1e-9 {
                row /= norm;
            }
        }

        mat.matrix = matrix;
        mat.chunk_ids = chunk_ids;
        mat.dirty = false;
        debug!("Loaded {} embeddings into matrix", n);
        Ok(())
    }

    fn ensure_matrix_loaded(&self) -> Result<()> {
        if self.embedding_matrix.lock().dirty {
            self.load_embedding_matrix()?;
        }
        Ok(())
    }

    /// Cosine similarity search using pre-loaded normalized matrix.
    pub fn vector_search(
        &self,
        query_embedding: &Array1<f32>,
        _level: i32,
        top_k: usize,
    ) -> Result<Vec<SearchHit>> {
        self.ensure_matrix_loaded()?;

        let mat = self.embedding_matrix.lock();
        if mat.matrix.nrows() == 0 {
            return Ok(Vec::new());
        }

        // Normalize query
        let q_norm = query_embedding.dot(query_embedding).sqrt();
        if q_norm < 1e-9 {
            return Ok(Vec::new());
        }
        let q = query_embedding / q_norm;

        // Matrix multiply: (N, dim) @ (dim,) → (N,)
        let similarities = mat.matrix.dot(&q);

        // Get top-k indices
        let k = top_k.min(similarities.len());
        let mut indexed: Vec<(usize, f32)> = similarities
            .iter()
            .enumerate()
            .map(|(i, &s)| (i, s))
            .collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        indexed.truncate(k);

        let top_chunk_ids: Vec<(i64, f64)> = indexed
            .iter()
            .map(|&(i, s)| (mat.chunk_ids[i], s as f64))
            .collect();
        drop(mat);

        // Fetch chunk data for top hits
        let mut results = Vec::with_capacity(k);
        for (cid, score) in top_chunk_ids {
            if let Some(chunk) = self.get_chunk(cid)? {
                results.push(SearchHit {
                    chunk_id: chunk.id,
                    doc_id: chunk.doc_id,
                    text: chunk.text,
                    score,
                    level: chunk.level,
                    metadata: chunk.metadata,
                    enriched_text: chunk.enriched_text,
                    parent_chunk_id: chunk.parent_chunk_id,
                    chunk_index: chunk.chunk_index,
                    char_start: chunk.char_start,
                    char_end: chunk.char_end,
                });
            }
        }
        Ok(results)
    }

    // ---------------------------------------------------------------
    // Reciprocal Rank Fusion
    // ---------------------------------------------------------------

    /// Fuse BM25 and vector search results using Reciprocal Rank Fusion.
    /// RRF score = sum(1 / (k + rank)) across result lists.
    pub fn reciprocal_rank_fusion(
        bm25_results: &[SearchHit],
        vector_results: &[SearchHit],
        k: usize,
    ) -> Vec<SearchHit> {
        let mut rrf_scores: HashMap<i64, f64> = HashMap::new();
        let mut chunk_map: HashMap<i64, &SearchHit> = HashMap::new();

        for (rank, hit) in bm25_results.iter().enumerate() {
            *rrf_scores.entry(hit.chunk_id).or_insert(0.0) +=
                1.0 / (k as f64 + rank as f64 + 1.0);
            chunk_map.entry(hit.chunk_id).or_insert(hit);
        }

        for (rank, hit) in vector_results.iter().enumerate() {
            *rrf_scores.entry(hit.chunk_id).or_insert(0.0) +=
                1.0 / (k as f64 + rank as f64 + 1.0);
            chunk_map.entry(hit.chunk_id).or_insert(hit);
        }

        let mut sorted: Vec<(i64, f64)> = rrf_scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        sorted
            .into_iter()
            .filter_map(|(cid, score)| {
                chunk_map.get(&cid).map(|hit| SearchHit {
                    chunk_id: hit.chunk_id,
                    doc_id: hit.doc_id,
                    text: hit.text.clone(),
                    score,
                    level: hit.level,
                    metadata: hit.metadata.clone(),
                    enriched_text: hit.enriched_text.clone(),
                    parent_chunk_id: hit.parent_chunk_id,
                    chunk_index: hit.chunk_index,
                    char_start: hit.char_start,
                    char_end: hit.char_end,
                })
            })
            .collect()
    }

    // ---------------------------------------------------------------
    // Hybrid Search (BM25 + Vector → RRF)
    // ---------------------------------------------------------------

    /// Combined BM25 + vector search with RRF fusion.
    pub fn hybrid_search(
        &self,
        query: &str,
        query_embedding: &Array1<f32>,
        level: i32,
        bm25_top_k: usize,
        vector_top_k: usize,
        rrf_k: usize,
    ) -> Result<Vec<SearchHit>> {
        let bm25_hits = self.bm25_search(query, level, bm25_top_k)?;
        let vector_hits = self.vector_search(query_embedding, level, vector_top_k)?;
        Ok(Self::reciprocal_rank_fusion(&bm25_hits, &vector_hits, rrf_k))
    }

    // ---------------------------------------------------------------
    // Context Expansion
    // ---------------------------------------------------------------

    /// Get the section-level parent text for context around a chunk.
    pub fn expand_to_parent_context(&self, chunk_id: i64) -> Result<Option<String>> {
        Ok(self.get_parent_chunk(chunk_id)?.map(|c| c.text))
    }

    // ---------------------------------------------------------------
    // Stats
    // ---------------------------------------------------------------

    /// Get store statistics.
    pub fn get_stats(&self) -> Result<StoreStats> {
        let doc_count = self.count_documents()?;
        let chunk_count = self.count_chunks(None)?;
        let para_count = self.count_chunks(Some(1))?;
        let section_count = self.count_chunks(Some(0))?;

        let conn = self.conn.lock();
        let emb_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunk_embeddings", [], |row| {
                row.get(0)
            })
            .map_err(|e| Error::Database(e.to_string()))?;
        drop(conn);

        let db_size = std::fs::metadata(&self.db_path)
            .map(|m| m.len())
            .unwrap_or(0);

        let mat = self.embedding_matrix.lock();
        let matrix_rows = mat.matrix.nrows();
        let matrix_loaded = matrix_rows > 0;

        Ok(StoreStats {
            total_documents: doc_count,
            total_chunks: chunk_count,
            paragraph_chunks: para_count,
            section_chunks: section_count,
            embeddings_stored: emb_count,
            embedding_dimension: self.embedding_dim,
            db_path: self.db_path.to_string_lossy().to_string(),
            db_size_mb: db_size as f64 / (1024.0 * 1024.0),
            matrix_loaded,
            matrix_rows,
        })
    }

    // ---------------------------------------------------------------
    // Row Mapping Helpers
    // ---------------------------------------------------------------

    fn row_to_document(row: &rusqlite::Row<'_>) -> Document {
        Document {
            id: row.get("id").unwrap_or(0),
            text: row.get("text").unwrap_or_default(),
            metadata: row
                .get::<_, Option<String>>("metadata_json")
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok()),
            content_hash: row.get("content_hash").ok().flatten(),
            created_at: row.get("created_at").unwrap_or(0),
            updated_at: row.get("updated_at").ok().flatten(),
        }
    }

    fn row_to_chunk(row: &rusqlite::Row<'_>) -> Chunk {
        Chunk {
            id: row.get("id").unwrap_or(0),
            doc_id: row.get("doc_id").unwrap_or(0),
            parent_chunk_id: row.get("parent_chunk_id").ok().flatten(),
            text: row.get("text").unwrap_or_default(),
            enriched_text: row.get("enriched_text").ok().flatten(),
            chunk_index: row.get("chunk_index").unwrap_or(0),
            char_start: row.get("char_start").ok().flatten(),
            char_end: row.get("char_end").ok().flatten(),
            level: row.get("level").unwrap_or(0),
            metadata: row
                .get::<_, Option<String>>("metadata_json")
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok()),
            created_at: row.get("created_at").unwrap_or(0),
        }
    }

    // ---------------------------------------------------------------
    // Consolidation Operations
    // ---------------------------------------------------------------

    /// Remove chunks whose parent document no longer exists.
    pub fn prune_orphan_chunks(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let count = conn
            .execute(
                "DELETE FROM chunks WHERE doc_id NOT IN (SELECT id FROM documents)",
                [],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        // Also clean up FTS for orphaned chunks
        conn.execute(
            "DELETE FROM chunks_fts WHERE rowid NOT IN (SELECT id FROM chunks)",
            [],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
        // Clean up orphaned embeddings
        conn.execute(
            "DELETE FROM chunk_embeddings WHERE chunk_id NOT IN (SELECT id FROM chunks)",
            [],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(count)
    }

    /// Remove documents with duplicate content_hash, keeping the newest.
    pub fn remove_duplicate_documents(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let count = conn
            .execute(
                "DELETE FROM documents WHERE id NOT IN (
                    SELECT MAX(id) FROM documents
                    WHERE content_hash IS NOT NULL
                    GROUP BY content_hash
                ) AND content_hash IS NOT NULL
                AND content_hash IN (
                    SELECT content_hash FROM documents
                    WHERE content_hash IS NOT NULL
                    GROUP BY content_hash
                    HAVING COUNT(*) > 1
                )",
                [],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        if count > 0 {
            drop(conn);
            // Cascade: prune orphaned chunks
            self.prune_orphan_chunks()?;
        }
        Ok(count)
    }

    /// Evict the oldest N documents by created_at timestamp.
    pub fn evict_oldest_documents(&self, count: usize) -> Result<usize> {
        let conn = self.conn.lock();
        let deleted = conn
            .execute(
                "DELETE FROM documents WHERE id IN (
                    SELECT id FROM documents ORDER BY created_at ASC LIMIT ?1
                )",
                [count],
            )
            .map_err(|e| Error::Database(e.to_string()))?;
        if deleted > 0 {
            drop(conn);
            self.prune_orphan_chunks()?;
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_store() -> (SqliteStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = SqliteStore::open(dir.path(), 384).unwrap();
        (store, dir)
    }

    #[test]
    fn test_add_and_get_document() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document(
                "Hello world, this is a test document.",
                AddDocumentOptions {
                    content_hash: Some("hash123".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let doc = store.get_document(doc_id).unwrap().unwrap();
        assert_eq!(doc.text, "Hello world, this is a test document.");
        assert_eq!(doc.content_hash.as_deref(), Some("hash123"));
    }

    #[test]
    fn test_duplicate_content_hash() {
        let (store, _dir) = test_store();

        store
            .add_document(
                "First doc",
                AddDocumentOptions {
                    content_hash: Some("dup_hash".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let result = store.add_document(
            "Second doc",
            AddDocumentOptions {
                content_hash: Some("dup_hash".into()),
                ..Default::default()
            },
        );
        assert!(matches!(result, Err(Error::DuplicateContent(_))));
    }

    #[test]
    fn test_add_chunk_and_bm25_search() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document("Rust programming guide", Default::default())
            .unwrap();

        // Add a searchable paragraph chunk (level=1)
        store
            .add_chunk(
                doc_id,
                "Rust is a systems programming language focused on safety and performance",
                0,
                1,
                None,
                Some(0),
                Some(72),
                None,
                None,
                None,
            )
            .unwrap();

        store
            .add_chunk(
                doc_id,
                "Python is great for data science and machine learning applications",
                1,
                1,
                None,
                Some(72),
                Some(137),
                None,
                None,
                None,
            )
            .unwrap();

        // Search for "rust programming"
        let results = store.bm25_search("rust programming", 1, 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].text.contains("Rust"));
    }

    #[test]
    fn test_enriched_text_search() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document("Technical document", Default::default())
            .unwrap();

        let chunk_id = store
            .add_chunk(
                doc_id,
                "We deployed the new microservice to production on Friday",
                0,
                1,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        // Add enriched text (simulating extraction)
        store
            .update_chunk_enriched_text(
                chunk_id,
                "topics: work technology | entities: microservice | activities: deployed",
            )
            .unwrap();

        // Search for "microservice" should find it via enriched text
        let results = store.bm25_search("microservice", 1, 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].enriched_text.is_some());
    }

    #[test]
    fn test_delete_document_cascades() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document("To be deleted", Default::default())
            .unwrap();

        store
            .add_chunk(doc_id, "Chunk text", 0, 1, None, None, None, None, None, None)
            .unwrap();

        assert_eq!(store.count_chunks(None).unwrap(), 1);

        store.delete_document(doc_id).unwrap();

        assert!(store.get_document(doc_id).unwrap().is_none());
        assert_eq!(store.count_chunks(None).unwrap(), 0);
    }

    #[test]
    fn test_document_metadata_update() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document(
                "Test doc",
                AddDocumentOptions {
                    metadata: Some(serde_json::json!({"source": "test"})),
                    ..Default::default()
                },
            )
            .unwrap();

        let updates = serde_json::json!({"topics": ["programming", "rust"]});
        store.update_document_metadata(doc_id, &updates).unwrap();

        let doc = store.get_document(doc_id).unwrap().unwrap();
        let meta = doc.metadata.unwrap();
        assert_eq!(meta["source"], "test");
        assert_eq!(meta["topics"][0], "programming");
    }

    #[test]
    fn test_pagination() {
        let (store, _dir) = test_store();

        for i in 0..5 {
            store
                .add_document(
                    &format!("Document number {}", i),
                    AddDocumentOptions {
                        content_hash: Some(format!("hash_{}", i)),
                        ..Default::default()
                    },
                )
                .unwrap();
        }

        let (docs, total) = store.get_documents_paginated(1, 2, false).unwrap();
        assert_eq!(total, 5);
        assert_eq!(docs.len(), 2);

        let (docs2, _) = store.get_documents_paginated(2, 2, false).unwrap();
        assert_eq!(docs2.len(), 2);
    }

    #[test]
    fn test_get_chunks_without_enrichment() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document("Test", Default::default())
            .unwrap();

        // Chunk without enrichment
        let c1 = store
            .add_chunk(doc_id, "Unenriched chunk", 0, 1, None, None, None, None, None, None)
            .unwrap();

        // Chunk with enrichment
        store
            .add_chunk(
                doc_id,
                "Enriched chunk",
                1,
                1,
                None,
                None,
                None,
                Some("topics: test"),
                None,
                None,
            )
            .unwrap();

        let pending = store.get_chunks_without_enrichment(10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, c1);
    }

    #[test]
    fn test_stats() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document("Stats test", Default::default())
            .unwrap();

        // Section chunk
        store
            .add_chunk(doc_id, "Section", 0, 0, None, None, None, None, None, None)
            .unwrap();

        // Paragraph chunk
        store
            .add_chunk(doc_id, "Paragraph", 0, 1, None, None, None, None, None, None)
            .unwrap();

        let stats = store.get_stats().unwrap();
        assert_eq!(stats.total_documents, 1);
        assert_eq!(stats.total_chunks, 2);
        assert_eq!(stats.section_chunks, 1);
        assert_eq!(stats.paragraph_chunks, 1);
        assert_eq!(stats.embedding_dimension, 384);
    }

    #[test]
    fn test_vector_search_with_embeddings() {
        let (store, _dir) = test_store();

        let doc_id = store
            .add_document("Vector test", Default::default())
            .unwrap();

        let c1 = store
            .add_chunk(doc_id, "Chunk one about Rust", 0, 1, None, None, None, None, None, None)
            .unwrap();
        let c2 = store
            .add_chunk(doc_id, "Chunk two about Python", 1, 1, None, None, None, None, None, None)
            .unwrap();

        // Create simple test embeddings (384-dim)
        let mut emb1 = Array1::zeros(384);
        emb1[0] = 1.0;
        emb1[1] = 0.5;

        let mut emb2 = Array1::zeros(384);
        emb2[0] = 0.1;
        emb2[2] = 1.0;

        store.add_chunk_embedding(c1, &emb1).unwrap();
        store.add_chunk_embedding(c2, &emb2).unwrap();

        // Reload matrix
        store.append_to_matrix(c1, &emb1).unwrap();
        store.append_to_matrix(c2, &emb2).unwrap();

        // Query similar to emb1
        let mut query = Array1::zeros(384);
        query[0] = 1.0;
        query[1] = 0.3;

        let results = store.vector_search(&query, 1, 5).unwrap();
        assert!(!results.is_empty());
        // First result should be chunk 1 (more similar to query)
        assert_eq!(results[0].chunk_id, c1);
    }
}
