//! LRU query cache for embedding results.
//!
//! Avoids re-computing embeddings for repeated search queries.
//! Default: 1000 entries, 1-hour TTL.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ndarray::Array1;
use parking_lot::Mutex;

/// Cached embedding entry with timestamp.
struct CacheEntry {
    embedding: Array1<f32>,
    inserted_at: Instant,
}

/// Thread-safe LRU query cache for embeddings.
pub struct QueryCache {
    inner: Mutex<CacheInner>,
}

struct CacheInner {
    entries: HashMap<String, CacheEntry>,
    order: Vec<String>,
    max_size: usize,
    ttl: Duration,
}

impl QueryCache {
    /// Create a new cache with the given capacity and TTL.
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                entries: HashMap::with_capacity(max_size),
                order: Vec::with_capacity(max_size),
                max_size,
                ttl,
            }),
        }
    }

    /// Create a cache with default settings (1000 entries, 1hr TTL).
    pub fn default_cache() -> Self {
        Self::new(1000, Duration::from_secs(3600))
    }

    /// Get a cached embedding. Returns None on miss or expired entry.
    pub fn get(&self, query: &str) -> Option<Array1<f32>> {
        let mut inner = self.inner.lock();

        let expired = inner
            .entries
            .get(query)
            .map(|e| e.inserted_at.elapsed() >= inner.ttl);

        match expired {
            Some(false) => {
                // Clone embedding before mutating order
                let embedding = inner.entries.get(query).unwrap().embedding.clone();
                if let Some(pos) = inner.order.iter().position(|k| k == query) {
                    let key = inner.order.remove(pos);
                    inner.order.push(key);
                }
                Some(embedding)
            }
            Some(true) => {
                // Expired — remove
                let key = query.to_string();
                inner.entries.remove(&key);
                inner.order.retain(|k| k != &key);
                None
            }
            None => None,
        }
    }

    /// Insert an embedding into the cache.
    pub fn put(&self, query: String, embedding: Array1<f32>) {
        let mut inner = self.inner.lock();

        // If already present, update and move to end
        if inner.entries.contains_key(&query) {
            inner.entries.insert(
                query.clone(),
                CacheEntry {
                    embedding,
                    inserted_at: Instant::now(),
                },
            );
            inner.order.retain(|k| k != &query);
            inner.order.push(query);
            return;
        }

        // Evict oldest if at capacity
        while inner.entries.len() >= inner.max_size && !inner.order.is_empty() {
            let oldest = inner.order.remove(0);
            inner.entries.remove(&oldest);
        }

        inner.order.push(query.clone());
        inner.entries.insert(
            query,
            CacheEntry {
                embedding,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Number of entries in the cache.
    pub fn len(&self) -> usize {
        self.inner.lock().entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all entries.
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.entries.clear();
        inner.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn test_cache_hit_and_miss() {
        let cache = QueryCache::new(10, Duration::from_secs(3600));
        assert!(cache.get("hello").is_none());

        cache.put("hello".into(), array![1.0, 2.0, 3.0]);
        let hit = cache.get("hello");
        assert!(hit.is_some());
        assert_eq!(hit.unwrap(), array![1.0, 2.0, 3.0]);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cache_eviction() {
        let cache = QueryCache::new(2, Duration::from_secs(3600));
        cache.put("a".into(), array![1.0]);
        cache.put("b".into(), array![2.0]);
        assert_eq!(cache.len(), 2);

        // Adding third should evict "a"
        cache.put("c".into(), array![3.0]);
        assert_eq!(cache.len(), 2);
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn test_cache_ttl_expiry() {
        let cache = QueryCache::new(10, Duration::from_millis(1));
        cache.put("ephemeral".into(), array![1.0]);

        // Sleep past TTL
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.get("ephemeral").is_none());
    }
}
