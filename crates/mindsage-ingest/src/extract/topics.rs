//! Keyword-based topic classification — port of Python's _classify_by_keywords().

use super::stemmer::simple_stem;
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Topic classification result.
pub struct TopicResult {
    pub topics: Vec<String>,
    pub primary_topic: String,
    pub confidence: f64,
}

/// Predefined topic list.
pub const DEFAULT_TOPICS: &[&str] = &[
    "health",
    "finance",
    "work",
    "personal",
    "social",
    "legal",
    "travel",
    "education",
    "programming",
    "sports",
    "technology",
    "shopping",
    "family",
    "general",
];

/// Keyword → topic mapping.
static KEYWORD_MAP: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    // Sports
    for kw in &[
        "basketball", "football", "soccer", "baseball", "tennis", "golf", "hockey",
        "game", "team", "player", "score", "match", "championship", "athlete",
        "winning", "overtime",
    ] {
        m.insert(*kw, "sports");
    }
    // Technology
    for kw in &[
        "smartphone", "computer", "laptop", "software", "hardware", "app",
        "processor", "camera", "device", "digital", "internet", "wifi",
    ] {
        m.insert(*kw, "technology");
    }
    // Shopping
    for kw in &[
        "bought", "purchased", "store", "mall", "sale", "discount", "price",
        "cart", "order", "delivery", "retail", "shop", "dress", "shoes",
        "clothes", "purchase",
    ] {
        m.insert(*kw, "shopping");
    }
    // Health / Medical
    for kw in &[
        "doctor", "medicine", "prescription", "hospital", "treatment", "diagnosis",
        "symptom", "patient", "clinic", "nurse", "surgery", "antibiotic",
        "antibiotics", "prescribed", "infection", "therapy", "medical", "dental",
        "dentist", "fitness", "exercise", "diet", "wellness", "nutrition",
        "workout", "gym",
    ] {
        m.insert(*kw, "health");
    }
    // Family
    for kw in &[
        "parents", "children", "kids", "siblings", "relatives", "grandparents",
        "cousins", "reunion", "mother", "father", "brother", "sister",
    ] {
        m.insert(*kw, "family");
    }
    // Programming
    for kw in &[
        "code", "python", "javascript", "function", "class", "api", "debug",
        "compile", "algorithm", "def", "return", "import", "variable", "loop",
        "array", "programming", "coding", "developer", "quicksort", "recursion",
        "recursive", "select", "sql", "database", "query", "table", "insert",
    ] {
        m.insert(*kw, "programming");
    }
    // Finance
    for kw in &[
        "money", "budget", "investment", "bank", "savings", "loan", "credit", "tax",
    ] {
        m.insert(*kw, "finance");
    }
    // Education
    for kw in &[
        "school", "university", "college", "learning", "student", "teacher",
        "course", "study", "exam",
    ] {
        m.insert(*kw, "education");
    }
    // Travel
    for kw in &[
        "vacation", "trip", "flight", "hotel", "destination", "airport", "tourism",
    ] {
        m.insert(*kw, "travel");
    }
    // Legal
    for kw in &[
        "lawyer", "court", "law", "contract", "attorney", "lawsuit", "legal",
    ] {
        m.insert(*kw, "legal");
    }
    // Work
    for kw in &[
        "job", "office", "meeting", "project", "deadline", "colleague", "boss", "career",
    ] {
        m.insert(*kw, "work");
    }
    // Personal
    for kw in &[
        "diary", "journal", "thoughts", "feelings", "myself", "private",
        "personal", "reflection", "friends",
    ] {
        m.insert(*kw, "personal");
    }
    // Social
    for kw in &[
        "party", "socializing", "hangout", "gathering", "community",
        "networking", "social",
    ] {
        m.insert(*kw, "social");
    }
    m
});

/// Pre-computed stemmed keyword map.
static STEMMED_MAP: Lazy<HashMap<String, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    for (&keyword, &topic) in KEYWORD_MAP.iter() {
        let stemmed = simple_stem(keyword);
        if stemmed != keyword {
            m.insert(stemmed, topic);
        }
    }
    m
});

/// Classify text by matching keywords (with stemming fallback).
pub fn classify_by_keywords(text: &str) -> TopicResult {
    let text_lower = text.to_lowercase();
    let predefined: std::collections::HashSet<&str> =
        DEFAULT_TOPICS.iter().copied().collect();

    let mut topic_counts: HashMap<&str, usize> = HashMap::new();

    // Split on whitespace and punctuation
    for word in text_lower.split(|c: char| c.is_whitespace() || ",.;:!?()[]{}\"'/\\".contains(c)) {
        let word = word.trim();
        if word.len() < 2 {
            continue;
        }

        let topic = if let Some(&t) = KEYWORD_MAP.get(word) {
            Some(t)
        } else {
            let stemmed = simple_stem(word);
            STEMMED_MAP.get(stemmed.as_str()).copied()
        };

        if let Some(t) = topic {
            if predefined.contains(t) {
                *topic_counts.entry(t).or_insert(0) += 1;
            }
        }
    }

    if !topic_counts.is_empty() {
        let mut sorted: Vec<(&str, usize)> = topic_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let topics: Vec<String> = sorted.iter().take(3).map(|(t, _)| t.to_string()).collect();
        let primary = topics[0].clone();
        TopicResult {
            topics,
            primary_topic: primary,
            confidence: 0.7,
        }
    } else {
        TopicResult {
            topics: vec!["general".to_string()],
            primary_topic: "general".to_string(),
            confidence: 0.3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_health() {
        let result = classify_by_keywords("I went to the gym for my workout today");
        assert_eq!(result.primary_topic, "health");
    }

    #[test]
    fn test_classify_programming() {
        let result = classify_by_keywords("debugging the Python function with recursion");
        assert_eq!(result.primary_topic, "programming");
    }

    #[test]
    fn test_classify_unknown() {
        let result = classify_by_keywords("lorem ipsum dolor sit amet");
        assert_eq!(result.primary_topic, "general");
    }
}
