use std::sync::RwLock;
use std::time::{Duration, Instant};

use lru::LruCache;
use std::num::NonZeroUsize;

use crate::ports::cache::ListingCache;

struct CacheEntry {
    value: String,
    expires_at: Instant,
}

pub struct MemoryCache {
    inner: RwLock<LruCache<String, CacheEntry>>,
}

impl MemoryCache {
    pub fn new(max_entries: usize) -> Self {
        let cap = NonZeroUsize::new(max_entries).unwrap_or_else(|| {
            tracing::warn!("Cache max_entries was 0, defaulting to 100");
            NonZeroUsize::new(100).unwrap()
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
        if let Ok(mut cache) = self.inner.write() {
            cache.put(
                key.to_string(),
                CacheEntry {
                    value: value.to_string(),
                    expires_at: Instant::now() + ttl,
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
        cache.set("key1", "value1", Duration::from_secs(60));
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
        cache.set("a", "1", Duration::from_secs(60));
        cache.set("b", "2", Duration::from_secs(60));
        cache.set("c", "3", Duration::from_secs(60));
        // "a" should be evicted (LRU)
        assert!(cache.get("a").is_none());
        assert_eq!(cache.get("b"), Some("2".to_string()));
        assert_eq!(cache.get("c"), Some("3".to_string()));
    }

    #[test]
    fn cache_overwrite_key() {
        let cache = MemoryCache::new(10);
        cache.set("key", "old_value", Duration::from_secs(60));
        cache.set("key", "new_value", Duration::from_secs(60));
        assert_eq!(cache.get("key"), Some("new_value".to_string()));
    }

    #[test]
    fn cache_zero_capacity_fallback() {
        // max_entries=0 should fall back to NonZeroUsize(100), not panic
        let cache = MemoryCache::new(0);
        cache.set("key", "value", Duration::from_secs(60));
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
                c.set(&key, &format!("val{i}"), Duration::from_secs(60));
                c.get(&key)
            }));
        }
        for handle in handles {
            let result = handle.join().unwrap();
            assert!(result.is_some());
        }
    }
}
