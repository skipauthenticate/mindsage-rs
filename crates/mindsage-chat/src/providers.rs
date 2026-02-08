//! External LLM provider streaming implementations.
//!
//! Each provider streams tokens via SSE from their respective APIs.
//! OpenAI and Groq use the same format. Anthropic uses a different one.

use std::pin::Pin;

use futures::Stream;
use reqwest::Client;
use serde_json::json;
use tokio_stream::StreamExt;
use tracing::{debug, error};

use crate::types::{ChatMessage, LLMProvider};

/// Boxed stream type for returning different stream implementations.
pub type BoxedStream = Pin<Box<dyn Stream<Item = StreamChunk> + Send>>;

/// A single streamed token or error.
pub enum StreamChunk {
    Token(String),
    Done { tokens_used: usize },
    Error(String),
}

/// Stream tokens from the appropriate provider.
pub fn stream_llm(
    client: &Client,
    provider: LLMProvider,
    messages: Vec<ChatMessage>,
    model: &str,
    api_key: &str,
    temperature: f64,
    max_tokens: usize,
) -> BoxedStream {
    match provider {
        LLMProvider::OpenAI => Box::pin(stream_openai_compat(
            client.clone(),
            "https://api.openai.com/v1/chat/completions",
            messages,
            model.to_string(),
            api_key.to_string(),
            temperature,
            max_tokens,
        )),
        LLMProvider::Groq => Box::pin(stream_openai_compat(
            client.clone(),
            "https://api.groq.com/openai/v1/chat/completions",
            messages,
            model.to_string(),
            api_key.to_string(),
            temperature,
            max_tokens,
        )),
        LLMProvider::Anthropic => Box::pin(stream_anthropic(
            client.clone(),
            messages,
            model.to_string(),
            api_key.to_string(),
            temperature,
            max_tokens,
        )),
    }
}

/// Stream from OpenAI-compatible APIs (OpenAI, Groq).
fn stream_openai_compat(
    client: Client,
    url: &str,
    messages: Vec<ChatMessage>,
    model: String,
    api_key: String,
    temperature: f64,
    max_tokens: usize,
) -> impl Stream<Item = StreamChunk> + Send + 'static {
    let url = url.to_string();
    let msgs: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();

    async_stream::stream! {
        let body = json!({
            "model": model,
            "messages": msgs,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": true,
        });

        debug!("Streaming from {} with model {}", url, model);

        let response = match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                yield StreamChunk::Error(format!("Request failed: {}", e));
                return;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            yield StreamChunk::Error(format!("API error {}: {}", status, body));
            return;
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut token_count = 0usize;

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    yield StreamChunk::Error(format!("Stream read error: {}", e));
                    return;
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&bytes));

            // Process complete SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if data.trim() == "[DONE]" {
                        yield StreamChunk::Done { tokens_used: token_count };
                        return;
                    }

                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            if !content.is_empty() {
                                token_count += 1;
                                yield StreamChunk::Token(content.to_string());
                            }
                        }
                    }
                }
            }
        }

        yield StreamChunk::Done { tokens_used: token_count };
    }
}

/// Stream from Anthropic's Messages API.
fn stream_anthropic(
    client: Client,
    messages: Vec<ChatMessage>,
    model: String,
    api_key: String,
    temperature: f64,
    max_tokens: usize,
) -> impl Stream<Item = StreamChunk> + Send + 'static {
    // Separate system message from conversation
    let system_msg: Option<String> = messages
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone());

    let conv_msgs: Vec<serde_json::Value> = messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();

    async_stream::stream! {
        let mut body = json!({
            "model": model,
            "messages": conv_msgs,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": true,
        });

        if let Some(sys) = system_msg {
            body["system"] = json!(sys);
        }

        debug!("Streaming from Anthropic with model {}", model);

        let response = match client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                yield StreamChunk::Error(format!("Request failed: {}", e));
                return;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            yield StreamChunk::Error(format!("API error {}: {}", status, body));
            return;
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut token_count = 0usize;

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    yield StreamChunk::Error(format!("Stream read error: {}", e));
                    return;
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                // Anthropic uses "event: " lines followed by "data: " lines
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                        match parsed["type"].as_str() {
                            Some("content_block_delta") => {
                                if let Some(text) = parsed["delta"]["text"].as_str() {
                                    if !text.is_empty() {
                                        token_count += 1;
                                        yield StreamChunk::Token(text.to_string());
                                    }
                                }
                            }
                            Some("message_stop") => {
                                yield StreamChunk::Done { tokens_used: token_count };
                                return;
                            }
                            Some("error") => {
                                let msg = parsed["error"]["message"]
                                    .as_str()
                                    .unwrap_or("Unknown error");
                                error!("Anthropic error: {}", msg);
                                yield StreamChunk::Error(msg.to_string());
                                return;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        yield StreamChunk::Done { tokens_used: token_count };
    }
}

/// Test an API key by making a minimal request.
pub async fn test_api_key(provider: &str, api_key: &str) -> Result<(), String> {
    let client = Client::new();

    match provider {
        "openai" => {
            let resp = client
                .get("https://api.openai.com/v1/models")
                .header("Authorization", format!("Bearer {}", api_key))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(format!("API returned status {}", resp.status()))
            }
        }
        "anthropic" => {
            let resp = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&json!({
                    "model": "claude-3-5-haiku-20241022",
                    "max_tokens": 1,
                    "messages": [{"role": "user", "content": "Hi"}],
                }))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() || resp.status().as_u16() == 400 {
                // 400 with valid key means key works (may be quota/model issue)
                Ok(())
            } else {
                Err(format!("API returned status {}", resp.status()))
            }
        }
        "groq" => {
            let resp = client
                .get("https://api.groq.com/openai/v1/models")
                .header("Authorization", format!("Bearer {}", api_key))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(format!("API returned status {}", resp.status()))
            }
        }
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}
