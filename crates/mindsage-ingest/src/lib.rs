//! MindSage Ingest — text chunking, file processing, document ingestion, metadata extraction.

pub mod chunking;
pub mod extract;
pub mod file;
pub mod ingest;

pub use chunking::{HierarchicalChunk, HierarchicalChunker, TextChunk};
pub use extract::{ExtractionResult, build_enriched_text, extract_all};
pub use ingest::Ingester;
