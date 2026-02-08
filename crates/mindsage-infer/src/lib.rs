//! MindSage Infer — embedding engine, model management, query cache.
//!
//! Provides the `EmbedderBackend` trait for generating embeddings.
//! When the `onnx` feature is enabled and model files are present,
//! `OnnxEmbedder` loads all-MiniLM-L6-v2 for 384-dim embeddings.
//! Without it, `NoopEmbedder` is used and search falls back to BM25-only.

pub mod cache;
pub mod embedder;
pub mod onnx_embedder;

pub use cache::QueryCache;
pub use embedder::{EmbedderBackend, EmbeddingResult, NoopEmbedder};

#[cfg(feature = "onnx")]
pub use onnx_embedder::OnnxEmbedder;

use std::path::Path;
use std::sync::Arc;

/// Create the best available embedder for the given model directory.
///
/// Tries ONNX first (if feature enabled and model files present),
/// falls back to NoopEmbedder.
pub fn create_embedder(model_dir: &Path) -> Arc<dyn EmbedderBackend> {
    #[cfg(feature = "onnx")]
    {
        match OnnxEmbedder::load(model_dir) {
            Ok(embedder) => {
                tracing::info!("Using ONNX embedder (dim={})", embedder.dimension());
                return Arc::new(embedder);
            }
            Err(e) => {
                tracing::warn!("ONNX embedder unavailable: {}. Falling back to BM25-only.", e);
            }
        }
    }

    #[cfg(not(feature = "onnx"))]
    {
        let _ = model_dir;
        tracing::info!("ONNX feature disabled. Using BM25-only search.");
    }

    Arc::new(NoopEmbedder::new(384))
}
