//! Embedding engine trait and implementations.
//!
//! The `EmbedderBackend` trait abstracts over embedding generation.
//! Implementations:
//! - `OnnxEmbedder`: ONNX Runtime with all-MiniLM-L6-v2 (Phase 2, requires `ort` crate)
//! - Placeholder: Returns None to signal no embeddings available (BM25-only fallback)

use ndarray::Array1;

/// Result of an embedding operation.
pub struct EmbeddingResult {
    /// Float32 embedding vector (384-dim for all-MiniLM-L6-v2).
    pub embedding: Array1<f32>,
    /// Whether this was served from cache.
    pub cached: bool,
}

/// Trait for embedding backends.
pub trait EmbedderBackend: Send + Sync {
    /// Generate an embedding for a text string.
    /// Returns None if the embedder is not available.
    fn embed(&self, text: &str) -> Option<EmbeddingResult>;

    /// Generate embeddings for a batch of texts.
    fn embed_batch(&self, texts: &[&str]) -> Vec<Option<EmbeddingResult>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// Get the embedding dimension.
    fn dimension(&self) -> usize;

    /// Check if the embedder is available (model loaded).
    fn is_available(&self) -> bool;
}

/// Placeholder embedder that always returns None (BM25-only mode).
pub struct NoopEmbedder {
    dim: usize,
}

impl NoopEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl EmbedderBackend for NoopEmbedder {
    fn embed(&self, _text: &str) -> Option<EmbeddingResult> {
        None
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn is_available(&self) -> bool {
        false
    }
}
