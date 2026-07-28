//! Small in-process TTL cache for `/api/v1/price_series` responses.
//!
//! Values are already-serialized JSON strings: the endpoint's cost is the
//! ClickHouse scan plus serialization, and caching the string skips both.
//!
//! Deliberately not an LRU. Eviction on overflow clears expired entries first
//! and then drops arbitrary ones — for a cache whose job is absorbing bursts
//! of identical requests, exact recency ordering is not worth the bookkeeping.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub item_id: i32,
    pub scope: String,
    pub from: i64,
    pub to: i64,
    pub bucket: i64,
    pub group: &'static str,
    pub hq: &'static str,
}

#[derive(Clone)]
pub(crate) struct PriceSeriesCache {
    inner: Arc<Mutex<HashMap<CacheKey, (Instant, String)>>>,
    capacity: usize,
}

impl PriceSeriesCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            capacity,
        }
    }

    pub fn get(&self, key: &CacheKey) -> Option<String> {
        let map = self.inner.lock().ok()?;
        let (expires_at, value) = map.get(key)?;
        (*expires_at > Instant::now()).then(|| value.clone())
    }

    pub fn insert(&self, key: CacheKey, value: String, ttl: Duration) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        if map.len() >= self.capacity {
            let now = Instant::now();
            map.retain(|_, (expires_at, _)| *expires_at > now);
            while map.len() >= self.capacity {
                let Some(victim) = map.keys().next().cloned() else {
                    break;
                };
                map.remove(&victim);
            }
        }
        map.insert(key, (Instant::now() + ttl, value));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }
}

impl Default for PriceSeriesCache {
    fn default() -> Self {
        Self::new(512)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn key(item: i32) -> CacheKey {
        CacheKey {
            item_id: item,
            scope: "Gilgamesh".to_string(),
            from: 0,
            to: 100,
            bucket: 3600,
            group: "world",
            hq: "any",
        }
    }

    #[test]
    fn returns_a_stored_value_within_ttl() {
        let cache = PriceSeriesCache::new(4);
        cache.insert(key(1), "a".to_string(), Duration::from_secs(60));
        assert_eq!(cache.get(&key(1)), Some("a".to_string()));
        assert_eq!(cache.get(&key(2)), None);
    }

    #[test]
    fn expired_entries_are_not_returned() {
        let cache = PriceSeriesCache::new(4);
        cache.insert(key(1), "a".to_string(), Duration::from_secs(0));
        assert_eq!(cache.get(&key(1)), None);
    }

    #[test]
    fn insert_past_capacity_evicts_rather_than_growing_forever() {
        let cache = PriceSeriesCache::new(2);
        for i in 0..5 {
            cache.insert(key(i), i.to_string(), Duration::from_secs(60));
        }
        assert!(cache.len() <= 2, "capacity must bound the map");
    }
}
