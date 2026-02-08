//! File text extraction for various formats.

use mindsage_core::Result;
use std::path::Path;

/// Supported file types for text extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    PlainText,
    Markdown,
    Code,
    Json,
    Pdf,
    Unknown,
}

impl FileType {
    /// Detect file type from extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "txt" => Self::PlainText,
            "md" | "mdx" => Self::Markdown,
            "py" | "js" | "ts" | "tsx" | "jsx" | "rs" | "go" | "java" | "cpp" | "c" | "h"
            | "hpp" | "cs" | "rb" | "php" | "swift" | "kt" | "scala" | "sh" | "bash" | "zsh"
            | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "xml" | "html" | "css"
            | "scss" | "sql" => Self::Code,
            "json" => Self::Json,
            "pdf" => Self::Pdf,
            _ => Self::Unknown,
        }
    }

    /// Check if this is a text-based file type.
    pub fn is_text(&self) -> bool {
        matches!(
            self,
            Self::PlainText | Self::Markdown | Self::Code | Self::Json
        )
    }
}

/// Extract text content from a file.
pub fn extract_text(path: &Path) -> Result<Option<String>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let file_type = FileType::from_extension(ext);

    match file_type {
        FileType::PlainText | FileType::Markdown | FileType::Code => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| mindsage_core::Error::Io(e))?;
            Ok(Some(content))
        }
        FileType::Json => extract_json(path),
        FileType::Pdf => {
            // PDF extraction — placeholder for pdf-extract crate integration
            tracing::warn!("PDF extraction not yet implemented: {}", path.display());
            Ok(None)
        }
        FileType::Unknown => {
            // Try reading as text
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    // Basic check: if content has too many non-UTF8-safe bytes, skip it
                    if content.chars().filter(|c| c.is_control() && *c != '\n' && *c != '\r' && *c != '\t').count()
                        > content.len() / 10
                    {
                        Ok(None) // Likely binary
                    } else {
                        Ok(Some(content))
                    }
                }
                Err(_) => Ok(None), // Binary file
            }
        }
    }
}

/// Extract text from a JSON file. Handles ChatGPT export format.
fn extract_json(path: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| mindsage_core::Error::Io(e))?;

    // Try ChatGPT export format: array of conversations
    if let Ok(conversations) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
        let mut texts = Vec::new();
        for conv in &conversations {
            if let Some(title) = conv.get("title").and_then(|v| v.as_str()) {
                texts.push(format!("# {}", title));
            }
            if let Some(mapping) = conv.get("mapping").and_then(|v| v.as_object()) {
                for (_key, node) in mapping {
                    if let Some(message) = node.get("message") {
                        if let Some(parts) = message
                            .get("content")
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.as_array())
                        {
                            for part in parts {
                                if let Some(text) = part.as_str() {
                                    if !text.is_empty() {
                                        let role = message
                                            .get("author")
                                            .and_then(|a| a.get("role"))
                                            .and_then(|r| r.as_str())
                                            .unwrap_or("unknown");
                                        texts.push(format!("[{}]: {}", role, text));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !texts.is_empty() {
            return Ok(Some(texts.join("\n\n")));
        }
    }

    // Generic JSON: just return the raw content for indexing
    Ok(Some(content))
}
