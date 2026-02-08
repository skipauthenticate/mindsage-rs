//! Chat routes — RAG chat with external LLM streaming.
//! Matches /api/chat/* endpoints from the Express server.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::Stream;
use tokio_stream::StreamExt;

use crate::state::AppState;
use mindsage_chat::providers::{self, StreamChunk};
use mindsage_chat::types::*;

type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat/status", get(get_status))
        .route("/chat", post(chat))
        .route("/chat/stream", post(stream_chat))
        .route("/chat/config", get(get_config).put(update_config))
        .route("/chat/config/test", post(test_key))
}

// ---------------------------------------------------------------
// Status
// ---------------------------------------------------------------

async fn get_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let config = state.llm_config.read();
    let resolved = config.resolve_provider();
    let store_stats = state.store.get_stats().ok();

    Json(serde_json::json!({
        "llmAvailable": resolved.is_some(),
        "llmProvider": resolved.as_ref().map(|(p, _, _)| p.to_string()),
        "vectorStoreAvailable": store_stats.is_some(),
        "defaultModel": resolved.as_ref().map(|(_, m, _)| m.clone()),
        "availableModels": config.available_models(),
        "gpuAvailable": false,
        "gpuStatus": "not_applicable",
        "ollamaAvailable": false,
    }))
}

// ---------------------------------------------------------------
// Non-streaming chat
// ---------------------------------------------------------------

async fn chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let start = Instant::now();

    let (provider, model, api_key) = {
        let config = state.llm_config.read();
        match config.resolve_provider() {
            Some(resolved) => resolved,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "No LLM provider configured",
                    })),
                );
            }
        }
    };

    // Build RAG context
    let context = if req.use_rag {
        build_rag_context(&state, &req.message, req.top_k, req.min_score)
    } else {
        Vec::new()
    };

    // Build messages
    let messages = build_messages(&context, &req.conversation_history, &req.message);

    let temperature = req.temperature.unwrap_or(0.7);
    let max_tokens = req.max_tokens.unwrap_or(2048);

    // Collect all tokens (non-streaming)
    let client = reqwest::Client::new();
    let stream = providers::stream_llm(
        &client, provider, messages,
        &model, &api_key,
        temperature, max_tokens,
    );

    tokio::pin!(stream);

    let mut full_response = String::new();
    let mut tokens_used = 0;

    while let Some(chunk) = stream.next().await {
        match chunk {
            StreamChunk::Token(text) => {
                full_response.push_str(&text);
            }
            StreamChunk::Done { tokens_used: t } => {
                tokens_used = t;
            }
            StreamChunk::Error(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                );
            }
        }
    }

    let duration = start.elapsed().as_millis() as u64;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": full_response,
            "model": model,
            "context": if context.is_empty() { None } else { Some(&context) },
            "tokensUsed": tokens_used,
            "duration": duration,
        })),
    )
}

// ---------------------------------------------------------------
// Streaming chat (SSE)
// ---------------------------------------------------------------

async fn stream_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Sse<SseStream> {
    let start = Instant::now();

    let resolved = {
        let config = state.llm_config.read();
        config.resolve_provider()
    };

    let (provider, model, api_key) = match resolved {
        Some(r) => r,
        None => {
            let error_stream: SseStream = Box::pin(async_stream::stream! {
                let event = StreamEvent::Error {
                    error: "No LLM provider configured".into(),
                };
                yield Ok::<_, Infallible>(Event::default().data(
                    serde_json::to_string(&event).unwrap()
                ));
            });
            return Sse::new(error_stream);
        }
    };

    // Build RAG context
    let context = if req.use_rag {
        build_rag_context(&state, &req.message, req.top_k, req.min_score)
    } else {
        Vec::new()
    };

    // Build messages
    let messages = build_messages(&context, &req.conversation_history, &req.message);

    let temperature = req.temperature.unwrap_or(0.7);
    let max_tokens = req.max_tokens.unwrap_or(2048);

    let client = reqwest::Client::new();
    let llm_stream = providers::stream_llm(
        &client, provider, messages,
        &model, &api_key,
        temperature, max_tokens,
    );

    let model_clone = model.clone();

    let sse_stream: SseStream = Box::pin(async_stream::stream! {
        // First: emit context event
        if !context.is_empty() {
            let event = StreamEvent::Context { context };
            yield Ok::<_, Infallible>(Event::default().data(
                serde_json::to_string(&event).unwrap()
            ));
        }

        // Stream tokens from LLM
        tokio::pin!(llm_stream);
        while let Some(chunk) = llm_stream.next().await {
            match chunk {
                StreamChunk::Token(text) => {
                    let event = StreamEvent::Token { content: text };
                    yield Ok(Event::default().data(
                        serde_json::to_string(&event).unwrap()
                    ));
                }
                StreamChunk::Done { tokens_used } => {
                    let duration = start.elapsed().as_millis() as u64;
                    let event = StreamEvent::Done {
                        model: model_clone.clone(),
                        tokens_used,
                        duration,
                    };
                    yield Ok(Event::default().data(
                        serde_json::to_string(&event).unwrap()
                    ));
                    // Final [DONE] marker
                    yield Ok(Event::default().data("[DONE]".to_string()));
                    return;
                }
                StreamChunk::Error(e) => {
                    let event = StreamEvent::Error { error: e };
                    yield Ok(Event::default().data(
                        serde_json::to_string(&event).unwrap()
                    ));
                    return;
                }
            }
        }
    });

    Sse::new(sse_stream)
}

// ---------------------------------------------------------------
// Config
// ---------------------------------------------------------------

async fn get_config(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let config = state.llm_config.read();
    Json(serde_json::to_value(config.to_response()).unwrap())
}

async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(update): Json<LLMConfigUpdate>,
) -> impl IntoResponse {
    let mut config = state.llm_config.write();
    config.apply_update(&update);

    if let Err(e) = config.save() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to save config: {}", e) })),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::to_value(config.to_response()).unwrap()),
    )
}

async fn test_key(
    Json(req): Json<TestKeyRequest>,
) -> impl IntoResponse {
    match providers::test_api_key(&req.provider, &req.api_key).await {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "success": true })),
        ),
        Err(e) => (
            StatusCode::OK,
            Json(serde_json::json!({ "success": false, "error": e })),
        ),
    }
}

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

/// Build RAG context from vector store search.
fn build_rag_context(
    state: &AppState,
    query: &str,
    top_k: usize,
    min_score: f64,
) -> Vec<ChatContext> {
    // Use hybrid search when embedder is available, else BM25
    let results = if state.embedder.is_available() {
        if let Some(emb_result) = state.embedder.embed(query) {
            match state.store.hybrid_search(query, &emb_result.embedding, 1, top_k, top_k, 60) {
                Ok(r) => r,
                Err(_) => match state.store.bm25_search(query, 1, top_k) {
                    Ok(r) => r,
                    Err(_) => return Vec::new(),
                },
            }
        } else {
            match state.store.bm25_search(query, 1, top_k) {
                Ok(r) => r,
                Err(_) => return Vec::new(),
            }
        }
    } else {
        match state.store.bm25_search(query, 1, top_k) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        }
    };

    results
        .iter()
        .filter(|hit| hit.score >= min_score)
        .map(|hit| {
            let (source, filename) = extract_source_filename(&hit.metadata);
            ChatContext {
                id: hit.chunk_id,
                excerpt: truncate_text(&hit.text, 500),
                score: hit.score,
                source,
                filename,
            }
        })
        .collect()
}

fn extract_source_filename(
    metadata: &Option<serde_json::Value>,
) -> (Option<String>, Option<String>) {
    let source = metadata
        .as_ref()
        .and_then(|m| m.get("source"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let filename = metadata
        .as_ref()
        .and_then(|m| m.get("filename"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    (source, filename)
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len])
    }
}

/// Build the message array for the LLM, including system prompt with RAG context.
fn build_messages(
    context: &[ChatContext],
    conversation_history: &[ChatMessage],
    user_message: &str,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    // System prompt with RAG context
    let system_prompt = if context.is_empty() {
        "You are a helpful assistant with access to the user's personal knowledge base. \
         Answer questions based on your knowledge."
            .to_string()
    } else {
        let context_str: String = context
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let source_info = c
                    .source
                    .as_ref()
                    .map(|s| format!(" (source: {})", s))
                    .unwrap_or_default();
                format!("[{}]{}: {}", i + 1, source_info, c.excerpt)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        format!(
            "You are a helpful assistant with access to the user's personal knowledge base. \
             Use the following context to answer the user's question. \
             If the context doesn't contain relevant information, say so.\n\n\
             Context:\n{}\n\n\
             Note: Some values in the context may have been modified for privacy protection.",
            context_str
        )
    };

    messages.push(ChatMessage {
        role: "system".into(),
        content: system_prompt,
    });

    // Conversation history
    for msg in conversation_history {
        messages.push(msg.clone());
    }

    // Current user message
    messages.push(ChatMessage {
        role: "user".into(),
        content: user_message.to_string(),
    });

    messages
}
