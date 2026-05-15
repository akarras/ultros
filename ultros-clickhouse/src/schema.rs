//! ClickHouse DDL. Idempotent; called on every web-server startup.
//!
//! Tables defined here:
//! - `sales` — raw mirror of `sale_history`
//! - `item_stats_window` (Task 1.1) — multi-window aggregates
//! - `item_quality_score` (Task 1.1) — trustworthiness per item
//! - `_backfill_state` (Task 0.6) — resumable backfill cursor

use clickhouse::Client;

use crate::ClickHouseError;

pub async fn apply(client: &Client) -> Result<(), ClickHouseError> {
    apply_sales_table(client).await?;
    apply_item_stats_window(client).await?;
    apply_item_quality_score(client).await?;
    apply_item_vendor_price(client).await?;
    Ok(())
}

/// Raw mirror of Postgres `sale_history`.
///
/// Engine: `ReplacingMergeTree(inserted_at)` on the natural key
/// `(item_id, hq, world_id, sold_date, buying_character_id)` makes dual-writes
/// idempotent — replaying the event bus or re-running backfill against an
/// already-populated partition is a no-op on merge.
///
/// Partitioning by month keeps retention / drop operations cheap and aligns
/// with the backfill chunk size.
///
/// `total_gil` is a MATERIALIZED column computed on insert from
/// `price_per_item * quantity` so consumers don't have to do the math and the
/// value lives compressed on disk like any other column.
async fn apply_sales_table(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS sales (
                pg_id                Int32,
                sold_date            DateTime,
                inserted_at          DateTime DEFAULT now(),
                item_id              Int32,
                hq                   UInt8,
                world_id             Int32,
                price_per_item       UInt32,
                quantity             UInt16,
                total_gil            UInt64 MATERIALIZED toUInt64(price_per_item) * toUInt64(quantity),
                buying_character_id  Int64,
                buyer_name           LowCardinality(String) DEFAULT ''
            )
            ENGINE = ReplacingMergeTree(inserted_at)
            PARTITION BY toYYYYMM(sold_date)
            ORDER BY (item_id, hq, world_id, sold_date, pg_id)
            SETTINGS index_granularity = 8192
            "#,
        )
        .execute()
        .await?;
    Ok(())
}

/// Multi-window aggregate per `(item_id, hq, world_id)`. The analyzer's
/// deep-scan reads from this table; rows are produced by
/// [`crate::rollups::refresh_window`] and the cleaned-sales filter applied
/// inline at refresh time.
///
/// `window_days` is the window size: 1, 7, 30, 90.
async fn apply_item_stats_window(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS item_stats_window (
                item_id              Int32,
                hq                   UInt8,
                world_id             Int32,
                window_days          UInt16,
                computed_at          DateTime,

                -- Total rows in the window, pre-filter
                sample_size          UInt32,
                -- Rows that survived both noise-filter layers
                cleaned_sample_size  UInt32,
                -- sample_size - cleaned_sample_size
                excluded_count       UInt32,

                -- Volume-weighted average price, computed on cleaned data
                vwap                 UInt32,
                p10                  UInt32,
                p25                  UInt32,
                p50                  UInt32,
                p75                  UInt32,
                p90                  UInt32,
                -- Median absolute deviation, used by the analyzer to gauge
                -- per-item price volatility
                median_abs_deviation UInt32,

                -- All cleaned, computed on the cleaned set
                unit_volume          UInt64,
                gil_volume           UInt64,
                sale_count           UInt32,
                unique_buyers        UInt32
            )
            ENGINE = ReplacingMergeTree(computed_at)
            ORDER BY (item_id, hq, world_id, window_days)
            SETTINGS index_granularity = 8192
            "#,
        )
        .execute()
        .await?;
    Ok(())
}

/// Static lookup of in-game NPC vendor sell prices, keyed by item_id.
///
/// Populated once at startup from `xiv-gen` via
/// [`crate::rollups::refresh_vendor_prices`]. Used by the rollup filter as
/// a ground-truth floor — a single-unit sale priced >100× the vendor price
/// is launder with near-certainty, because the buyer could just walk to an
/// NPC instead.
///
/// Only items with `Item.PriceMid > 0` get a row here. Items that aren't
/// sold by any NPC (gear, materia, raid drops) are absent, and the rollup
/// filter degrades to the existing relative-price checks for those.
async fn apply_item_vendor_price(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS item_vendor_price (
                item_id      Int32,
                vendor_price UInt32,
                updated_at   DateTime DEFAULT now()
            )
            ENGINE = ReplacingMergeTree(updated_at)
            ORDER BY item_id
            "#,
        )
        .execute()
        .await?;
    Ok(())
}

/// Trustworthiness score per item, derived from `item_stats_window`.
/// The analyzer uses this single column to decide whether to surface,
/// downrank, or suppress a recommendation.
///
/// `confidence_band`:
///   1 high      → enough samples + buyer diversity, low launder suspicion
///   2 medium    → usable but flagged in UI
///   3 low       → only return as a rough hint
///   4 unusable  → suppress from recommendations entirely
async fn apply_item_quality_score(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS item_quality_score (
                item_id               Int32,
                hq                    UInt8,
                world_id              Int32,
                computed_at           DateTime,
                quality_score         UInt8,
                confidence_band       Enum8('high'=1,'medium'=2,'low'=3,'unusable'=4),
                sample_size_30d       UInt32,
                launder_suspicion_pct Float32
            )
            ENGINE = ReplacingMergeTree(computed_at)
            ORDER BY (item_id, hq, world_id)
            SETTINGS index_granularity = 8192
            "#,
        )
        .execute()
        .await?;
    Ok(())
}
