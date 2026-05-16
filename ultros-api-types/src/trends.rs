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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrendsData {
    pub high_velocity: Vec<TrendItem>,
    pub rising_price: Vec<TrendItem>,
    pub falling_price: Vec<TrendItem>,
}
