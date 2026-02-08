//! Consolidation pipeline — dedup, prune, compress, evict.
//!
//! Maintains storage health by removing orphaned chunks, deduplicating
//! content, and evicting stale data based on tier-adaptive thresholds.

pub mod pipeline;
pub mod types;

pub use pipeline::ConsolidationPipeline;
pub use types::*;
