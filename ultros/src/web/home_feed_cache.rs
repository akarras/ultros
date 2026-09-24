//! In-process TTL cache for the home-page market feeds (`market_pulse`,
//! `movers`, `recentSales` for a datacenter/region).
//!
//! The logged-out home page shows the visitor's guessed *region*, so every
//! anonymous page view lands on one of a handful of identical requests.
//! Without a server-side cache each of those views paid a ClickHouse scan
//! (plus a Postgres count); `Cache-Control` alone only helps a browser that
//! already asked. Values are serialized JSON bodies keyed by the full request
//! (`"<feed>:<scope>:<params>"`), so a hit skips both the query and the
//! serialization.
//!
//! Same shape as [`crate::web::price_series_cache`]: not an LRU — overflow
//! drops expired entries first, then arbitrary ones. The key space is small
//! (feeds × scopes × directions), so eviction is a backstop, not a policy.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct HomeFeedCache {
    inner: Arc<Mutex<HashMap<String, (Instant, String)>>>,
    capacity: usize,
}

impl HomeFeedCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            capacity,
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let map = self.inner.lock().ok()?;
        let (expires_at, value) = map.get(key)?;
        (*expires_at > Instant::now()).then(|| value.clone())
    }

    pub fn insert(&self, key: String, value: String, ttl: Duration) {
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

impl Default for HomeFeedCache {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_a_stored_value_within_ttl() {
        let cache = HomeFeedCache::new(4);
        cache.insert("pulse:3".into(), "a".into(), Duration::from_secs(60));
        assert_eq!(cache.get("pulse:3"), Some("a".to_string()));
        assert_eq!(cache.get("pulse:4"), None);
    }

    #[test]
    fn expired_entries_are_not_returned() {
        let cache = HomeFeedCache::new(4);
        cache.insert("pulse:3".into(), "a".into(), Duration::from_secs(0));
        assert_eq!(cache.get("pulse:3"), None);
    }

    #[test]
    fn insert_past_capacity_evicts_rather_than_growing_forever() {
        let cache = HomeFeedCache::new(2);
        for i in 0..5 {
            cache.insert(format!("k{i}"), i.to_string(), Duration::from_secs(60));
        }
        assert!(cache.len() <= 2, "capacity must bound the map");
    }
}
