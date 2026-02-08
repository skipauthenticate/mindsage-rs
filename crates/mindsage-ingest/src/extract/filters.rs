//! Document filter generation — port of Python's _generate_filters_heuristic().
//!
//! Classifies documents by content_type (conversation, code, note, etc.)
//! and domain (work, technical, personal, etc.) using pattern matching.

use regex::Regex;

use super::DocumentFilters;

/// Generate document content type and domain filters.
pub fn generate_filters(
    text: &str,
    source: Option<&str>,
    filename: Option<&str>,
) -> DocumentFilters {
    let text_lower = text.to_lowercase();
    let filename_lower = filename.unwrap_or("").to_lowercase();

    let content_type = classify_content_type(&text_lower, &filename_lower, source, text);
    let domain = classify_domain(&text_lower);

    DocumentFilters {
        content_type,
        domain,
    }
}

fn classify_content_type(
    text_lower: &str,
    filename_lower: &str,
    source: Option<&str>,
    raw_text: &str,
) -> String {
    // Source-based hints
    let mut content_type = match source {
        Some("chatgpt") => "conversation",
        Some("readwise") => "highlight",
        Some("github") => "code",
        Some("notion") => "note",
        Some("todoist") => "list",
        _ => "note",
    };

    // Pattern-based overrides
    let conversation_patterns = [
        r"\b(user|assistant|human|ai)\s*:",
        r"(?m)^(q:|a:|question:|answer:)",
        r"\[message\]|\[reply\]",
    ];
    if any_match(text_lower, &conversation_patterns) {
        content_type = "conversation";
    }

    // Code patterns (check on raw text to preserve case)
    let code_patterns = [
        r"```[\w]*\n",
        r"def\s+\w+\s*\(|function\s+\w+\s*\(|class\s+\w+",
        r"import\s+[\w.]+|from\s+[\w.]+\s+import",
    ];
    if any_match(raw_text, &code_patterns) {
        content_type = "code";
    }

    // Documentation
    if (filename_lower.contains("readme") || filename_lower.contains("doc"))
        && (text_lower.contains("documentation")
            || text_lower.contains("api reference")
            || text_lower.contains("getting started"))
    {
        content_type = "documentation";
    }

    // Meeting notes
    let meeting_patterns = [
        r"meeting\s+notes?|agenda|attendees|action\s+items",
        r"discussed|agreed|decided|next\s+steps",
    ];
    if any_match(text_lower, &meeting_patterns) {
        content_type = "meeting";
    }

    // Lists
    let list_patterns = [
        r"(?m)^\s*[-*]\s+\[[ x]\]",
        r"(?m)^\s*\d+\.\s+\w+",
    ];
    if any_match(text_lower, &list_patterns)
        || text_lower.contains("todo")
        || text_lower.contains("checklist")
    {
        content_type = "list";
    }

    // Email
    let email_patterns = [
        r"from:\s*\S+@\S+|to:\s*\S+@\S+|subject:",
        r"dear\s+\w+|regards,|sincerely,",
    ];
    if any_match(text_lower, &email_patterns) {
        content_type = "email";
    }

    content_type.to_string()
}

fn classify_domain(text_lower: &str) -> String {
    let domains: &[(&str, &[&str])] = &[
        ("work", &[
            "project", "deadline", "client", "meeting", "team", "report",
            "quarterly", "kpi", "revenue", "stakeholder", "deliverable",
            "sprint", "standup", "roadmap", "milestone",
        ]),
        ("technical", &[
            "code", "api", "database", "server", "deploy", "bug", "feature",
            "function", "class", "variable", "algorithm", "architecture",
            "docker", "kubernetes", "python", "javascript", "git",
        ]),
        ("learning", &[
            "learn", "study", "course", "tutorial", "lesson", "chapter",
            "concept", "understand", "example", "practice", "exercise",
        ]),
        ("creative", &[
            "idea", "story", "write", "draft", "creative", "inspiration",
            "brainstorm", "imagine", "design", "concept", "sketch",
        ]),
        ("personal", &[
            "journal", "diary", "today i", "feeling", "thought", "memory",
            "family", "friend", "weekend", "vacation", "birthday",
        ]),
        ("finance", &[
            "budget", "expense", "income", "investment", "savings", "tax",
            "payment", "invoice", "salary", "cost", "price", "money",
        ]),
    ];

    let mut best_domain = "personal";
    let mut best_score = 0;

    for &(domain, keywords) in domains {
        let score = keywords.iter().filter(|kw| text_lower.contains(**kw)).count();
        if score > best_score && score >= 2 {
            best_score = score;
            best_domain = domain;
        }
    }

    best_domain.to_string()
}

fn any_match(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| {
        Regex::new(p)
            .map(|re| re.is_match(text))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation() {
        let filters = generate_filters("user: hello\nassistant: hi there", None, None);
        assert_eq!(filters.content_type, "conversation");
    }

    #[test]
    fn test_code() {
        let filters = generate_filters(
            "def main():\n    print('hello')\n\nimport os",
            None,
            Some("script.py"),
        );
        assert_eq!(filters.content_type, "code");
    }

    #[test]
    fn test_work_domain() {
        let filters = generate_filters(
            "The project deadline is next sprint. Team meeting about deliverables and roadmap.",
            None,
            None,
        );
        assert_eq!(filters.domain, "work");
    }
}
