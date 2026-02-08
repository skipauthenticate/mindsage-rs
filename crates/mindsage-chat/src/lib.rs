//! RAG chat service with external LLM streaming (OpenAI/Anthropic/Groq).
//!
//! Provides streaming chat with retrieval-augmented generation.
//! LLM calls go to external APIs — no local model required.

pub mod config;
pub mod providers;
pub mod types;

pub use config::LLMConfig;
pub use types::*;
