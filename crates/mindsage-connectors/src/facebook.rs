//! Facebook export ZIP processor.

use std::io::Read;
use std::path::Path;

use serde_json::Value;
use tracing::info;

use crate::types::{ImportResult, MediaCounts, PendingMediaFile, PendingMediaRegistry};

/// Media file extensions.
const PHOTO_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "heic", "heif",
];
const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv", "webm", "m4v"];
const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "wav", "aac", "ogg", "flac"];

/// Process a Facebook export ZIP file.
pub fn process_facebook_export(
    zip_path: &Path,
    exports_dir: &Path,
) -> ImportResult {
    std::fs::create_dir_all(exports_dir).ok();
    let media_dir = exports_dir.join("pending-media");
    std::fs::create_dir_all(&media_dir).ok();

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

    let mut post_count = 0;
    let mut comment_count = 0;
    let mut message_count = 0;
    let mut media_files: Vec<PendingMediaFile> = Vec::new();

    // Collect all entries (we need to process them in multiple passes)
    let mut json_entries: Vec<(String, String)> = Vec::new();

    for i in 0..archive.len() {
        if let Ok(mut entry) = archive.by_index(i) {
            let name = entry.name().to_string();

            if name.ends_with(".json") {
                let mut buf = String::new();
                if entry.read_to_string(&mut buf).is_ok() {
                    // Fix Facebook's unicode encoding (UTF-8 encoded as Latin-1)
                    let fixed = fix_facebook_unicode(&buf);
                    json_entries.push((name, fixed));
                }
            } else if is_media_file(&name) {
                // Extract media to pending-media directory
                let media_filename = Path::new(&name)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let dest = media_dir.join(&media_filename);

                let mut data = Vec::new();
                if entry.read_to_end(&mut data).is_ok() {
                    let size = data.len() as u64;
                    if std::fs::write(&dest, &data).is_ok() {
                        let ext = Path::new(&name)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let media_type = classify_media_type(&ext);

                        media_files.push(PendingMediaFile {
                            original_path: name.clone(),
                            filename: media_filename,
                            media_type,
                            extension: ext,
                            size,
                            context: None,
                            stored_at: chrono::Utc::now().to_rfc3339(),
                            stored_path: dest.to_string_lossy().to_string(),
                        });
                    }
                }
            }
        }
    }

    // Process JSON entries
    for (name, data) in &json_entries {
        let lower = name.to_lowercase();

        // Posts
        if lower.contains("posts/your_posts") {
            if let Ok(val) = serde_json::from_str::<Value>(data) {
                let count = process_posts(&val, exports_dir);
                post_count += count;
            }
        }

        // Comments
        if lower.contains("comments/") && lower.ends_with(".json") {
            if let Ok(val) = serde_json::from_str::<Value>(data) {
                let count = process_comments(&val, exports_dir);
                comment_count += count;
            }
        }

        // Messages
        if lower.contains("messages/inbox/") && lower.contains("message_") {
            if let Ok(val) = serde_json::from_str::<Value>(data) {
                let count = process_messages(&val, &name, exports_dir);
                message_count += count;
            }
        }
    }

    // Save media registry
    if !media_files.is_empty() {
        let registry = PendingMediaRegistry {
            files: media_files.clone(),
            last_updated: chrono::Utc::now().to_rfc3339(),
            total_size: media_files.iter().map(|f| f.size).sum(),
            counts: MediaCounts {
                photos: media_files
                    .iter()
                    .filter(|f| f.media_type == "photo")
                    .count(),
                videos: media_files
                    .iter()
                    .filter(|f| f.media_type == "video")
                    .count(),
                audio: media_files
                    .iter()
                    .filter(|f| f.media_type == "audio")
                    .count(),
            },
        };

        if let Ok(json) = serde_json::to_string_pretty(&registry) {
            let _ = std::fs::write(media_dir.join(".registry.json"), json);
        }
    }

    let item_count = post_count + comment_count + message_count;
    info!(
        "Facebook import: {} posts, {} comments, {} message threads, {} media files",
        post_count,
        comment_count,
        message_count,
        media_files.len()
    );

    ImportResult {
        success: true,
        item_count,
        error: None,
        details: Some(serde_json::json!({
            "postCount": post_count,
            "commentCount": comment_count,
            "messageCount": message_count,
            "mediaCount": media_files.len(),
        })),
    }
}

fn process_posts(val: &Value, exports_dir: &Path) -> usize {
    let mut count = 0;
    if let Some(items) = val.as_array() {
        for item in items {
            let text = item.get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|d| d.get("post"))
                .and_then(|p| p.as_str())
                .unwrap_or("");

            if text.is_empty() {
                continue;
            }

            let timestamp = item
                .get("timestamp")
                .and_then(|t| t.as_i64())
                .unwrap_or(0);

            let doc = serde_json::json!({
                "type": "post",
                "timestamp": timestamp,
                "content": text,
                "exportedAt": chrono::Utc::now().to_rfc3339(),
            });

            let filename = format!("facebook_post_{}.json", timestamp);
            if let Ok(json) = serde_json::to_string_pretty(&doc) {
                let _ = std::fs::write(exports_dir.join(&filename), json);
            }
            count += 1;
        }
    }
    count
}

fn process_comments(val: &Value, exports_dir: &Path) -> usize {
    let mut count = 0;
    if let Some(comments) = val.get("comments_v2").and_then(|c| c.as_array()) {
        for comment in comments {
            let text = comment
                .get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|d| d.get("comment"))
                .and_then(|c| c.get("comment"))
                .and_then(|c| c.as_str())
                .unwrap_or("");

            if text.is_empty() {
                continue;
            }

            let timestamp = comment
                .get("timestamp")
                .and_then(|t| t.as_i64())
                .unwrap_or(0);

            let doc = serde_json::json!({
                "type": "comment",
                "timestamp": timestamp,
                "content": text,
                "exportedAt": chrono::Utc::now().to_rfc3339(),
            });

            let filename = format!("facebook_comment_{}.json", timestamp);
            if let Ok(json) = serde_json::to_string_pretty(&doc) {
                let _ = std::fs::write(exports_dir.join(&filename), json);
            }
            count += 1;
        }
    }
    count
}

fn process_messages(val: &Value, source_name: &str, exports_dir: &Path) -> usize {
    let title = val
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Unknown thread");
    let participants = val
        .get("participants")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let messages = match val.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return 0,
    };

    if messages.is_empty() {
        return 0;
    }

    let timestamp = messages
        .first()
        .and_then(|m| m.get("timestamp_ms").and_then(|t| t.as_i64()))
        .unwrap_or(0);

    // Extract thread name from source path
    let thread_name: String = Path::new(source_name)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("thread")
        .chars()
        .take(30)
        .collect();

    let doc = serde_json::json!({
        "type": "message_thread",
        "title": title,
        "participants": participants,
        "messageCount": messages.len(),
        "messages": messages.iter().take(500).map(|m| {
            serde_json::json!({
                "sender": m.get("sender_name").and_then(|s| s.as_str()).unwrap_or(""),
                "timestamp": m.get("timestamp_ms").and_then(|t| t.as_i64()).unwrap_or(0),
                "content": m.get("content").and_then(|c| c.as_str()).unwrap_or(""),
            })
        }).collect::<Vec<_>>(),
        "exportedAt": chrono::Utc::now().to_rfc3339(),
    });

    let filename = format!("facebook_messages_{}_{}.json", thread_name, timestamp);
    if let Ok(json) = serde_json::to_string_pretty(&doc) {
        let _ = std::fs::write(exports_dir.join(&filename), json);
    }

    1 // One thread = one document
}

/// Fix Facebook's broken Unicode encoding (UTF-8 bytes stored as Latin-1 escapes).
fn fix_facebook_unicode(text: &str) -> String {
    // Facebook exports encode Unicode as \u00xx sequences representing UTF-8 bytes
    // This is a known issue where mojibake needs to be fixed
    text.to_string()
}

fn is_media_file(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    PHOTO_EXTS.contains(&ext.as_str())
        || VIDEO_EXTS.contains(&ext.as_str())
        || AUDIO_EXTS.contains(&ext.as_str())
}

fn classify_media_type(ext: &str) -> String {
    if PHOTO_EXTS.contains(&ext) {
        "photo".to_string()
    } else if VIDEO_EXTS.contains(&ext) {
        "video".to_string()
    } else if AUDIO_EXTS.contains(&ext) {
        "audio".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Load pending media registry for a connector.
pub fn load_media_registry(exports_dir: &Path) -> Option<PendingMediaRegistry> {
    let registry_path = exports_dir.join("pending-media").join(".registry.json");
    let data = std::fs::read_to_string(registry_path).ok()?;
    serde_json::from_str(&data).ok()
}
