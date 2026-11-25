//! `GET /openapi.json` — OpenAPI 3.1.0 specification generated via `utoipa`
//! for all unauthenticated Ultros `/api/*` endpoints.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::Value;
use ultros_api_types::alert::VapidPublicKey;
use ultros_api_types::cheapest_listings::{
    CheapestListingItem, CheapestListings, CheapestListingsColumnar,
};
use ultros_api_types::floor_history::{
    FloorBounds, FloorHistory, FloorHistoryBatch, FloorHistoryRequest, FloorInterval, FloorPoint,
    ItemFloorHistory,
};
use ultros_api_types::game_history::{GamePatch, PatchTrack};
use ultros_api_types::item_stats::{ItemStatsResponse, ItemStatsVariant};
use ultros_api_types::listing_stats::{
    BulkListingStats, BulkListingStatsColumnar, HistoryCoverage, ItemListingStats,
    ListingAgeOrigin, ListingWindowStats, MatchedSalesStats, StockStatus,
};
use ultros_api_types::market_heat::{CategoryHeat, HeatBand, MarketHeatResponse};
use ultros_api_types::market_pulse::MarketPulseDto;
use ultros_api_types::price_density::{DensityCell, PriceDensity};
use ultros_api_types::price_series::{
    HqFilter, PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup,
};
use ultros_api_types::recent_sales::{RecentSales, RecentSalesColumnar, SaleData, Sales};
use ultros_api_types::resale_quality::{
    ResaleQualityRequest, ResaleQualityResponse, ResaleQualityRow,
};
use ultros_api_types::retainer::RetainerListings;
use ultros_api_types::sale_stats::{BulkSaleStats, BulkSaleStatsColumnar, ItemSaleStats};
use ultros_api_types::search::SearchResult;
use ultros_api_types::sparklines::{
    MoverItem, MoversResponse, SparklineSeries, SparklinesRequest, SparklinesResponse,
};
use ultros_api_types::trends::{ConfidenceBand, TrendItem, TrendsData};
use ultros_api_types::undercut_pressure::{
    PressureBucket, PressureState, PressureSummary, UndercutPressure, WarSpan, WarStatus,
};
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::{
    ActiveListing, CompactSale, CurrentlyShownItem, ExtendedSaleHistory, Retainer, SaleHistory,
    WorldItemLastUpdated,
};
use ultros_changelog::{ChangelogCategory, ChangelogEntry, ChangelogImportance};
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::web::api::{
    BestDealsQuery, CheapestFormat, ListingStatsQuery, MoversQuery, RecentSalesQuery,
    ResaleStatsDto, SaleStatsQuery, TrendsQuery,
};
use crate::web::{
    ExtendedHistoryQuery, GameHistoryQuery, PressureQuery, PriceDensityQuery, PriceSeriesQuery,
    SearchQuery,
};

const TAG_MARKET_DATA: &str = "Market Data";
const TAG_PRICE_HISTORY: &str = "Price History & Charts";
const TAG_ANALYTICS: &str = "Market Analytics";
const TAG_RETAINERS: &str = "Retainers";
const TAG_METADATA: &str = "Worlds, Search & Metadata";

/// Response for `GET /api/v1/cheapest/{world}`: row objects by default, or
/// struct-of-arrays when `?format=columnar` is supplied.
#[derive(Serialize, ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum CheapestResponse {
    Rows(CheapestListings),
    Columnar(CheapestListingsColumnar),
}

/// Response for `GET /api/v1/recentSales/{world}`: row objects by default, or
/// struct-of-arrays when `?format=columnar` is supplied.
#[derive(Serialize, ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum RecentSalesResponse {
    Rows(RecentSales),
    Columnar(RecentSalesColumnar),
}

/// Response for `GET /api/v1/sale_stats/{world}`: row objects by default, or
/// struct-of-arrays when `?format=columnar` is supplied.
#[derive(Serialize, ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum BulkSaleStatsResponse {
    Rows(BulkSaleStats),
    Columnar(Box<BulkSaleStatsColumnar>),
}

/// Response for `GET /api/v1/listing_stats/{world}`: row objects by default, or
/// struct-of-arrays when `?format=columnar` is supplied.
#[derive(Serialize, ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum BulkListingStatsResponse {
    Rows(BulkListingStats),
    Columnar(BulkListingStatsColumnar),
}

/// Map from stringified item ID to active `(ActiveListing, Option<Retainer>)` pairs.
#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
struct BulkListingsResponse(BTreeMap<String, Vec<(ActiveListing, Option<Retainer>)>>);

/// Query parameters accepted by `GET /api/v1/floor_history/{world}/{itemid}`.
#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
struct FloorHistoryQuery {
    /// Start of window (Unix timestamp in seconds). Defaults to 0.
    from: Option<i64>,
    /// End of window (Unix timestamp in seconds). Defaults to now.
    to: Option<i64>,
    /// Quality filter: `any` (default), `hq`, or `nq`.
    hq: Option<String>,
}

#[allow(dead_code)]
mod paths {
    use super::*;

    /// Get active listings, recent sales, and ingest timestamps for an item
    ///
    /// Returns active market board listings paired with their retainer metadata,
    /// the last 200 sales, and per-world ingest timestamps across the requested
    /// world, datacenter, or region.
    #[utoipa::path(
        get,
        path = "/api/v1/listings/{world}/{itemid}",
        operation_id = "getItemListings",
        tag = TAG_MARKET_DATA,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name (e.g. Gilgamesh, Aether, North-America)"),
            ("itemid" = i32, Path, description = "FFXIV item ID (e.g. 44178)")
        ),
        responses(
            (status = 200, description = "Active listings, recent sales, and per-world update timestamps", body = CurrentlyShownItem),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_item_listings() {}

    /// Bulk lookup active listings for multiple items
    ///
    /// Returns a map keyed by item ID (as a JSON object key string) containing
    /// `(ActiveListing, Option<Retainer>)` pairs across all worlds in the scope.
    /// Ordering is not preserved.
    #[utoipa::path(
        get,
        path = "/api/v1/bulkListings/{world}/{itemids}",
        operation_id = "getBulkListings",
        tag = TAG_MARKET_DATA,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name (e.g. Gilgamesh, Aether, North-America)"),
            ("itemids" = String, Path, description = "Comma-separated FFXIV item IDs (e.g. 44178,44179)")
        ),
        responses(
            (status = 200, description = "Map of item ID to active listings and optional retainer metadata", body = BulkListingsResponse),
            (status = 400, description = "Invalid item ID list or world scope"),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_bulk_listings() {}

    /// Get cheapest NQ and HQ listing for every item in a scope
    ///
    /// Returns the lowest active listing per `(item_id, hq)` across a world,
    /// datacenter, or region. Pass `?format=columnar` for the compact struct-of-arrays
    /// wire shape (`CheapestListingsColumnar`).
    #[utoipa::path(
        get,
        path = "/api/v1/cheapest/{world}",
        operation_id = "getCheapestPerWorld",
        tag = TAG_MARKET_DATA,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name (e.g. Gilgamesh, Aether, North-America)"),
            CheapestFormat
        ),
        responses(
            (status = 200, description = "Cheapest listing per (item_id, hq) in row or columnar format", body = CheapestResponse),
            (status = 404, description = "World, datacenter, or region not found"),
            (status = 503, description = "Market analyzer warm-up in progress")
        )
    )]
    pub(super) fn get_cheapest() {}

    /// Get recent sales buffer for a world, datacenter, or region
    ///
    /// For a single world, returns the analyzer's full recent-sales buffer for every
    /// item. For a datacenter or region, returns the newest sale per `(item_id, hq)`
    /// capped to the 50 most recent items. Pass `?format=columnar` for the struct-of-arrays
    /// shape (`RecentSalesColumnar`).
    #[utoipa::path(
        get,
        path = "/api/v1/recentSales/{world}",
        operation_id = "getRecentSales",
        tag = TAG_MARKET_DATA,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name (e.g. Gilgamesh, Aether, North-America)"),
            RecentSalesQuery
        ),
        responses(
            (status = 200, description = "Recent sales in row or columnar format", body = RecentSalesResponse),
            (status = 404, description = "World, datacenter, or region not found"),
            (status = 503, description = "Market analyzer warm-up in progress")
        )
    )]
    pub(super) fn get_recent_sales() {}

    /// Get compact extended sale history for an item
    ///
    /// Returns up to `limit` compact sale records (default 1,000, maximum 10,000)
    /// containing price, quantity, quality, timestamp, and world ID without buyer metadata.
    #[utoipa::path(
        get,
        path = "/api/v1/extended_history/{world}/{itemid}",
        operation_id = "getExtendedSaleHistory",
        tag = TAG_MARKET_DATA,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            ("itemid" = i32, Path, description = "FFXIV item ID"),
            ExtendedHistoryQuery
        ),
        responses(
            (status = 200, description = "Compact historical sale rows", body = ExtendedSaleHistory),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_extended_history() {}

    /// WebSocket stream for live market listings and sales
    ///
    /// Upgrades an HTTP connection to a WebSocket (`wss://ultros.app/api/v1/realtime/events`).
    /// Unauthenticated connections can subscribe to live `Listings` and `Sales` events by sending
    /// JSON `ClientMessage::AddSubscribe` frames (filtered by item ID and/or world/datacenter/region).
    /// List and notification subscriptions on the same socket require a signed-in session cookie.
    #[utoipa::path(
        get,
        path = "/api/v1/realtime/events",
        operation_id = "connectRealtimeEvents",
        tag = TAG_MARKET_DATA,
        responses(
            (status = 101, description = "Switching Protocols — upgraded to WebSocket streaming JSON ServerClient frames"),
            (status = 426, description = "Upgrade Required when called over plain HTTP without WebSocket upgrade headers")
        )
    )]
    pub(super) fn get_realtime_events() {}

    /// Get server-bucketed OHLC price and volume series
    ///
    /// Returns time-bucketed price statistics (`open`, `high`, `low`, `close`,
    /// `p25`, `p50`, `p75`, `gil`, `units`, `sales`) grouped by world, datacenter, or
    /// region. When total sales in the window are at or below 2,000, raw sale points
    /// are included in `raw`.
    #[utoipa::path(
        get,
        path = "/api/v1/price_series/{world}/{itemid}",
        operation_id = "getPriceSeries",
        tag = TAG_PRICE_HISTORY,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            ("itemid" = i32, Path, description = "FFXIV item ID"),
            PriceSeriesQuery
        ),
        responses(
            (status = 200, description = "Bucketed price/volume series and optional raw sales", body = PriceSeries),
            (status = 400, description = "Invalid time window (from >= to or invalid timestamp)"),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_price_series() {}

    /// Get sparse time × price sale-count density heatmap
    ///
    /// Returns populated `(ts, bin, n)` cells over uniform gil price bins for the
    /// requested window and quality filter.
    #[utoipa::path(
        get,
        path = "/api/v1/price_density/{world}/{itemid}",
        operation_id = "getPriceDensity",
        tag = TAG_PRICE_HISTORY,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            ("itemid" = i32, Path, description = "FFXIV item ID"),
            PriceDensityQuery
        ),
        responses(
            (status = 200, description = "Sparse price-density grid", body = PriceDensity),
            (status = 400, description = "Invalid time window"),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_price_density() {}

    /// Get sampled listing floor price history for an item
    ///
    /// Samples the lowest active asking price across all worlds in the scope at
    /// adaptive interval boundaries, independently of completed sales.
    #[utoipa::path(
        get,
        path = "/api/v1/floor_history/{world}/{itemid}",
        operation_id = "getFloorHistory",
        tag = TAG_PRICE_HISTORY,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            ("itemid" = i32, Path, description = "FFXIV item ID"),
            FloorHistoryQuery
        ),
        responses(
            (status = 200, description = "Sampled listing floor time series", body = FloorHistory),
            (status = 400, description = "Invalid time bounds"),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_floor_history() {}

    /// Batch query listing floor history for up to 20 items
    ///
    /// Bounded multi-item extension of `floor_history` returning exact min/max bounds,
    /// coverage seconds, and explicit unknown timestamps. Body limit is 16 KiB; requests
    /// are bounded to at most 20 items, 90 days, and 10,000 total sample points.
    #[utoipa::path(
        post,
        path = "/api/v1/floor_history/{world}",
        operation_id = "postFloorHistoryBatch",
        tag = TAG_PRICE_HISTORY,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name")
        ),
        request_body = FloorHistoryRequest,
        responses(
            (status = 200, description = "Per-item floor history, exact bounds, and unknown timestamps", body = FloorHistoryBatch),
            (status = 400, description = "Request failed bounds validation (item count, span, or point budget exceeded)"),
            (status = 404, description = "World, datacenter, or region not found"),
            (status = 503, description = "Query concurrency cap reached or timed out")
        )
    )]
    pub(super) fn post_floor_history_batch() {}

    /// Get undercut pressure and price-war analysis for an item on a world
    ///
    /// Measures same-listing trims (<1% drop), cuts (>=1% drop), competing seller
    /// counts, war spans, and median floor-hold durations. Requires a single world
    /// name (datacenter and region scopes return `400 Bad Request`).
    #[utoipa::path(
        get,
        path = "/api/v1/undercut_pressure/{world}/{itemid}",
        operation_id = "getUndercutPressure",
        tag = TAG_PRICE_HISTORY,
        params(
            ("world" = String, Path, description = "Single FFXIV world name (e.g. Gilgamesh)"),
            ("itemid" = i32, Path, description = "FFXIV item ID"),
            PressureQuery
        ),
        responses(
            (status = 200, description = "Bucketed undercut pressure, war spans, and summary metrics", body = UndercutPressure),
            (status = 400, description = "Invalid time window or non-world scope"),
            (status = 404, description = "World not found"),
            (status = 503, description = "Query concurrency cap reached or timed out")
        )
    )]
    pub(super) fn get_undercut_pressure() {}

    /// Find cross-world arbitrage flips to sell on a target world
    ///
    /// Compares regional buy prices against estimated sell prices on `{world}` (net of
    /// 5% market-board tax) and enriches results with ClickHouse deep-scan confidence,
    /// 30-day VWAP, and buffer velocity. Total match count before `limit` is returned
    /// in the `x-total-count` response header.
    #[utoipa::path(
        get,
        path = "/api/v1/best_deals/{world}",
        operation_id = "getBestDeals",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "Single target sell world name (e.g. Gilgamesh)"),
            BestDealsQuery
        ),
        responses(
            (status = 200, description = "Ranked arbitrage flip opportunities", body = Vec<ResaleStatsDto>, headers(
                ("x-total-count" = usize, description = "Total matching opportunities before applying `limit`")
            )),
            (status = 400, description = "Scope is not a single world"),
            (status = 404, description = "World not found")
        )
    )]
    pub(super) fn get_best_deals() {}

    /// Get trending, high-velocity, and moving items on a world
    ///
    /// Pass `?window=7|30|90` for the v2 ClickHouse-backed flat item list (`items`).
    /// Omitting `window` returns the legacy pre-bucketed lists (`high_velocity`,
    /// `rising_price`, `falling_price`). Requires a single world name.
    #[utoipa::path(
        get,
        path = "/api/v1/trends/{world}",
        operation_id = "getTrends",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "Single FFXIV world name (e.g. Gilgamesh)"),
            TrendsQuery
        ),
        responses(
            (status = 200, description = "Market trend items for the requested window", body = TrendsData),
            (status = 400, description = "Scope is not a single world"),
            (status = 404, description = "World not found")
        )
    )]
    pub(super) fn get_trends() {}

    /// Get 24-hour market activity KPIs vs the prior 24 hours
    ///
    /// Returns sale count, gil volume, unit volume (today vs yesterday), and active
    /// listing count. Accepts a world, datacenter, or region name (multi-world scopes
    /// sum their worlds and report `world_id: 0`).
    #[utoipa::path(
        get,
        path = "/api/v1/market_pulse/{world}",
        operation_id = "getMarketPulse",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name")
        ),
        responses(
            (status = 200, description = "24h vs previous-24h market KPI summary", body = MarketPulseDto),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_market_pulse() {}

    /// Get 24-hour price heat across top-level item categories
    ///
    /// Always returns five category rows (1=Weapons, 2=Tools, 3=Armor, 4=Items,
    /// 5=Housing) with volume-weighted 24h price change and a heat band label.
    /// Requires a single world name.
    #[utoipa::path(
        get,
        path = "/api/v1/market_heat/{world}",
        operation_id = "getMarketHeat",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "Single FFXIV world name (e.g. Gilgamesh)")
        ),
        responses(
            (status = 200, description = "Category heat breakdown", body = MarketHeatResponse),
            (status = 400, description = "Scope is not a single world"),
            (status = 404, description = "World not found")
        )
    )]
    pub(super) fn get_market_heat() {}

    /// Get top market movers with 24-hour sparklines
    ///
    /// Ranks items over the trailing 24 hours by `rising` (default), `falling`,
    /// `volume` (units), or `gil` (gil volume), attaching a 24-point hourly VWAP
    /// sparkline to each row. Accepts a world, datacenter, or region name.
    #[utoipa::path(
        get,
        path = "/api/v1/movers/{world}",
        operation_id = "getMovers",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            MoversQuery
        ),
        responses(
            (status = 200, description = "Ranked market movers with 24h sparklines", body = MoversResponse),
            (status = 400, description = "Invalid `direction` parameter"),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_movers() {}

    /// Batch fetch hourly VWAP sparklines for up to 200 items
    ///
    /// Accepts a list of `[item_id, hq]` pairs and an optional `hours` window
    /// (clamped to 6..=168, default 24). Requires a single world name.
    #[utoipa::path(
        post,
        path = "/api/v1/sparklines/{world}",
        operation_id = "postSparklines",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "Single FFXIV world name (e.g. Gilgamesh)")
        ),
        request_body = SparklinesRequest,
        responses(
            (status = 200, description = "Hourly VWAP sparkline series for matching items", body = SparklinesResponse),
            (status = 400, description = "More than 200 items requested or non-world scope"),
            (status = 404, description = "World not found")
        )
    )]
    pub(super) fn post_sparklines() {}

    /// Batch fetch deep-scan resale quality metrics for up to 250 items
    ///
    /// Returns ClickHouse confidence band, VWAP, sample size, cleaned velocity, and
    /// launder suspicion for each `[item_id, hq]` pair that has rollup data on `{world}`.
    #[utoipa::path(
        post,
        path = "/api/v1/resale_quality/{world}",
        operation_id = "postResaleQuality",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "Single FFXIV world name (e.g. Gilgamesh)")
        ),
        request_body = ResaleQualityRequest,
        responses(
            (status = 200, description = "Per-item confidence and VWAP enrichment rows", body = ResaleQualityResponse),
            (status = 400, description = "More than 250 items requested or non-world scope"),
            (status = 404, description = "World not found")
        )
    )]
    pub(super) fn post_resale_quality() {}

    /// Get 30-day deep-scan quality and VWAP stats for one item
    ///
    /// Returns both NQ and HQ variants when present in the 30-day rollup. Multi-world
    /// scopes (datacenter or region) fold all member worlds and return `world_id: null`.
    #[utoipa::path(
        get,
        path = "/api/v1/item_stats/{world}/{itemid}",
        operation_id = "getItemStats",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            ("itemid" = i32, Path, description = "FFXIV item ID")
        ),
        responses(
            (status = 200, description = "Per-quality 30-day rolled-up item statistics", body = ItemStatsResponse),
            (status = 404, description = "World, datacenter, or region not found")
        )
    )]
    pub(super) fn get_item_stats() {}

    /// Get bulk sale-history statistics across all traded items in a scope
    ///
    /// Returns min, median, mean, VWAP, sale count, unit volume, gil volume, and
    /// sales/day for every `(item_id, hq)` traded in the trailing 1, 7, 30, or 90-day
    /// window. Pass `?format=columnar` for struct-of-arrays (`BulkSaleStatsColumnar`).
    #[utoipa::path(
        get,
        path = "/api/v1/sale_stats/{world}",
        operation_id = "getSaleStats",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            SaleStatsQuery
        ),
        responses(
            (status = 200, description = "Bulk sale statistics in row or columnar format", body = BulkSaleStatsResponse),
            (status = 400, description = "Unsupported `window` (must be 1, 7, 30, or 90)"),
            (status = 404, description = "World, datacenter, or region not found"),
            (status = 503, description = "Sale statistics rollup is still seeding")
        )
    )]
    pub(super) fn get_sale_stats() {}

    /// Get bulk active listing statistics with optional historical window
    ///
    /// Returns current alive listing count, total units, distinct retainers, oldest
    /// review timestamp, median age, and floor price per `(item_id, hq)`. Supply
    /// `?window=1|7|30|90` to attach historical additions/removals, undercuts, matched
    /// sales, and days-of-stock estimates. Pass `?format=columnar` for struct-of-arrays.
    #[utoipa::path(
        get,
        path = "/api/v1/listing_stats/{world}",
        operation_id = "getListingStats",
        tag = TAG_ANALYTICS,
        params(
            ("world" = String, Path, description = "World, datacenter, or region name"),
            ListingStatsQuery
        ),
        responses(
            (status = 200, description = "Bulk alive listing statistics in row or columnar format", body = BulkListingStatsResponse),
            (status = 400, description = "Unsupported `window` (must be 1, 7, 30, or 90)"),
            (status = 404, description = "World, datacenter, or region not found"),
            (status = 503, description = "Requested window snapshot is temporarily unavailable while refreshing")
        )
    )]
    pub(super) fn get_listing_stats() {}

    /// Search retainers by name
    ///
    /// Returns matching retainer records known to Ultros's market ingest.
    #[utoipa::path(
        get,
        path = "/api/v1/retainer/search/{query}",
        operation_id = "searchRetainers",
        tag = TAG_RETAINERS,
        params(
            ("query" = String, Path, description = "Partial or exact retainer name")
        ),
        responses(
            (status = 200, description = "Matching retainers", body = Vec<Retainer>)
        )
    )]
    pub(super) fn search_retainers() {}

    /// Get active market board listings for a retainer
    ///
    /// Returns retainer metadata and all currently active listings attributed to that retainer ID.
    #[utoipa::path(
        get,
        path = "/api/v1/retainer/listings/{id}",
        operation_id = "getRetainerListings",
        tag = TAG_RETAINERS,
        params(
            ("id" = i32, Path, description = "Ultros retainer ID (from `/api/v1/retainer/search/{query}` or listing payloads)")
        ),
        responses(
            (status = 200, description = "Retainer details and active listings", body = RetainerListings),
            (status = 404, description = "Retainer not found")
        )
    )]
    pub(super) fn get_retainer_listings() {}

    /// Get the complete FFXIV region, datacenter, and world tree
    ///
    /// Lists every region, datacenter, and world ID/name accepted by `{world}` path
    /// parameters across Ultros. Cached for 24 hours.
    #[utoipa::path(
        get,
        path = "/api/v1/world_data",
        operation_id = "getWorldData",
        tag = TAG_METADATA,
        responses(
            (status = 200, description = "Region → datacenter → world hierarchy", body = WorldData)
        )
    )]
    pub(super) fn get_world_data() {}

    /// Search FFXIV items, NPCs, currencies, and Ultros tools
    ///
    /// Performs full-text search across Ultros's in-memory search index and returns
    /// scored results with relative URLs, result types, icon IDs, and categories.
    #[utoipa::path(
        get,
        path = "/api/v1/search",
        operation_id = "searchUltros",
        tag = TAG_METADATA,
        params(SearchQuery),
        responses(
            (status = 200, description = "Ranked search matches", body = Vec<SearchResult>)
        )
    )]
    pub(super) fn search_ultros() {}

    /// Get FFXIV patch and expansion release calendar
    ///
    /// Returns chronological patch/expansion release dates used to annotate historical
    /// market charts. Filter by `?track=global|china|korea` or omit for all tracks.
    #[utoipa::path(
        get,
        path = "/api/v1/game-history",
        operation_id = "getGameHistory",
        tag = TAG_METADATA,
        params(GameHistoryQuery),
        responses(
            (status = 200, description = "Released FFXIV patches", body = Vec<GamePatch>),
            (status = 400, description = "Unknown `track` value")
        )
    )]
    pub(super) fn get_game_history() {}

    /// Get Ultros changelog entries
    ///
    /// Returns player-facing release notes ordered newest-first, then by importance.
    #[utoipa::path(
        get,
        path = "/api/v1/changelog",
        operation_id = "getChangelog",
        tag = TAG_METADATA,
        responses(
            (status = 200, description = "Ordered list of release notes", body = Vec<ChangelogEntry>)
        )
    )]
    pub(super) fn get_changelog() {}

    /// Detect the caller's closest FFXIV region
    ///
    /// Returns a plain-text region name (`North-America`, `Europe`, `Japan`, `Oceania`,
    /// `中国`, or `한국`) inferred from the Cloudflare `CF-IPCountry` header, defaulting
    /// to `North-America` when unavailable.
    #[utoipa::path(
        get,
        path = "/api/v1/detectregion",
        operation_id = "detectRegion",
        tag = TAG_METADATA,
        responses(
            (status = 200, description = "Detected FFXIV region name", content_type = "text/plain", body = String)
        )
    )]
    pub(super) fn detect_region() {}

    /// Get the server's Web Push VAPID public key
    ///
    /// Returns the base64url-encoded VAPID public key needed to call
    /// `PushManager.subscribe` in a browser. Returns `503 Service Unavailable` when
    /// Web Push is not configured on this server.
    #[utoipa::path(
        get,
        path = "/api/v1/push/vapid-public-key",
        operation_id = "getVapidPublicKey",
        tag = TAG_METADATA,
        responses(
            (status = 200, description = "Base64url VAPID application server key", body = VapidPublicKey),
            (status = 503, description = "Web Push is not configured on this deployment")
        )
    )]
    pub(super) fn get_vapid_public_key() {}
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ultros Market API",
        version = "1.0.0",
        description = "Unauthenticated HTTP and WebSocket API for real-time Final Fantasy XIV market board listings, sale history, server-bucketed price charts, undercut pressure, arbitrage deals, market trends, and world metadata. All endpoints listed here require no login or API key. Please respect `Cache-Control` headers and pass `?format=columnar` on bulk endpoints (`cheapest`, `recentSales`, `sale_stats`, `listing_stats`) when fetching full-market snapshots.",
        contact(name = "Ultros", url = "https://ultros.app"),
        license(name = "MIT OR Apache-2.0", url = "https://github.com/akarras/ultros")
    ),
    servers(
        (url = "https://ultros.app", description = "Ultros Production")
    ),
    tags(
        (name = TAG_MARKET_DATA, description = "Live market board listings, recent sales, bulk snapshots, and real-time WebSocket streams."),
        (name = TAG_PRICE_HISTORY, description = "Server-bucketed price/volume series, sale density heatmaps, listing floor history, and undercut pressure."),
        (name = TAG_ANALYTICS, description = "Cross-world arbitrage flips, trends, 24h KPIs, movers, sparklines, and bulk sale/listing statistics."),
        (name = TAG_RETAINERS, description = "Unauthenticated retainer search and active listing lookups."),
        (name = TAG_METADATA, description = "World hierarchy, full-text search, FFXIV patch calendar, changelog, region detection, and Web Push public key.")
    ),
    paths(
        paths::get_item_listings,
        paths::get_bulk_listings,
        paths::get_cheapest,
        paths::get_recent_sales,
        paths::get_extended_history,
        paths::get_realtime_events,
        paths::get_price_series,
        paths::get_price_density,
        paths::get_floor_history,
        paths::post_floor_history_batch,
        paths::get_undercut_pressure,
        paths::get_best_deals,
        paths::get_trends,
        paths::get_market_pulse,
        paths::get_market_heat,
        paths::get_movers,
        paths::post_sparklines,
        paths::post_resale_quality,
        paths::get_item_stats,
        paths::get_sale_stats,
        paths::get_listing_stats,
        paths::search_retainers,
        paths::get_retainer_listings,
        paths::get_world_data,
        paths::search_ultros,
        paths::get_game_history,
        paths::get_changelog,
        paths::detect_region,
        paths::get_vapid_public_key,
    ),
    components(schemas(
        ActiveListing,
        BulkListingStats,
        BulkListingStatsColumnar,
        BulkListingStatsResponse,
        BulkListingsResponse,
        BulkSaleStats,
        BulkSaleStatsColumnar,
        BulkSaleStatsResponse,
        CategoryHeat,
        ChangelogCategory,
        ChangelogEntry,
        ChangelogImportance,
        CheapestListingItem,
        CheapestListings,
        CheapestListingsColumnar,
        CheapestResponse,
        CompactSale,
        ConfidenceBand,
        CurrentlyShownItem,
        Datacenter,
        DensityCell,
        ExtendedSaleHistory,
        FloorBounds,
        FloorHistory,
        FloorHistoryBatch,
        FloorHistoryRequest,
        FloorInterval,
        FloorPoint,
        GamePatch,
        HeatBand,
        HistoryCoverage,
        HqFilter,
        ItemFloorHistory,
        ItemListingStats,
        ItemSaleStats,
        ItemStatsResponse,
        ItemStatsVariant,
        ListingAgeOrigin,
        ListingWindowStats,
        MarketHeatResponse,
        MarketPulseDto,
        MatchedSalesStats,
        MoverItem,
        MoversResponse,
        PatchTrack,
        PressureBucket,
        PressureState,
        PressureSummary,
        PriceBucket,
        PriceDensity,
        PriceSeries,
        PriceSeriesEntry,
        RecentSales,
        RecentSalesColumnar,
        RecentSalesResponse,
        Region,
        ResaleQualityRequest,
        ResaleQualityResponse,
        ResaleQualityRow,
        ResaleStatsDto,
        Retainer,
        RetainerListings,
        SaleData,
        SaleHistory,
        Sales,
        SearchResult,
        SeriesGroup,
        SparklineSeries,
        SparklinesRequest,
        SparklinesResponse,
        StockStatus,
        TrendItem,
        TrendsData,
        UndercutPressure,
        VapidPublicKey,
        WarSpan,
        WarStatus,
        World,
        WorldData,
        WorldItemLastUpdated,
    ))
)]
pub(crate) struct ApiDoc;

pub(crate) fn openapi_spec() -> &'static Value {
    static SPEC: OnceLock<Value> = OnceLock::new();
    SPEC.get_or_init(|| {
        serde_json::to_value(ApiDoc::openapi()).expect("utoipa OpenApi serializes to JSON")
    })
}

pub(crate) async fn get_openapi_json() -> Response {
    let body = serde_json::to_string_pretty(openapi_spec()).unwrap_or_else(|_| "{}".to_string());
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/openapi+json; charset=utf-8"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every unauthenticated `/api/*` route registered in `web::api_router()` must
    /// appear in the generated OpenAPI spec, and none of the deleted `/api/v1/public/*`
    /// paths may remain.
    #[test]
    fn openapi_spec_documents_every_unauthenticated_api_route() {
        let spec = openapi_spec();
        assert_eq!(spec["openapi"], "3.1.0");
        let paths = spec["paths"].as_object().expect("paths object");

        let expected_routes = [
            "/api/v1/listings/{world}/{itemid}",
            "/api/v1/bulkListings/{world}/{itemids}",
            "/api/v1/cheapest/{world}",
            "/api/v1/recentSales/{world}",
            "/api/v1/extended_history/{world}/{itemid}",
            "/api/v1/realtime/events",
            "/api/v1/price_series/{world}/{itemid}",
            "/api/v1/price_density/{world}/{itemid}",
            "/api/v1/floor_history/{world}/{itemid}",
            "/api/v1/floor_history/{world}",
            "/api/v1/undercut_pressure/{world}/{itemid}",
            "/api/v1/best_deals/{world}",
            "/api/v1/trends/{world}",
            "/api/v1/market_pulse/{world}",
            "/api/v1/market_heat/{world}",
            "/api/v1/movers/{world}",
            "/api/v1/sparklines/{world}",
            "/api/v1/resale_quality/{world}",
            "/api/v1/item_stats/{world}/{itemid}",
            "/api/v1/sale_stats/{world}",
            "/api/v1/listing_stats/{world}",
            "/api/v1/retainer/search/{query}",
            "/api/v1/retainer/listings/{id}",
            "/api/v1/world_data",
            "/api/v1/search",
            "/api/v1/game-history",
            "/api/v1/changelog",
            "/api/v1/detectregion",
            "/api/v1/push/vapid-public-key",
        ];

        assert_eq!(paths.len(), expected_routes.len());
        for route in expected_routes {
            assert!(paths.contains_key(route), "missing route {route}");
        }
        for path in paths.keys() {
            assert!(
                !path.starts_with("/api/v1/public/"),
                "deprecated public path still present: {path}"
            );
        }

        let schemas = spec["components"]["schemas"]
            .as_object()
            .expect("components.schemas object");
        for schema_name in [
            "CurrentlyShownItem",
            "ActiveListing",
            "CheapestListings",
            "CheapestListingsColumnar",
            "PriceSeries",
            "ResaleStatsDto",
            "TrendsData",
            "WorldData",
            "ChangelogEntry",
        ] {
            assert!(
                schemas.contains_key(schema_name),
                "missing schema {schema_name}"
            );
        }
    }

    #[test]
    fn openapi_spec_has_no_dangling_schema_refs() {
        fn check_refs(node: &Value, schemas: &serde_json::Map<String, Value>) {
            match node {
                Value::Object(map) => {
                    if let Some(Value::String(reference)) = map.get("$ref")
                        && let Some(name) = reference.strip_prefix("#/components/schemas/")
                    {
                        assert!(
                            schemas.contains_key(name),
                            "dangling $ref to missing schema: {reference}"
                        );
                    }
                    for value in map.values() {
                        check_refs(value, schemas);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        check_refs(item, schemas);
                    }
                }
                _ => {}
            }
        }

        let spec = openapi_spec();
        let schemas = spec["components"]["schemas"]
            .as_object()
            .expect("components.schemas object");
        check_refs(spec, schemas);
    }
}
