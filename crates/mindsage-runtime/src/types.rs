//! Runtime types.

use serde::Serialize;

/// SDK verb that can be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verb {
    /// Ingest text → chunk → embed → store → queue extraction.
    Ingest,
    /// Run extraction on pending documents.
    Distill,
    /// Query resolution with tier-aware strategy selection.
    Recall,
    /// Run consolidation pipeline.
    Consolidate,
}

/// Resource budget for operation scheduling.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceBudget {
    /// Maximum memory usage in MB.
    #[serde(rename = "maxMemoryMb")]
    pub max_memory_mb: usize,
    /// Maximum GPU memory in MB (0 if no GPU).
    #[serde(rename = "maxGpuMemoryMb")]
    pub max_gpu_memory_mb: usize,
    /// Maximum concurrent operations.
    #[serde(rename = "maxConcurrency")]
    pub max_concurrency: usize,
}

impl ResourceBudget {
    pub fn for_tier(tier: mindsage_core::CapabilityTier) -> Self {
        match tier {
            mindsage_core::CapabilityTier::Base => Self {
                max_memory_mb: 256,
                max_gpu_memory_mb: 0,
                max_concurrency: 1,
            },
            mindsage_core::CapabilityTier::Enhanced => Self {
                max_memory_mb: 512,
                max_gpu_memory_mb: 2048,
                max_concurrency: 2,
            },
            mindsage_core::CapabilityTier::Advanced => Self {
                max_memory_mb: 1024,
                max_gpu_memory_mb: 4096,
                max_concurrency: 4,
            },
            mindsage_core::CapabilityTier::Full => Self {
                max_memory_mb: 2048,
                max_gpu_memory_mb: 8192,
                max_concurrency: 8,
            },
        }
    }
}

/// Runtime status information.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatus {
    pub tier: mindsage_core::CapabilityTier,
    pub budget: ResourceBudget,
    #[serde(rename = "activeVerbs")]
    pub active_verbs: Vec<Verb>,
    #[serde(rename = "pendingDistill")]
    pub pending_distill: usize,
}
