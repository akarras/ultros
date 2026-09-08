//! Scheduled rollup refreshers.
//!
//! Two refreshers run on independent schedules:
//!
//! - [`refresh_window`] populates `item_stats_window` for a given window size
//!   (1, 7, 30, or 90 days). The analyzer's deep-scan reads from this table.
//! - [`refresh_quality_scores`] derives the trustworthiness `item_quality_score`
//!   row from the latest `item_stats_window` data.
//!
//! ## Noise filter
//!
//! Both layers documented in `docs/superpowers/plans/2026-05-15-clickhouse-analyzer-rebuild.md`:
//!
//! - **Layer 1 (statistical)**: drop sales where `|price - p50| > 5 × MAD`,
//!   where MAD is the median absolute deviation computed on the
//!   heuristic-clean subset.
//! - **Layer 2 (heuristic)**: drop sales where `quantity = 1` and price is
//!   either `> 10×` or `< 0.1×` the per-item median. Catches the most common
//!   currency-transfer launder shape (single-unit, off-market price).
//!
//! Filters are query-time only — no flags stored on the raw `sales` table.
//! This keeps the source data faithful and lets us re-tune the filter without
//! a backfill.

use clickhouse::Client;
use tracing::{info, instrument};

use crate::{
    ClickHouseClient, ClickHouseError,
    schema::{
        LISTING_ALIVE_STATE_TABLE, LISTING_EVENTS_SEED_MARKER_TABLE, LISTING_LAST_EVENT_TABLE,
    },
};

/// Refresh `item_stats_window` for a single window size.
///
/// Strategy:
/// 1. Compute per-(item,hq,world) p50 over the raw window data.
/// 2. Apply Layer 2 (heuristic) filter — flags single-unit obvious outliers.
/// 3. Compute MAD on the Layer-2-clean subset.
/// 4. Apply Layer 1 (statistical) filter — flags `> 5×MAD` outliers.
/// 5. Compute final aggregates on the doubly-clean subset and insert.
///
/// All five passes run as a single `INSERT ... SELECT` with CTEs so
/// ClickHouse can execute them as one pipeline. Per-window cost on the dev
/// dataset (3M sales / 90 days) is sub-second.
#[instrument(skip(ch))]
pub async fn refresh_window(
    ch: &ClickHouseClient,
    window_days: u16,
) -> Result<u64, ClickHouseError> {
    let sql = build_refresh_sql(window_days);
    ch.client().query(&sql).execute().await?;

    // ReplacingMergeTree keeps both old and new rows until a merge runs.
    // For an accurate "how many (item, world) tuples did we just refresh?"
    // count, look at the latest computed_at per key. This is informational
    // only — the actual refresh succeeded once execute() returned Ok.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query(
            "SELECT count() AS n FROM item_stats_window FINAL \
             WHERE window_days = ?",
        )
        .bind(window_days)
        .fetch_one()
        .await?;
    info!(
        window_days,
        refreshed_tuples = count.n,
        "rollup refresh done"
    );
    Ok(count.n)
}

/// Build the refresh SQL for a window. Extracted for testability.
fn build_refresh_sql(window_days: u16) -> String {
    // The CTE chain matches the comment on `refresh_window` step-for-step.
    // We use `quantileExact` (not `quantileTDigest`) because the windows are
    // small enough that exactness is cheap and we want the filter thresholds
    // to be deterministic across runs.
    //
    // Notes on the math:
    //   - VWAP = sum(quantity * price) / sum(quantity), but to avoid double
    //     access we use `total_gil` (MATERIALIZED on sales) for the numerator.
    //   - MAD is computed on the Layer-2-clean subset and applied as the
    //     Layer-1 filter. This avoids cyclic dependencies (computing MAD on
    //     data that includes obvious launder rows would inflate the MAD and
    //     defeat the statistical filter).
    format!(
        r#"
        INSERT INTO item_stats_window
        WITH
            window_sales AS (
                SELECT item_id, hq, world_id, price_per_item, quantity,
                       total_gil, buying_character_id
                FROM sales FINAL
                WHERE sold_date > now() - INTERVAL {window_days} DAY
            ),
            medians AS (
                SELECT item_id, hq, world_id,
                       quantileExact(0.5)(price_per_item) AS p50_raw
                FROM window_sales
                GROUP BY item_id, hq, world_id
            ),
            flagged AS (
                -- After the USING joins below, `item_id`, `hq`, `world_id`
                -- live in a merged scope (no `s.` prefix). We project them
                -- explicitly so downstream CTEs (mads, clean_aggs) can
                -- reference them by unqualified name.
                SELECT item_id, hq, world_id,
                       s.price_per_item, s.quantity, s.total_gil,
                       s.buying_character_id, m.p50_raw,
                       -- Layer 2 (heuristic): single-unit launder catch.
                       -- Three independent tripwires, OR'd together:
                       --   (a) >10x the item's own p50 (relative)
                       --   (b) <0.1x the item's own p50 (relative, inverse)
                       --   (c) >100x the in-game NPC vendor price (absolute).
                       -- (c) is the strongest signal — anchored to game
                       -- ground truth, so it works even when (a)/(b) get
                       -- defeated by a sale history that's >50% laundered.
                       -- 100x leaves room for legitimate convenience
                       -- premiums on housing items where the NPC vendor
                       -- is obscure. v.vendor_price = 0 (missing or LEFT-
                       -- JOIN miss) makes (c) a no-op for that row.
                       (s.quantity = 1 AND s.price_per_item > 10 * m.p50_raw)
                       OR (s.quantity = 1 AND s.price_per_item * 10 < m.p50_raw)
                       OR (s.quantity = 1
                           AND v.vendor_price > 0
                           AND s.price_per_item > 100 * v.vendor_price)
                       AS l2_excluded
                FROM window_sales s
                INNER JOIN medians m USING (item_id, hq, world_id)
                LEFT JOIN item_vendor_price v FINAL USING (item_id)
            ),
            mads AS (
                SELECT item_id, hq, world_id,
                       quantileExact(0.5)(abs(toInt64(price_per_item) - toInt64(p50_raw)))
                           AS mad_raw
                FROM flagged
                WHERE NOT l2_excluded
                GROUP BY item_id, hq, world_id
            ),
            both_flagged AS (
                SELECT f.item_id, f.hq, f.world_id, f.price_per_item, f.quantity,
                       f.total_gil, f.buying_character_id, f.p50_raw, f.l2_excluded,
                       mad.mad_raw,
                       -- Layer 1 (statistical): > 5×MAD outlier.
                       -- mad_raw = 0 means the market is too sparse to filter
                       -- statistically; in that case we trust Layer 2 only.
                       (mad.mad_raw > 0
                        AND abs(toInt64(f.price_per_item) - toInt64(f.p50_raw)) > 5 * mad.mad_raw)
                       AS l1_excluded
                FROM flagged f
                INNER JOIN mads mad USING (item_id, hq, world_id)
            ),
            clean AS (
                SELECT *
                FROM both_flagged
                WHERE NOT l1_excluded AND NOT l2_excluded
            ),
            totals AS (
                SELECT item_id, hq, world_id,
                       count() AS sample_size,
                       sum(toUInt32(l1_excluded OR l2_excluded)) AS excluded_count
                FROM both_flagged
                GROUP BY item_id, hq, world_id
            ),
            -- Compute clean medians + percentiles in a separate pass so we
            -- can reference the clean p50 by name when computing MAD below.
            -- ClickHouse rejects nested aggregates so we can't compute
            -- quantileExact(abs(x - quantileExact(x))) in one shot.
            clean_aggs AS (
                SELECT item_id, hq, world_id,
                       count() AS sale_count,
                       sum(quantity) AS unit_volume,
                       sum(total_gil) AS gil_volume,
                       sum(total_gil) / greatest(sum(quantity), 1) AS vwap_raw,
                       quantileExact(0.10)(price_per_item) AS p10,
                       quantileExact(0.25)(price_per_item) AS p25,
                       quantileExact(0.50)(price_per_item) AS p50,
                       quantileExact(0.75)(price_per_item) AS p75,
                       quantileExact(0.90)(price_per_item) AS p90,
                       uniqExact(buying_character_id) AS unique_buyers
                FROM clean
                GROUP BY item_id, hq, world_id
            ),
            clean_mads AS (
                SELECT c.item_id, c.hq, c.world_id,
                       quantileExact(0.5)(abs(toInt64(c.price_per_item) - toInt64(a.p50)))
                           AS mad_clean
                FROM clean c
                INNER JOIN clean_aggs a USING (item_id, hq, world_id)
                GROUP BY c.item_id, c.hq, c.world_id
            )
        SELECT
            t.item_id, t.hq, t.world_id,
            toUInt16({window_days}) AS window_days,
            now() AS computed_at,
            toUInt32(t.sample_size) AS sample_size,
            toUInt32(t.sample_size - t.excluded_count) AS cleaned_sample_size,
            toUInt32(t.excluded_count) AS excluded_count,
            toUInt32(a.vwap_raw) AS vwap,
            toUInt32(a.p10) AS p10,
            toUInt32(a.p25) AS p25,
            toUInt32(a.p50) AS p50,
            toUInt32(a.p75) AS p75,
            toUInt32(a.p90) AS p90,
            toUInt32(m.mad_clean) AS median_abs_deviation,
            a.unit_volume,
            a.gil_volume,
            toUInt32(a.sale_count) AS sale_count,
            toUInt32(a.unique_buyers) AS unique_buyers
        FROM totals t
        INNER JOIN clean_aggs a USING (item_id, hq, world_id)
        INNER JOIN clean_mads m USING (item_id, hq, world_id)
        "#
    )
}

/// Refresh `item_quality_score` from the latest `item_stats_window` rows.
///
/// Score is a weighted combination of:
///   - Sample size (more = better, capped at 100)
///   - Buyer diversity ratio (unique_buyers / sale_count)
///   - Launder suspicion (excluded_count / sample_size, inverted)
///
/// Bands:
///   - high     : score >= 75
///   - medium   : score 40-74
///   - low      : score 15-39
///   - unusable : score < 15
///
/// Tuning lives here so it's adjustable without touching the analyzer code.
#[instrument(skip(ch))]
pub async fn refresh_quality_scores(ch: &ClickHouseClient) -> Result<u64, ClickHouseError> {
    ch.client()
        .query(
            r#"
            INSERT INTO item_quality_score
            WITH stats_30 AS (
                SELECT item_id, hq, world_id, sample_size, cleaned_sample_size,
                       excluded_count, unique_buyers, sale_count
                FROM item_stats_window FINAL
                WHERE window_days = 30
            )
            SELECT
                item_id, hq, world_id,
                now() AS computed_at,
                -- Component scores in 0-100 each, then averaged with weights.
                -- sample: log-scaled cap at 100 samples = full marks
                -- diversity: unique_buyers / sale_count, scaled
                -- cleanliness: 1 - excluded/sample, scaled
                toUInt8(least(100,
                    0.40 * least(100, sample_size)                    -- sample_size component
                    + 0.30 * if(sale_count > 0,
                                100.0 * unique_buyers / sale_count,
                                0)                                     -- diversity
                    + 0.30 * if(sample_size > 0,
                                100.0 * (sample_size - excluded_count) / sample_size,
                                0)                                     -- cleanliness
                )) AS quality_score,
                -- Confidence band derivation. `quality_score` rarely
                -- bottoms below 15 in practice (even thin items get ~30
                -- from buyer-diversity + cleanliness), so we also gate
                -- on launder rate: anything where the filter dropped
                -- 50%+ of samples is unusable regardless of score.
                multiIf(
                    sample_size > 0 AND excluded_count >= sample_size / 2,
                        CAST('unusable' AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4)),
                    quality_score >= 75,
                        CAST('high' AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4)),
                    quality_score >= 40,
                        CAST('medium' AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4)),
                    quality_score >= 15,
                        CAST('low' AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4)),
                        CAST('unusable' AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4))
                ) AS confidence_band,
                sample_size AS sample_size_30d,
                if(sample_size > 0,
                   toFloat32(excluded_count) / toFloat32(sample_size),
                   toFloat32(0)) AS launder_suspicion_pct
            FROM stats_30
            "#,
        )
        .execute()
        .await?;
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query("SELECT count() AS n FROM item_quality_score FINAL")
        .fetch_one()
        .await?;
    info!(refreshed = count.n, "quality score refresh done");
    Ok(count.n)
}

/// Refresh `world_kpi_5min` for the trailing 50 hours.
///
/// 50 hours covers "yesterday" (24-48h ago) plus "today" (0-24h ago) plus a
/// small buffer in case the refresh schedule slips. We rebuild rather than
/// upsert because:
///   - The latest bucket is always still filling — a strict "only new
///     buckets" approach would freeze in-progress numbers.
///   - The cost is trivial: one GROUP BY toStartOfInterval over ~60k sales
///     (typical for 50h on the dev corpus) is sub-100ms.
///
/// Older buckets remain immutable in the table — only the trailing window
/// gets recomputed. The ReplacingMergeTree engine handles the dedup on
/// merge.
#[instrument(skip(ch))]
pub async fn refresh_world_kpi_5min(ch: &ClickHouseClient) -> Result<u64, ClickHouseError> {
    let sql = r#"
        INSERT INTO world_kpi_5min
        SELECT
            world_id,
            toStartOfInterval(sold_date, INTERVAL 5 MINUTE) AS bucket,
            now() AS computed_at,
            toUInt32(count()) AS sale_count,
            sum(quantity) AS unit_volume,
            sum(total_gil) AS gil_volume,
            toUInt32(uniqExact(item_id)) AS unique_items,
            toUInt32(uniqExact(buying_character_id)) AS unique_buyers
        FROM sales FINAL
        WHERE sold_date > now() - INTERVAL 50 HOUR
        GROUP BY world_id, bucket
    "#;
    ch.client().query(sql).execute().await?;

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query(
            "SELECT count() AS n FROM world_kpi_5min FINAL \
             WHERE bucket > now() - INTERVAL 50 HOUR",
        )
        .fetch_one()
        .await?;
    tracing::info!(buckets = count.n, "world_kpi_5min refresh done");
    Ok(count.n)
}

/// Refresh `sales_hourly` for the trailing 30 hours.
///
/// 30 hours covers the 24h sparkline plus a small buffer so the trailing
/// edge is always populated. We rebuild rather than upsert because the
/// most-recent hour is always still filling and ReplacingMergeTree merges
/// duplicates by `computed_at` anyway.
#[instrument(skip(ch))]
pub async fn refresh_sales_hourly(ch: &ClickHouseClient) -> Result<u64, ClickHouseError> {
    let sql = r#"
        INSERT INTO sales_hourly
        SELECT
            item_id, hq, world_id,
            toStartOfInterval(sold_date, INTERVAL 1 HOUR) AS bucket,
            now() AS computed_at,
            toUInt32(count()) AS sale_count,
            toUInt32(sum(quantity)) AS unit_volume,
            toUInt32(sum(total_gil) / greatest(sum(quantity), 1)) AS vwap,
            toUInt32(min(price_per_item)) AS min_price,
            toUInt32(max(price_per_item)) AS max_price
        FROM sales FINAL
        WHERE sold_date > now() - INTERVAL 30 HOUR
        GROUP BY item_id, hq, world_id, bucket
    "#;
    ch.client().query(sql).execute().await?;

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query(
            "SELECT count() AS n FROM sales_hourly FINAL \
             WHERE bucket > now() - INTERVAL 30 HOUR",
        )
        .fetch_one()
        .await?;
    tracing::info!(rows = count.n, "sales_hourly refresh done");
    Ok(count.n)
}

/// Refresh the mergeable whole-market sale-stat snapshot for one supported
/// trailing window.
///
/// This is deliberately a scheduled raw-sales scan, not a request-time scan.
/// The cost is therefore fixed by the four refresh cadences rather than by
/// page traffic. The t-digest state remains mergeable across worlds, allowing
/// one small table to serve world, datacenter, and region selectors.
#[instrument(skip(ch))]
pub async fn refresh_sale_stats_window(
    ch: &ClickHouseClient,
    window_days: u16,
) -> Result<u64, ClickHouseError> {
    let sql = build_sale_stats_refresh_sql(window_days);
    ch.client().query(&sql).execute().await?;

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query(
            "SELECT count() AS n FROM sale_stats_window FINAL \
             WHERE window_days = ?",
        )
        .bind(window_days)
        .fetch_one()
        .await?;
    tracing::info!(
        window_days,
        rows = count.n,
        "sale_stats_window refresh done"
    );
    Ok(count.n)
}

fn build_sale_stats_refresh_sql(window_days: u16) -> String {
    format!(
        r#"
        INSERT INTO sale_stats_window
        SELECT
            world_id,
            toUInt16({window_days}) AS window_days,
            item_id,
            hq,
            now() AS computed_at,
            toUInt32(min(price_per_item)) AS min_price,
            quantileTDigestState(0.5)(price_per_item) AS price_quantile,
            toUInt64(sum(toUInt64(price_per_item))) AS price_sum,
            toUInt64(count()) AS sale_count,
            toInt64(max(toUnixTimestamp(sold_date))) AS last_sold_unix,
            toUInt64(sum(quantity)) AS units_sold,
            toUInt64(sum(total_gil)) AS gil_volume
        FROM sales FINAL
        WHERE sold_date >= now() - INTERVAL {window_days} DAY
        GROUP BY world_id, item_id, hq
        "#
    )
}

/// How far back of already-consumed `listing_events` each refresh re-reads.
///
/// The fold into `listing_last_event` is idempotent — re-reading an event can
/// only recompute the same winner — so overlap is free insurance against an
/// event whose `event_time` (the observation timestamp) landed behind the
/// cursor because the ClickHouse writer batched it across the boundary.
const LISTING_ALIVE_OVERLAP_SECS: i64 = 3600;

/// Wall-clock ceiling on one `listing_alive` refresh, enforced both by
/// ClickHouse (`max_execution_time`, so a runaway query stops consuming server
/// resources) and by the caller's `tokio::time::timeout`, which is what keeps
/// the shared rollup ticker moving.
const LISTING_ALIVE_MAX_EXECUTION_SECS: u64 = 540;

/// Widest slice of the event log one fold statement will read.
///
/// Steady state is one 15-minute tick plus the overlap, so a day is one
/// statement per refresh and this bound never binds. It binds on the one run
/// that matters: the first fold after a deployment, which starts at the seed
/// and may have a month of events to catch up on. Chunking makes that run
/// resumable — the cursor advances after every chunk, so a fold cut short by
/// the timeout resumes from where it stopped instead of restarting.
const LISTING_ALIVE_FOLD_CHUNK_SECS: i64 = 24 * 3600;

/// Per-query memory ceiling and the point at which a `GROUP BY` spills to
/// disk instead of failing. The seed's rows all carry one `observed_at`, so
/// the first chunk aggregates every seeded listing at once however narrow the
/// chunk is (11.45M groups on prod) — one small state each, but worth spilling
/// rather than risking `MEMORY_LIMIT_EXCEEDED` (Code 241) against the
/// container's cap. Every fold after the catch-up sees minutes of events.
const LISTING_ALIVE_MAX_MEMORY_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const LISTING_ALIVE_EXTERNAL_GROUP_BY_BYTES: u64 = 1024 * 1024 * 1024;

/// A `DateTime` read back with the row count that produced it, so "no rows"
/// and "the epoch" stay distinguishable. A scalar subquery in a `SELECT` list
/// comes back `Nullable`, which the row decoder refuses, so every stamp is its
/// own plain aggregate.
#[derive(clickhouse::Row, serde::Deserialize)]
struct Stamp {
    n: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    at: chrono::DateTime<chrono::Utc>,
}

fn epoch() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(0, 0).expect("epoch is a valid timestamp")
}

/// The earliest `listing_events` row the alive set may trust.
///
/// Anything older may be an `added` whose removal was never recorded. The
/// seed's snapshot rows carry the time the stream *started* while the marker
/// is stamped when it ends, so the cutoff is the snapshot's own time — a
/// removal observed mid-stream must not be dropped, or its listing stays alive
/// forever. A database that has never been seeded replays everything it has.
///
/// Resolved here rather than as a nested scalar subquery inside the refresh:
/// two cheap aggregates that cannot come back `Nullable` and poison the
/// comparison they feed.
async fn listing_events_seed_cutoff(
    ch: &ClickHouseClient,
) -> Result<chrono::DateTime<chrono::Utc>, ClickHouseError> {
    let snapshot: Stamp = ch
        .client()
        .query(
            "SELECT count() AS n, min(event_time) AS at \
             FROM listing_events WHERE source = 'snapshot'",
        )
        .fetch_one()
        .await?;
    if snapshot.n > 0 {
        return Ok(snapshot.at);
    }
    let marker: Stamp = ch
        .client()
        .query(&format!(
            "SELECT count() AS n, max(seeded_at) AS at FROM {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .fetch_one()
        .await?;
    Ok(if marker.n > 0 { marker.at } else { epoch() })
}

/// How far `listing_last_event` has consumed `listing_events`, or `None` on a
/// database that has never folded.
async fn listing_alive_cursor(
    ch: &ClickHouseClient,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, ClickHouseError> {
    let cursor: Stamp = ch
        .client()
        .query(&format!(
            "SELECT count() AS n, max(consumed_through) AS at FROM {LISTING_ALIVE_STATE_TABLE}"
        ))
        .fetch_one()
        .await?;
    Ok((cursor.n > 0).then_some(cursor.at))
}

async fn set_listing_alive_cursor(
    ch: &ClickHouseClient,
    consumed_through: chrono::DateTime<chrono::Utc>,
) -> Result<(), ClickHouseError> {
    #[derive(serde::Serialize, clickhouse::Row)]
    struct CursorRow {
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        consumed_through: chrono::DateTime<chrono::Utc>,
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        updated_at: chrono::DateTime<chrono::Utc>,
    }
    let mut insert = ch
        .client()
        .insert::<CursorRow>(LISTING_ALIVE_STATE_TABLE)
        .await?;
    insert
        .write(&CursorRow {
            consumed_through,
            updated_at: chrono::Utc::now(),
        })
        .await?;
    insert.end().await?;
    Ok(())
}

/// Where a refresh starts reading the event log: the cursor moved back by the
/// overlap, but never behind the seed — a pre-seed `added` whose removal was
/// never recorded must stay unreplayed — and the seed itself when nothing has
/// been folded yet.
fn fold_lower_bound(
    seed_cutoff: chrono::DateTime<chrono::Utc>,
    cursor: Option<chrono::DateTime<chrono::Utc>>,
) -> chrono::DateTime<chrono::Utc> {
    cursor
        .map(|c| c - chrono::TimeDelta::seconds(LISTING_ALIVE_OVERLAP_SECS))
        .filter(|overlapped| *overlapped > seed_cutoff)
        .unwrap_or(seed_cutoff)
}

/// The half-open windows one refresh folds, in order. Empty when the cursor has
/// already caught up.
fn fold_chunks(
    from: chrono::DateTime<chrono::Utc>,
    consumed_through: chrono::DateTime<chrono::Utc>,
) -> Vec<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> {
    let mut chunks = Vec::new();
    let mut start = from;
    while start < consumed_through {
        let end = std::cmp::min(
            start + chrono::TimeDelta::seconds(LISTING_ALIVE_FOLD_CHUNK_SECS),
            consumed_through,
        );
        chunks.push((start, end));
        start = end;
    }
    chunks
}

/// Refresh `listing_alive`: fold new `listing_events` into `listing_last_event`
/// and aggregate that into the alive listing set for every `(world, item, hq)`.
///
/// **Why two statements.** The obvious shape — one `GROUP BY listing_key` over
/// the whole event log — is a hash aggregate whose group count is every listing
/// ever observed since the seed (11.45M on prod at day 0, growing by roughly
/// 1.9M listings a day), held entirely in memory, every 15 minutes forever.
/// Splitting it moves the per-listing state into a table: the fold reads only
/// the events since the cursor (minutes, not months), and the aggregation reads
/// `listing_last_event FINAL`, a streaming merge over a sorted table rather
/// than a hash map that grows with the log. The one aggregate left is keyed on
/// `(world, item, hq)` — millions of groups at most, not hundreds of millions.
///
/// The fold is chunked a day at a time and advances the cursor after every
/// chunk, so the one genuinely expensive run — the first fold after a
/// deployment, which starts at the seed — is resumable: cut short by the
/// timeout, the next tick carries on from where it stopped instead of starting
/// over and never finishing.
///
/// **Identity.** A listing's identity is its Universalis `listing_id`, falling
/// back to the Postgres row id for rows recorded before the identity migration
/// (an empty `listing_id`); without the fallback every legacy listing would
/// collapse into one. The two are namespaced (`u:` / `p:`) so a numeric
/// Universalis id can never be confused with a stringified row id.
///
/// **Ordering.** A listing's state is the event with the greatest
/// `(event_time, pg_listing_id, kind_rank)`, resolved with a *single*
/// `argMax` over a tuple of the whole payload rather than one `argMax` state
/// per column: five states each hold their own copy of the comparator, and a
/// tuple comparator is a heap-allocated generic `Field`, so the naive shape
/// pays that cost five times per group. `event_time` has second resolution and
/// a reprice arrives as `removed` + `added` on one `listing_id` inside the same
/// second, so a tie goes to the higher Postgres row id (the re-added row is the
/// newer one) and, at an equal row id, to the larger `kind_rank` — `removed` is
/// a row's final state. (`updated` outranking `added` is unreachable in
/// practice: an update is written as a *new* Postgres row, so the row id has
/// already decided. It is ordered anyway so the comparator is total.)
/// `event_time` is `observed_at`, stamped after the Postgres round-trip, so
/// this is last-write-wins by observation order rather than by truth; the row
/// id tie-break only rescues the same-second case.
///
/// A listing is alive when its last event is not `removed`.
///
/// **Aliases.** Every alias differs from the columns it reads: ClickHouse
/// resolves a same-scope alias in preference to the column, so reusing a column
/// name nests aggregates at runtime (see [`crate::queries::bulk_sale_stats`]).
#[instrument(skip(ch))]
pub async fn refresh_listing_alive(ch: &ClickHouseClient) -> Result<u64, ClickHouseError> {
    let seed_cutoff = listing_events_seed_cutoff(ch).await?;
    let cursor = listing_alive_cursor(ch).await?;
    // Overlap only ever moves the lower bound backwards, and never past the
    // seed: a pre-seed `added` must stay unreplayed.
    let from = fold_lower_bound(seed_cutoff, cursor);
    // Captured before the fold so an event written while it runs is picked up
    // by the next one rather than skipped.
    let consumed_through = chrono::Utc::now();

    let client = ch
        .client()
        .clone()
        .with_setting(
            "max_execution_time",
            LISTING_ALIVE_MAX_EXECUTION_SECS.to_string(),
        )
        .with_setting(
            "max_memory_usage",
            LISTING_ALIVE_MAX_MEMORY_BYTES.to_string(),
        )
        .with_setting(
            "max_bytes_before_external_group_by",
            LISTING_ALIVE_EXTERNAL_GROUP_BY_BYTES.to_string(),
        );

    // Chunked so one statement never reads more than a day of the log, and so
    // a catch-up too long for one refresh resumes rather than restarts.
    let chunks = fold_chunks(from, consumed_through);
    let chunk_count = chunks.len();
    for (chunk_from, chunk_to) in chunks {
        client
            .query(&build_listing_last_event_fill_sql(
                chunk_from.timestamp(),
                chunk_to.timestamp(),
            ))
            .execute()
            .await?;
        set_listing_alive_cursor(ch, chunk_to).await?;
    }

    client
        .query(&build_listing_alive_refresh_sql())
        .execute()
        .await?;

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query("SELECT count() AS n FROM listing_alive FINAL WHERE alive_count > 0")
        .fetch_one()
        .await?;
    metrics::gauge!("ultros_listing_alive_last_refresh_unix")
        .set(consumed_through.timestamp() as f64);
    tracing::info!(
        keys_with_stock = count.n,
        chunks = chunk_count,
        consumed_through = %consumed_through,
        "listing_alive refresh done"
    );
    Ok(count.n)
}

/// Fold every `listing_events` row in `[from_unix, to_unix)` into one row per
/// listing in `listing_last_event`.
///
/// The chunk's own upper bound is the `ReplacingMergeTree` version, not
/// `now()`: chunks are processed in increasing time order, so `to_unix` is
/// strictly increasing both within a refresh and across refreshes, while two
/// chunks folded inside the same wall-clock second would tie under `now()` and
/// let the engine keep the earlier one.
fn build_listing_last_event_fill_sql(from_unix: i64, to_unix: i64) -> String {
    format!(
        r#"
        INSERT INTO {LISTING_LAST_EVENT_TABLE}
            (world_id, item_id, hq, listing_key, refreshed_at, event_time,
             pg_listing_id, kind_rank, quantity, retainer_id, price_per_unit,
             reviewed_at)
        SELECT
            world_id,
            item_id,
            hq,
            listing_key,
            toDateTime({to_unix}) AS folded_at,
            tupleElement(last_event, 1) AS last_event_time,
            tupleElement(last_event, 2) AS last_pg_listing_id,
            tupleElement(last_event, 3) AS last_kind_rank,
            tupleElement(last_event, 4) AS last_quantity,
            tupleElement(last_event, 5) AS last_retainer_id,
            tupleElement(last_event, 6) AS last_price,
            tupleElement(last_event, 7) AS last_reviewed_at
        FROM
        (
            SELECT
                world_id,
                item_id,
                hq,
                listing_key,
                argMax(
                    (event_time, pg_listing_id, kind_rank, quantity,
                     retainer_id, price_per_unit, reviewed_at),
                    ord
                ) AS last_event
            FROM
            (
                SELECT
                    world_id,
                    item_id,
                    hq,
                    if(listing_id != '',
                       concat('u:', listing_id),
                       concat('p:', toString(pg_listing_id))) AS listing_key,
                    event_time,
                    pg_listing_id,
                    multiIf(kind = 'removed', 3, kind = 'updated', 2, 1) AS kind_rank,
                    quantity,
                    retainer_id,
                    price_per_unit,
                    reviewed_at,
                    (event_time, pg_listing_id, kind_rank) AS ord
                FROM listing_events
                WHERE event_time >= toDateTime({from_unix})
                  AND event_time < toDateTime({to_unix})
            )
            GROUP BY world_id, item_id, hq, listing_key
        )
        "#
    )
}

fn build_listing_alive_refresh_sql() -> String {
    format!(
        r#"
        INSERT INTO listing_alive
        SELECT
            world_id,
            item_id,
            hq,
            now() AS computed_at,
            toUInt32(countIf(is_alive)) AS alive_count,
            toUInt64(sumIf(last_quantity, is_alive)) AS alive_units,
            toUInt32(uniqExactIf(last_retainer_id, is_alive)) AS distinct_retainers,
            minIf(last_reviewed_at, is_alive) AS oldest_reviewed_at,
            quantileTDigestStateIf(0.5)(age_secs, is_alive) AS age_quantile,
            toUInt32(minIf(last_price, is_alive)) AS floor_alive
        FROM
        (
            SELECT
                world_id,
                item_id,
                hq,
                kind_rank != 3 AS is_alive,
                quantity AS last_quantity,
                retainer_id AS last_retainer_id,
                reviewed_at AS last_reviewed_at,
                toUInt32(greatest(0, toInt64(now()) - toInt64(reviewed_at))) AS age_secs,
                price_per_unit AS last_price
            FROM {LISTING_LAST_EVENT_TABLE} FINAL
        )
        GROUP BY world_id, item_id, hq
        "#
    )
}

/// Compare `floor_alive` against the last `floor_changes` row for the same
/// `(world, item, hq)` and report how often the two disagree.
///
/// Issue #1342 asks the alive floor to be cross-checked against the floor
/// series; this is the bounded version of that. It compares only keys whose
/// floor moved in the last day, which keeps the aggregate proportional to a
/// day of floor moves instead of the whole `floor_changes` history — the same
/// mistake this rollup was just rewritten to stop making. Keys whose floor has
/// not moved recently are therefore *not* sampled; this is a drift signal, not
/// a proof.
///
/// Disagreement is expected to be nonzero and is deliberately not an error:
/// `floor_changes` is written by the analyzer from Postgres state while
/// `floor_alive` is replayed from the event log, and the two snapshots are
/// taken at different instants. What matters is the ratio moving.
async fn listing_alive_floor_crosscheck(
    ch: &ClickHouseClient,
) -> Result<(u64, u64), ClickHouseError> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Crosscheck {
        compared: u64,
        disagreements: u64,
    }
    let row: Crosscheck = ch
        .client()
        .query(
            r#"
            SELECT
                toUInt64(count()) AS compared,
                toUInt64(countIf(floor_alive != last_floor)) AS disagreements
            FROM
            (
                SELECT world_id, item_id, hq, floor_alive
                FROM listing_alive FINAL
                WHERE alive_count > 0
            ) a
            INNER JOIN
            (
                SELECT
                    world_id,
                    item_id,
                    hq,
                    argMax(price_per_unit, event_time) AS last_floor
                FROM floor_changes
                WHERE event_time >= now() - INTERVAL 1 DAY
                GROUP BY world_id, item_id, hq
            ) f USING (world_id, item_id, hq)
            WHERE last_floor > 0
            "#,
        )
        .fetch_one()
        .await?;
    Ok((row.compared, row.disagreements))
}

/// [`refresh_listing_alive`] plus the things that make its failure visible: a
/// hard wall-clock bound so the shared rollup ticker cannot be held up by a
/// long fold, a failure counter, and the `floor_changes` cross-check.
///
/// A refresh that fails or times out leaves `listing_alive` at its last good
/// snapshot, which the endpoint serves as a perfectly healthy-looking `200`
/// with an empty or stale `stats` array — hence the counter and the
/// `computed_at` the endpoint now returns.
async fn refresh_listing_alive_observed(ch: &ClickHouseClient) {
    let timeout = std::time::Duration::from_secs(LISTING_ALIVE_MAX_EXECUTION_SECS + 60);
    match tokio::time::timeout(timeout, refresh_listing_alive(ch)).await {
        Ok(Ok(_)) => match listing_alive_floor_crosscheck(ch).await {
            Ok((compared, disagreements)) => {
                metrics::gauge!("ultros_listing_alive_floor_compared").set(compared as f64);
                metrics::gauge!("ultros_listing_alive_floor_disagreements")
                    .set(disagreements as f64);
                tracing::info!(
                    compared,
                    disagreements,
                    "listing_alive floor cross-check done"
                );
            }
            Err(error) => {
                tracing::warn!(?error, "listing_alive floor cross-check failed");
            }
        },
        Ok(Err(error)) => {
            metrics::counter!("ultros_listing_alive_refresh_failures_total").increment(1);
            tracing::warn!(?error, "listing_alive refresh failed");
        }
        Err(_) => {
            metrics::counter!("ultros_listing_alive_refresh_failures_total").increment(1);
            tracing::warn!(
                timeout_secs = timeout.as_secs(),
                "listing_alive refresh timed out"
            );
        }
    }
}

/// Refresh `item_category_map` from xiv-gen.
///
/// Maps every item with a known ItemSearchCategory to that category's
/// top-level group (1=Weapons, 2=Tools, 3=Armor, 4=Items, 5=Housing).
/// Items with category=0 (uncategorized) are skipped — they'd just add
/// noise to the heat band.
///
/// Idempotent and runs once at startup; static-per-patch data.
pub async fn refresh_item_category_map(ch: &ClickHouseClient) -> Result<u64, ClickHouseError> {
    let data = xiv_gen_db::data_for(xiv_gen::Language::En);

    #[derive(serde::Serialize, clickhouse::Row)]
    struct MapRow {
        item_id: i32,
        category_id: u8,
    }

    let mut insert = ch.client().insert::<MapRow>("item_category_map").await?;
    let mut n: u64 = 0;
    for (item_id, item) in &data.items {
        if item.item_search_category == 0 {
            continue;
        }
        let category = data
            .item_search_categorys
            .get(&xiv_gen::ItemSearchCategoryId(item.item_search_category))
            .map(|c| c.category)
            .unwrap_or(0);
        if category == 0 {
            continue;
        }
        insert
            .write(&MapRow {
                item_id: item_id.0,
                category_id: category,
            })
            .await?;
        n += 1;
    }
    insert.end().await?;
    tracing::info!(rows = n, "item_category_map refreshed from xiv-gen");
    Ok(n)
}

/// Refresh `item_vendor_price` from xiv-gen.
///
/// xiv-gen ships with the game's Item table baked in; `Item.PriceMid` is
/// the in-game NPC vendor sell price. We pull every item with
/// `price_mid > 0` and stream them into the lookup table.
///
/// Idempotent and cheap (~thousands of rows, one bulk insert). Runs once
/// on web-server startup before the first rollup refresh; doesn't need to
/// run on a schedule because the data only changes when the game patches.
pub async fn refresh_vendor_prices(ch: &ClickHouseClient) -> Result<u64, ClickHouseError> {
    let data = xiv_gen_db::data_for(xiv_gen::Language::En);
    #[derive(serde::Serialize, clickhouse::Row)]
    struct VendorRow {
        item_id: i32,
        vendor_price: u32,
    }

    let mut insert = ch.client().insert::<VendorRow>("item_vendor_price").await?;
    let mut n: u64 = 0;
    for (item_id, item) in &data.items {
        if item.price_mid == 0 {
            continue;
        }
        insert
            .write(&VendorRow {
                item_id: item_id.0,
                vendor_price: item.price_mid,
            })
            .await?;
        n += 1;
    }
    insert.end().await?;
    tracing::info!(rows = n, "item_vendor_price refreshed from xiv-gen");
    Ok(n)
}

/// Refresh all standard windows once, then quality scores. Used by tests and
/// by an initial-seed run on first deploy.
pub async fn refresh_all(ch: &ClickHouseClient) -> Result<(), ClickHouseError> {
    // Vendor prices first — the window refresh's noise filter references
    // them. Skipping or failing this leaves the table empty, in which case
    // the LEFT JOIN inside build_refresh_sql makes the vendor-anchored
    // rule a no-op for every item.
    if let Err(e) = refresh_vendor_prices(ch).await {
        tracing::warn!(error = ?e, "vendor-price refresh failed; rollups will skip the vendor-anchored rule");
    }
    if let Err(e) = refresh_item_category_map(ch).await {
        tracing::warn!(error = ?e, "category map refresh failed; Market Heat will show no data");
    }
    for w in [1u16, 7, 30, 90] {
        refresh_window(ch, w).await?;
        refresh_sale_stats_window(ch, w).await?;
    }
    refresh_quality_scores(ch).await?;
    if let Err(e) = refresh_world_kpi_5min(ch).await {
        tracing::warn!(error = ?e, "world_kpi_5min refresh failed");
    }
    if let Err(e) = refresh_sales_hourly(ch).await {
        tracing::warn!(error = ?e, "sales_hourly refresh failed");
    }
    refresh_listing_alive_observed(ch).await;
    Ok(())
}

// Convenience re-export so callers can pass &Client directly if they have
// one (e.g. in tests that already hold a clickhouse::Client).
pub async fn refresh_window_with(client: &Client, window_days: u16) -> Result<(), ClickHouseError> {
    let sql = build_refresh_sql(window_days);
    client.query(&sql).execute().await?;
    Ok(())
}

/// Run the background scheduler that keeps `item_stats_window`,
/// `sale_stats_window`, and `item_quality_score` fresh on independent cadences:
///
/// - 1-day window:  every 15 minutes (cheap, drives the hottest dashboards)
/// - 7-day window:  every 60 minutes
/// - 30-day window: every 6 hours
/// - 90-day window: every 6 hours
/// - Quality score: every 60 minutes (depends on the 30d window)
/// - `listing_alive`: every 15 minutes (an incremental listing-event fold,
///   independent of the sale windows, and bounded by a `tokio::time::timeout`
///   so a slow fold cannot hold up the tickers it shares this task with)
///
/// All four window refreshers share a single tokio task with a `select!`
/// over named intervals, so there's no resource contention between cadences
/// and one stuck refresh can't block the others.
///
/// Does an immediate seed-refresh of all windows before entering the schedule
/// loop. The caller must ensure only one process runs this future at a time;
/// the web binary holds a Postgres advisory lock for its lifetime so adding
/// replicas cannot multiply the raw-sales scans.
pub async fn run_scheduler(ch: ClickHouseClient, token: tokio_util::sync::CancellationToken) {
    // Seed: do one pass of everything before entering the schedule. If this
    // fails (e.g. CH unreachable), log and continue — scheduled ticks retry.
    if let Err(e) = refresh_all(&ch).await {
        tracing::warn!(error = ?e, "initial rollup seed failed");
    } else {
        tracing::info!("initial rollup seed complete");
    }

    let mut tick_1d = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
    let mut tick_7d = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
    let mut tick_30d_90d = tokio::time::interval(std::time::Duration::from_secs(6 * 60 * 60));
    let mut tick_quality = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
    let mut tick_kpi = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
    // sales_hourly drives the home-page sparklines + Market Movers, so
    // it wants to be reasonably fresh. 15 min is the same cadence as the
    // 1-day rollup window and stays well ahead of the 60s browser cache
    // on the consuming endpoint.
    let mut tick_hourly = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
    // listing_alive feeds "how long has this sat on the board"; 15 min keeps
    // it inside the consuming endpoint's 5 min fresh / 30 min stale window.
    // Its refresh is awaited inline like its siblings, so it carries its own
    // timeout (see `refresh_listing_alive_observed`): three tickers share this
    // cadence and become ready together, and a fold that ran long would
    // otherwise delay tick_1d and tick_kpi by exactly that much every cycle.
    let mut tick_listing_alive = tokio::time::interval(std::time::Duration::from_secs(15 * 60));

    // All intervals fire immediately on first .tick() — burn those since
    // we already seeded above.
    tick_1d.tick().await;
    tick_7d.tick().await;
    tick_30d_90d.tick().await;
    tick_quality.tick().await;
    tick_kpi.tick().await;
    tick_hourly.tick().await;
    tick_listing_alive.tick().await;

    // If we miss a deadline (e.g. CH was slow), delay the next tick rather
    // than firing back-to-back catch-up ticks.
    tick_1d.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick_7d.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick_30d_90d.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick_quality.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick_kpi.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick_hourly.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick_listing_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
                biased;
                _ = token.cancelled() => {
                    tracing::info!("rollup scheduler exiting");
                    break;
                }
                _ = tick_1d.tick() => {
                    if let Err(e) = refresh_window(&ch, 1).await {
                        tracing::warn!(error = ?e, "1d rollup refresh failed");
                    }
                    if let Err(e) = refresh_sale_stats_window(&ch, 1).await {
                        tracing::warn!(error = ?e, "1d sale-stats refresh failed");
                    }
                }
                _ = tick_7d.tick() => {
                    if let Err(e) = refresh_window(&ch, 7).await {
                        tracing::warn!(error = ?e, "7d rollup refresh failed");
                    }
                    if let Err(e) = refresh_sale_stats_window(&ch, 7).await {
                        tracing::warn!(error = ?e, "7d sale-stats refresh failed");
                    }
                }
                _ = tick_30d_90d.tick() => {
                    if let Err(e) = refresh_window(&ch, 30).await {
                        tracing::warn!(error = ?e, "30d rollup refresh failed");
                    }
                    if let Err(e) = refresh_sale_stats_window(&ch, 30).await {
                        tracing::warn!(error = ?e, "30d sale-stats refresh failed");
                    }
                    if let Err(e) = refresh_window(&ch, 90).await {
                        tracing::warn!(error = ?e, "90d rollup refresh failed");
                    }
                    if let Err(e) = refresh_sale_stats_window(&ch, 90).await {
                        tracing::warn!(error = ?e, "90d sale-stats refresh failed");
                    }
                }
                _ = tick_quality.tick() => {
                    if let Err(e) = refresh_quality_scores(&ch).await {
                        tracing::warn!(error = ?e, "quality score refresh failed");
                    }
                }
                _ = tick_kpi.tick() => {
                    if let Err(e) = refresh_world_kpi_5min(&ch).await {
                        tracing::warn!(error = ?e, "world_kpi_5min refresh failed");
                    }
                }
                _ = tick_hourly.tick() => {
                    if let Err(e) = refresh_sales_hourly(&ch).await {
                        tracing::warn!(error = ?e, "sales_hourly refresh failed");
                    }
                }
                _ = tick_listing_alive.tick() => {
                    refresh_listing_alive_observed(&ch).await;
                }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sale_stats_refresh_is_scope_first_and_window_bounded() {
        let sql = build_sale_stats_refresh_sql(7);
        assert!(sql.contains("INTERVAL 7 DAY"));
        assert!(sql.contains("GROUP BY world_id, item_id, hq"));
        assert!(sql.contains("quantileTDigestState(0.5)(price_per_item)"));
        assert!(sql.contains("toUInt16(7) AS window_days"));
    }

    /// The shape the smoke test then proves against a server: the namespaced
    /// legacy identity fallback, the deterministic tie-break, and — the point
    /// of the rewrite — one aggregate state per listing rather than one per
    /// column, over a bounded slice of the log rather than all of it.
    #[test]
    fn listing_last_event_fill_is_bounded_and_resolves_one_state_per_listing() {
        let sql = build_listing_last_event_fill_sql(1_757_000_000, 1_757_086_400);
        assert!(sql.contains("INSERT INTO listing_last_event"));
        assert!(sql.contains("GROUP BY world_id, item_id, hq, listing_key"));
        assert!(sql.contains("WHERE event_time >= toDateTime(1757000000)"));
        // Half-open, and the chunk's own upper bound is the row version.
        assert!(sql.contains("AND event_time < toDateTime(1757086400)"));
        assert!(sql.contains("toDateTime(1757086400) AS folded_at"));
        assert!(sql.contains("concat('u:', listing_id)"));
        assert!(sql.contains("concat('p:', toString(pg_listing_id))"));
        assert!(sql.contains("multiIf(kind = 'removed', 3, kind = 'updated', 2, 1) AS kind_rank"));
        assert!(sql.contains("(event_time, pg_listing_id, kind_rank) AS ord"));
        // One `argMax` over a payload tuple, not one per column: five states
        // would each hold their own copy of the tuple comparator.
        assert_eq!(sql.matches("argMax(").count(), 1);
        assert!(sql.contains("retainer_id, price_per_unit, reviewed_at),"));
    }

    /// The alive aggregation reads the per-listing table, not the event log,
    /// and still stores a mergeable age state rather than a finalized median.
    #[test]
    fn listing_alive_refresh_reads_the_per_listing_table() {
        let sql = build_listing_alive_refresh_sql();
        assert!(sql.contains("INSERT INTO listing_alive"));
        assert!(sql.contains("GROUP BY world_id, item_id, hq\n"));
        assert!(sql.contains("FROM listing_last_event FINAL"));
        // The whole point: no reference to the raw event log.
        assert!(!sql.contains("listing_events"));
        assert!(sql.contains("kind_rank != 3 AS is_alive"));
        assert!(sql.contains("quantileTDigestStateIf(0.5)(age_secs, is_alive) AS age_quantile"));
        // `-StateIf`, never `-IfState`: the latter's state type is
        // `quantileTDigestIf`, which the column would refuse.
        assert!(!sql.contains("IfState"));
    }

    /// The overlap may widen the fold's window, but never past the seed: a
    /// pre-seed `added` whose removal was never recorded must stay unreplayed.
    #[test]
    fn the_overlap_never_reaches_behind_the_seed() {
        let seed = chrono::DateTime::from_timestamp(1_757_000_000, 0).unwrap();
        // Never folded, and a cursor barely past the seed: start at the seed.
        assert_eq!(fold_lower_bound(seed, None), seed);
        assert_eq!(
            fold_lower_bound(seed, Some(seed + chrono::TimeDelta::seconds(60))),
            seed
        );
        // A cursor well past the seed keeps the full overlap.
        let cursor = seed + chrono::TimeDelta::seconds(LISTING_ALIVE_OVERLAP_SECS * 3);
        assert_eq!(
            fold_lower_bound(seed, Some(cursor)),
            cursor - chrono::TimeDelta::seconds(LISTING_ALIVE_OVERLAP_SECS)
        );
    }

    /// Chunks tile the window exactly once, in order, with no gap and no
    /// overlap — the cursor is advanced to each chunk's end, so a gap would be
    /// events silently never folded.
    #[test]
    fn fold_chunks_tile_the_window_in_order() {
        let start = chrono::DateTime::from_timestamp(1_757_000_000, 0).unwrap();
        let day = chrono::TimeDelta::seconds(LISTING_ALIVE_FOLD_CHUNK_SECS);

        // Steady state: one tick's worth is a single chunk.
        let short = fold_chunks(start, start + chrono::TimeDelta::seconds(900));
        assert_eq!(short.len(), 1);
        assert_eq!(short[0].1, start + chrono::TimeDelta::seconds(900));

        // Catch-up: split, contiguous, last chunk ends exactly at the cursor.
        let end = start + day * 3 + chrono::TimeDelta::seconds(60);
        let chunks = fold_chunks(start, end);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].0, start);
        assert_eq!(chunks.last().unwrap().1, end);
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
            assert!(pair[0].1 > pair[0].0);
        }

        // Already caught up (or a clock that went backwards): fold nothing.
        assert!(fold_chunks(start, start).is_empty());
        assert!(fold_chunks(start, start - day).is_empty());
    }
}
