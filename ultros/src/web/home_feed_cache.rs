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
//! Shares [`super::json_cache::JsonCache`] with chart responses: overflow
//! drops expired entries first, then arbitrary ones. The key space is small
//! (feeds × scopes × directions), so eviction is a backstop, not a policy.

pub(crate) type HomeFeedCache = super::json_cache::JsonCache<String>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn returns_a_stored_value_within_ttl() {
        let cache = HomeFeedCache::new(4);
        cache.insert("pulse:3".into(), "a", Duration::from_secs(60));
        assert_eq!(
            cache.get("pulse:3"),
            Some(axum::body::Bytes::from_static(b"a"))
        );
        assert_eq!(cache.get("pulse:4"), None);
    }

    #[test]
    fn expired_entries_are_not_returned() {
        let cache = HomeFeedCache::new(4);
        cache.insert("pulse:3".into(), "a", Duration::from_secs(0));
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
