//! PII detection and anonymization using regex patterns.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use uuid::Uuid;

/// Types of PII that can be detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PiiType {
    Email,
    Phone,
    Ssn,
    CreditCard,
    IpAddress,
    Url,
}

impl PiiType {
    pub fn label(&self) -> &'static str {
        match self {
            PiiType::Email => "EMAIL",
            PiiType::Phone => "PHONE",
            PiiType::Ssn => "SSN",
            PiiType::CreditCard => "CREDIT_CARD",
            PiiType::IpAddress => "IP_ADDRESS",
            PiiType::Url => "URL",
        }
    }
}

/// A detected PII entity with position information.
#[derive(Debug, Clone, Serialize)]
pub struct PiiEntity {
    #[serde(rename = "type")]
    pub pii_type: PiiType,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Result of anonymizing text.
#[derive(Debug, Clone, Serialize)]
pub struct AnonymizationResult {
    pub text: String,
    pub entities: Vec<PiiEntity>,
    #[serde(rename = "tokenCount")]
    pub token_count: usize,
}

/// PII detector using compiled regex patterns.
pub struct PiiDetector {
    patterns: Vec<(PiiType, &'static Regex)>,
    /// Session mapping: token_id → original text (for de-anonymization).
    tokens: Mutex<HashMap<String, (String, PiiType)>>,
}

// Compiled regex patterns (compiled once, reused).
static EMAIL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());
static PHONE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}").unwrap()
});
static SSN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap());
static CC_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(?:\d{4}[-\s]?){3}\d{4}\b").unwrap());
static IP_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\b")
        .unwrap()
});
static URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"https?://[^\s<>"']+"#).unwrap());

impl PiiDetector {
    /// Create a new PII detector.
    pub fn new() -> Self {
        Self {
            patterns: vec![
                (PiiType::Email, &EMAIL_RE),
                (PiiType::Ssn, &SSN_RE),
                (PiiType::CreditCard, &CC_RE),
                (PiiType::Phone, &PHONE_RE),
                (PiiType::IpAddress, &IP_RE),
                (PiiType::Url, &URL_RE),
            ],
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Detect PII entities in text.
    pub fn detect(&self, text: &str) -> Vec<PiiEntity> {
        let mut entities = Vec::new();

        for (pii_type, regex) in &self.patterns {
            for m in regex.find_iter(text) {
                entities.push(PiiEntity {
                    pii_type: *pii_type,
                    start: m.start(),
                    end: m.end(),
                    text: m.as_str().to_string(),
                });
            }
        }

        // Sort by position, longest match first for overlapping
        entities.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

        // Remove overlapping entities (keep first/longest)
        let mut filtered = Vec::new();
        let mut last_end = 0;
        for entity in entities {
            if entity.start >= last_end {
                last_end = entity.end;
                filtered.push(entity);
            }
        }

        filtered
    }

    /// Anonymize text by replacing PII with tokens.
    pub fn anonymize(&self, text: &str) -> AnonymizationResult {
        let entities = self.detect(text);
        if entities.is_empty() {
            return AnonymizationResult {
                text: text.to_string(),
                entities: Vec::new(),
                token_count: 0,
            };
        }

        let mut result = String::new();
        let mut last_end = 0;
        let mut tokens = self.tokens.lock();
        let mut token_count = 0;

        for entity in &entities {
            result.push_str(&text[last_end..entity.start]);

            let token_id = Uuid::new_v4().to_string()[..8].to_string();
            let replacement = format!("<PII:{}:{}>", entity.pii_type.label(), token_id);

            tokens.insert(
                token_id,
                (entity.text.clone(), entity.pii_type),
            );

            result.push_str(&replacement);
            last_end = entity.end;
            token_count += 1;
        }
        result.push_str(&text[last_end..]);

        AnonymizationResult {
            text: result,
            entities: entities.to_vec(),
            token_count,
        }
    }

    /// De-anonymize text by restoring PII tokens to original values.
    pub fn deanonymize(&self, text: &str) -> String {
        let tokens = self.tokens.lock();
        let mut result = text.to_string();

        for (token_id, (original, pii_type)) in tokens.iter() {
            let placeholder = format!("<PII:{}:{}>", pii_type.label(), token_id);
            result = result.replace(&placeholder, original);
        }

        result
    }

    /// Get PII detection status (counts per type).
    pub fn get_status(&self) -> HashMap<String, usize> {
        let tokens = self.tokens.lock();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (_, (_, pii_type)) in tokens.iter() {
            *counts.entry(pii_type.label().to_string()).or_insert(0) += 1;
        }
        counts
    }

    /// Clear all stored tokens.
    pub fn clear_tokens(&self) {
        self.tokens.lock().clear();
    }
}

impl Default for PiiDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_email() {
        let detector = PiiDetector::new();
        let entities = detector.detect("Contact me at user@example.com for details.");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].pii_type, PiiType::Email);
        assert_eq!(entities[0].text, "user@example.com");
    }

    #[test]
    fn test_detect_phone() {
        let detector = PiiDetector::new();
        let entities = detector.detect("Call me at (555) 123-4567 today.");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].pii_type, PiiType::Phone);
    }

    #[test]
    fn test_detect_ssn() {
        let detector = PiiDetector::new();
        let entities = detector.detect("My SSN is 123-45-6789.");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].pii_type, PiiType::Ssn);
        assert_eq!(entities[0].text, "123-45-6789");
    }

    #[test]
    fn test_detect_ip() {
        let detector = PiiDetector::new();
        let entities = detector.detect("Server at 192.168.1.100 is down.");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].pii_type, PiiType::IpAddress);
    }

    #[test]
    fn test_anonymize_and_deanonymize() {
        let detector = PiiDetector::new();
        let original = "Email me at test@example.com about the issue.";
        let anonymized = detector.anonymize(original);
        assert!(anonymized.text.contains("<PII:EMAIL:"));
        assert!(!anonymized.text.contains("test@example.com"));
        assert_eq!(anonymized.token_count, 1);

        let restored = detector.deanonymize(&anonymized.text);
        assert_eq!(restored, original);
    }

    #[test]
    fn test_multiple_pii() {
        let detector = PiiDetector::new();
        let text = "Email: user@test.com, SSN: 123-45-6789";
        let entities = detector.detect(text);
        assert_eq!(entities.len(), 2);
    }
}
