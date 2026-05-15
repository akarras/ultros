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

use crate::{ClickHouseClient, ClickHouseError};

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
                SELECT s.item_id, s.hq, s.world_id,
                       s.price_per_item, s.quantity, s.total_gil,
                       s.buying_character_id, m.p50_raw,
                       -- Layer 2 (heuristic): single-unit off-market prices
                       (s.quantity = 1 AND s.price_per_item > 10 * m.p50_raw)
                       OR (s.quantity = 1 AND s.price_per_item * 10 < m.p50_raw)
                       AS l2_excluded
                FROM window_sales s
                INNER JOIN medians m USING (item_id, hq, world_id)
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
                multiIf(
                    quality_score >= 75, CAST('high'   AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4)),
                    quality_score >= 40, CAST('medium' AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4)),
                    quality_score >= 15, CAST('low'    AS Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4)),
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

/// Refresh all standard windows once, then quality scores. Used by tests and
/// by an initial-seed run on first deploy.
pub async fn refresh_all(ch: &ClickHouseClient) -> Result<(), ClickHouseError> {
    for w in [1u16, 7, 30, 90] {
        refresh_window(ch, w).await?;
    }
    refresh_quality_scores(ch).await?;
    Ok(())
}

// Convenience re-export so callers can pass &Client directly if they have
// one (e.g. in tests that already hold a clickhouse::Client).
pub async fn refresh_window_with(client: &Client, window_days: u16) -> Result<(), ClickHouseError> {
    let sql = build_refresh_sql(window_days);
    client.query(&sql).execute().await?;
    Ok(())
}
