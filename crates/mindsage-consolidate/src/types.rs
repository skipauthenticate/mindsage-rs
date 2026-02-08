//! Consolidation types.

use serde::Serialize;

/// Pipeline stages that can be run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationStage {
    PruneOrphans,
    Deduplicate,
    Compress,
    Evict,
}

impl ConsolidationStage {
    pub fn all() -> &'static [ConsolidationStage] {
        &[
            Self::PruneOrphans,
            Self::Deduplicate,
            Self::Compress,
            Self::Evict,
        ]
    }
}

/// Result of running the consolidation pipeline.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConsolidationReport {
    #[serde(rename = "orphansPruned")]
    pub orphans_pruned: usize,
    #[serde(rename = "duplicatesRemoved")]
    pub duplicates_removed: usize,
    #[serde(rename = "chunksCompressed")]
    pub chunks_compressed: usize,
    #[serde(rename = "documentsEvicted")]
    pub documents_evicted: usize,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
}

/// Tier-adaptive consolidation thresholds.
#[derive(Debug, Clone)]
pub struct ConsolidationThresholds {
    /// Maximum number of documents before eviction.
    pub max_documents: usize,
    /// Maximum number of chunks before eviction.
    pub max_chunks: usize,
    /// Similarity threshold for deduplication (0.0 - 1.0).
    pub dedup_threshold: f64,
}

impl ConsolidationThresholds {
    pub fn for_tier(tier: mindsage_core::CapabilityTier) -> Self {
        match tier {
            mindsage_core::CapabilityTier::Base => Self {
                max_documents: 1000,
                max_chunks: 10_000,
                dedup_threshold: 0.95,
            },
            mindsage_core::CapabilityTier::Enhanced => Self {
                max_documents: 5000,
                max_chunks: 50_000,
                dedup_threshold: 0.92,
            },
            mindsage_core::CapabilityTier::Advanced => Self {
                max_documents: 20_000,
                max_chunks: 200_000,
                dedup_threshold: 0.90,
            },
            mindsage_core::CapabilityTier::Full => Self {
                max_documents: 100_000,
                max_chunks: 1_000_000,
                dedup_threshold: 0.88,
            },
        }
    }
}
