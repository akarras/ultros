//! Home-page Market Movers + sparklines wire types.
//!
//! Two endpoints feed this:
//!   - `/api/v1/movers/{world}?direction=rising|falling|volume` — top N
//!     items by 24h pct change (or volume), each carrying a 24-point VWAP
//!     sparkline.
//!   - `/api/v1/sparklines/{world}` (POST) — bulk-fetch sparklines for
//!     arbitrary (item, hq) pairs. Used by Continue Tracking, Top Deals,
//!     and other surfaces that already have item IDs in hand.

use serde::{Deserialize, Serialize};

/// One row in the Market Movers list. The frontend renders this as a
/// row with item icon + name + price + pct-change pill + inline
/// sparkline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MoverItem {
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    pub price_now: u32,
    pub pct_change_24h: f32,
    pub volume_24h: u32,
    /// Total gil that changed hands on this item over the 24h window
    /// (price × quantity). The gil-denominated "market value" metric — the
    /// complement to `volume_24h`'s raw unit count.
    pub gil_volume_24h: u64,
    /// Trailing 24h VWAP series, oldest first. Always 24 elements (gaps =
    /// 0). Frontend's Sparkline component renders this directly.
    pub sparkline: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MoversResponse {
    pub world_id: i32,
    pub direction: String,
    pub items: Vec<MoverItem>,
}

/// Bulk sparkline-only payload. Used by surfaces that already have item
/// IDs and just want a trace per row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SparklineSeries {
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    pub points: Vec<u32>,
    pub first_price: u32,
    pub last_price: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SparklinesResponse {
    pub world_id: i32,
    pub series: Vec<SparklineSeries>,
}

/// POST body for /api/v1/sparklines/{world}.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SparklinesRequest {
    /// Each tuple = (item_id, hq). World comes from the URL.
    pub items: Vec<(i32, bool)>,
    /// Window length in hours; clamped to [6, 168] server-side.
    /// Default 24 if omitted.
    #[serde(default)]
    pub hours: Option<u16>,
}
