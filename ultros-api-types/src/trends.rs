use serde::{Deserialize, Serialize};

/// Trustworthiness band attached to analyzer outputs.
///
/// Maps directly to the `item_quality_score.confidence_band` column in
/// ClickHouse. The analyzer derives this from sample size, buyer diversity,
/// and launder-suspicion. Surfaces in the UI as a colored chip.
///
/// Default = `Unknown` — used by Pass-1 (in-memory) results before the
/// deep-scan refines them. The frontend renders this as a neutral grey
/// chip with a "loading…" tooltip until the second pass arrives.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConfidenceBand {
    /// Pass-1 result without deep-scan data yet.
    #[default]
    Unknown,
    /// Recommend confidently — strong sample, diverse buyers, clean filter.
    High,
    /// Usable but flag in UI.
    Medium,
    /// Show as rough estimate; don't lead with this row.
    Low,
    /// Suppress from recommendation surfaces entirely.
    Unusable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrendItem {
    pub item_id: i32,
    pub hq: bool,
    pub price: i32,
    pub world_id: i32,
    pub average_sale_price: f32,
    pub sales_per_week: f32,

    // === Phase 2 additions ===
    //
    // All defaulted so existing wire payloads from older servers / older
    // builds of the bot keep deserializing — the analyzer enrichment is
    // additive.
    //
    /// Volume-weighted average price over the last 30 days, on the cleaned
    /// (noise-filtered) sample. More honest than `average_sale_price` for
    /// stack-traded items because it weights by quantity.
    #[serde(default)]
    pub vwap_30d: i32,
    /// Where the current `price` falls in the 30-day price distribution
    /// (0-100). Useful sanity check: "currently at the 12th percentile of
    /// 30-day price" tells you a lot in one number.
    #[serde(default)]
    pub price_percentile_30d: u8,
    /// Confidence band for this row's data. Pass-1 results carry `Unknown`
    /// until the deep-scan refines them.
    #[serde(default)]
    pub confidence_band: ConfidenceBand,
    /// Number of sales in the 30-day window (pre-filter) — drives the
    /// confidence band, exposed so the UI can show "based on N sales".
    #[serde(default)]
    pub sample_size_30d: u32,
    /// Fraction of the 30-day sample flagged as noise by the filter
    /// (0.0-1.0). High values are the strongest single signal that the
    /// item is being used for currency-transfer launder.
    #[serde(default)]
    pub launder_suspicion: f32,

    // === Trends v2 additions ===
    //
    // The Trends page rebuild sources its table directly from CH's
    // `item_stats_window` rather than the 6-sample in-memory bucketing.
    // These fields are populated by `get_trends_v2`; the legacy
    // `get_trends` leaves them at zero/empty so the old buckets still
    // render correctly for any holdout consumer.
    //
    /// Echo of the queried window (7, 30, or 90). The UI uses this when
    /// formatting labels like "Sales / 30d".
    #[serde(default)]
    pub window_days: u16,
    /// VWAP for the selected window.
    #[serde(default)]
    pub vwap_window: i32,
    /// Cleaned sample size for the selected window — drives sales/day.
    #[serde(default)]
    pub sales_in_window: u32,
    /// Units traded in the selected window.
    #[serde(default)]
    pub unit_volume_window: u64,
    /// Gil traded in the selected window.
    #[serde(default)]
    pub gil_volume_window: u64,
    /// `sales_in_window / window_days`. Pre-computed server-side so the
    /// FE can sort/filter without having to know the window length math.
    #[serde(default)]
    pub sales_per_day: f32,
    /// `(price - vwap_window) / vwap_window * 100` — current price vs
    /// window VWAP, as a percent. Sortable proxy for "how stretched is
    /// the cheapest listing right now."
    #[serde(default)]
    pub pct_change_window: f32,
    /// Trailing-24h hourly VWAP series, zero-filled. Length always 24.
    #[serde(default)]
    pub sparkline_24h: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrendsData {
    /// Trends v2: single flat list, sorted server-side, FE applies
    /// further sort/filter/pagination locally. Empty when the legacy
    /// pre-bucketed response is in use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<TrendItem>,
    /// Legacy pre-bucketed lists. Still populated by the v1 server path
    /// so older clients keep working; the v2 page reads from `items`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub high_velocity: Vec<TrendItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rising_price: Vec<TrendItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub falling_price: Vec<TrendItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_band_serialization_deserialization() {
        // Test serialization to lowercase string
        assert_eq!(
            serde_json::to_string(&ConfidenceBand::Unknown).unwrap(),
            "\"unknown\""
        );
        assert_eq!(
            serde_json::to_string(&ConfidenceBand::High).unwrap(),
            "\"high\""
        );
        assert_eq!(
            serde_json::to_string(&ConfidenceBand::Medium).unwrap(),
            "\"medium\""
        );
        assert_eq!(
            serde_json::to_string(&ConfidenceBand::Low).unwrap(),
            "\"low\""
        );
        assert_eq!(
            serde_json::to_string(&ConfidenceBand::Unusable).unwrap(),
            "\"unusable\""
        );

        // Test deserialization from lowercase string
        assert_eq!(
            serde_json::from_str::<ConfidenceBand>("\"unknown\"").unwrap(),
            ConfidenceBand::Unknown
        );
        assert_eq!(
            serde_json::from_str::<ConfidenceBand>("\"high\"").unwrap(),
            ConfidenceBand::High
        );
        assert_eq!(
            serde_json::from_str::<ConfidenceBand>("\"medium\"").unwrap(),
            ConfidenceBand::Medium
        );
        assert_eq!(
            serde_json::from_str::<ConfidenceBand>("\"low\"").unwrap(),
            ConfidenceBand::Low
        );
        assert_eq!(
            serde_json::from_str::<ConfidenceBand>("\"unusable\"").unwrap(),
            ConfidenceBand::Unusable
        );

        // Test default value
        let default_band: ConfidenceBand = Default::default();
        assert_eq!(default_band, ConfidenceBand::Unknown);
    }
}
