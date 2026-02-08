//! Heuristic key sentence extraction — port of Python's _extract_key_sentences_heuristic().
//!
//! Scores sentences by position, length, indicator words, and information density.

use regex::Regex;

/// Split text into sentences (no lookbehind — Rust regex doesn't support it).
fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if (b == b'.' || b == b'!' || b == b'?')
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_whitespace()
        {
            let s = text[start..=i].trim();
            if !s.is_empty() {
                sentences.push(s);
            }
            start = i + 1;
        }
    }
    // Remainder
    let s = text[start..].trim();
    if !s.is_empty() {
        sentences.push(s);
    }
    sentences
}

/// Extract key sentences from text using scoring heuristics.
pub fn extract_key_sentences(text: &str, max_sentences: usize) -> Vec<String> {
    let sentences: Vec<&str> = split_sentences(text)
        .into_iter()
        .filter(|s| s.len() > 20)
        .collect();

    if sentences.is_empty() {
        let truncated = if text.len() > 500 { &text[..500] } else { text };
        return vec![truncated.to_string()];
    }

    let total = sentences.len();

    // Score each sentence
    let mut scored: Vec<(i32, usize, &str)> = sentences
        .iter()
        .enumerate()
        .map(|(i, &sent)| {
            let mut score = 0i32;

            // Position bonus: first few and last few sentences
            if i < 3 {
                score += (3 - i) as i32;
            }
            if total > 5 && i >= total.saturating_sub(2) {
                score += 2;
            }

            // Medium length bonus
            let len = sent.len();
            if len > 50 && len < 200 {
                score += 2;
            } else if len >= 200 {
                score += 1;
            }

            // Key indicator words
            let sent_lower = sent.to_lowercase();
            let key_words = [
                "important", "key", "main", "conclusion", "summary", "result",
                "finding", "therefore", "thus", "shows", "demonstrates",
                "reveals", "significant", "notably",
            ];
            let matches = key_words.iter().filter(|kw| sent_lower.contains(**kw)).count();
            score += (matches * 2) as i32;

            // Information density: capitalized words (proper nouns)
            let words: Vec<&str> = sent.split_whitespace().collect();
            let capitalized = words
                .iter()
                .skip(1) // skip first word (sentence start)
                .filter(|w| {
                    w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                        && !w.chars().all(|c| c.is_uppercase()) // skip ALL_CAPS
                })
                .count();
            score += capitalized.min(3) as i32;

            // Technical terms bonus
            if sent.contains('_') || sent.bytes().any(|b| b.is_ascii_lowercase()) && sent.bytes().any(|b| b.is_ascii_uppercase()) {
                // Very rough camelCase / snake_case detection
                let camel_re = Regex::new(r"\b[a-z]+[A-Z][a-zA-Z]*\b").unwrap();
                let snake_re = Regex::new(r"\b[a-z]+_[a-z]+\b").unwrap();
                if camel_re.is_match(sent) {
                    score += 1;
                }
                if snake_re.is_match(sent) {
                    score += 1;
                }
            }

            (score, i, sent)
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));

    // For large documents, ensure position diversity
    if total > 10 && max_sentences >= 3 {
        let third = total / 3;
        let mut selected: Vec<String> = Vec::new();

        // Best from each third
        for range in [(0, third), (third, 2 * third), (2 * third, total)] {
            let best = scored
                .iter()
                .find(|(_, i, sent)| {
                    *i >= range.0 && *i < range.1 && !selected.contains(&sent.to_string())
                });
            if let Some((_, _, sent)) = best {
                selected.push(sent.to_string());
            }
        }

        // Fill remaining with highest scored
        for (_, _, sent) in &scored {
            if selected.len() >= max_sentences {
                break;
            }
            let s = sent.to_string();
            if !selected.contains(&s) {
                selected.push(s);
            }
        }

        selected.truncate(max_sentences);
        selected
    } else {
        scored
            .iter()
            .take(max_sentences)
            .map(|(_, _, s)| s.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_key_sentences() {
        let text = "This is the first important sentence about the project. \
                     The second sentence has some details. \
                     The conclusion shows significant results in the analysis. \
                     This is just filler text that nobody cares about.";
        let result = extract_key_sentences(text, 2);
        assert_eq!(result.len(), 2);
        // The sentence with "important" and "conclusion"/"significant" should score higher
    }

    #[test]
    fn test_short_text() {
        let result = extract_key_sentences("Hello world", 3);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Hello world");
    }
}
