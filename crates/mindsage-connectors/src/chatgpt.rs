//! ChatGPT export ZIP processor.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde_json::Value;
use tracing::{info, warn};

use crate::types::ImportResult;

/// Process a ChatGPT export ZIP file.
/// Extracts conversations.json, saves individual conversation files to exports_dir.
pub fn process_chatgpt_export(
    zip_path: &Path,
    exports_dir: &Path,
) -> ImportResult {
    std::fs::create_dir_all(exports_dir).ok();

    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(e) => {
            return ImportResult {
                success: false,
                item_count: 0,
                error: Some(format!("Failed to open ZIP: {}", e)),
                details: None,
            }
        }
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            return ImportResult {
                success: false,
                item_count: 0,
                error: Some(format!("Invalid ZIP file: {}", e)),
                details: None,
            }
        }
    };

    let mut conversation_count = 0;
    let mut total_messages = 0;

    // Find and extract conversations.json
    let conversations_data = {
        let mut data = None;
        for i in 0..archive.len() {
            if let Ok(mut entry) = archive.by_index(i) {
                let name = entry.name().to_string();
                if name == "conversations.json" || name.ends_with("/conversations.json") {
                    let mut buf = String::new();
                    if entry.read_to_string(&mut buf).is_ok() {
                        data = Some(buf);
                    }
                    break;
                }
            }
        }
        data
    };

    if let Some(raw) = conversations_data {
        if let Ok(conversations) = serde_json::from_str::<Vec<Value>>(&raw) {
            for conv in &conversations {
                let conv_id = conv
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let title = conv
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Untitled");

                // Extract messages from mapping tree
                let messages = extract_messages(conv);
                if messages.is_empty() {
                    continue;
                }

                total_messages += messages.len();
                conversation_count += 1;

                // Build output document
                let doc = serde_json::json!({
                    "id": conv_id,
                    "title": title,
                    "create_time": conv.get("create_time"),
                    "update_time": conv.get("update_time"),
                    "messages": messages,
                });

                // Sanitize title for filename
                let safe_title: String = title
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
                    .take(50)
                    .collect();
                let filename = format!("chatgpt_{}_{}.json", conv_id, safe_title.trim());
                let out_path = exports_dir.join(&filename);

                if let Ok(json) = serde_json::to_string_pretty(&doc) {
                    if let Err(e) = std::fs::write(&out_path, json) {
                        warn!("Failed to write {}: {}", filename, e);
                    }
                }
            }
        }
    }

    // Also extract user.json if present
    for i in 0..archive.len() {
        if let Ok(mut entry) = archive.by_index(i) {
            let name = entry.name().to_string();
            if name == "user.json" || name.ends_with("/user.json") {
                let mut buf = String::new();
                if entry.read_to_string(&mut buf).is_ok() {
                    let _ = std::fs::write(exports_dir.join("user_profile.json"), buf);
                }
                break;
            }
        }
    }

    info!(
        "ChatGPT import: {} conversations, {} messages",
        conversation_count, total_messages
    );

    ImportResult {
        success: true,
        item_count: conversation_count,
        error: None,
        details: Some(serde_json::json!({
            "conversationCount": conversation_count,
            "messageCount": total_messages,
        })),
    }
}

/// Extract messages from a ChatGPT conversation's mapping tree.
fn extract_messages(conv: &Value) -> Vec<Value> {
    let mut messages = Vec::new();
    let mapping = match conv.get("mapping").and_then(|m| m.as_object()) {
        Some(m) => m,
        None => return messages,
    };

    let mut msg_list: Vec<(f64, Value)> = Vec::new();

    for (_node_id, node) in mapping {
        if let Some(message) = node.get("message") {
            let content = message.get("content");
            let role = message
                .get("author")
                .and_then(|a| a.get("role"))
                .and_then(|r| r.as_str())
                .unwrap_or("unknown");

            // Extract text content
            let text = extract_content_text(content);
            if text.is_empty() || role == "system" {
                continue;
            }

            let create_time = message
                .get("create_time")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);

            let mapped_role = match role {
                "user" => "user",
                "assistant" => "assistant",
                _ => "unknown",
            };

            msg_list.push((
                create_time,
                serde_json::json!({
                    "role": mapped_role,
                    "content": text,
                    "create_time": create_time,
                }),
            ));
        }
    }

    // Sort by creation time
    msg_list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    messages.extend(msg_list.into_iter().map(|(_, m)| m));
    messages
}

/// Extract text from a message content object.
fn extract_content_text(content: Option<&Value>) -> String {
    let content = match content {
        Some(c) => c,
        None => return String::new(),
    };

    // content.parts is an array of strings or objects
    if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
        let texts: Vec<String> = parts
            .iter()
            .filter_map(|p| {
                if let Some(s) = p.as_str() {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
                None
            })
            .collect();
        return texts.join("\n");
    }

    // Fallback: content.text
    content
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

/// Build a minimal ChatGPT-format ZIP in memory for testing.
#[cfg(test)]
fn build_test_zip(conversations: &serde_json::Value) -> Vec<u8> {
    use std::io::Write;
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("conversations.json", options).unwrap();
    zip.write_all(serde_json::to_string(conversations).unwrap().as_bytes())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

/// Build indexable documents from ChatGPT export files.
/// Returns (text, metadata) pairs ready for vector store indexing.
pub fn build_index_documents(
    exports_dir: &Path,
) -> Vec<(String, serde_json::Value)> {
    let mut documents = Vec::new();

    let entries = match std::fs::read_dir(exports_dir) {
        Ok(e) => e,
        Err(_) => return documents,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if !name.starts_with("chatgpt_") || !name.ends_with(".json") {
            continue;
        }

        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let conv: HashMap<String, Value> = match serde_json::from_str(&data) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let title = conv
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled");
        let conv_id = conv
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if let Some(messages) = conv.get("messages").and_then(|m| m.as_array()) {
            // Build text from messages
            let text: String = messages
                .iter()
                .filter_map(|m| {
                    let role = m.get("role").and_then(|r| r.as_str())?;
                    let content = m.get("content").and_then(|c| c.as_str())?;
                    Some(format!("{}: {}", role, content))
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            if text.is_empty() {
                continue;
            }

            let metadata = serde_json::json!({
                "source": "chatgpt",
                "conversationId": conv_id,
                "title": title,
                "exportFile": name,
            });

            documents.push((text, metadata));
        }
    }

    documents
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_chatgpt_export() {
        let dir = tempfile::tempdir().unwrap();
        let exports_dir = dir.path().join("exports");
        std::fs::create_dir_all(&exports_dir).unwrap();

        let conversations = serde_json::json!([
            {
                "id": "conv-1",
                "title": "Test Conversation",
                "create_time": 1700000000.0,
                "mapping": {
                    "node-1": {
                        "message": {
                            "author": { "role": "user" },
                            "content": { "parts": ["Hello!"] },
                            "create_time": 1700000001.0
                        }
                    },
                    "node-2": {
                        "message": {
                            "author": { "role": "assistant" },
                            "content": { "parts": ["Hi there!"] },
                            "create_time": 1700000002.0
                        }
                    }
                }
            }
        ]);

        let zip_data = build_test_zip(&conversations);
        let zip_path = dir.path().join("export.zip");
        std::fs::write(&zip_path, zip_data).unwrap();

        let result = process_chatgpt_export(&zip_path, &exports_dir);
        assert!(result.success);
        assert_eq!(result.item_count, 1);

        let details = result.details.unwrap();
        assert_eq!(details["conversationCount"], 1);
        assert_eq!(details["messageCount"], 2);
    }

    #[test]
    fn test_process_empty_zip() {
        let dir = tempfile::tempdir().unwrap();
        let exports_dir = dir.path().join("exports");

        // Create ZIP without conversations.json
        let buf = std::io::Cursor::new(Vec::new());
        let zip = zip::ZipWriter::new(buf);
        let data = zip.finish().unwrap().into_inner();
        let zip_path = dir.path().join("empty.zip");
        std::fs::write(&zip_path, data).unwrap();

        let result = process_chatgpt_export(&zip_path, &exports_dir);
        assert!(result.success);
        assert_eq!(result.item_count, 0);
    }

    #[test]
    fn test_invalid_zip() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bad.zip");
        std::fs::write(&zip_path, b"not a zip").unwrap();

        let result = process_chatgpt_export(&zip_path, &dir.path().join("exports"));
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_build_index_documents() {
        let dir = tempfile::tempdir().unwrap();
        let exports_dir = dir.path().join("exports");
        std::fs::create_dir_all(&exports_dir).unwrap();

        // Write a chatgpt export file
        let doc = serde_json::json!({
            "id": "conv-1",
            "title": "My Chat",
            "messages": [
                { "role": "user", "content": "What is Rust?" },
                { "role": "assistant", "content": "A systems programming language." }
            ]
        });
        std::fs::write(
            exports_dir.join("chatgpt_conv-1_My Chat.json"),
            serde_json::to_string_pretty(&doc).unwrap(),
        )
        .unwrap();

        let docs = build_index_documents(&exports_dir);
        assert_eq!(docs.len(), 1);
        assert!(docs[0].0.contains("What is Rust?"));
        assert_eq!(docs[0].1["source"], "chatgpt");
    }

    #[test]
    fn test_extract_content_text_parts() {
        let content = serde_json::json!({
            "parts": ["Hello", "World"]
        });
        assert_eq!(extract_content_text(Some(&content)), "Hello\nWorld");
    }

    #[test]
    fn test_extract_content_text_fallback() {
        let content = serde_json::json!({ "text": "Fallback text" });
        assert_eq!(extract_content_text(Some(&content)), "Fallback text");
    }

    #[test]
    fn test_system_messages_filtered() {
        let conversations = serde_json::json!([{
            "id": "conv-sys",
            "title": "System Test",
            "mapping": {
                "sys-node": {
                    "message": {
                        "author": { "role": "system" },
                        "content": { "parts": ["System prompt"] },
                        "create_time": 1.0
                    }
                }
            }
        }]);

        let zip_data = build_test_zip(&conversations);
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("sys.zip");
        std::fs::write(&zip_path, zip_data).unwrap();

        let result = process_chatgpt_export(&zip_path, &dir.path().join("exports"));
        assert!(result.success);
        assert_eq!(result.item_count, 0); // system messages filtered
    }
}
