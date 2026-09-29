//! Cached 30-day median baselines for below-median alerts.
//!
//! The listing path must never wait on ClickHouse, so baselines are fetched
//! ahead of time: every `(item, scope)` some alert references is refreshed on
//! an hourly schedule (the 30-day rollup itself only changes every 6 hours),
//! and a newly referenced key is fetched as soon as its rule appears. Reads
//! are synchronous.
//!
//! A failed fetch keeps the previous values; an entry older than
//! [`MAX_BASELINE_AGE_SECS`] reads as missing, and a missing baseline never
//! fires. So a ClickHouse outage degrades to "alerts use a slightly old
//! median" for a day, then to "below-median alerts pause" — never to alerts
//! fired against no baseline at all.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::warn;
use ultros_api_types::{
    alert::QualityBaselines,
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::ClickHouseClient;

/// Cache key: the item and the alert's world scope.
pub type BaselineKey = (i32, AnySelector);

/// How long a baseline stays usable after its last successful fetch.
pub const MAX_BASELINE_AGE_SECS: i64 = 24 * 60 * 60;

/// How often every referenced baseline is refetched.
pub const BASELINE_REFRESH_SECS: u64 = 60 * 60;

/// Keys per fetch call. A region scope expands to ~30 worlds × 2 qualities,
/// so 20 keys keep the generated tuple list well inside ClickHouse's default
/// `max_query_size`, and one failed chunk only costs its own keys.
const KEYS_PER_FETCH: usize = 20;

/// The rollup window the baseline is read from, in days.
const BASELINE_WINDOW_DAYS: u16 = 30;

/// Where baselines come from. The production source is ClickHouse; tests
/// inject a fake.
#[async_trait]
pub trait BaselineSource: Send + Sync {
    /// Fetch baselines for `keys`. A key absent from a successful result has
    /// no rollup data (no sales in the window) — distinct from an `Err`,
    /// which means the whole call failed and nothing is known.
    async fn fetch(
        &self,
        keys: &[BaselineKey],
    ) -> anyhow::Result<HashMap<BaselineKey, QualityBaselines>>;
}

/// Reads `item_stats_window` exactly like `/api/v1/item_stats` does, so an
/// alert's median is the number the item page's confidence badge shows.
pub struct ClickHouseBaselineSource {
    ch: ClickHouseClient,
    worlds: Arc<WorldHelper>,
}

impl ClickHouseBaselineSource {
    pub fn new(ch: ClickHouseClient, worlds: Arc<WorldHelper>) -> Self {
        Self { ch, worlds }
    }

    fn world_ids(&self, selector: AnySelector) -> Vec<i32> {
        self.worlds
            .lookup_selector(selector)
            .map(|scope| scope.all_worlds().map(|w| w.id).collect())
            .unwrap_or_default()
    }
}

#[async_trait]
impl BaselineSource for ClickHouseBaselineSource {
    async fn fetch(
        &self,
        keys: &[BaselineKey],
    ) -> anyhow::Result<HashMap<BaselineKey, QualityBaselines>> {
        let scopes: Vec<(BaselineKey, Vec<i32>)> = keys
            .iter()
            .map(|key| (*key, self.world_ids(key.1)))
            .collect();
        let tuples: Vec<(i32, u8, i32)> = scopes
            .iter()
            .flat_map(|((item_id, _), worlds)| {
                worlds
                    .iter()
                    .flat_map(move |w| [(*item_id, 0u8, *w), (*item_id, 1u8, *w)])
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let scans =
            ultros_clickhouse::queries::deep_scan_batch(&self.ch, BASELINE_WINDOW_DAYS, &tuples)
                .await?;
        let mut out = HashMap::new();
        for (key, worlds) in scopes {
            let in_scope: Vec<_> = scans
                .iter()
                .filter(|s| s.item_id == key.0 && worlds.contains(&s.world_id))
                .cloned()
                .collect();
            if in_scope.is_empty() {
                continue;
            }
            let variants = ultros_clickhouse::queries::aggregate_item_stats_variants(&in_scope);
            out.insert(key, QualityBaselines::from_variants(variants));
        }
        Ok(out)
    }
}

#[derive(Clone, Debug)]
struct Entry {
    baselines: QualityBaselines,
    fetched_at: DateTime<Utc>,
}

/// See the module docs.
pub struct MedianCache {
    source: Arc<dyn BaselineSource>,
    entries: RwLock<HashMap<BaselineKey, Entry>>,
}

impl MedianCache {
    pub fn new(source: Arc<dyn BaselineSource>) -> Self {
        Self {
            source,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// The cached baselines for `key`, or `None` when never fetched or older
    /// than [`MAX_BASELINE_AGE_SECS`] at `now`.
    pub fn get(&self, key: &BaselineKey, now: DateTime<Utc>) -> Option<QualityBaselines> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let entry = entries.get(key)?;
        if now.signed_duration_since(entry.fetched_at).num_seconds() > MAX_BASELINE_AGE_SECS {
            return None;
        }
        Some(entry.baselines.clone())
    }

    /// Refetch every key in `keys` and evict keys no longer referenced.
    pub async fn refresh_all(&self, keys: &HashSet<BaselineKey>, now: DateTime<Utc>) {
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            entries.retain(|k, _| keys.contains(k));
        }
        let keys: Vec<BaselineKey> = keys.iter().copied().collect();
        self.fetch_into(&keys, now).await;
    }

    /// Fetch only the keys in `keys` that have never been fetched — used when
    /// rules change, so a new alert has a baseline within seconds instead of
    /// waiting for the next hourly refresh.
    pub async fn fill_missing(&self, keys: &HashSet<BaselineKey>, now: DateTime<Utc>) {
        let missing: Vec<BaselineKey> = {
            let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
            keys.iter()
                .filter(|k| !entries.contains_key(k))
                .copied()
                .collect()
        };
        self.fetch_into(&missing, now).await;
    }

    async fn fetch_into(&self, keys: &[BaselineKey], now: DateTime<Utc>) {
        for chunk in keys.chunks(KEYS_PER_FETCH) {
            match self.source.fetch(chunk).await {
                Ok(mut fetched) => {
                    let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
                    for key in chunk {
                        // A key the source had no rows for is still a
                        // successful answer ("no sales"): record it empty so
                        // `fill_missing` doesn't re-ask on every rule change.
                        let baselines = fetched.remove(key).unwrap_or_default();
                        entries.insert(
                            *key,
                            Entry {
                                baselines,
                                fetched_at: now,
                            },
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        keys = chunk.len(),
                        "below-median baseline fetch failed; keeping previous values: {e:#}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use ultros_api_types::{item_stats::ItemStatsVariant, trends::ConfidenceBand};

    /// Answers from a fixed table, or fails every call while `failing`.
    #[derive(Default)]
    struct FakeSource {
        answers: HashMap<BaselineKey, QualityBaselines>,
        failing: Mutex<bool>,
        calls: Mutex<Vec<Vec<BaselineKey>>>,
    }

    #[async_trait]
    impl BaselineSource for FakeSource {
        async fn fetch(
            &self,
            keys: &[BaselineKey],
        ) -> anyhow::Result<HashMap<BaselineKey, QualityBaselines>> {
            self.calls.lock().unwrap().push(keys.to_vec());
            if *self.failing.lock().unwrap() {
                anyhow::bail!("clickhouse down");
            }
            Ok(keys
                .iter()
                .filter_map(|k| self.answers.get(k).map(|b| (*k, b.clone())))
                .collect())
        }
    }

    fn nq(p50: u32) -> QualityBaselines {
        QualityBaselines {
            nq: Some(ItemStatsVariant {
                hq: false,
                sample_size_30d: 40,
                cleaned_sample_size_30d: 40,
                vwap_30d: p50,
                p50_30d: p50,
                confidence_band: ConfidenceBand::High,
                launder_suspicion: 0.0,
            }),
            hq: None,
        }
    }

    const A: BaselineKey = (1, AnySelector::World(100));
    const B: BaselineKey = (2, AnySelector::Datacenter(10));

    fn source_with(answers: &[(BaselineKey, QualityBaselines)]) -> Arc<FakeSource> {
        Arc::new(FakeSource {
            answers: answers.iter().cloned().collect(),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn refresh_stores_answers_and_marks_empty_keys_fetched() {
        let source = source_with(&[(A, nq(100))]);
        let cache = MedianCache::new(source.clone());
        let now = Utc::now();
        cache.refresh_all(&[A, B].into_iter().collect(), now).await;
        assert_eq!(cache.get(&A, now), Some(nq(100)));
        // B had no rows: known-empty, not missing.
        assert_eq!(cache.get(&B, now), Some(QualityBaselines::default()));
        // …so fill_missing doesn't ask for it again.
        let before = source.calls.lock().unwrap().len();
        cache.fill_missing(&[A, B].into_iter().collect(), now).await;
        assert_eq!(source.calls.lock().unwrap().len(), before);
    }

    #[tokio::test]
    async fn failed_refresh_keeps_last_good_value_until_it_expires() {
        let source = source_with(&[(A, nq(100))]);
        let cache = MedianCache::new(source.clone());
        let t0 = Utc::now();
        cache.refresh_all(&[A].into_iter().collect(), t0).await;
        *source.failing.lock().unwrap() = true;
        let t1 = t0 + chrono::Duration::hours(2);
        cache.refresh_all(&[A].into_iter().collect(), t1).await;
        assert_eq!(cache.get(&A, t1), Some(nq(100)));
        let expired = t0 + chrono::Duration::seconds(MAX_BASELINE_AGE_SECS + 1);
        assert_eq!(cache.get(&A, expired), None);
    }

    #[tokio::test]
    async fn refresh_evicts_unreferenced_keys() {
        let source = source_with(&[(A, nq(100)), (B, nq(5))]);
        let cache = MedianCache::new(source);
        let now = Utc::now();
        cache.refresh_all(&[A, B].into_iter().collect(), now).await;
        cache.refresh_all(&[A].into_iter().collect(), now).await;
        assert_eq!(cache.get(&B, now), None);
        assert!(cache.get(&A, now).is_some());
    }

    #[tokio::test]
    async fn fill_missing_fetches_only_new_keys() {
        let source = source_with(&[(A, nq(100)), (B, nq(5))]);
        let cache = MedianCache::new(source.clone());
        let now = Utc::now();
        cache.fill_missing(&[A].into_iter().collect(), now).await;
        cache.fill_missing(&[A, B].into_iter().collect(), now).await;
        let calls = source.calls.lock().unwrap();
        assert_eq!(calls.as_slice(), &[vec![A], vec![B]]);
    }
}
