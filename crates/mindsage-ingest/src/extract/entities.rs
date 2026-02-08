//! Heuristic entity extraction — port of Python's _extract_entities_heuristic()
//! and _extract_structured_metadata_heuristic().

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

use super::StructuredMetadata;

/// Split text into sentences without lookbehind.
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
    let s = text[start..].trim();
    if !s.is_empty() {
        sentences.push(s);
    }
    sentences
}

/// Extract key entities using heuristics (capitalized words, technical terms, quoted terms).
pub fn extract_entities(text: &str, max_entities: usize) -> Vec<String> {
    let mut entities: HashSet<String> = HashSet::new();

    // Capitalized words (proper nouns) — skip sentence-start words
    for sentence in split_sentences(text) {
        let words: Vec<&str> = sentence.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            if i > 0 && word.len() > 2 {
                let cleaned: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
                if !cleaned.is_empty()
                    && cleaned.chars().next().unwrap().is_uppercase()
                    && !cleaned.chars().all(|c| c.is_uppercase())
                {
                    entities.insert(cleaned);
                }
            }
        }
    }

    // Technical terms: camelCase, snake_case, CONSTANTS
    let patterns = [
        r"\b[a-z]+[A-Z][a-zA-Z]*\b",   // camelCase
        r"\b[a-z]+_[a-z_]+\b",          // snake_case
        r"\b[A-Z][A-Z_]{2,}\b",         // CONSTANTS
    ];
    for pattern in &patterns {
        let re = Regex::new(pattern).unwrap();
        for m in re.find_iter(text) {
            entities.insert(m.as_str().to_string());
        }
    }

    // Quoted terms
    let quoted_re = Regex::new(r#"["']([^"']{2,30})["']"#).unwrap();
    for cap in quoted_re.captures_iter(text) {
        if let Some(m) = cap.get(1) {
            entities.insert(m.as_str().to_string());
        }
    }

    // Sort by frequency in text (most frequent first)
    let text_lower = text.to_lowercase();
    let mut entity_list: Vec<String> = entities.into_iter().collect();
    entity_list.sort_by(|a, b| {
        let count_a = text_lower.matches(&a.to_lowercase()).count();
        let count_b = text_lower.matches(&b.to_lowercase()).count();
        count_b.cmp(&count_a)
    });
    entity_list.truncate(max_entities);
    entity_list
}

// Known technology keywords for structured metadata extraction
static TECH_KEYWORDS: Lazy<Vec<&'static str>> = Lazy::new(|| {
    vec![
        "Python", "JavaScript", "TypeScript", "Java", "C++", "C#", "Go", "Rust",
        "Ruby", "PHP", "Swift", "Kotlin", "React", "Angular", "Vue", "Node.js",
        "Django", "Flask", "FastAPI", "Spring", "Rails", "PostgreSQL", "MySQL",
        "MongoDB", "Redis", "Elasticsearch", "SQLite", "Docker", "Kubernetes",
        "AWS", "Azure", "GCP", "Terraform", "Ansible", "Git", "GitHub", "GitLab",
        "Jenkins", "TensorFlow", "PyTorch", "Keras", "REST", "GraphQL", "gRPC",
        "WebSocket", "HTTP", "API", "Linux", "Windows", "macOS", "Ubuntu",
        "OAuth", "JWT", "SSL", "TLS", "Kafka", "RabbitMQ", "Jira", "Slack",
    ]
});

/// Extract structured metadata using regex patterns.
pub fn extract_structured_metadata(text: &str, max_per_category: usize) -> StructuredMetadata {
    StructuredMetadata {
        dates: extract_dates(text, max_per_category),
        times: extract_times(text, max_per_category),
        temporal_refs: extract_temporal_refs(text, max_per_category),
        quantities: extract_quantities(text, max_per_category),
        technologies: extract_technologies(text, max_per_category),
        activities: extract_activities(text, max_per_category),
        persons: extract_persons(text, max_per_category),
        organizations: extract_organizations(text, max_per_category),
        locations: Vec::new(), // Would need NER for reliable location extraction
    }
}

fn extract_dates(text: &str, max: usize) -> Vec<String> {
    let patterns = [
        r"\b(?:January|February|March|April|May|June|July|August|September|October|November|December)\s+\d{1,2}(?:st|nd|rd|th)?,?\s*\d{4}\b",
        r"\b(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\.?\s+\d{1,2}(?:st|nd|rd|th)?,?\s*\d{4}\b",
        r"\b\d{1,2}[-/]\d{1,2}[-/]\d{2,4}\b",
        r"\b\d{4}[-/]\d{1,2}[-/]\d{1,2}\b",
        r"\bQ[1-4]\s*\d{4}\b",
    ];
    extract_with_patterns(text, &patterns, max)
}

fn extract_times(text: &str, max: usize) -> Vec<String> {
    let patterns = [
        r"\b\d{1,2}:\d{2}\s*(?:AM|PM|am|pm)?\b",
        r"\b\d{1,2}\s*(?:AM|PM|am|pm)\b",
    ];
    extract_with_patterns(text, &patterns, max)
}

fn extract_temporal_refs(text: &str, max: usize) -> Vec<String> {
    let patterns = [
        r"\b(?i:last|next|this|previous|upcoming)\s+(?i:week|month|year|quarter|day|monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b",
        r"\b(?i:yesterday|today|tomorrow)\b",
        r"\b(?i:recently|soon|earlier|later)\b",
    ];
    extract_with_patterns(text, &patterns, max)
}

fn extract_quantities(text: &str, max: usize) -> Vec<String> {
    let patterns = [
        r"\$[\d,]+(?:\.\d{2})?\s*(?:million|billion|M|B|K)?\b",
        r"\b\d+(?:,\d{3})*(?:\.\d+)?\s*(?:users|customers|employees|people|items|orders|requests|GB|MB|KB|TB|ms|seconds|minutes|hours|days|%|percent)\b",
        r"\b\d+(?:\.\d+)?[xX]\b",
    ];
    extract_with_patterns(text, &patterns, max)
}

fn extract_technologies(text: &str, max: usize) -> Vec<String> {
    let mut techs = Vec::new();
    for &tech in TECH_KEYWORDS.iter() {
        let pattern = format!(r"\b{}\b", regex::escape(tech));
        if let Ok(re) = Regex::new(&pattern) {
            if re.is_match(text) {
                techs.push(tech.to_string());
            }
        }
    }
    // Also find camelCase and snake_case terms
    let camel = Regex::new(r"\b[a-z]+(?:[A-Z][a-z]+)+\b").unwrap();
    let snake = Regex::new(r"\b[a-z]+(?:_[a-z]+)+\b").unwrap();
    for m in camel.find_iter(text) {
        let s = m.as_str().to_string();
        if s.len() > 3 && !techs.contains(&s) {
            techs.push(s);
        }
    }
    for m in snake.find_iter(text) {
        let s = m.as_str().to_string();
        if s.len() > 3 && !techs.contains(&s) {
            techs.push(s);
        }
    }
    techs.truncate(max);
    techs
}

fn extract_activities(text: &str, max: usize) -> Vec<String> {
    let patterns = [
        r"\b(?i:deployed|released|launched|shipped|implemented|developed|built|created|designed|reviewed|analyzed|tested|fixed|updated|migrated|refactored|optimized|integrated|configured|monitored|debugged|resolved|completed|approved|merged|committed)\b",
        r"\b(?i:deploying|releasing|launching|shipping|implementing|developing|building|creating|designing|reviewing|analyzing|testing|fixing|updating|migrating|refactoring|optimizing|integrating|configuring|monitoring|debugging|resolving|completing|approving|merging|committing)\b",
    ];
    let mut activities = extract_with_patterns(text, &patterns, max * 2);
    activities.iter_mut().for_each(|a| *a = a.to_lowercase());
    activities.dedup();
    activities.truncate(max);
    activities
}

fn extract_persons(text: &str, max: usize) -> Vec<String> {
    // Look for title + name patterns
    let title_re = Regex::new(r"\b(?:Mr|Mrs|Ms|Dr|Prof)\.\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+)?)").unwrap();
    let mut persons: Vec<String> = title_re
        .captures_iter(text)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();

    // Two consecutive capitalized words (likely a name) — not at sentence start
    // Can't use lookbehind, so find all two-cap-word sequences and filter
    let name_re = Regex::new(r"\b([A-Z][a-z]+\s+[A-Z][a-z]+)\b").unwrap();
    for m in name_re.find_iter(text) {
        let name = m.as_str().to_string();
        // Skip if at very start of text (likely sentence start, not a name)
        if m.start() > 2 && !persons.contains(&name) {
            persons.push(name);
        }
    }

    persons.truncate(max);
    persons
}

fn extract_organizations(text: &str, max: usize) -> Vec<String> {
    let org_re = Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)*)\s+(?:Inc\.|Corp\.|LLC|Ltd\.|Co\.)").unwrap();
    let mut orgs: Vec<String> = org_re
        .captures_iter(text)
        .filter_map(|cap| cap.get(0).map(|m| m.as_str().to_string()))
        .collect();
    orgs.truncate(max);
    orgs
}

/// Helper: extract matches from multiple regex patterns, deduplicated.
fn extract_with_patterns(text: &str, patterns: &[&str], max: usize) -> Vec<String> {
    let mut results: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            for m in re.find_iter(text) {
                let s = m.as_str().to_string();
                if seen.insert(s.clone()) {
                    results.push(s);
                }
            }
        }
    }
    results.truncate(max);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_entities() {
        let text = "John Smith from Google visited the React conference. \
                     He discussed the new API with fetchData function.";
        let entities = extract_entities(text, 10);
        assert!(entities.iter().any(|e| e.contains("John") || e.contains("Smith")));
        assert!(entities.iter().any(|e| e == "Google" || e == "React"));
        assert!(entities.iter().any(|e| e == "fetchData"));
    }

    #[test]
    fn test_extract_dates() {
        let text = "Meeting on January 15, 2025 and follow-up on 2025-02-01";
        let dates = extract_dates(text, 5);
        assert!(dates.len() >= 2);
    }

    #[test]
    fn test_extract_technologies() {
        let text = "We deployed the Python API on Docker with PostgreSQL database";
        let techs = extract_technologies(text, 10);
        assert!(techs.contains(&"Python".to_string()));
        assert!(techs.contains(&"Docker".to_string()));
        assert!(techs.contains(&"PostgreSQL".to_string()));
    }
}
