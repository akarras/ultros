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
