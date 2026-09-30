use std::sync::RwLock;
use std::time::{Duration, Instant};

use lru::LruCache;
use std::num::NonZeroUsize;

use crate::ports::cache::ListingCache;

/// Longest time an entry may live; larger TTLs are capped instead of letting
/// `Instant + ttl` overflow and abort the process (`panic = "abort"`).
const MAX_ENTRY_TTL: Duration = Duration::from_hours(30 * 24);

struct CacheEntry {
    value: String,
    expires_at: Instant,
}

pub struct MemoryCache {
    inner: RwLock<LruCache<String, CacheEntry>>,
}

/// Capacity used when `max_entries` is 0. `Config::validate` rejects 0, but
/// `MemoryCache::new` is public and must not panic.
const FALLBACK_CAPACITY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(99);

impl MemoryCache {
    pub fn new(max_entries: usize) -> Self {
        let cap = NonZeroUsize::new(max_entries).unwrap_or_else(|| {
            tracing::warn!("Cache max_entries was 0, defaulting to {FALLBACK_CAPACITY}");
            FALLBACK_CAPACITY
        });
        Self {
            inner: RwLock::new(LruCache::new(cap)),
        }
    }
}

impl ListingCache for MemoryCache {
    fn get(&self, key: &str) -> Option<String> {
        let mut cache = self.inner.write().map_or_else(
            |_| {
                tracing::error!("Cache lock poisoned on get('{key}'), returning miss");
                None
            },
            Some,
        )?;
        let entry = cache.get(key)?;
        if Instant::now() > entry.expires_at {
            cache.pop(key);
            return None;
        }
        Some(entry.value.clone())
    }

    fn set(&self, key: &str, value: &str, ttl: Duration) {
        let now = Instant::now();
        let expires_at = now.checked_add(ttl).unwrap_or_else(|| now + MAX_ENTRY_TTL);
        if let Ok(mut cache) = self.inner.write() {
            cache.put(
                key.to_string(),
                CacheEntry {
                    value: value.to_string(),
                    expires_at,
                },
            );
        } else {
            tracing::error!("Cache lock poisoned on set('{key}'), skipping write");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_returns_none_for_missing_key() {
        let cache = MemoryCache::new(10);
        assert!(cache.get("missing").is_none());
    }

    #[test]
    fn set_then_get_returns_value() {
        let cache = MemoryCache::new(10);
        cache.set("key1", "value1", Duration::from_mins(1));
        assert_eq!(cache.get("key1"), Some("value1".to_string()));
    }

    #[test]
    fn expired_entry_returns_none() {
        let cache = MemoryCache::new(10);
        cache.set("key1", "value1", Duration::from_millis(0));
        // Entry expires immediately
        std::thread::sleep(Duration::from_millis(1));
        assert!(cache.get("key1").is_none());
    }

    #[test]
    fn cache_eviction_at_capacity() {
        let cache = MemoryCache::new(2);
        cache.set("a", "1", Duration::from_mins(1));
        cache.set("b", "2", Duration::from_mins(1));
        cache.set("c", "3", Duration::from_mins(1));
        // "a" should be evicted (LRU)
        assert!(cache.get("a").is_none());
        assert_eq!(cache.get("b"), Some("2".to_string()));
        assert_eq!(cache.get("c"), Some("3".to_string()));
    }

    #[test]
    fn get_refreshes_recency_so_least_recently_read_is_evicted() {
        let cache = MemoryCache::new(2);
        cache.set("a", "1", Duration::from_mins(1));
        cache.set("b", "2", Duration::from_mins(1));
        // Reading "a" makes "b" the least recently used entry.
        assert_eq!(cache.get("a"), Some("1".to_string()));
        cache.set("c", "3", Duration::from_mins(1));
        assert!(cache.get("b").is_none(), "b must be evicted, not a");
        assert_eq!(cache.get("a"), Some("1".to_string()));
        assert_eq!(cache.get("c"), Some("3".to_string()));
    }

    #[test]
    fn expired_entry_is_removed_and_frees_its_slot() {
        let cache = MemoryCache::new(2);
        cache.set("stale", "0", Duration::from_millis(0));
        cache.set("keep", "1", Duration::from_mins(1));
        std::thread::sleep(Duration::from_millis(1));
        // The expired read promotes "stale" to most recently used, then must pop it.
        assert!(cache.get("stale").is_none());
        // Without the pop, inserting "new" would evict "keep" (the LRU entry).
        cache.set("new", "2", Duration::from_mins(1));
        assert_eq!(cache.get("keep"), Some("1".to_string()));
        assert_eq!(cache.get("new"), Some("2".to_string()));
    }

    #[test]
    fn cache_overwrite_key() {
        let cache = MemoryCache::new(10);
        cache.set("key", "old_value", Duration::from_mins(1));
        cache.set("key", "new_value", Duration::from_mins(1));
        assert_eq!(cache.get("key"), Some("new_value".to_string()));
    }

    #[test]
    fn cache_zero_capacity_fallback() {
        // max_entries=0 should fall back to NonZeroUsize(100), not panic
        let cache = MemoryCache::new(0);
        cache.set("key", "value", Duration::from_mins(1));
        assert_eq!(cache.get("key"), Some("value".to_string()));
    }

    #[test]
    fn cache_concurrent_access() {
        use std::sync::Arc;
        let cache = Arc::new(MemoryCache::new(100));
        let mut handles = Vec::new();
        for i in 0..10 {
            let c = Arc::clone(&cache);
            handles.push(std::thread::spawn(move || {
                let key = format!("key{i}");
                c.set(&key, &format!("val{i}"), Duration::from_mins(1));
                c.get(&key)
            }));
        }
        for handle in handles {
            let result = handle.join().unwrap();
            assert!(result.is_some());
        }
    }

    #[test]
    fn huge_ttl_is_capped_instead_of_panicking() {
        let cache = MemoryCache::new(10);
        cache.set("key", "value", Duration::MAX);
        assert_eq!(cache.get("key"), Some("value".to_string()));
    }
}
