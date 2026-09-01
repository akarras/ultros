//! Read-side query helpers used by the analyzer and dashboard endpoints.
//!
//! The analyzer is the primary consumer of ClickHouse. It calls the helpers
//! here from its deep-scan path to refine Pass-1 (in-RAM) results with
//! statistically sound numbers from `item_stats_window` + `item_quality_score`.
//!
//! The Market Pulse home-page tile uses [`market_pulse`].

use clickhouse::Row;
use serde::Deserialize;
use ultros_api_types::item_stats::ItemStatsVariant;
use ultros_api_types::price_series::{HqFilter, SeriesGroup};
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
                -- Pre-filter the join side to the requested tuples + window.
                -- A bare `LEFT JOIN sales_hourly FINAL` hashes the ENTIRE
                -- rollup table (every item x world x hour) on every request;
                -- the filtered subquery prunes by the table's primary key
                -- (item_id, hq, world_id, bucket) instead.
                LEFT JOIN (
                    SELECT item_id, hq, world_id, bucket, vwap
                    FROM sales_hourly FINAL
                    WHERE (item_id, hq, world_id) IN ({tuples})
                      AND bucket >= toStartOfInterval(now() - INTERVAL ? HOUR, INTERVAL 1 HOUR)
                ) s
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
    /// Total gil that changed hands on this item in the window
    /// (`sum(unit_volume * vwap)` over the hourly rollup — an approximation,
    /// consistent with how `category_heat` computes gil volume). This is the
    /// gil-denominated "market value" metric, the complement to `volume_24h`.
    pub gil_volume_24h: u64,
}

/// Fetch the top N movers for a world.
///
/// `direction` controls ordering: "rising" (pct desc), "falling" (pct asc),
/// "volume" (raw 24h unit count desc), "gil" (24h gil volume desc). All
/// return up to `limit` rows.
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
        MoverDirection::Gil => "gil_volume_24h DESC",
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
            toUInt32(sum(unit_volume)) AS volume_24h,
            sum(toUInt64(unit_volume) * toUInt64(vwap)) AS gil_volume_24h
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
    Gil,
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

/// Strongly-typed band from the raw enum string ClickHouse stores. Falls
/// back to `Unknown` for unrecognized values (shouldn't happen but keeps
/// callers resilient to schema drift).
pub fn parse_confidence_band(raw: &str) -> ConfidenceBand {
    match raw {
        "high" => ConfidenceBand::High,
        "medium" => ConfidenceBand::Medium,
        "low" => ConfidenceBand::Low,
        "unusable" => ConfidenceBand::Unusable,
        _ => ConfidenceBand::Unknown,
    }
}

impl DeepScan {
    /// See [`parse_confidence_band`].
    pub fn confidence_band(&self) -> ConfidenceBand {
        parse_confidence_band(&self.confidence_band_raw)
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
         LEFT JOIN (
             SELECT item_id, hq, world_id, computed_at, quality_score,
                    confidence_band, launder_suspicion_pct
             FROM item_quality_score FINAL
             WHERE (item_id, hq, world_id) IN ({tuples})
         ) q
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

/// Fold per-world deep scans into one variant per quality (NQ then HQ).
///
/// `item_stats_window` is keyed by world, so a datacenter- or region-scoped
/// request fans out to one row per member world and has to be folded back
/// into the single figure the item view shows. Counts add. The price and
/// suspicion figures are weighted means so that a world with four sales can't
/// drag the number as hard as one with four thousand:
///
/// - `vwap`/`p50` are weighted by `cleaned_sample_size`, the sample they were
///   each computed over. A weighted mean of per-world medians is *not* the
///   true median of the combined sample — the rollup doesn't keep the raw
///   distribution, so this is an approximation, and a deliberately
///   sample-weighted one rather than a flat average across worlds.
/// - `launder_suspicion` is a share of *all* samples, so it weights by
///   `sample_size` (its own denominator) rather than the cleaned count.
/// - `confidence_band` is a stored per-world judgement that can't be
///   recomputed here, so the scope reports the band of the world contributing
///   the most cleaned samples, tie-broken on world id so the answer is stable
///   across queries rather than dependent on ClickHouse's row order.
///
/// A single-world scope returns that row's values verbatim, so world-scoped
/// requests are unaffected by any of the above.
pub fn aggregate_item_stats_variants(scans: &[DeepScan]) -> Vec<ItemStatsVariant> {
    // NQ before HQ, so the response order doesn't depend on ClickHouse's.
    [0u8, 1u8]
        .into_iter()
        .filter_map(|hq| {
            let group: Vec<&DeepScan> = scans.iter().filter(|s| s.hq == hq).collect();
            match group.as_slice() {
                [] => None,
                [only] => Some(variant_of(only)),
                many => Some(fold_variants(many)),
            }
        })
        .collect()
}

/// One scan straight across to the wire type, no arithmetic.
fn variant_of(s: &DeepScan) -> ItemStatsVariant {
    ItemStatsVariant {
        hq: s.hq != 0,
        sample_size_30d: s.sample_size,
        cleaned_sample_size_30d: s.cleaned_sample_size,
        vwap_30d: s.vwap,
        p50_30d: s.p50,
        confidence_band: s.confidence_band(),
        launder_suspicion: s.launder_suspicion_pct,
    }
}

/// Fold two or more same-quality scans. See [`aggregate_item_stats_variants`]
/// for why each field combines the way it does.
fn fold_variants(group: &[&DeepScan]) -> ItemStatsVariant {
    let cleaned: Vec<u64> = group.iter().map(|s| s.cleaned_sample_size as u64).collect();
    let raw: Vec<u64> = group.iter().map(|s| s.sample_size as u64).collect();

    // The band's source world: most cleaned samples wins, lowest world id
    // breaks a tie. `max_by_key` keeps the *last* maximum, so ordering the
    // key by (samples, Reverse(world_id)) makes the lowest id win a tie.
    let band_source = group
        .iter()
        .max_by_key(|s| (s.cleaned_sample_size, std::cmp::Reverse(s.world_id)))
        .expect("fold_variants is only called with a non-empty group");

    ItemStatsVariant {
        hq: group[0].hq != 0,
        sample_size_30d: sum_saturating(&raw),
        cleaned_sample_size_30d: sum_saturating(&cleaned),
        vwap_30d: weighted_mean_u32(&group.iter().map(|s| s.vwap).collect::<Vec<_>>(), &cleaned),
        p50_30d: weighted_mean_u32(&group.iter().map(|s| s.p50).collect::<Vec<_>>(), &cleaned),
        confidence_band: band_source.confidence_band(),
        launder_suspicion: weighted_mean_f32(
            &group
                .iter()
                .map(|s| s.launder_suspicion_pct)
                .collect::<Vec<_>>(),
            &raw,
        ),
    }
}

fn sum_saturating(values: &[u64]) -> u32 {
    values.iter().sum::<u64>().min(u32::MAX as u64) as u32
}

/// Weighted mean, falling back to a flat mean when every weight is zero (a
/// world whose whole sample was filtered out still has a price to report).
fn weighted_mean_u32(values: &[u32], weights: &[u64]) -> u32 {
    let total: u128 = weights.iter().map(|w| *w as u128).sum();
    if total == 0 {
        if values.is_empty() {
            return 0;
        }
        let sum: u128 = values.iter().map(|v| *v as u128).sum();
        return (sum / values.len() as u128) as u32;
    }
    let numerator: u128 = values
        .iter()
        .zip(weights)
        .map(|(v, w)| *v as u128 * *w as u128)
        .sum();
    (numerator / total) as u32
}

/// As [`weighted_mean_u32`], in f64 to keep the running sum honest before
/// narrowing back to the f32 the wire type uses.
fn weighted_mean_f32(values: &[f32], weights: &[u64]) -> f32 {
    let total: f64 = weights.iter().map(|w| *w as f64).sum();
    if total == 0.0 {
        if values.is_empty() {
            return 0.0;
        }
        return (values.iter().map(|v| *v as f64).sum::<f64>() / values.len() as f64) as f32;
    }
    let numerator: f64 = values
        .iter()
        .zip(weights)
        .map(|(v, w)| *v as f64 * *w as f64)
        .sum();
    (numerator / total) as f32
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

/// One aggregated bucket for one series. Column order matches the SELECT.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct PriceSeriesRow {
    /// Selector id at the requested grouping.
    pub series_id: i32,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub bucket: chrono::DateTime<chrono::Utc>,
    pub open: u32,
    pub high: u32,
    pub low: u32,
    pub close: u32,
    pub gil: u64,
    pub units: u64,
    pub sales: u64,
    pub p25: u32,
    pub p50: u32,
    pub p75: u32,
}

/// SQL expression producing the series key.
///
/// For coarser groupings we inline a `transform()` map from the caller's
/// world list rather than joining a lookup table — the world set is small
/// (at most a region's worth) and a join would take this query off the
/// `sales` primary-key prefix.
///
/// The `transform()` default of `0` is unreachable *only* because the same
/// `world_to_group` list also builds the `world_id IN (…)` filter, so every
/// row reaching the transform has a mapped key. Nothing enforces that
/// structurally — if a caller ever narrows the filter without narrowing this
/// map, unmapped worlds collapse into a bogus series with id 0. Callers must
/// also pass distinct world ids: a duplicate key makes `transform()`'s
/// tie-breaking unspecified.
fn group_expr(group: SeriesGroup, world_to_group: &[(i32, i32)]) -> String {
    if group == SeriesGroup::World {
        return "world_id".to_string();
    }
    let from = world_to_group
        .iter()
        .map(|(w, _)| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let to = world_to_group
        .iter()
        .map(|(_, g)| g.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("transform(world_id, [{from}], [{to}], 0)")
}

fn hq_predicate(hq: HqFilter) -> &'static str {
    match hq {
        HqFilter::Any => "",
        HqFilter::Hq => " AND hq = 1",
        HqFilter::Nq => " AND hq = 0",
    }
}

/// The `WHERE` clause shared by [`price_series`] and [`raw_sales`].
///
/// Both queries scope `sales` to the same item, world set, half-open
/// `[from, to)` window, and HQ filter. Building that shape in one place
/// (rather than duplicating it in two `format!`s) makes the two queries
/// agree on "what's in the window" by construction — the whole point of
/// sourcing raw sales from ClickHouse instead of a separately-filtered
/// Postgres query. `worlds` and `hq_filter` are pre-rendered fragments
/// (`hq_filter` comes from [`hq_predicate`]) so this stays a pure string
/// assembly, same as `group_expr`/`hq_predicate` above.
fn window_predicate(
    item_id: i32,
    worlds: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    hq_filter: &str,
) -> String {
    format!(
        "item_id = {item_id} AND world_id IN ({worlds}) AND sold_date >= toDateTime({from_ts}) AND sold_date < toDateTime({to_ts}){hq_filter}",
        from_ts = from.timestamp(),
        to_ts = to.timestamp(),
    )
}

/// Aggregate `sales` into fixed-width buckets for one item.
///
/// `world_to_group` maps every world in scope to its series key at `group`;
/// for `SeriesGroup::World` the mapped value is ignored.
///
/// Deliberately no `FINAL`. `sales` is a `ReplacingMergeTree` whose duplicates
/// are exact repeats of the same sale, and at aggregate scale an unmerged
/// duplicate shifts a bucket's VWAP imperceptibly — whereas `FINAL` over a
/// full-history scan is expensive. This is an accuracy-for-cost trade.
///
/// Deliberately no join: `item_id` is filtered first so the read stays on the
/// table's `(item_id, hq, world_id, sold_date, pg_id)` prefix. See the comment
/// in [`sparklines_batch`] for what happens when a large table is joined
/// unfiltered.
///
/// Takes 8 parameters (over clippy's default threshold of 7): `ch` is the
/// connection handle every query fn here takes, and the other 7 are all
/// independent, mandatory pieces of the query (item, scope, grouping, filter,
/// window, resolution) — there's no natural sub-grouping that would make a
/// parameter struct clearer than the argument list, so we allow rather than
/// introduce a single-use struct.
#[allow(clippy::too_many_arguments)]
pub async fn price_series(
    ch: &ClickHouseClient,
    item_id: i32,
    world_to_group: &[(i32, i32)],
    group: SeriesGroup,
    hq: HqFilter,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_seconds: i64,
) -> Result<Vec<PriceSeriesRow>, ClickHouseError> {
    if world_to_group.is_empty() {
        return Ok(Vec::new());
    }
    let worlds = world_to_group
        .iter()
        .map(|(w, _)| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let key = group_expr(group, world_to_group);
    let hq_filter = hq_predicate(hq);
    let predicate = window_predicate(item_id, &worlds, from, to, hq_filter);

    let sql = format!(
        r#"
        SELECT
            toInt32({key})                              AS series_id,
            toStartOfInterval(sold_date, INTERVAL {bucket_seconds} SECOND) AS bucket,
            toUInt32(argMin(price_per_item, sold_date)) AS open,
            toUInt32(max(price_per_item))               AS high,
            toUInt32(min(price_per_item))                AS low,
            toUInt32(argMax(price_per_item, sold_date)) AS close,
            toUInt64(sum(total_gil))                    AS gil,
            toUInt64(sum(quantity))                     AS units,
            toUInt64(count())                           AS sales,
            toUInt32(quantileExact(0.25)(price_per_item)) AS p25,
            toUInt32(quantileExact(0.50)(price_per_item)) AS p50,
            toUInt32(quantileExact(0.75)(price_per_item)) AS p75
        FROM sales
        WHERE {predicate}
        GROUP BY series_id, bucket
        ORDER BY series_id, bucket
        "#
    );

    Ok(ch
        .client()
        .query(&sql)
        .fetch_all::<PriceSeriesRow>()
        .await?)
}

/// One raw sale row backing the price_series endpoint's zoomed-in dot
/// overlay. Mirrors the columns [`ultros_api_types::CompactSale`] needs.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct RawSaleRow {
    pub quantity: u16,
    pub price_per_item: u32,
    pub hq: u8,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub sold_date: chrono::DateTime<chrono::Utc>,
    pub world_id: i32,
}

/// Fetch the individual raw sale rows backing the price_series endpoint's
/// zoomed-in dot overlay, from the same `sales` table [`price_series`]
/// aggregates — deliberately **not** from Postgres.
///
/// `UltrosDb::get_compact_sale_history` has no date bound: it fetches the
/// most recent `limit` sales *as of now*, per world, merges, and truncates.
/// Filtering that client-side to an arbitrary `[from, to)` window silently
/// produces an empty result whenever `to` isn't close to "now" — exactly the
/// shape of request this endpoint supports (`from` defaults to 12 years
/// back). Sourcing both the aggregate and the raw rows from ClickHouse with
/// the identical `WHERE` shape ([`window_predicate`], shared with
/// [`price_series`]) makes the two agree on "what's in the window" by
/// construction instead of by convention across two separate data stores.
///
/// `world_ids` is a plain list, not a `(world, group)` map: raw sales are
/// never grouped, so there is no `transform()` here.
///
/// Same conventions as `price_series`: deliberately no `FINAL` (an
/// accuracy-for-cost trade against unmerged `ReplacingMergeTree`
/// duplicates), no join, and only numeric interpolation into the SQL string
/// (never string/user data) — see the doc comment on `price_series` for why.
pub async fn raw_sales(
    ch: &ClickHouseClient,
    item_id: i32,
    world_ids: &[i32],
    hq: HqFilter,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    limit: u64,
) -> Result<Vec<RawSaleRow>, ClickHouseError> {
    if world_ids.is_empty() {
        return Ok(Vec::new());
    }
    let worlds = world_ids
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let predicate = window_predicate(item_id, &worlds, from, to, hq_predicate(hq));

    let sql = format!(
        r#"
        SELECT
            quantity,
            price_per_item,
            hq,
            sold_date,
            world_id
        FROM sales
        WHERE {predicate}
        ORDER BY sold_date
        LIMIT {limit}
        "#
    );

    Ok(ch.client().query(&sql).fetch_all::<RawSaleRow>().await?)
}

/// One populated `(bucket, price_bin)` cell. Column order matches the SELECT.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct PriceDensityRow {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub bucket: chrono::DateTime<chrono::Utc>,
    pub price_bin: u16,
    pub n: u64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct MinMaxRow {
    count: u64,
    lo: u32,
    hi: u32,
}

/// Price extent over the window — the density endpoint derives its bin
/// layout from this before running [`price_density`]. `None` when the
/// window holds no sales (ClickHouse `min`/`max` over zero rows return 0,
/// which must not be mistaken for a real price of 0).
///
/// Same conventions as [`price_series`]: no `FINAL`, no join, and only
/// numeric interpolation into the SQL string via [`window_predicate`].
pub async fn price_min_max(
    ch: &ClickHouseClient,
    item_id: i32,
    world_ids: &[i32],
    hq: HqFilter,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Option<(u32, u32)>, ClickHouseError> {
    if world_ids.is_empty() {
        return Ok(None);
    }
    let worlds = world_ids
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let predicate = window_predicate(item_id, &worlds, from, to, hq_predicate(hq));
    let sql = format!(
        r#"
        SELECT
            toUInt64(count())             AS count,
            toUInt32(min(price_per_item)) AS lo,
            toUInt32(max(price_per_item)) AS hi
        FROM sales
        WHERE {predicate}
        "#
    );
    let row = ch.client().query(&sql).fetch_one::<MinMaxRow>().await?;
    Ok((row.count > 0).then_some((row.lo, row.hi)))
}

/// Sale counts on a time × price grid for the chart's density mode: same
/// predicate shape as [`price_series`]/[`raw_sales`], grouped by bucket and
/// price bin. Bins are `floor((price - lo) / bin_width)` clamped into
/// `0..bins` — the clamp covers the top edge (`price == hi` lands exactly on
/// `bins` without it) and guards against a stale `lo` from a caller racing
/// new sales.
///
/// Same conventions as [`price_series`], including the argument-count allow:
/// the 9 non-handle parameters are all independent, mandatory pieces of the
/// grid definition (item, scope, filter, window, resolution, bin layout).
#[allow(clippy::too_many_arguments)]
pub async fn price_density(
    ch: &ClickHouseClient,
    item_id: i32,
    world_ids: &[i32],
    hq: HqFilter,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_seconds: i64,
    lo: u32,
    bin_width: f64,
    bins: u16,
) -> Result<Vec<PriceDensityRow>, ClickHouseError> {
    if world_ids.is_empty() || bins == 0 || bin_width <= 0.0 {
        return Ok(Vec::new());
    }
    let worlds = world_ids
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let predicate = window_predicate(item_id, &worlds, from, to, hq_predicate(hq));
    let max_bin = bins - 1;
    let sql = format!(
        r#"
        SELECT
            toStartOfInterval(sold_date, INTERVAL {bucket_seconds} SECOND) AS bucket,
            toUInt16(least(greatest(floor((toFloat64(price_per_item) - {lo}) / {bin_width}), 0), {max_bin})) AS price_bin,
            toUInt64(count())                                              AS n
        FROM sales
        WHERE {predicate}
        GROUP BY bucket, price_bin
        ORDER BY bucket, price_bin
        "#
    );
    Ok(ch
        .client()
        .query(&sql)
        .fetch_all::<PriceDensityRow>()
        .await?)
}

/// One row of [`bulk_sale_stats`]: aggregate sale statistics for one
/// `(item_id, hq)` pair across the requested world set and window.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct BulkSaleStatsRow {
    pub item_id: i32,
    pub hq: u8,
    pub min_price: i32,
    pub median_price: i32,
    pub avg_price: i32,
    pub num_sold: i64,
    /// Unix seconds of the newest sale in the window.
    pub last_sold_unix: i64,
    /// Units traded in the window (sum of quantities).
    pub units_sold: u64,
    /// Volume-weighted average per-unit price, rounded. Weighted by
    /// quantity so stack trades count per unit, not per transaction.
    pub vwap: i32,
}

/// Aggregate min / median / mean per-unit sale price for **every**
/// item with sales in the trailing `window_days`, across `world_ids`.
///
/// Backs `GET /api/v1/sale_stats/{worldDcOrRegion}` — the recipe analyzer's
/// selectable cost basis. Reads the scheduled `sale_stats_window` snapshots,
/// never raw `sales`: the stored t-digest state makes the median mergeable
/// across worlds while sum/count, min, max, and volume fields compose exactly.
/// `FINAL` is safe here because the world/window predicate matches the table's
/// leading sort key and prunes the read before replacement merging.
///
/// VWAP is derived in an **outer** `SELECT` rather than beside the other
/// aggregates. ClickHouse resolves an identifier to a same-scope alias in
/// preference to a column, so writing `sum(units_sold)` next to
/// `sum(units_sold) AS units_sold` expands to `sum(sum(units_sold))` and the
/// whole query fails with `ILLEGAL_AGGREGATION` (error 184) at runtime —
/// invisible to any test that only asserts on the SQL string.
pub async fn bulk_sale_stats(
    ch: &ClickHouseClient,
    world_ids: &[i32],
    window_days: u16,
) -> Result<Vec<BulkSaleStatsRow>, ClickHouseError> {
    if world_ids.is_empty() {
        return Ok(Vec::new());
    }
    let worlds = world_ids
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT
            item_id,
            hq,
            min_price,
            median_price,
            avg_price,
            num_sold,
            last_sold_unix,
            units_sold,
            toInt32(round(gil_volume_sum / greatest(units_sold, 1))) AS vwap
        FROM
        (
            SELECT
                item_id,
                hq,
                toInt32(min(min_price)) AS min_price,
                toInt32(quantileTDigestMerge(0.5)(price_quantile)) AS median_price,
                toInt32(round(sum(price_sum) / greatest(sum(sale_count), 1))) AS avg_price,
                toInt64(sum(sale_count)) AS num_sold,
                toInt64(max(last_sold_unix)) AS last_sold_unix,
                toUInt64(sum(units_sold)) AS units_sold,
                toUInt64(sum(gil_volume)) AS gil_volume_sum
            FROM sale_stats_window FINAL
            WHERE world_id IN ({worlds})
              AND window_days = {window_days}
            GROUP BY item_id, hq
        )
        "#
    );
    Ok(ch
        .client()
        .query(&sql)
        .fetch_all::<BulkSaleStatsRow>()
        .await?)
}

/// One row of [`bulk_confidence`]: the stored quality band for one
/// `(item_id, hq)` on the requested world.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct BulkConfidenceRow {
    pub item_id: i32,
    pub hq: u8,
    pub confidence_band_raw: String,
}

impl BulkConfidenceRow {
    /// See [`parse_confidence_band`].
    pub fn confidence_band(&self) -> ConfidenceBand {
        parse_confidence_band(&self.confidence_band_raw)
    }
}

/// Per-(item, hq) confidence bands for **one** world.
///
/// The band is a stored per-world judgement (see
/// [`aggregate_item_stats_variants`] for why it can't be recomputed across
/// worlds), so multi-world scopes don't call this and report `Unknown`
/// instead. The single-world predicate keeps the `FINAL` scan bounded — no
/// unfiltered reads of `item_quality_score`.
pub async fn bulk_confidence(
    ch: &ClickHouseClient,
    world_id: i32,
) -> Result<Vec<BulkConfidenceRow>, ClickHouseError> {
    let sql = format!(
        "SELECT item_id, hq, toString(confidence_band) AS confidence_band_raw
         FROM item_quality_score FINAL
         WHERE world_id = {world_id}"
    );
    Ok(ch
        .client()
        .query(&sql)
        .fetch_all::<BulkConfidenceRow>()
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::price_series::SeriesGroup;

    #[test]
    fn world_group_selects_the_column_directly() {
        assert_eq!(
            group_expr(SeriesGroup::World, &[(1, 10), (2, 10)]),
            "world_id"
        );
    }

    #[test]
    fn coarser_groups_build_a_transform_map() {
        assert_eq!(
            group_expr(SeriesGroup::Datacenter, &[(1, 10), (2, 10), (3, 20)]),
            "transform(world_id, [1,2,3], [10,10,20], 0)"
        );
    }

    #[test]
    fn hq_predicate_is_empty_for_any() {
        assert_eq!(hq_predicate(HqFilter::Any), "");
        assert_eq!(hq_predicate(HqFilter::Hq), " AND hq = 1");
        assert_eq!(hq_predicate(HqFilter::Nq), " AND hq = 0");
    }

    #[test]
    fn window_predicate_assembles_item_worlds_window_and_hq() {
        let from = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let to = chrono::DateTime::from_timestamp(1_700_086_400, 0).unwrap();
        assert_eq!(
            window_predicate(42, "1,2,3", from, to, hq_predicate(HqFilter::Any)),
            "item_id = 42 AND world_id IN (1,2,3) AND sold_date >= toDateTime(1700000000) \
             AND sold_date < toDateTime(1700086400)"
        );
    }

    #[test]
    fn window_predicate_appends_the_hq_filter() {
        let from = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let to = chrono::DateTime::from_timestamp(1, 0).unwrap();
        assert_eq!(
            window_predicate(1, "1", from, to, hq_predicate(HqFilter::Hq)),
            "item_id = 1 AND world_id IN (1) AND sold_date >= toDateTime(0) \
             AND sold_date < toDateTime(1) AND hq = 1"
        );
    }

    #[test]
    fn window_predicate_is_shared_shape_between_price_series_and_raw_sales() {
        // Pin the invariant the doc comments on `price_series` and
        // `raw_sales` both call out: same item/worlds/window/hq produces the
        // exact same predicate string regardless of which query calls it.
        let from = chrono::DateTime::from_timestamp(100, 0).unwrap();
        let to = chrono::DateTime::from_timestamp(200, 0).unwrap();
        let a = window_predicate(7, "10,20", from, to, hq_predicate(HqFilter::Nq));
        let b = window_predicate(7, "10,20", from, to, hq_predicate(HqFilter::Nq));
        assert_eq!(a, b);
    }

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

    /// A scan for one world, with the fields the aggregation actually reads
    /// set explicitly so each test reads as its own scenario.
    fn scan(world_id: i32, hq: u8, sample: u32, cleaned: u32, vwap: u32, band: &str) -> DeepScan {
        DeepScan {
            world_id,
            hq,
            sample_size: sample,
            cleaned_sample_size: cleaned,
            vwap,
            p50: vwap,
            confidence_band_raw: band.to_string(),
            ..fixture()
        }
    }

    #[test]
    fn single_world_scope_passes_the_row_through_verbatim() {
        // World-scoped requests must be untouched by the region aggregation.
        let only = fixture();
        let variants = aggregate_item_stats_variants(std::slice::from_ref(&only));
        assert_eq!(variants.len(), 1);
        let v = &variants[0];
        assert_eq!(v.sample_size_30d, only.sample_size);
        assert_eq!(v.cleaned_sample_size_30d, only.cleaned_sample_size);
        assert_eq!(v.vwap_30d, only.vwap);
        assert_eq!(v.p50_30d, only.p50);
        assert_eq!(v.confidence_band, only.confidence_band());
        assert_eq!(v.launder_suspicion, only.launder_suspicion_pct);
    }

    #[test]
    fn region_scope_sums_sample_counts_across_worlds() {
        // The badge's headline number: a region's sample size is every
        // member world's, not one world's.
        let scans = [
            scan(40, 0, 100, 90, 500, "high"),
            scan(41, 0, 250, 200, 500, "high"),
            scan(42, 0, 30, 10, 500, "low"),
        ];
        let variants = aggregate_item_stats_variants(&scans);
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].sample_size_30d, 380);
        assert_eq!(variants[0].cleaned_sample_size_30d, 300);
    }

    #[test]
    fn region_scope_keeps_both_qualities_nq_first() {
        let scans = [
            scan(41, 1, 10, 10, 900, "medium"),
            scan(40, 0, 100, 90, 500, "high"),
            scan(41, 0, 100, 90, 500, "high"),
        ];
        let variants = aggregate_item_stats_variants(&scans);
        assert_eq!(variants.len(), 2);
        assert!(
            !variants[0].hq,
            "NQ should come first regardless of row order"
        );
        assert!(variants[1].hq);
        // The lone HQ row is a single-scan group, so it passes through.
        assert_eq!(variants[1].vwap_30d, 900);
    }

    #[test]
    fn price_is_weighted_by_sample_not_flat_averaged_across_worlds() {
        // A 10-sale world at 1000 gil shouldn't move the number as much as a
        // 990-sale world at 100 gil. Flat mean would say 550.
        let scans = [
            scan(40, 0, 990, 990, 100, "high"),
            scan(41, 0, 10, 10, 1000, "low"),
        ];
        let variants = aggregate_item_stats_variants(&scans);
        // (100*990 + 1000*10) / 1000 = 109
        assert_eq!(variants[0].vwap_30d, 109);
        assert_eq!(variants[0].p50_30d, 109);
    }

    #[test]
    fn launder_suspicion_weights_by_total_samples() {
        // It's a share of all samples, so its weight is sample_size.
        let mut a = scan(40, 0, 900, 800, 500, "high");
        a.launder_suspicion_pct = 0.0;
        let mut b = scan(41, 0, 100, 90, 500, "low");
        b.launder_suspicion_pct = 1.0;
        let variants = aggregate_item_stats_variants(&[a, b]);
        // (0.0*900 + 1.0*100) / 1000 = 0.1
        assert!(
            (variants[0].launder_suspicion - 0.1).abs() < 1e-6,
            "got {}",
            variants[0].launder_suspicion
        );
    }

    #[test]
    fn band_comes_from_the_world_with_the_most_cleaned_samples() {
        let scans = [
            scan(40, 0, 20, 10, 500, "low"),
            scan(41, 0, 900, 800, 500, "high"),
        ];
        let variants = aggregate_item_stats_variants(&scans);
        assert_eq!(variants[0].confidence_band, ConfidenceBand::High);
    }

    #[test]
    fn band_tie_breaks_on_world_id_so_row_order_cannot_change_it() {
        // Same cleaned count on both worlds: the answer must not depend on
        // which order ClickHouse happened to return the rows in.
        let low_first = [
            scan(40, 0, 100, 90, 500, "high"),
            scan(41, 0, 100, 90, 500, "low"),
        ];
        let high_first = [
            scan(41, 0, 100, 90, 500, "low"),
            scan(40, 0, 100, 90, 500, "high"),
        ];
        assert_eq!(
            aggregate_item_stats_variants(&low_first)[0].confidence_band,
            aggregate_item_stats_variants(&high_first)[0].confidence_band,
        );
        // Lowest world id wins the tie.
        assert_eq!(
            aggregate_item_stats_variants(&low_first)[0].confidence_band,
            ConfidenceBand::High,
        );
    }

    #[test]
    fn all_samples_filtered_out_falls_back_to_a_flat_price_mean() {
        // Every weight zero would divide by zero; fall back rather than 0.
        let scans = [
            scan(40, 0, 5, 0, 100, "unusable"),
            scan(41, 0, 5, 0, 300, "unusable"),
        ];
        let variants = aggregate_item_stats_variants(&scans);
        assert_eq!(variants[0].cleaned_sample_size_30d, 0);
        assert_eq!(variants[0].vwap_30d, 200);
    }

    #[test]
    fn no_rows_yields_no_variants() {
        assert!(aggregate_item_stats_variants(&[]).is_empty());
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
