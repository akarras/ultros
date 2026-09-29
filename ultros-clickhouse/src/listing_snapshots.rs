//! Exact listing windows computed off the request path and published atomically.
//!
//! Per-world extrema and durations cannot reconstruct a scope's floor timeline.
//! Each generation therefore uses the authoritative full-scope reducer. A
//! manifest publishes the generation only after all its immutable rows exist.
//! Recomputing also incorporates late events/receipts, unlike a one-way cursor.
use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
    time::{Duration, Instant},
};

use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use ultros_api_types::listing_stats::ListingWindowStats;

use crate::{ClickHouseClient, ClickHouseError, listing_history, rows::TableRow};

const LIMITS: &str = " SETTINGS max_execution_time=10, max_result_rows=2000000, result_overflow_mode='throw', max_memory_usage=536870912";
const MAX_JOBS: usize = 64;
const MAX_WORLD_IDS: usize = 256;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const GENERATION_BUDGET: Duration = Duration::from_secs(120);
const RETRY_DELAY: Duration = Duration::from_secs(60);
const ACTIVE_FOR: Duration = Duration::from_secs(24 * 60 * 60);

pub fn cadence(days: u16) -> Option<u64> {
    use crate::rollups::{
        SALE_STATS_1D_REFRESH_SECS, SALE_STATS_7D_REFRESH_SECS, SALE_STATS_LONG_REFRESH_SECS,
    };
    match days {
        1 => Some(SALE_STATS_1D_REFRESH_SECS),
        7 => Some(SALE_STATS_7D_REFRESH_SECS),
        30 | 90 => Some(SALE_STATS_LONG_REFRESH_SECS),
        _ => None,
    }
}

fn unavailable(reason: &str) -> ClickHouseError {
    ClickHouseError::SnapshotUnavailable(reason.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    worlds: Vec<i32>,
    days: u16,
}

impl Key {
    fn new(worlds: &[i32], days: u16) -> Result<Self, ClickHouseError> {
        let mut worlds = worlds.to_vec();
        worlds.sort_unstable();
        worlds.dedup();
        if cadence(days).is_none()
            || worlds.is_empty()
            || worlds.len() > MAX_WORLD_IDS
            || worlds.iter().any(|id| *id <= 0)
        {
            return Err(unavailable("invalid scope/window"));
        }
        Ok(Self { worlds, days })
    }

    fn scope(&self) -> String {
        self.worlds
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

struct Job {
    requested: Instant,
    due: Instant,
}

/// Shared by clones of one app client. A single worker bounds expensive work;
/// the finite queue prevents arbitrary cold keys creating detached tasks.
#[derive(Default)]
pub(crate) struct Queue {
    jobs: Mutex<HashMap<Key, Job>>,
    notify: Notify,
}

impl Queue {
    fn request(&self, key: Key) {
        let now = Instant::now();
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.retain(|_, job| now.duration_since(job.requested) < ACTIVE_FOR);
        if let Some(job) = jobs.get_mut(&key) {
            job.requested = now;
        } else if jobs.len() < MAX_JOBS {
            jobs.insert(
                key,
                Job {
                    requested: now,
                    due: now,
                },
            );
        }
        drop(jobs);
        self.notify.notify_one();
    }

    fn next(&self) -> Option<Key> {
        let now = Instant::now();
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.retain(|_, job| now.duration_since(job.requested) < ACTIVE_FOR);
        jobs.iter()
            .filter(|(_, job)| job.due <= now)
            .min_by_key(|(_, job)| job.due)
            .map(|(key, _)| key.clone())
    }

    fn finish(&self, key: &Key, success: bool) {
        if let Some(job) = self
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(key)
        {
            // Begin before expiry; the read still independently checks freshness.
            let delay = if success {
                Duration::from_secs(cadence(key.days).unwrap_or(900) / 2)
            } else {
                RETRY_DELAY
            };
            job.due = Instant::now() + delay;
        }
    }
}

/// Own this task with the app shutdown token; never run generations inside an
/// HTTP loader or the existing rollup scheduler's sequential tick branch.
pub async fn run(ch: ClickHouseClient, token: CancellationToken) {
    loop {
        if token.is_cancelled() {
            return;
        }
        if let Some(key) = ch.snapshot_queue.next() {
            let started = Instant::now();
            let result = tokio::select! {
                biased;
                _ = token.cancelled() => return,
                result = tokio::time::timeout(GENERATION_BUDGET, async {
                    ch.migrate().await?;
                    refresh(&ch, &key.worlds, key.days, chrono::Utc::now().timestamp()).await
                }) => result.unwrap_or_else(|_| Err(unavailable("generation deadline"))),
            };
            ch.snapshot_queue.finish(&key, result.is_ok());
            match result {
                Ok(keys) => tracing::info!(
                    days = key.days,
                    worlds = key.worlds.len(),
                    keys,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "listing snapshot published"
                ),
                Err(error) => tracing::warn!(
                    ?error,
                    days = key.days,
                    worlds = key.worlds.len(),
                    "listing snapshot refresh did not confirm publication"
                ),
            }
            continue;
        }
        tokio::select! {
            _ = token.cancelled() => return,
            _ = ch.snapshot_queue.notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(5)) => {},
        }
    }
}

pub(crate) async fn apply_schema(client: &Client) -> Result<(), ClickHouseError> {
    // Generation-specific rows allow readers to ignore partial writes and old
    // keys without joins or per-item tombstones. Orphan generations expire too.
    client
        .query(
            "CREATE TABLE IF NOT EXISTS listing_window_snapshot_rows (
        scope String, window_days UInt16, generation String, item_id Int32, hq UInt8,
        payload String, written_at DateTime
    ) ENGINE = MergeTree ORDER BY (scope, window_days, generation, item_id, hq)
      TTL written_at + INTERVAL 2 DAY",
        )
        .execute()
        .await?;
    client
        .query(
            "CREATE TABLE IF NOT EXISTS listing_window_snapshot_manifests (
        scope String, window_days UInt16, generation String, snapshot_to Int64,
        row_count UInt64, written_at DateTime
    ) ENGINE = MergeTree ORDER BY (scope, window_days, snapshot_to, generation)
      TTL written_at + INTERVAL 2 DAY",
        )
        .execute()
        .await?;
    Ok(())
}

#[derive(Row, Serialize, Deserialize)]
struct StoredRow {
    scope: String,
    window_days: u16,
    generation: String,
    item_id: i32,
    hq: u8,
    payload: String,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    written_at: chrono::DateTime<chrono::Utc>,
}

impl TableRow for StoredRow {
    const TABLE: &'static str = "listing_window_snapshot_rows";
}

#[derive(Row, Deserialize)]
struct Manifest {
    generation: String,
    snapshot_to: i64,
    row_count: u64,
}

#[derive(Row, Deserialize)]
struct ReadRow {
    item_id: i32,
    hq: u8,
    payload: String,
}

/// Publish an exact snapshot at a fixed endpoint. The worker supplies the
/// generation deadline/cancellation; tests can call this in a disposable DB.
pub async fn refresh(
    ch: &ClickHouseClient,
    worlds: &[i32],
    days: u16,
    to: i64,
) -> Result<u64, ClickHouseError> {
    let key = Key::new(worlds, days)?;
    let stats = listing_history::window(ch, &key.worlds, days, to).await?;
    publish(ch, &key, to, &stats).await
}

async fn publish(
    ch: &ClickHouseClient,
    key: &Key,
    to: i64,
    stats: &BTreeMap<(i32, bool), ListingWindowStats>,
) -> Result<u64, ClickHouseError> {
    let generation = ch
        .client()
        .query(&format!("SELECT toString(generateUUIDv4()){LIMITS}"))
        .fetch_one::<String>()
        .await?;
    let scope = key.scope();
    let mut bytes = 0usize;
    // Typed row inserts avoid interpolating JSON. Insert acknowledgement precedes
    // the manifest; a transport error leaves this generation unpublished.
    let mut rows = Vec::with_capacity(256);
    for (&(item_id, hq), value) in stats {
        let payload = serde_json::to_string(value).map_err(|_| unavailable("serialize"))?;
        bytes = bytes.saturating_add(payload.len());
        if bytes > MAX_BYTES {
            return Err(unavailable("snapshot byte limit"));
        }
        rows.push(StoredRow {
            scope: scope.clone(),
            window_days: key.days,
            generation: generation.clone(),
            item_id,
            hq: u8::from(hq),
            payload,
            written_at: chrono::Utc::now(),
        });
        if rows.len() == 256 {
            tokio::task::yield_now().await;
            write_rows(ch, &rows).await?;
            rows.clear();
        }
    }
    write_rows(ch, &rows).await?;
    let publish = ch
        .client()
        .query("INSERT INTO listing_window_snapshot_manifests VALUES (?, ?, ?, ?, ?, now())")
        .bind(&scope)
        .bind(key.days)
        .bind(&generation)
        .bind(to)
        .bind(stats.len() as u64)
        .execute();
    tokio::time::timeout(Duration::from_secs(10), publish)
        .await
        .map_err(|_| unavailable("manifest acknowledgement timeout"))??;
    Ok(stats.len() as u64)
}

async fn write_rows(ch: &ClickHouseClient, rows: &[StoredRow]) -> Result<(), ClickHouseError> {
    tokio::time::timeout(
        Duration::from_secs(10),
        crate::writer::insert_all(ch, rows, 256),
    )
    .await
    .map_err(|_| unavailable("row acknowledgement timeout"))??;
    Ok(())
}

/// Read only one complete, fresh generation, while requesting a background
/// refresh. No manifest is unavailable; a committed zero-row manifest is empty.
/// Stale fallback remains the existing StatsCache's explicit stale response.
pub async fn read(
    ch: &ClickHouseClient,
    worlds: &[i32],
    days: u16,
    now: i64,
) -> Result<(i64, BTreeMap<(i32, bool), ListingWindowStats>), ClickHouseError> {
    let key = Key::new(worlds, days)?;
    ch.snapshot_queue.request(key.clone());
    let manifest = ch
        .client()
        .query(&format!(
            "SELECT generation, snapshot_to, row_count
         FROM listing_window_snapshot_manifests WHERE scope = ? AND window_days = ?
         ORDER BY snapshot_to DESC, written_at DESC, generation DESC LIMIT 1{LIMITS}"
        ))
        .bind(key.scope())
        .bind(days)
        .fetch_optional::<Manifest>()
        .await?
        .ok_or_else(|| unavailable("not ready"))?;
    if manifest.snapshot_to < i64::from(days) * 86400 + 600
        || manifest.snapshot_to > i64::from(u32::MAX)
    {
        return Err(unavailable("invalid generation endpoint"));
    }
    let age = now
        .checked_sub(manifest.snapshot_to)
        .ok_or_else(|| unavailable("invalid read endpoint"))?;
    if age < 0 || age > cadence(days).unwrap_or(0) as i64 {
        return Err(unavailable("expired or future generation"));
    }
    let rows = ch
        .client()
        .query(&format!(
            "SELECT DISTINCT item_id, hq, payload FROM listing_window_snapshot_rows
         WHERE scope = ? AND window_days = ? AND generation = ?{LIMITS}, max_result_bytes=100663296"
        ))
        .bind(key.scope())
        .bind(days)
        .bind(&manifest.generation)
        .fetch_all::<ReadRow>()
        .await?;
    if rows.len() as u64 != manifest.row_count {
        return Err(unavailable("incomplete generation"));
    }
    let mut output = BTreeMap::new();
    let mut bytes = 0usize;
    for (index, row) in rows.into_iter().enumerate() {
        if index.is_multiple_of(4096) {
            tokio::task::yield_now().await;
        }
        bytes = bytes.saturating_add(row.payload.len());
        if bytes > MAX_BYTES {
            return Err(unavailable("snapshot byte limit"));
        }
        let value: ListingWindowStats =
            serde_json::from_str(&row.payload).map_err(|_| unavailable("invalid payload"))?;
        if value.to != manifest.snapshot_to
            || value.window_days != days
            || value.from != value.to - i64::from(days) * 86400
            || row.hq > 1
            || output.insert((row.item_id, row.hq != 0), value).is_some()
        {
            return Err(unavailable("mixed or invalid generation"));
        }
    }
    Ok((manifest.snapshot_to, output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demand_coalesces_canonical_scopes_without_bypassing_failure_backoff() {
        let queue = Queue::default();
        let key = Key::new(&[2, 1, 2], 7).unwrap();
        assert_eq!(key, Key::new(&[1, 2], 7).unwrap());
        queue.request(key.clone());
        assert_eq!(queue.next(), Some(key.clone()));
        queue.finish(&key, false);
        queue.request(Key::new(&[2, 1], 7).unwrap());
        assert!(
            queue.next().is_none(),
            "repeated HTTP requests must not reset failure backoff"
        );
        assert_eq!(queue.jobs.lock().unwrap().len(), 1);
    }

    #[test]
    fn demand_queue_is_finite_and_existing_keys_stay_coalesced() {
        let queue = Queue::default();
        for world in 1..=(MAX_JOBS as i32 + 20) {
            queue.request(Key::new(&[world], 1).unwrap());
        }
        assert_eq!(queue.jobs.lock().unwrap().len(), MAX_JOBS);
        queue.request(Key::new(&[1], 1).unwrap());
        assert_eq!(queue.jobs.lock().unwrap().len(), MAX_JOBS);
        assert!(Key::new(&[], 1).is_err());
        assert!(Key::new(&[0], 1).is_err());
        assert!(Key::new(&[1], 2).is_err());
    }
}
