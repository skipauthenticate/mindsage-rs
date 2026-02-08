//! Hybrid resolver — BM25 + vector search with RRF fusion.

use mindsage_core::CapabilityTier;
use mindsage_store::SqliteStore;
use crate::types::*;

/// Hybrid resolver combining BM25 and vector search.
pub struct HybridResolver;

impl HybridResolver {
    /// Resolve a query using the appropriate strategy for the given tier.
    pub fn resolve(
        store: &SqliteStore,
        query: &ResolveQuery,
        tier: CapabilityTier,
    ) -> ResolveResult {
        let resolver_kind = query.resolver.unwrap_or_else(|| Self::select_resolver(tier));

        match resolver_kind {
            ResolverKind::Keyword => Self::keyword_resolve(store, query),
            ResolverKind::Entity => Self::entity_resolve(store, query),
            // Vector, Hybrid, Timeline, Answer all use BM25 for now (vector needs embeddings)
            _ => Self::keyword_resolve(store, query),
        }
    }

    /// Select the best resolver for the given capability tier.
    fn select_resolver(tier: CapabilityTier) -> ResolverKind {
        match tier {
            CapabilityTier::Base => ResolverKind::Keyword,
            CapabilityTier::Enhanced | CapabilityTier::Advanced | CapabilityTier::Full => {
                ResolverKind::Hybrid
            }
        }
    }

    /// BM25-only keyword search.
    fn keyword_resolve(store: &SqliteStore, query: &ResolveQuery) -> ResolveResult {
        let results = store.bm25_search(&query.query, 1, query.limit).unwrap_or_default();
        let items: Vec<ResolvedItem> = results
            .into_iter()
            .map(|r| ResolvedItem {
                id: r.chunk_id,
                text: r.text,
                score: r.score,
                source: String::new(),
                metadata: r.metadata,
                passage: None,
            })
            .collect();

        let total = items.len();
        ResolveResult {
            items,
            resolver_used: ResolverKind::Keyword,
            total_found: total,
            answer: None,
        }
    }

    /// Entity-focused search — boost results with matching entities.
    fn entity_resolve(store: &SqliteStore, query: &ResolveQuery) -> ResolveResult {
        let mut result = Self::keyword_resolve(store, query);
        result.resolver_used = ResolverKind::Entity;

        // Boost items whose text contains query terms
        let query_lower = query.query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();

        for item in &mut result.items {
            let text_lower = item.text.to_lowercase();
            let match_count = terms.iter().filter(|t| text_lower.contains(*t)).count();
            if match_count > 0 {
                item.score += 0.15 * (match_count as f64 / terms.len() as f64);
            }
        }

        result.items.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mindsage_store::{AddDocumentOptions, SqliteStore};

    fn test_store() -> (SqliteStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path(), 384).unwrap();
        (store, dir)
    }

    #[test]
    fn test_keyword_resolve_empty() {
        let (store, _dir) = test_store();
        let query = ResolveQuery {
            query: "test".into(),
            resolver: Some(ResolverKind::Keyword),
            limit: 10,
            filters: None,
        };
        let result = HybridResolver::resolve(&store, &query, CapabilityTier::Base);
        assert_eq!(result.items.len(), 0);
        assert_eq!(result.resolver_used, ResolverKind::Keyword);
    }

    /// Helper: add a document and a level=1 chunk (searchable via FTS).
    fn add_searchable_doc(store: &SqliteStore, text: &str) -> i64 {
        let doc_id = store
            .add_document(text, AddDocumentOptions::default())
            .unwrap();
        store
            .add_chunk(doc_id, text, 0, 1, None, Some(0), Some(text.len() as i32), None, None, None)
            .unwrap();
        doc_id
    }

    #[test]
    fn test_keyword_resolve_with_data() {
        let (store, _dir) = test_store();
        add_searchable_doc(&store, "Rust is a systems programming language focused on safety");
        add_searchable_doc(&store, "Python is great for data science and machine learning");

        let query = ResolveQuery {
            query: "Rust programming".into(),
            resolver: Some(ResolverKind::Keyword),
            limit: 10,
            filters: None,
        };
        let result = HybridResolver::resolve(&store, &query, CapabilityTier::Base);
        assert!(result.total_found > 0);
        assert!(result.items[0].text.contains("Rust"));
    }

    #[test]
    fn test_entity_resolve_boost() {
        let (store, _dir) = test_store();
        add_searchable_doc(&store, "Rust programming language is memory safe");
        add_searchable_doc(&store, "Programming in various languages");

        let query = ResolveQuery {
            query: "Rust".into(),
            resolver: Some(ResolverKind::Entity),
            limit: 10,
            filters: None,
        };
        let result = HybridResolver::resolve(&store, &query, CapabilityTier::Enhanced);
        assert_eq!(result.resolver_used, ResolverKind::Entity);

        if result.items.len() >= 2 {
            // Item mentioning "Rust" should score higher due to entity boost
            assert!(result.items[0].text.to_lowercase().contains("rust"));
        }
    }

    #[test]
    fn test_tier_selects_resolver() {
        let (store, _dir) = test_store();
        add_searchable_doc(&store, "Some test content");

        // Base tier → Keyword
        let query = ResolveQuery {
            query: "test".into(),
            resolver: None,
            limit: 10,
            filters: None,
        };
        let result = HybridResolver::resolve(&store, &query, CapabilityTier::Base);
        assert_eq!(result.resolver_used, ResolverKind::Keyword);
    }

    #[test]
    fn test_explicit_resolver_overrides_tier() {
        let (store, _dir) = test_store();
        add_searchable_doc(&store, "Content for search");

        let query = ResolveQuery {
            query: "search".into(),
            resolver: Some(ResolverKind::Entity),
            limit: 5,
            filters: None,
        };
        let result = HybridResolver::resolve(&store, &query, CapabilityTier::Base);
        assert_eq!(result.resolver_used, ResolverKind::Entity);
    }
}
