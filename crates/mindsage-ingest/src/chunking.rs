//! Text chunking — port of Python's chunking.py.
//!
//! Provides hierarchical chunking: section (level=0) → paragraph (level=1).
//! Only level=1 paragraphs get embeddings and are searched.
//! Default chunk size 512 chars aligned with all-MiniLM-L6-v2 (256 tokens).

use regex::Regex;

/// Default chunk size aligned with embedding model (all-MiniLM-L6-v2: 256 tokens ≈ 512 chars).
pub const DEFAULT_CHUNK_SIZE: usize = 512;
/// Default overlap between chunks.
pub const DEFAULT_CHUNK_OVERLAP: usize = 100;

/// A flat text chunk with position metadata.
#[derive(Debug, Clone)]
pub struct TextChunk {
    pub text: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub start_char: usize,
    pub end_char: usize,
}

/// A chunk in the hierarchical document structure.
///
/// Levels:
/// - 0 = section (split on headers / double-newlines)
/// - 1 = paragraph (RecursiveChunker ~512 chars) — these get embeddings and are searched
#[derive(Debug, Clone)]
pub struct HierarchicalChunk {
    pub text: String,
    pub level: i32,
    pub chunk_index: usize,
    pub char_start: usize,
    pub char_end: usize,
    /// Index of parent in the flattened list (for level=1 chunks).
    pub parent_index: Option<usize>,
}

/// Recursive chunker that respects document structure.
pub struct RecursiveChunker {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    separators: Vec<&'static str>,
}

impl RecursiveChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
            separators: vec!["\n\n", "\n", ". ", " ", ""],
        }
    }

    pub fn chunk(&self, text: &str) -> Vec<TextChunk> {
        let raw_chunks = self.split_text(text, &self.separators);
        let mut result = Vec::new();
        let mut position = 0;

        for chunk_text in &raw_chunks {
            result.push(TextChunk {
                text: chunk_text.clone(),
                chunk_index: result.len(),
                total_chunks: raw_chunks.len(),
                start_char: position,
                end_char: position + chunk_text.len(),
            });
            position += chunk_text.len();
        }
        result
    }

    fn split_text(&self, text: &str, separators: &[&str]) -> Vec<String> {
        if separators.is_empty() {
            return vec![text.to_string()];
        }

        let separator = separators[0];
        let remaining = &separators[1..];

        let splits: Vec<&str> = if separator.is_empty() {
            text.chars().map(|_| "").collect::<Vec<_>>() // fallback
        } else {
            text.split(separator).collect()
        };

        // Handle empty separator edge case
        if separator.is_empty() {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut current_parts: Vec<&str> = Vec::new();
        let mut current_size = 0usize;

        for split in &splits {
            let split_size = split.len();

            if split_size > self.chunk_size {
                // Flush current
                if !current_parts.is_empty() {
                    chunks.push(current_parts.join(separator));
                    current_parts.clear();
                    current_size = 0;
                }
                // Recursively split this large piece
                let sub_chunks = self.split_text(split, remaining);
                chunks.extend(sub_chunks);
            } else if current_size + split_size + separator.len() > self.chunk_size
                && !current_parts.is_empty()
            {
                chunks.push(current_parts.join(separator));
                current_parts = vec![split];
                current_size = split_size;
            } else {
                current_parts.push(split);
                current_size += split_size + separator.len();
            }
        }

        if !current_parts.is_empty() {
            chunks.push(current_parts.join(separator));
        }

        chunks
    }
}

/// Three-level hierarchical chunker: section → paragraph.
pub struct HierarchicalChunker {
    paragraph_chunker: RecursiveChunker,
}

impl HierarchicalChunker {
    pub fn new(paragraph_size: usize, paragraph_overlap: usize) -> Self {
        Self {
            paragraph_chunker: RecursiveChunker::new(paragraph_size, paragraph_overlap),
        }
    }

    /// Split text into a hierarchical chunk list.
    /// Returns level=0 (sections) and level=1 (paragraphs) interleaved.
    pub fn chunk(&self, text: &str) -> Vec<HierarchicalChunk> {
        let sections = self.split_sections(text);
        let mut all_chunks = Vec::new();

        for (sec_text, sec_start) in &sections {
            let section_idx = all_chunks.len();
            all_chunks.push(HierarchicalChunk {
                text: sec_text.clone(),
                level: 0,
                chunk_index: section_idx,
                char_start: *sec_start,
                char_end: sec_start + sec_text.len(),
                parent_index: None,
            });

            // Split section into paragraph-level chunks
            let para_chunks = self.paragraph_chunker.chunk(sec_text);
            for pc in para_chunks {
                let para_idx = all_chunks.len();
                all_chunks.push(HierarchicalChunk {
                    text: pc.text,
                    level: 1,
                    chunk_index: para_idx,
                    char_start: sec_start + pc.start_char,
                    char_end: sec_start + pc.end_char,
                    parent_index: Some(section_idx),
                });
            }
        }

        all_chunks
    }

    /// Split text into sections by headers or large gaps.
    fn split_sections(&self, text: &str) -> Vec<(String, usize)> {
        let re = Regex::new(r"(\n#{1,6}\s)|(\n\n\n+)").unwrap();
        let matches: Vec<_> = re.find_iter(text).collect();

        if matches.is_empty() {
            return vec![(text.to_string(), 0)];
        }

        let mut sections = Vec::new();
        let mut prev_end = 0;

        for m in &matches {
            if m.start() > prev_end {
                let section_text = text[prev_end..m.start()].trim().to_string();
                if !section_text.is_empty() {
                    sections.push((section_text, prev_end));
                }
            }
            prev_end = m.start();
        }

        // Last section
        let remaining = text[prev_end..].trim().to_string();
        if !remaining.is_empty() {
            sections.push((remaining, prev_end));
        }

        if sections.is_empty() {
            return vec![(text.to_string(), 0)];
        }

        sections
    }
}

impl Default for HierarchicalChunker {
    fn default() -> Self {
        Self::new(DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP)
    }
}

/// Determine if text should be chunked based on size and content.
pub fn should_chunk(text: &str, file_extension: Option<&str>) -> bool {
    let text_length = text.len();
    if text_length < 2000 {
        return false;
    }

    let code_extensions = [
        ".py", ".js", ".java", ".cpp", ".c", ".go", ".rs", ".ts", ".tsx", ".jsx",
    ];
    if let Some(ext) = file_extension {
        let ext_lower = ext.to_lowercase();
        if code_extensions.contains(&ext_lower.as_str()) {
            return text_length > 5000;
        }
    }

    let doc_extensions = [".md", ".txt", ".rst", ".tex", ".html", ".xml"];
    if let Some(ext) = file_extension {
        let ext_lower = ext.to_lowercase();
        if doc_extensions.contains(&ext_lower.as_str()) {
            return text_length > 2000;
        }
    }

    text_length > 3000
}

/// Calculate optimal chunk size and overlap based on text length and type.
pub fn calculate_chunk_size(file_extension: Option<&str>) -> (usize, usize) {
    let code_extensions = [
        ".py", ".js", ".java", ".cpp", ".c", ".go", ".rs", ".ts", ".tsx", ".jsx",
    ];
    if let Some(ext) = file_extension {
        let ext_lower = ext.to_lowercase();
        if code_extensions.contains(&ext_lower.as_str()) {
            return (400, 80);
        }
    }

    let doc_extensions = [".md", ".rst", ".tex", ".txt"];
    if let Some(ext) = file_extension {
        let ext_lower = ext.to_lowercase();
        if doc_extensions.contains(&ext_lower.as_str()) {
            return (600, 120);
        }
    }

    (DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recursive_chunker_short_text() {
        let chunker = RecursiveChunker::new(512, 100);
        let chunks = chunker.chunk("Hello, world!");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "Hello, world!");
    }

    #[test]
    fn test_hierarchical_chunker() {
        let chunker = HierarchicalChunker::default();
        let text = "# Section 1\n\nParagraph one about topic A.\n\nParagraph two about topic B.\n\n\n\n# Section 2\n\nAnother paragraph here.";
        let chunks = chunker.chunk(text);

        // Should have at least one section (level=0) and paragraph (level=1) chunks
        assert!(chunks.iter().any(|c| c.level == 0));
        assert!(chunks.iter().any(|c| c.level == 1));
    }

    #[test]
    fn test_should_chunk() {
        assert!(!should_chunk("short text", None));
        assert!(should_chunk(&"x".repeat(5001), Some(".py")));
        assert!(should_chunk(&"x".repeat(2001), Some(".md")));
    }
}
