//! # Thread-Safe LRU Compiled Query Cache (`voyager-core::cache`)
//!
//! Provides an in-memory, thread-safe Least Recently Used (LRU) query compilation cache.
//! Eliminates repetitive AST traversals and string allocations for high-frequency queries
//! while preserving deterministic parameter keys (`$p0`, `$p1`) to maximize server-side
//! database execution plan cache hit rates.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use crate::visitor::CompiledQuery;

/// Snapshot of cache performance metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheMetrics {
    /// Number of cache lookup hits.
    pub hits: u64,
    /// Number of cache lookup misses.
    pub misses: u64,
    /// Current number of compiled query templates cached.
    pub len: usize,
    /// Maximum configured capacity of the cache.
    pub capacity: usize,
    /// Number of entries evicted due to capacity saturation.
    pub evictions: u64,
}

impl CacheMetrics {
    /// Returns the cache hit ratio between 0.0 and 1.0 (or 0.0 if no requests).
    pub fn hit_ratio(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Internal state of the LRU cache.
#[derive(Debug)]
struct LruState {
    map: HashMap<String, (CompiledQuery, u64)>, // key -> (query, generation)
    order: VecDeque<(String, u64)>,             // (key, generation)
    current_gen: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

/// Thread-safe Least Recently Used (LRU) query compilation cache.
#[derive(Debug)]
pub struct CompiledQueryCache {
    capacity: usize,
    inner: Mutex<LruState>,
}

impl CompiledQueryCache {
    /// Creates a new `CompiledQueryCache` with the specified maximum capacity.
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            inner: Mutex::new(LruState {
                map: HashMap::with_capacity(cap),
                order: VecDeque::with_capacity(cap),
                current_gen: 0,
                hits: 0,
                misses: 0,
                evictions: 0,
            }),
        }
    }

    /// Looks up a compiled query by its unique structural key.
    ///
    /// On a cache hit, updates the entry's recency and returns a clone of the `CompiledQuery`.
    pub fn get(&self, key: &str) -> Option<CompiledQuery> {
        let mut state = self.inner.lock().unwrap();
        state.current_gen += 1;
        let gen_id = state.current_gen;

        let result = if let Some((compiled, generation)) = state.map.get_mut(key) {
            *generation = gen_id;
            Some(compiled.clone())
        } else {
            None
        };

        if let Some(res) = result {
            state.hits += 1;
            state.order.push_back((key.to_string(), gen_id));
            Some(res)
        } else {
            state.misses += 1;
            None
        }
    }

    /// Stores a compiled query in the cache, evicting the least recently used entry if full.
    pub fn put(&self, key: String, query: CompiledQuery) {
        let mut state = self.inner.lock().unwrap();
        state.current_gen += 1;
        let gen_id = state.current_gen;

        let was_existing = if let Some((existing_query, existing_gen)) = state.map.get_mut(&key) {
            *existing_query = query.clone();
            *existing_gen = gen_id;
            true
        } else {
            false
        };

        if was_existing {
            state.order.push_back((key, gen_id));
            return;
        }

        // Evict expired entries if at or above capacity
        while state.map.len() >= self.capacity {
            if let Some((oldest_key, oldest_gen)) = state.order.pop_front() {
                if let Some((_, current_gen)) = state.map.get(&oldest_key)
                    && *current_gen == oldest_gen
                {
                    state.map.remove(&oldest_key);
                    state.evictions += 1;
                    break;
                }
            } else {
                break;
            }
        }

        state.order.push_back((key.clone(), gen_id));
        state.map.insert(key, (query, gen_id));

        // Periodic maintenance to prevent unbounded VecDeque growth from duplicates
        if state.order.len() > self.capacity * 4 {
            let drained: Vec<_> = state.order.drain(..).collect();
            let mut compact_order = VecDeque::with_capacity(state.map.len());
            let mut seen = HashMap::new();
            for (k, g) in drained.into_iter().rev() {
                if let Some((_, current_g)) = state.map.get(&k)
                    && *current_g == g
                    && !seen.contains_key(&k)
                {
                    seen.insert(k.clone(), ());
                    compact_order.push_front((k, g));
                }
            }
            state.order = compact_order;
        }
    }

    /// Returns the current number of cached compiled queries.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().map.len()
    }

    /// Returns `true` if the cache is currently empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the maximum capacity of the cache.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Clears all entries and resets the cache.
    pub fn clear(&self) {
        let mut state = self.inner.lock().unwrap();
        state.map.clear();
        state.order.clear();
    }

    /// Returns a snapshot of current cache telemetry metrics.
    pub fn metrics(&self) -> CacheMetrics {
        let state = self.inner.lock().unwrap();
        CacheMetrics {
            hits: state.hits,
            misses: state.misses,
            len: state.map.len(),
            capacity: self.capacity,
            evictions: state.evictions,
        }
    }
}

/// Global shared query compilation cache singleton with default capacity of 1,024 entries.
pub fn global_query_cache() -> &'static CompiledQueryCache {
    static CACHE: OnceLock<CompiledQueryCache> = OnceLock::new();
    CACHE.get_or_init(|| CompiledQueryCache::new(1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_cache_lru_eviction() {
        let cache = CompiledQueryCache::new(2);
        assert_eq!(cache.len(), 0);

        let q1 = CompiledQuery::new("MATCH (n) RETURN n".into(), HashMap::new());
        let q2 = CompiledQuery::new("MATCH (p:Person) RETURN p".into(), HashMap::new());
        let q3 = CompiledQuery::new("MATCH (m:Movie) RETURN m".into(), HashMap::new());

        cache.put("k1".into(), q1.clone());
        cache.put("k2".into(), q2.clone());
        assert_eq!(cache.len(), 2);

        // Access k1, making k2 the LRU
        assert_eq!(cache.get("k1"), Some(q1));

        // Insert k3; k2 should be evicted
        cache.put("k3".into(), q3.clone());
        assert_eq!(cache.len(), 2);

        assert!(cache.get("k1").is_some());
        assert!(cache.get("k2").is_none());
        assert!(cache.get("k3").is_some());

        let m = cache.metrics();
        assert_eq!(m.hits, 3);
        assert_eq!(m.misses, 1);
        assert_eq!(m.evictions, 1);
        assert!(m.hit_ratio() > 0.7);
    }
}
