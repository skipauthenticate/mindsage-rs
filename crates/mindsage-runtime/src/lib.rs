//! Runtime orchestrator — coordinates SDK verbs, budget tracking, scheduling.
//!
//! Provides the high-level SDK verbs (ingest, distill, recall, consolidate)
//! and manages resource budgets and power-aware scheduling.

pub mod orchestrator;
pub mod types;

pub use orchestrator::Orchestrator;
pub use types::*;
