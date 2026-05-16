//! Read-side query helpers used by the analyzer and dashboard endpoints.
//!
//! The analyzer is the primary consumer of ClickHouse. It calls the helpers
//! here from its deep-scan path to refine Pass-1 (in-RAM) results with
//! statistically sound numbers from `item_stats_window` + `item_quality_score`.
//!
//! The Market Pulse home-page tile uses [`market_pulse`].

use clickhouse::Row;
use serde::Deserialize;
use ultros_api_types::trends::ConfidenceBand;

use crate::{ClickHouseClient, ClickHouseError};

/// Rolled-up KPIs for one world: "today" (last 24h) + "yesterday"
/// (24-48h ago). The frontend renders delta-vs-yesterday on each tile.
#[derive(Debug, Clone, Row, Deserialize, serde::Serialize)]
pub struct MarketPulse {
    pub world_id: i32,
    pub sales_today: u64,
    pub sales_yesterday: u64,
    pub gil_volume_today: u64,
    pub gil_volume_yesterday: u64,
    pub unit_volume_today: u64,
    pub unit_volume_yesterday: u64,
}

impl MarketPulse {
    /// % change today vs yesterday for sale_count. Returns `None` when
    /// yesterday was zero (avoids division-by-zero; UI treats as "—").
    pub fn sales_delta_pct(&self) -> Option<f32> {
        pct_delta(self.sales_today, self.sales_yesterday)
    }
    pub fn gil_volume_delta_pct(&self) -> Option<f32> {
        pct_delta(self.gil_volume_today, self.gil_volume_yesterday)
    }
    pub fn unit_volume_delta_pct(&self) -> Option<f32> {
        pct_delta(self.unit_volume_today, self.unit_volume_yesterday)
    }
}

fn pct_delta(today: u64, yesterday: u64) -> Option<f32> {
    if yesterday == 0 {
        None
    } else {
        Some(((today as f64 - yesterday as f64) / yesterday as f64 * 100.0) as f32)
    }
}

/// One row per item with 24 hourly buckets of VWAP. Used by the home-page
/// sparklines + Market Movers.
///
/// Buckets that contained no sales are emitted as zero so the array length
/// is always exactly the requested window length — the frontend can index
/// into it without worrying about gaps. A `points` array is more compact
/// than `Vec<HourlyBucket>` because the sparkline renderer only needs the
/// price points, not the timestamps (they're implied by index + window
/// length).
#[derive(Debug, Clone, Row, Deserialize, serde::Serialize)]
pub struct SparklineRow {
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    /// Trailing-window VWAP per hour, oldest first, length = hours requested.
    pub points: Vec<u32>,
    /// First non-zero point in the series (oldest price), for %change math.
    pub first_price: u32,
    /// Last non-zero point in the series (newest price), for %change math.
    pub last_price: u32,
}

impl SparklineRow {
    /// Pct change from first to last, or 0 when one side is missing.
    pub fn pct_change(&self) -> f32 {
        if self.first_price == 0 || self.last_price == 0 {
            return 0.0;
        }
        ((self.last_price as f64 - self.first_price as f64) / self.first_price as f64 * 100.0)
            as f32
    }
}

/// Batch fetch trailing-24h hourly VWAP series for many (item, hq, world)
/// tuples. Used by the home-page Market Movers + Top Deals retrofit.
///
/// `hours` controls window length (default 24). The query right-aligns
/// each row to "now": bucket 0 is N hours ago, bucket N-1 is the latest
/// completed hour.
pub async fn sparklines_batch(
    ch: &ClickHouseClient,
    requests: &[(i32, u8, i32)],
    hours: u16,
) -> Result<Vec<SparklineRow>, ClickHouseError> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let mut tuples = String::with_capacity(requests.len() * 24);
    for (i, (item_id, hq, world_id)) in requests.iter().enumerate() {
        if i > 0 {
            tuples.push(',');
        }
        tuples.push_str(&format!("({item_id},{hq},{world_id})"));
    }

    // The CTE builds a complete hour grid right-aligned to now() so missing
    // hours appear as 0 rather than being dropped (which would break index
    // alignment client-side). arrayMap+arrayFill could close gaps with
    // last-known value, but for sparklines a zero gap reads "no trade in
    // this hour" honestly — preferred over a misleading flat line.
    let sql = format!(
        r#"
        WITH
            req AS (
                -- CH infers UInt8 from small literal tuples; cast to the
                -- column types of sales_hourly so the LEFT JOIN below
                -- matches without implicit conversion, and so the
                -- SparklineRow deserializer sees Int32/UInt8/Int32.
                SELECT
                    toInt32(tupleElement(t, 1)) AS item_id,
                    toUInt8(tupleElement(t, 2)) AS hq,
                    toInt32(tupleElement(t, 3)) AS world_id
                FROM (SELECT arrayJoin([{tuples}]) AS t)
            ),
            buckets AS (
                SELECT toStartOfInterval(now() - INTERVAL n HOUR, INTERVAL 1 HOUR) AS bucket,
                       (? - 1 - n) AS slot
                FROM (SELECT arrayJoin(range(0, ?)) AS n)
            ),
            grid AS (
                SELECT r.item_id, r.hq, r.world_id, b.bucket, b.slot
                FROM req r
                CROSS JOIN buckets b
            ),
            data AS (
                SELECT g.item_id, g.hq, g.world_id, g.slot,
                       coalesce(s.vwap, 0) AS vwap
                FROM grid g
                LEFT JOIN sales_hourly s FINAL
                  ON g.item_id = s.item_id
                 AND g.hq = s.hq
                 AND g.world_id = s.world_id
                 AND g.bucket = s.bucket
            )
        SELECT
            item_id, toUInt8(hq) AS hq, world_id,
            groupArray(vwap) AS points,
            -- first/last non-zero in the array — drives %change math.
            arrayElement(
                arrayFilter(x -> x > 0, points),
                1
            ) AS first_price,
            arrayElement(
                reverse(arrayFilter(x -> x > 0, points)),
                1
            ) AS last_price
        FROM (
            SELECT * FROM data
            ORDER BY item_id, hq, world_id, slot
        )
        GROUP BY item_id, hq, world_id
        "#
    );

    let rows: Vec<SparklineRow> = ch
        .client()
        .query(&sql)
        .bind(hours as u32)
        .bind(hours as u32)
        .fetch_all()
        .await?;
    Ok(rows)
}

/// Per-item % change in VWAP from N hours ago to now, with the most-recent
/// sale price and volume. Drives the Market Movers home page section
/// (Rising / Falling / High Volume tabs).
#[derive(Debug, Clone, Row, Deserialize, serde::Serialize)]
pub struct MoverRow {
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub price_now: u32,
    pub pct_change_24h: f32,
    pub volume_24h: u32,
}

/// Fetch the top N movers for a world.
///
/// `direction` controls ordering: "rising" (pct desc), "falling" (pct asc),
/// "volume" (raw 24h volume desc). All three return up to `limit` rows.
///
/// Filtered to items with at least `min_samples_24h` to weed out items
/// where a single sale would dominate the metric.
pub async fn top_movers(
    ch: &ClickHouseClient,
    world_id: i32,
    direction: MoverDirection,
    limit: u32,
) -> Result<Vec<MoverRow>, ClickHouseError> {
    let order_by = match direction {
        MoverDirection::Rising => "pct_change_24h DESC",
        MoverDirection::Falling => "pct_change_24h ASC",
        MoverDirection::Volume => "volume_24h DESC",
    };
    // argMin/argMax pick the value at the earliest/latest bucket per
    // group — exactly the first vs last VWAP we need for %change. Items
    // with < 3 sales in 24h are filtered out so a single noisy trade
    // doesn't dominate the rankings.
    let sql = format!(
        r#"
        SELECT
            item_id, toUInt8(hq) AS hq, world_id,
            argMax(vwap, bucket) AS price_now,
            if(argMin(vwap, bucket) > 0,
               toFloat32((toFloat64(argMax(vwap, bucket))
                          - toFloat64(argMin(vwap, bucket)))
                         / toFloat64(argMin(vwap, bucket)) * 100),
               toFloat32(0)) AS pct_change_24h,
            toUInt32(sum(unit_volume)) AS volume_24h
        FROM sales_hourly FINAL
        WHERE world_id = toInt32(?)
          AND bucket > now() - INTERVAL 24 HOUR
          AND vwap > 0
        GROUP BY item_id, hq, world_id
        HAVING sum(sale_count) >= 3
           AND argMin(vwap, bucket) > 0
           AND argMax(vwap, bucket) > 0
        ORDER BY {order_by}
        LIMIT ?
        "#
    );

    let rows: Vec<MoverRow> = ch
        .client()
        .query(&sql)
        .bind(world_id)
        .bind(limit)
        .fetch_all()
        .await?;
    Ok(rows)
}

/// Which sort to apply for [`top_movers`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoverDirection {
    Rising,
    Falling,
    Volume,
}

/// One row of the home-page Market Heat band. The frontend buckets
/// `avg_pct_change_24h` into Hot/Warm/Stable/Cool labels with a colored
/// indicator. `gil_volume_24h` is shown as a sparkline-adjacent stat.
#[derive(Debug, Clone, Row, Deserialize, serde::Serialize)]
pub struct CategoryHeatRow {
    pub category_id: u8,
    pub item_count: u32,
    pub avg_pct_change_24h: f32,
    pub gil_volume_24h: u64,
}

/// Fetch the Market Heat rollup for a world.
///
/// For each (category, world), compute the volume-weighted average of
/// each item's pct_change over the trailing 24h. The weighting avoids
/// a sleepy-but-volatile item dragging a whole category's signal:
/// categories with one item swinging 1000% don't go "Hot" unless that
/// item is also actually moving volume.
pub async fn category_heat(
    ch: &ClickHouseClient,
    world_id: i32,
) -> Result<Vec<CategoryHeatRow>, ClickHouseError> {
    // Inner CTE aliases `gil_volume_24h` per item; the outer aggregate
    // can't reuse that name without ClickHouse parsing it as nested
    // aggregation. Inner column = `item_gil_volume`, outer aggregate =
    // `gil_volume_24h`.
    let sql = r#"
        WITH per_item AS (
            SELECT s.item_id, m.category_id,
                   argMin(s.vwap, s.bucket) AS first_vwap,
                   argMax(s.vwap, s.bucket) AS last_vwap,
                   sum(toUInt64(s.unit_volume) * toUInt64(s.vwap)) AS item_gil_volume,
                   sum(s.sale_count) AS sales_24h
            FROM sales_hourly s FINAL
            INNER JOIN item_category_map m FINAL USING (item_id)
            WHERE s.world_id = toInt32(?)
              AND s.bucket > now() - INTERVAL 24 HOUR
              AND s.vwap > 0
            GROUP BY s.item_id, m.category_id
            HAVING first_vwap > 0 AND last_vwap > 0 AND sales_24h >= 2
        )
        SELECT
            toUInt8(category_id) AS category_id,
            toUInt32(count()) AS item_count,
            -- Volume-weighted average pct change. Items that don't move
            -- volume have negligible weight; items with serious traffic
            -- dominate the category's signal.
            toFloat32(
                sum(toFloat64(item_gil_volume)
                    * (toFloat64(last_vwap) - toFloat64(first_vwap))
                    / toFloat64(first_vwap)) * 100.0
                / greatest(sum(toFloat64(item_gil_volume)), 1)
            ) AS avg_pct_change_24h,
            sum(item_gil_volume) AS gil_volume_24h
        FROM per_item
        GROUP BY category_id
        ORDER BY category_id
    "#;
    let rows: Vec<CategoryHeatRow> = ch.client().query(sql).bind(world_id).fetch_all().await?;
    Ok(rows)
}

/// Fetch today's + yesterday's rolled-up KPIs for a world.
///
/// One query for both windows via conditional `sumIf` — the alternative
/// (two queries) would double the round-trip on every home-page load.
pub async fn market_pulse(
    ch: &ClickHouseClient,
    world_id: i32,
) -> Result<MarketPulse, ClickHouseError> {
    let row: MarketPulse = ch
        .client()
        .query(
            "SELECT
                toInt32(?) AS world_id,
                sumIf(sale_count,  bucket >  now() - INTERVAL 24 HOUR)
                    AS sales_today,
                sumIf(sale_count,  bucket <= now() - INTERVAL 24 HOUR
                                AND bucket >  now() - INTERVAL 48 HOUR)
                    AS sales_yesterday,
                sumIf(gil_volume,  bucket >  now() - INTERVAL 24 HOUR)
                    AS gil_volume_today,
                sumIf(gil_volume,  bucket <= now() - INTERVAL 24 HOUR
                                AND bucket >  now() - INTERVAL 48 HOUR)
                    AS gil_volume_yesterday,
                sumIf(unit_volume, bucket >  now() - INTERVAL 24 HOUR)
                    AS unit_volume_today,
                sumIf(unit_volume, bucket <= now() - INTERVAL 24 HOUR
                                AND bucket >  now() - INTERVAL 48 HOUR)
                    AS unit_volume_yesterday
            FROM world_kpi_5min FINAL
            WHERE world_id = ?
              AND bucket > now() - INTERVAL 48 HOUR",
        )
        .bind(world_id)
        .bind(world_id)
        .fetch_one()
        .await?;
    Ok(row)
}

/// One row of deep-scan data for a single (item_id, hq, world_id) tuple at
/// a given window. Maps the analyzer's enrichment fields directly to the
/// rollup table columns.
///
/// Missing data (e.g. item not in the rollup yet) is represented by
/// `quality_score == 0` and `confidence_band == Unknown`. Callers should
/// treat that as "no deep-scan available; show Pass-1 result with low
/// confidence" rather than as a hard error.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct DeepScan {
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub window_days: u16,

    /// Volume-weighted average price on the cleaned sample.
    pub vwap: u32,
    /// Cleaned-sample median (used when fewer samples than the percentile
    /// quantiles can resolve).
    pub p50: u32,
    /// 10th/25th/75th/90th percentile prices, for chart bands.
    pub p10: u32,
    pub p25: u32,
    pub p75: u32,
    pub p90: u32,
    pub median_abs_deviation: u32,

    /// Total samples in the window pre-filter.
    pub sample_size: u32,
    /// Samples that survived both noise-filter layers.
    pub cleaned_sample_size: u32,
    /// Excluded count = sample_size - cleaned_sample_size.
    pub excluded_count: u32,

    pub unit_volume: u64,
    pub gil_volume: u64,
    pub unique_buyers: u32,

    /// 0-100 trustworthiness score.
    pub quality_score: u8,
    /// Bucketed confidence band for the analyzer to branch on.
    pub confidence_band_raw: String,
    /// 0.0-1.0 — share of samples flagged as noise.
    pub launder_suspicion_pct: f32,
}

impl DeepScan {
    /// Strongly-typed band derived from the raw enum string. Falls back to
    /// `Unknown` for unrecognized values (shouldn't happen but keeps the
    /// analyzer resilient to schema drift).
    pub fn confidence_band(&self) -> ConfidenceBand {
        match self.confidence_band_raw.as_str() {
            "high" => ConfidenceBand::High,
            "medium" => ConfidenceBand::Medium,
            "low" => ConfidenceBand::Low,
            "unusable" => ConfidenceBand::Unusable,
            _ => ConfidenceBand::Unknown,
        }
    }

    /// Where `current_price` falls in the cleaned 30-day distribution
    /// (0-100). Uses linear interpolation between the p10/p25/p50/p75/p90
    /// breakpoints — good enough for a UI percentile chip without paying
    /// for a separate quantile query per item.
    pub fn price_percentile(&self, current_price: u32) -> u8 {
        let breakpoints: [(u32, u8); 5] = [
            (self.p10, 10),
            (self.p25, 25),
            (self.p50, 50),
            (self.p75, 75),
            (self.p90, 90),
        ];
        if current_price <= self.p10 {
            return 0;
        }
        if current_price >= self.p90 {
            return 100;
        }
        for w in breakpoints.windows(2) {
            let (lo_p, lo_pct) = w[0];
            let (hi_p, hi_pct) = w[1];
            if current_price >= lo_p && current_price <= hi_p {
                if hi_p == lo_p {
                    return lo_pct;
                }
                let span = (hi_p - lo_p) as f32;
                let delta = (current_price - lo_p) as f32;
                let pct = lo_pct as f32 + (delta / span) * (hi_pct - lo_pct) as f32;
                return pct.round() as u8;
            }
        }
        50
    }
}

/// Batch fetch deep-scan data for many (item, hq, world) tuples at a
/// single window. Used by the analyzer to enrich a page of Pass-1 results
/// in one round trip rather than N.
///
/// Caller passes the request as separate parallel vectors (item_ids,
/// hqs, world_ids) because ClickHouse parameter binding doesn't support
/// arrays-of-tuples cleanly across the HTTP interface. The query uses an
/// `IN (SELECT ...)` against the unioned triples table built inline.
pub async fn deep_scan_batch(
    ch: &ClickHouseClient,
    window_days: u16,
    requests: &[(i32, u8, i32)],
) -> Result<Vec<DeepScan>, ClickHouseError> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    // Build a tuple-list expression. Each item_id is i32 (max 10 chars),
    // hq is 0/1, world_id is i32. With N=50 tuples that's ~1.5KB of SQL
    // — well under ClickHouse's default max_query_size of 256KB.
    let mut tuples = String::with_capacity(requests.len() * 24);
    for (i, (item_id, hq, world_id)) in requests.iter().enumerate() {
        if i > 0 {
            tuples.push(',');
        }
        tuples.push_str(&format!("({item_id},{hq},{world_id})"));
    }

    let sql = format!(
        "SELECT w.item_id, w.hq, w.world_id, w.window_days,
                w.vwap, w.p50, w.p10, w.p25, w.p75, w.p90,
                w.median_abs_deviation,
                w.sample_size, w.cleaned_sample_size, w.excluded_count,
                w.unit_volume, w.gil_volume, w.unique_buyers,
                if(q.computed_at > 0, q.quality_score, toUInt8(0)) AS quality_score,
                if(q.computed_at > 0, toString(q.confidence_band), 'unknown')
                    AS confidence_band_raw,
                if(q.computed_at > 0, q.launder_suspicion_pct, toFloat32(0))
                    AS launder_suspicion_pct
         FROM item_stats_window w FINAL
         LEFT JOIN item_quality_score q FINAL
           ON w.item_id = q.item_id AND w.hq = q.hq AND w.world_id = q.world_id
         WHERE (w.item_id, w.hq, w.world_id) IN ({tuples})
           AND w.window_days = ?"
    );

    let rows: Vec<DeepScan> = ch
        .client()
        .query(&sql)
        .bind(window_days)
        .fetch_all()
        .await?;
    Ok(rows)
}

/// Single-item convenience wrapper.
pub async fn deep_scan_one(
    ch: &ClickHouseClient,
    item_id: i32,
    hq: bool,
    world_id: i32,
    window_days: u16,
) -> Result<Option<DeepScan>, ClickHouseError> {
    let rows = deep_scan_batch(ch, window_days, &[(item_id, hq as u8, world_id)]).await?;
    Ok(rows.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> DeepScan {
        DeepScan {
            item_id: 1,
            hq: 0,
            world_id: 40,
            window_days: 30,
            vwap: 500,
            p10: 200,
            p25: 350,
            p50: 500,
            p75: 700,
            p90: 1000,
            median_abs_deviation: 50,
            sample_size: 100,
            cleaned_sample_size: 95,
            excluded_count: 5,
            unit_volume: 200,
            gil_volume: 100_000,
            unique_buyers: 20,
            quality_score: 80,
            confidence_band_raw: "high".to_string(),
            launder_suspicion_pct: 0.05,
        }
    }

    #[test]
    fn price_percentile_below_p10_floors_at_zero() {
        let d = fixture();
        assert_eq!(d.price_percentile(100), 0);
        assert_eq!(d.price_percentile(200), 0);
    }

    #[test]
    fn price_percentile_above_p90_ceils_at_hundred() {
        let d = fixture();
        assert_eq!(d.price_percentile(1000), 100);
        assert_eq!(d.price_percentile(5000), 100);
    }

    #[test]
    fn price_percentile_at_breakpoint_returns_band() {
        let d = fixture();
        // p25 = 350 → exactly 25
        assert_eq!(d.price_percentile(350), 25);
        // p50 = 500 → exactly 50
        assert_eq!(d.price_percentile(500), 50);
        // p75 = 700 → exactly 75
        assert_eq!(d.price_percentile(700), 75);
    }

    #[test]
    fn price_percentile_interpolates_between_breakpoints() {
        let d = fixture();
        // Midway between p25 (350) and p50 (500): expect ~ 37
        // 350 -> 25, 500 -> 50, span = 150, midpoint = 425
        // pct = 25 + (75/150)*25 = 25 + 12.5 = 37.5 → rounds to 38
        assert_eq!(d.price_percentile(425), 38);
    }

    #[test]
    fn confidence_band_parses_known_values() {
        let mut d = fixture();
        d.confidence_band_raw = "high".to_string();
        assert_eq!(d.confidence_band(), ConfidenceBand::High);
        d.confidence_band_raw = "medium".to_string();
        assert_eq!(d.confidence_band(), ConfidenceBand::Medium);
        d.confidence_band_raw = "low".to_string();
        assert_eq!(d.confidence_band(), ConfidenceBand::Low);
        d.confidence_band_raw = "unusable".to_string();
        assert_eq!(d.confidence_band(), ConfidenceBand::Unusable);
        d.confidence_band_raw = "garbage".to_string();
        assert_eq!(d.confidence_band(), ConfidenceBand::Unknown);
    }
}
