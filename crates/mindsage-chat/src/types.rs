//! Chat types matching the Express server API surface.

use serde::{Deserialize, Serialize};

/// LLM provider identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LLMProvider {
    OpenAI,
    Anthropic,
    Groq,
}

impl std::fmt::Display for LLMProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LLMProvider::OpenAI => write!(f, "openai"),
            LLMProvider::Anthropic => write!(f, "anthropic"),
            LLMProvider::Groq => write!(f, "groq"),
        }
    }
}

/// Chat message in conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Incoming chat request.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default, rename = "conversationHistory")]
    pub conversation_history: Vec<ChatMessage>,
    pub model: Option<String>,
    #[serde(default = "default_use_rag", rename = "useRAG")]
    pub use_rag: bool,
    #[serde(default = "default_top_k", rename = "topK")]
    pub top_k: usize,
    #[serde(default = "default_min_score", rename = "minScore")]
    pub min_score: f64,
    pub temperature: Option<f64>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: Option<usize>,
    #[serde(rename = "consentSessionId")]
    pub consent_session_id: Option<String>,
}

fn default_use_rag() -> bool {
    true
}
fn default_top_k() -> usize {
    5
}
fn default_min_score() -> f64 {
    0.3
}

/// Non-streaming chat response.
#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub message: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<ChatContext>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "tokensUsed")]
    pub tokens_used: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
}

/// RAG context entry (search result excerpt).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatContext {
    pub id: i64,
    pub excerpt: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

/// SSE stream event types.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "context")]
    Context { context: Vec<ChatContext> },
    #[serde(rename = "token")]
    Token { content: String },
    #[serde(rename = "done")]
    Done {
        model: String,
        #[serde(rename = "tokensUsed")]
        tokens_used: usize,
        duration: u64,
    },
    #[serde(rename = "error")]
    Error { error: String },
}

/// Chat status response.
#[derive(Debug, Clone, Serialize)]
pub struct ChatStatus {
    #[serde(rename = "llmAvailable")]
    pub llm_available: bool,
    #[serde(rename = "llmProvider")]
    pub llm_provider: Option<String>,
    #[serde(rename = "vectorStoreAvailable")]
    pub vector_store_available: bool,
    #[serde(rename = "defaultModel")]
    pub default_model: Option<String>,
    #[serde(rename = "availableModels")]
    pub available_models: Vec<String>,
    #[serde(rename = "gpuAvailable")]
    pub gpu_available: bool,
    #[serde(rename = "gpuStatus")]
    pub gpu_status: String,
    #[serde(rename = "ollamaAvailable")]
    pub ollama_available: bool,
}

/// LLM config response (keys masked).
#[derive(Debug, Clone, Serialize)]
pub struct LLMConfigResponse {
    #[serde(rename = "preferredProvider")]
    pub preferred_provider: String,
    #[serde(rename = "openaiConfigured")]
    pub openai_configured: bool,
    #[serde(rename = "anthropicConfigured")]
    pub anthropic_configured: bool,
    #[serde(rename = "groqConfigured")]
    pub groq_configured: bool,
    #[serde(rename = "openaiModel")]
    pub openai_model: String,
    #[serde(rename = "anthropicModel")]
    pub anthropic_model: String,
    #[serde(rename = "groqModel")]
    pub groq_model: String,
    #[serde(rename = "activeProvider")]
    pub active_provider: Option<String>,
}

/// LLM config update request.
#[derive(Debug, Clone, Deserialize)]
pub struct LLMConfigUpdate {
    #[serde(rename = "preferredProvider")]
    pub preferred_provider: Option<String>,
    #[serde(rename = "openaiApiKey")]
    pub openai_api_key: Option<String>,
    #[serde(rename = "anthropicApiKey")]
    pub anthropic_api_key: Option<String>,
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: Option<String>,
    #[serde(rename = "openaiModel")]
    pub openai_model: Option<String>,
    #[serde(rename = "anthropicModel")]
    pub anthropic_model: Option<String>,
    #[serde(rename = "groqModel")]
    pub groq_model: Option<String>,
}

/// API key test request.
#[derive(Debug, Clone, Deserialize)]
pub struct TestKeyRequest {
    pub provider: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
}
