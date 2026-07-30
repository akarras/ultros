//! Per-world ingest freshness, exported to `/metrics`.
//!
//! Every one of the ingest failure modes this module exists for — a half-open
//! websocket, a lagging event bus, a panicked consumer task, a stale snapshot —
//! looks identical from outside the process: the app is up, requests succeed,
//! and the numbers simply stop moving. Nothing in the request path can tell you
//! that, because serving stale data is not an error.
//!
//! `listing_last_updated` already records an ingest timestamp per (world, item)
//! on every successful update, so `now - max(date_time)` per world is a
//! ready-made "seconds since we last heard anything about this world" signal.
//! Publishing it as a gauge turns every silent failure into an alertable one.

use std::{collections::HashSet, sync::Arc, time::Duration};

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};
use ultros_db::{
    UltrosDb,
    world_data::world_cache::{AnySelector, WorldCache},
};

/// How often the gauge is recomputed.
///
/// One grouped aggregate over `listing_last_updated` per tick. A minute is fast
/// enough that a Prometheus scrape never sees a value more than one interval
/// behind, and slow enough that the query is noise next to ingest traffic.
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// What the previous tick reported, so a steady-state problem is logged once
/// rather than once a minute forever.
///
/// This matters more than it looks: `error!` events are forwarded to Glitchtip
/// (see `docs/error-reporting.md`), and a task on a fixed interval that logs
/// unconditionally turns one broken dependency into an unbounded stream of
/// issues — the exact failure the ClickHouse wiring in `main.rs` calls out.
/// The gauges are published every tick regardless; only the logging is deduped.
#[derive(Default)]
struct LastReported {
    query_failed: bool,
    never_ingested: Option<usize>,
}

/// Publishes per-world ingest staleness until `token` is cancelled.
pub(crate) fn spawn_staleness_gauge(
    db: UltrosDb,
    world_cache: Arc<WorldCache>,
    token: CancellationToken,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(REFRESH_INTERVAL);
        let mut last_reported = LastReported::default();
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = interval.tick() => {
                    record_staleness(&db, &world_cache, &mut last_reported).await
                }
            }
        }
    });
}

async fn record_staleness(
    db: &UltrosDb,
    world_cache: &WorldCache,
    last_reported: &mut LastReported,
) {
    let last_ingest = match db.get_last_ingest_per_world().await {
        Ok(rows) => rows,
        Err(e) => {
            if !last_reported.query_failed {
                error!(error = ?e, "unable to compute per-world ingest staleness");
                last_reported.query_failed = true;
            }
            return;
        }
    };
    last_reported.query_failed = false;
    let now = Utc::now().naive_utc();
    let mut ingested: HashSet<i32> = HashSet::with_capacity(last_ingest.len());
    for (world_id, ingested_at) in &last_ingest {
        // A world in the table but not in the cache still gets a series, keyed
        // by id — losing the label is better than losing the alert.
        let world_name = world_cache
            .lookup_selector(&AnySelector::World(*world_id))
            .ok()
            .and_then(|r| r.as_world().ok())
            .map(|w| w.name.clone())
            .unwrap_or_else(|| world_id.to_string());
        let age = (now - *ingested_at).num_seconds();
        metrics::gauge!("ultros_world_ingest_staleness_seconds", "world" => world_name)
            .set(age as f64);
        ingested.insert(*world_id);
    }

    // Worlds we have never ingested for produce no row above, so they'd have no
    // series to alert on at all. Count them instead — a nonzero value on a
    // long-running process means a world is being ignored entirely.
    let never_ingested = world_cache
        .get_all_worlds()
        .filter(|w| !ingested.contains(&w.id))
        .count();
    metrics::gauge!("ultros_worlds_never_ingested").set(never_ingested as f64);
    if never_ingested > 0 && last_reported.never_ingested != Some(never_ingested) {
        warn!(
            never_ingested,
            "some worlds have never had a listing ingested"
        );
    }
    last_reported.never_ingested = Some(never_ingested);
}
