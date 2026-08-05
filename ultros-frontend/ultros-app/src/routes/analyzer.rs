use crate::analysis::{
    DerivedConfidence, SaleSummary, derived_confidence, get_sales_cadence, price_drift_pct,
    return_on_investment, roi_badge_class, velocity_per_day,
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use crate::{
    api::{
        get_cheapest_listings_live, get_recent_sales_for_world, get_resale_quality, post_sparklines,
    },
    components::{
        add_to_list::AddToList,
        clipboard::*,
        confidence_badge::ConfidenceBadge,
        dismissable::use_dismissable,
        filter_chip::{FilterChip, STICKY_BAR_HEIGHT},
        gil::*,
        icon::Icon,
        item_icon::*,
        meta::*,
        query_button::QueryButton,
        realtime_status::RealtimeStatus,
        sales_cadence_badge::SalesCadenceBadge,
        saved_views::SavedViewsMenu,
        skeleton::{SingleLineSkeleton, SkeletonCell, SkeletonColumn, TableSkeleton},
        sparkline::Sparkline,
        toggle::Toggle,
        tool_help::{ActionableEmptyState, ToolHeader},
        tooltip::*,
        virtual_scroller::*,
        world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
    math::filter_outliers_iqr_in_place,
    query_defaults::{
        DEFAULT_MAX_SALE_TIME, filter_query_signal, seed_flip_finder_default_view,
        seed_query_default,
    },
    routes::world_nav::world_nav_url,
};
use ultros_api_types::{
    resale_quality::ResaleQualityRow, sparklines::SparklinesRequest, trends::ConfidenceBand,
};

/// ClickHouse-backed per-row enrichment for the analyzer table. Built
/// asynchronously from one `resale_quality` + one `sparklines` batch
/// fetch and looked up by `(item_id, hq)` while rendering rows.
#[derive(Clone, Debug, Default)]
struct EnrichmentMaps {
    quality: HashMap<(i32, bool), ResaleQualityRow>,
    sparkline: HashMap<(i32, bool), Vec<u32>>,
    /// Keys whose fetch has completed (with OR without data). Lets cells tell
    /// "still loading" (absent) from "fetched, no CH data" (present, but no
    /// entry in `quality` / `sparkline`).
    settled: std::collections::HashSet<(i32, bool)>,
}

impl EnrichmentMaps {
    fn quality_for(&self, key: &(i32, bool)) -> Option<&ResaleQualityRow> {
        self.quality.get(key)
    }
    fn sparkline_for(&self, key: &(i32, bool)) -> Option<&Vec<u32>> {
        self.sparkline.get(key)
    }
    fn is_settled(&self, key: &(i32, bool)) -> bool {
        self.settled.contains(key)
    }
}

/// Stable URL IDs for optional columns. Required columns (HQ, Item,
/// Profit, Buy Price) are not in this list — they always render.
///
/// Order here is the columns-picker + `?cols=` serialization order:
/// default-on columns first, opt-ins after. It is deliberately *not* the
/// DOM order — the markup interleaves the required columns — but with the
/// default set the two coincide.
const COL_PROFIT_PER_DAY: &str = "profit_per_day";
const COL_VELOCITY: &str = "velocity";
const COL_DRIFT: &str = "drift";
const COL_CONFIDENCE: &str = "confidence";
const COL_WORLD: &str = "world";
const COL_LAST_SOLD: &str = "last_sold";
const COL_ROI: &str = "roi";
const COL_DATACENTER: &str = "datacenter";
const COL_TREND: &str = "trend";
const COL_SALES_PER_DAY: &str = "sales_per_day";
const COL_VOLUME_30D: &str = "volume_30d";

const ALL_OPTIONAL_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
    COL_ROI,
    COL_DATACENTER,
    COL_TREND,
    COL_SALES_PER_DAY,
    COL_VOLUME_30D,
];

/// Default visible set when `?cols=` is absent from the URL. Once the
/// user explicitly sets the param (even to ""), we respect that exact
/// set instead of falling back to defaults.
///
/// ClickHouse-only columns (trend, sales/day, 30d volume) are off because
/// the rollup covers ~7% of traded items, so they would be blank on most
/// rows. ROI is off because it ranks by ratio, which is the wrong
/// objective when retainer slots are the scarce resource.
const DEFAULT_VISIBLE_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
];

fn parse_visible_cols(raw: Option<&str>) -> std::collections::HashSet<&'static str> {
    match raw {
        None => DEFAULT_VISIBLE_COLS.iter().copied().collect(),
        Some(s) => s
            .split(',')
            .filter_map(|tok| ALL_OPTIONAL_COLS.iter().find(|c| **c == tok).copied())
            .collect(),
    }
}

fn serialize_visible_cols(visible: &std::collections::HashSet<&'static str>) -> String {
    ALL_OPTIONAL_COLS
        .iter()
        .filter(|c| visible.contains(*c))
        .copied()
        .collect::<Vec<_>>()
        .join(",")
}
use chrono::{Duration, Utc};
use gloo_timers::future::TimeoutFuture;
use humantime::parse_duration;
use icondata as i;
use leptos::{either::Either, prelude::*, reactive::wrappers::write::SignalSetter};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_location, use_navigate, use_params_map, use_query_map},
    location::Location,
};
use std::{
    cmp::Reverse,
    collections::{HashMap, hash_map::Entry},
    str::FromStr,
    sync::Arc,
};
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
    websocket::{FilterPredicate, SocketMessageType, is_analyzer_market_update_relevant},
    world_helper::{AnyResult, AnySelector, WorldHelper},
};
#[cfg(feature = "hydrate")]
use web_sys::wasm_bindgen::JsCast;
#[cfg(feature = "hydrate")]
use web_sys::wasm_bindgen::closure::Closure;
use xiv_gen::ItemId;

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
struct ProfitKey {
    item_id: i32,
    hq: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct ProfitData {
    estimated_sale_price: i32,
    cheapest_price: i32,
    cheapest_world_id: i32,
    /// Raw sale prices in the API's newest-first order, captured before
    /// `compute_summary` sorts its own copy. `price_drift_pct` needs the
    /// chronological order; every other consumer wants the sorted one.
    prices: Vec<i32>,
    sale_summary: SaleSummary,
}

#[derive(Clone, Debug, PartialEq)]
struct CalculatedProfitData {
    inner: Arc<ProfitData>,
    profit: i32,
    return_on_investment: i32,
    profit_per_day: i32,
}

/// Output of the filter + sort pass over the profit table.
#[derive(Clone, Debug, PartialEq, Default)]
struct FilteredRows {
    rows: Vec<(usize, CalculatedProfitData)>,
    /// Rows an active drift / confidence / volume floor could not evaluate
    /// on real data — dropped for lack of it (drift) or passed / judged on
    /// a substitute (volume, confidence). Zero whenever none of those
    /// floors is active. Drives the sticky bar's transparency note.
    rows_lacking_data: usize,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
    ProfitPerDay,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
enum SortDir {
    Asc,
    #[default]
    Desc,
}

impl FromStr for SortDir {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asc" => Ok(SortDir::Asc),
            "desc" => Ok(SortDir::Desc),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        })
    }
}

/// `?quality=` — show only HQ or only NQ rows. Param absent = both.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum QualityFilter {
    Hq,
    Nq,
}

impl FromStr for QualityFilter {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hq" => Ok(QualityFilter::Hq),
            "nq" => Ok(QualityFilter::Nq),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for QualityFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            QualityFilter::Hq => "hq",
            QualityFilter::Nq => "nq",
        })
    }
}

/// `?confidence=` — minimum confidence band a row must reach.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ConfidenceFloor {
    Low,
    Medium,
    High,
}

impl FromStr for ConfidenceFloor {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(ConfidenceFloor::Low),
            "medium" => Ok(ConfidenceFloor::Medium),
            "high" => Ok(ConfidenceFloor::High),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for ConfidenceFloor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ConfidenceFloor::Low => "low",
            ConfidenceFloor::Medium => "medium",
            ConfidenceFloor::High => "high",
        })
    }
}

/// Sort rows in place. Extracted from the `sorted_data` memo so the
/// ordering is unit-testable without a reactive runtime.
fn sort_rows(rows: &mut [CalculatedProfitData], mode: SortMode, dir: SortDir) {
    let key = |d: &CalculatedProfitData| -> i32 {
        match mode {
            SortMode::Roi => d.return_on_investment,
            SortMode::Profit => d.profit,
            SortMode::ProfitPerDay => d.profit_per_day,
        }
    };
    match dir {
        SortDir::Desc => rows.sort_by_key(|d| Reverse(key(d))),
        SortDir::Asc => rows.sort_by_key(key),
    }
}

#[derive(Clone, Debug)]
struct ProfitTable(Vec<Arc<ProfitData>>);

fn listings_to_map(listings: CheapestListings) -> HashMap<ProfitKey, (i32, i32)> {
    listings
        .cheapest_listings
        .into_iter()
        .map(|listing| {
            (
                ProfitKey {
                    item_id: listing.item_id,
                    hq: listing.hq,
                },
                (listing.cheapest_price, listing.world_id),
            )
        })
        .collect()
}

/// Sniper-clamp threshold: drop any sale priced below this fraction of the raw median.
const SNIPER_FRACTION: f64 = 0.1;

fn median_in_place_i32(sorted: &mut [i32]) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        let (_, &mut val, _) = sorted.select_nth_unstable(n / 2);
        val
    } else {
        let (left, &mut right, _) = sorted.select_nth_unstable(n / 2);
        let left_max = *left.iter().max().unwrap();
        ((left_max as i64 + right as i64) / 2) as i32
    }
}

fn compute_summary(sale: SaleData, filter_outliers: bool) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;

    if sales.is_empty() {
        return SaleSummary {
            item_id,
            hq,
            num_sold: 0,
            avg_sale_duration: None,
            days_since_last_sale: None,
            max_price: 0,
            avg_price: 0,
            median_price: 0,
            min_price: 0,
        };
    }

    // 1. Raw-median pass for the sniper threshold.
    let mut raw: Vec<i32> = sales.iter().map(|s| s.price_per_unit).collect();
    let raw_median = median_in_place_i32(&mut raw);
    let floor = (raw_median as f64 * SNIPER_FRACTION) as i32;

    // 2. Build the clamped vector. If the clamp would remove everything, keep the raw set.
    let mut clamped: Vec<i32> = raw.iter().copied().filter(|p| *p >= floor).collect();
    if clamped.is_empty() {
        clamped = raw;
    }
    let min_price = clamped.iter().copied().min().unwrap_or(0);
    let max_price = clamped.iter().copied().max().unwrap_or(0);
    let median_price = median_in_place_i32(&mut clamped);

    // 3. Average price respects the existing IQR filter-outliers toggle.
    let avg_price = if filter_outliers {
        let mut prices = clamped.clone();
        let filtered = filter_outliers_iqr_in_place(&mut prices);
        if filtered.is_empty() {
            0
        } else {
            (filtered.iter().map(|&p| p as i64).sum::<i64>() / filtered.len() as i64) as i32
        }
    } else {
        (clamped.iter().map(|&p| p as i64).sum::<i64>() / clamped.len() as i64) as i32
    };

    // 4. Velocity. Newest first in the API's response.
    let newest = sales.first().map(|s| s.sale_date);
    let oldest = sales.last().map(|s| s.sale_date);
    let avg_sale_duration = oldest.map(|last| {
        let ms = (last - now).num_milliseconds().abs() / sales.len() as i64;
        Duration::milliseconds(ms)
    });
    let days_since_last_sale =
        newest.map(|n| Duration::milliseconds((now - n).num_milliseconds().max(0)));

    SaleSummary {
        item_id,
        hq,
        num_sold: sales.len(),
        avg_sale_duration,
        days_since_last_sale,
        max_price,
        avg_price,
        median_price,
        min_price,
    }
}

// Add FromStr and ToString implementations for SortMode
impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            "profit-per-day" => Ok(SortMode::ProfitPerDay),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Roi => "roi",
            SortMode::Profit => "profit",
            SortMode::ProfitPerDay => "profit-per-day",
        };
        f.write_str(val)
    }
}

/// Listings whose price is at least this multiple of the row's median sale are treated as troll
/// listings and ignored when picking the world floor.
const TROLL_MULTIPLE: i64 = 50;

fn is_troll_listing(price: i32, median: i32) -> bool {
    median > 0 && (price as i64) > (median as i64).saturating_mul(TROLL_MULTIPLE)
}

impl ProfitTable {
    fn new(
        sales: RecentSales,
        global_cheapest_listings: CheapestListings,
        world_cheapest_listings: CheapestListings,
        cross_region: Vec<CheapestListings>,
        filter_outliers: bool,
    ) -> Self {
        let mut region_cheapest = listings_to_map(global_cheapest_listings);
        let world_cheapest = listings_to_map(world_cheapest_listings);

        for cross in cross_region.into_iter().map(listings_to_map) {
            for (key, (new_price, world_id)) in cross {
                match region_cheapest.entry(key) {
                    Entry::Occupied(mut entry) => {
                        let (current_price, _) = entry.get();
                        if *current_price > new_price {
                            entry.insert((new_price, world_id));
                        }
                    }
                    Entry::Vacant(e) => {
                        e.insert((new_price, world_id));
                    }
                }
            }
        }

        let table = sales
            .sales
            .into_iter()
            .flat_map(|sale| {
                let item_id = sale.item_id;
                let hq = sale.hq;
                let key = ProfitKey { item_id, hq };
                let (raw_region_price, region_world_id) = *region_cheapest.get(&key)?;
                // Capture the wire-order prices before `compute_summary` consumes
                // the SaleData — the Drift column reads them newest-first.
                let prices: Vec<i32> = sale.sales.iter().map(|s| s.price_per_unit).collect();
                let summary = compute_summary(sale, filter_outliers);

                // Troll-listing guard: if the region floor is implausibly high vs the median,
                // drop the row entirely — the displayed "deal" would be fictional.
                if is_troll_listing(raw_region_price, summary.median_price) {
                    return None;
                }

                // Same guard on the local world floor — if it's a troll, ignore it and fall
                // through to the median as the estimate.
                let world_floor = world_cheapest.get(&key).and_then(|(price, _)| {
                    if is_troll_listing(*price, summary.median_price) {
                        None
                    } else {
                        Some(*price)
                    }
                });

                let estimated_sale_price = match world_floor {
                    Some(floor) => summary.median_price.min(floor),
                    None => summary.median_price,
                };

                Some(ProfitData {
                    estimated_sale_price,
                    prices,
                    sale_summary: summary,
                    cheapest_world_id: region_world_id,
                    cheapest_price: raw_region_price,
                })
            })
            .map(Arc::new)
            .collect();

        ProfitTable(table)
    }
}

/// Rows fetched above & below the rendered window, so enrichment lands just
/// before a row scrolls into view. Keep small enough that
/// `rendered (~26) + 2 * PREFETCH_MARGIN` stays well under the 200-item
/// sparklines cap (no chunking needed).
const PREFETCH_MARGIN: usize = 30;
/// Debounce window for scroll-driven fetches (ms). Mirrors search_box.rs.
const DEBOUNCE_MS: u32 = 150;

/// Keys in the `[start - margin, end + margin)` slice of `data`, minus `seen`.
/// Generic over the row type + a key extractor so it unit-tests with plain
/// `(i32, bool)` fixtures — no `CalculatedProfitData` / DOM needed. Wired into
/// the lazy-enrichment effect in `AnalyzerTable`.
fn visible_keys<T>(
    data: &[T],
    range: (usize, usize),
    margin: usize,
    seen: &std::collections::HashSet<(i32, bool)>,
    key_of: impl Fn(&T) -> (i32, bool),
) -> Vec<(i32, bool)> {
    let (start, end) = range;
    let lo = start.saturating_sub(margin);
    let hi = (end + margin).min(data.len());
    data.get(lo..hi)
        .unwrap_or(&[])
        .iter()
        .map(key_of)
        .filter(|k| !seen.contains(k))
        .collect()
}

/// Normalize a raw `?vel=` value into a usable floor.
///
/// `"NaN".parse::<f32>()` succeeds, and `v >= NaN` is false for every row,
/// so a crafted `?vel=NaN` would silently empty the table. Non-finite
/// values are treated as "no floor". Normalizing in one place keeps the
/// filter, the chip and the toolbar input agreeing about whether a floor
/// is active.
fn normalize_velocity_floor(raw: Option<f32>) -> Option<f32> {
    raw.filter(|v| v.is_finite())
}

/// Does a row clear an explicit velocity floor?
///
/// Prefers the ClickHouse rate so the number the Velocity column displays
/// is the number the filter evaluates, and falls back to the rate derived
/// from the 6-sale buffer for the ~93% of rows the rollup does not cover.
/// A row with no rate at all cannot clear a floor.
fn passes_velocity_floor(min: f32, ch_rate: Option<f32>, derived: Option<f32>) -> bool {
    ch_rate.or(derived).map(|v| v >= min).unwrap_or(false)
}

fn passes_quality(filter: QualityFilter, hq: bool) -> bool {
    match filter {
        QualityFilter::Hq => hq,
        QualityFilter::Nq => !hq,
    }
}

/// Normalize a raw `?name=` query for matching: trim + lowercase, once per
/// recompute rather than once per row. A blank query maps to `None` (no
/// filter) — the chip seeds empty so the user can type into it, and that
/// state must not blank the table.
fn normalize_name_query(raw: &str) -> Option<String> {
    let q = raw.trim().to_lowercase();
    (!q.is_empty()).then_some(q)
}

/// Case-insensitive substring match against a query pre-normalized by
/// [`normalize_name_query`]. Runs per row, so only the item name is
/// lowercased here.
fn matches_normalized_name(query_lower: &str, item_name: &str) -> bool {
    item_name.to_lowercase().contains(query_lower)
}

/// Does a row clear the `?drift=` floor? Drift comes off the row's own
/// price buffer, but `price_drift_pct` needs at least 4 of the (up to 6)
/// buffered sales, so thinly-traded rows have no drift at all. A row with
/// too few sales to compute a drift fails an explicit floor — the velocity
/// floor's rule — and the sticky bar counts it toward the "rows lack data"
/// note so the drop is visible rather than silent.
fn passes_drift_floor(min: f32, drift: Option<f32>) -> bool {
    drift.map(|d| d >= min).unwrap_or(false)
}

/// Bands on one scale: Unusable=0 < Low=1 < Medium=2 < High=3. The CH
/// `Unknown` variant is "no deep scan yet", not a verdict, so it defers to
/// the derived band — the same preference the Confidence column renders.
fn confidence_rank(ch: Option<ConfidenceBand>, derived: DerivedConfidence) -> u8 {
    match ch {
        Some(ConfidenceBand::High) => 3,
        Some(ConfidenceBand::Medium) => 2,
        Some(ConfidenceBand::Low) => 1,
        Some(ConfidenceBand::Unusable) => 0,
        Some(ConfidenceBand::Unknown) | None => match derived {
            DerivedConfidence::High => 3,
            DerivedConfidence::Medium => 2,
            DerivedConfidence::Low => 1,
        },
    }
}

fn passes_confidence_floor(
    floor: ConfidenceFloor,
    ch: Option<ConfidenceBand>,
    derived: DerivedConfidence,
) -> bool {
    let floor_rank = match floor {
        ConfidenceFloor::Low => 1,
        ConfidenceFloor::Medium => 2,
        ConfidenceFloor::High => 3,
    };
    confidence_rank(ch, derived) >= floor_rank
}

/// Does a row clear the `?min-volume=` floor? 30-day volume is ClickHouse-
/// only (~7% item coverage) AND lazily enriched per visible window — if
/// unknown failed, the un-enriched initial table would filter to zero rows
/// and the visible-window fetch would never fire. So unknown rows pass,
/// and only a *known* volume below the floor drops a row (the suspicious
/// filter's rule).
fn passes_volume_floor(min: u32, ch_volume: Option<u32>) -> bool {
    ch_volume.map(|v| v >= min).unwrap_or(true)
}

// --- Filter registry -------------------------------------------------------
// Each id is the `query_signal` key it drives, so the list doubles as the
// URL contract. The sticky bar renders one chip per *set* filter and the
// `+ Filter` menu offers the rest — no filter is ever drawn twice.
const FILTER_PROFIT: &str = "profit";
const FILTER_PROFIT_PER_DAY: &str = "ppd";
const FILTER_ROI: &str = "roi";
const FILTER_SALES: &str = "sales";
const FILTER_VELOCITY: &str = "vel";
const FILTER_MIN_BUY: &str = "min-buy";
const FILTER_MAX_PRICE: &str = "max-price";
const FILTER_NEXT_SALE: &str = "next-sale";
const FILTER_LAST_SOLD: &str = "last-sold";
const FILTER_PRE_TAX: &str = "tax";
const FILTER_SHOW_SUSPICIOUS: &str = "show-suspicious";
const FILTER_QUALITY: &str = "quality";
const FILTER_NAME: &str = "name";
// The *values* of the next two deliberately equal the COL_DRIFT /
// COL_CONFIDENCE tokens — they live in different namespaces (`?cols=`
// tokens vs. query-param keys) and the param names are public wire format
// (bookmarks, saved views), so only the Rust identifiers disambiguate.
const FILTER_MIN_DRIFT: &str = "drift";
const FILTER_MIN_CONFIDENCE: &str = "confidence";
const FILTER_MIN_VOLUME: &str = "min-volume";
// Chip-only filters: picked from a list or off a row rather than typed, so
// they are never offered by the `+ Filter` menu.
const FILTER_CATEGORY: &str = "category";
const FILTER_WORLD: &str = "world";
const FILTER_DATACENTER: &str = "datacenter";

/// Filters the `+ Filter` menu can add, in menu order.
const ADDABLE_FILTERS: &[&str] = &[
    FILTER_PROFIT,
    FILTER_PROFIT_PER_DAY,
    FILTER_ROI,
    FILTER_SALES,
    FILTER_VELOCITY,
    FILTER_MIN_BUY,
    FILTER_MAX_PRICE,
    FILTER_NEXT_SALE,
    FILTER_LAST_SOLD,
    FILTER_PRE_TAX,
    FILTER_SHOW_SUSPICIOUS,
    FILTER_QUALITY,
    FILTER_NAME,
    FILTER_MIN_DRIFT,
    FILTER_MIN_CONFIDENCE,
    FILTER_MIN_VOLUME,
];

/// Value a filter takes when it is added from the `+ Filter` menu.
///
/// A filter with no starting value would render a chip with nothing in it,
/// so every entry in [`ADDABLE_FILTERS`] must have one, except `FILTER_NAME`,
/// whose chip deliberately mounts in edit state instead (see the arm below).
fn default_filter_value(id: &str) -> &'static str {
    match id {
        FILTER_PROFIT => "100000",
        FILTER_PROFIT_PER_DAY => "10000",
        FILTER_ROI => "200",
        FILTER_SALES => "2",
        FILTER_VELOCITY => "0.2",
        FILTER_MIN_BUY => "5000",
        FILTER_MAX_PRICE => "500000",
        FILTER_NEXT_SALE => "7d",
        FILTER_LAST_SOLD => "1d",
        // Booleans: the chip's presence *is* the value, so `x` restores the
        // default (post-tax, suspicious rows hidden).
        FILTER_PRE_TAX => "false",
        FILTER_SHOW_SUSPICIOUS => "true",
        FILTER_QUALITY => "hq",
        // Name search deliberately seeds empty: its chip mounts in edit
        // state (`start_editing`) so there is never an empty resting chip.
        FILTER_NAME => "",
        FILTER_MIN_DRIFT => "-10",
        FILTER_MIN_CONFIDENCE => "medium",
        FILTER_MIN_VOLUME => "10",
        _ => "",
    }
}

/// Apply an edited chip value to a numeric filter.
///
/// A parse failure keeps `current`. The toolbar these chips replaced did the
/// same (`if let Ok(v) = … { set } else if value.is_empty() { clear }`), and
/// the alternative is worse than it sounds: `set(raw.parse().ok())` deletes
/// the filter the user is in the middle of editing the moment they type
/// something the target type rejects — `-5` into a `usize` count, say — with
/// no message and no undo. Only an explicit clear removes a filter, and by
/// the time a value reaches here `committed_value` has already mapped blank
/// input to `None`.
fn commit_numeric<T: FromStr>(current: Option<T>, raw: Option<String>) -> Option<T> {
    match raw {
        None => None,
        Some(s) => s.parse::<T>().ok().or(current),
    }
}

/// Render a velocity floor for the chip's resting state.
///
/// `f32::to_string` prints the round-tripped `?vel=0.2` as
/// `0.20000000298023224`. Two decimals is finer than the filter's own
/// resolution, and the result parses back to the same value, so the input
/// shows this too rather than a second, uglier spelling of the same number.
fn format_velocity_floor(v: f32) -> String {
    let s = format!("{v:.2}");
    match s.contains('.') {
        // `trim_end_matches('0')` alone would turn "10.00" into "1".
        true => s.trim_end_matches('0').trim_end_matches('.').to_string(),
        false => s,
    }
}

/// Rendered width of the optional columns that are *not* in the default set,
/// in px, bucketed by the breakpoint at which each column actually renders.
///
/// The grid's base width lives in the stylesheet, which is the only place that
/// can know which columns a breakpoint hides. What it cannot know is which
/// optional columns the user switched on, so that part is measured here and
/// handed over as `--analyzer-extra-cols-{base,md,xl}`. Under-reserving is the
/// failure that matters: the two scrollports would stop short of the last
/// column and it would be unreachable.
///
/// The bucketing exists for the opposite failure: several opt-in columns are
/// `hidden md:flex` / `hidden xl:flex`, and reserving their width below the
/// breakpoint that reveals them gives a phone a horizontal scroll range whose
/// far end is empty space. Each bucket is only added by the stylesheet's media
/// query for that breakpoint (see `style/tailwind.css`), which keeps the whole
/// mechanism CSS-driven — no `matchMedia` read, so SSR and the first client
/// render stay identical.
#[derive(Debug, Default, PartialEq, Eq)]
struct ExtraColumnWidths {
    /// Columns visible at every viewport width.
    base: u32,
    /// Columns hidden below `md` (768px).
    md: u32,
    /// Columns hidden below `xl` (1280px).
    xl: u32,
}

fn extra_column_widths_px(visible: &std::collections::HashSet<&'static str>) -> ExtraColumnWidths {
    // Width AND breakpoint here must match the column's header/cell markup
    // (`w-[..]` + `hidden md:flex` etc.) in the view below.
    const ALWAYS: &[(&str, u32)] = &[(COL_ROI, 112)];
    const MD: &[(&str, u32)] = &[
        (COL_TREND, 100),
        (COL_SALES_PER_DAY, 140),
        (COL_VOLUME_30D, 88),
    ];
    const XL: &[(&str, u32)] = &[(COL_DATACENTER, 112)];
    let sum = |widths: &[(&str, u32)]| {
        widths
            .iter()
            .filter(|(col, _)| visible.contains(col))
            .map(|(_, w)| w)
            .sum()
    };
    ExtraColumnWidths {
        base: sum(ALWAYS),
        md: sum(MD),
        xl: sum(XL),
    }
}

/// The loading skeleton's version of the grid, in DOM order.
///
/// Each entry's class string is the matching cell's class from the row markup
/// below — same width, same responsive visibility, same alignment — so the
/// placeholder columns sit exactly where the real ones will. Keep the two in
/// step: a column added to the row markup but not here makes the table appear
/// to gain a column when it loads.
///
/// Three cells differ from their real counterparts on purpose. World,
/// datacenter and last-sold are written `hidden lg:block flex` / `hidden
/// md:block flex` in the row markup — `block` and `flex` on the same element,
/// where which one wins is down to stylesheet order rather than intent — so
/// the skeleton spells them `hidden lg:flex` / `hidden md:flex`, which is what
/// the `items-center` beside them was reaching for. The widths, which are all
/// the alignment depends on, are identical either way.
fn analyzer_skeleton_columns(
    visible: &std::collections::HashSet<&'static str>,
) -> Vec<SkeletonColumn> {
    /// `(gate, class, cell)` in DOM order. A `None` gate is a column that
    /// always renders; the rest follow `?cols=`.
    const COLUMNS: &[(Option<&str>, &str, SkeletonCell)] = &[
        // HQ. Most rows are NQ, so this one stays empty.
        (
            None,
            "px-2 py-2 w-[44px] shrink-0 flex items-center justify-center",
            SkeletonCell::Blank,
        ),
        (
            None,
            "px-4 py-2 flex flex-row flex-1 min-w-[14rem] items-center gap-2",
            SkeletonCell::IconText,
        ),
        // Profit.
        (
            None,
            "px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        (
            Some(COL_PROFIT_PER_DAY),
            "px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        (
            Some(COL_VELOCITY),
            "px-3 py-2 w-[88px] shrink-0 hidden md:flex items-center justify-end",
            SkeletonCell::Number,
        ),
        (
            Some(COL_DRIFT),
            "px-3 py-2 w-[88px] shrink-0 hidden md:flex items-center justify-end",
            SkeletonCell::Number,
        ),
        (
            Some(COL_CONFIDENCE),
            "px-3 py-2 w-[72px] shrink-0 hidden md:flex items-center justify-center",
            SkeletonCell::Badge,
        ),
        (
            Some(COL_ROI),
            "px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end",
            SkeletonCell::Badge,
        ),
        // Buy price. Always on, and it sits after ROI in the row markup.
        (
            None,
            "px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        (
            Some(COL_WORLD),
            "px-3 py-2 w-28 shrink-0 hidden lg:flex items-center",
            SkeletonCell::Text,
        ),
        (
            Some(COL_DATACENTER),
            "px-3 py-2 w-28 shrink-0 hidden xl:flex items-center",
            SkeletonCell::Text,
        ),
        (
            Some(COL_TREND),
            "px-3 py-2 w-[100px] shrink-0 hidden md:flex items-center justify-center",
            SkeletonCell::Spark,
        ),
        (
            Some(COL_SALES_PER_DAY),
            "px-3 py-2 w-[140px] shrink-0 hidden md:flex items-center justify-center",
            SkeletonCell::Badge,
        ),
        (
            Some(COL_VOLUME_30D),
            "px-3 py-2 w-[88px] shrink-0 hidden md:flex items-center justify-end",
            SkeletonCell::Number,
        ),
        (
            Some(COL_LAST_SOLD),
            "px-3 py-2 w-28 shrink-0 hidden md:flex items-center",
            SkeletonCell::Text,
        ),
    ];
    COLUMNS
        .iter()
        .filter(|(gate, _, _)| gate.is_none_or(|col| visible.contains(col)))
        .map(|(_, class, cell)| SkeletonColumn::new(class, *cell))
        .collect()
}

/// The Flip Finder's loading state: the results grid, drawn empty.
///
/// Reads `?cols=` the same way the table does, so the skeleton shows the
/// columns this particular user has switched on rather than a generic set —
/// and reproduces the container's `--analyzer-extra-cols-*` variables, which
/// is what makes `.analyzer-grid-row` give the placeholder rows the same
/// min-width as the real ones.
#[component]
fn AnalyzerTableSkeleton() -> impl IntoView {
    let (cols_param, _) = query_signal::<String>("cols");
    let visible = parse_visible_cols(cols_param.get_untracked().as_deref());
    let widths = extra_column_widths_px(&visible);
    view! {
        <TableSkeleton
            columns=analyzer_skeleton_columns(&visible)
            rows=14
            class="analyzer-table border border-[color:var(--color-outline)]"
            row_class="analyzer-grid-row"
            style=format!(
                "--analyzer-extra-cols-base: {}px; --analyzer-extra-cols-md: {}px; --analyzer-extra-cols-xl: {}px;",
                widths.base,
                widths.md,
                widths.xl,
            )
        />
    }
}

/// Tailwind class that hides a column's "desktop only" note in the Columns
/// picker once the viewport is wide enough to actually render the column.
/// `None` for columns visible at every width. Must mirror the `hidden
/// md:flex` / `lg:flex` / `xl:flex` classes on the column's own markup.
///
/// Ticking a hidden column on a phone changes nothing on screen, which reads
/// as a broken checkbox; the note explains it. The gating is pure CSS so SSR
/// and the first client render agree.
fn col_hidden_note_class(col: &str) -> Option<&'static str> {
    match col {
        c if c == COL_VELOCITY
            || c == COL_DRIFT
            || c == COL_CONFIDENCE
            || c == COL_TREND
            || c == COL_SALES_PER_DAY
            || c == COL_VOLUME_30D
            || c == COL_LAST_SOLD =>
        {
            Some("md:hidden")
        }
        c if c == COL_WORLD => Some("lg:hidden"),
        c if c == COL_DATACENTER => Some("xl:hidden"),
        _ => None,
    }
}

/// Filters the `+ Filter` menu should offer: everything addable that is not
/// already on screen as a chip.
fn available_filters(active: &[&str]) -> Vec<&'static str> {
    ADDABLE_FILTERS
        .iter()
        .copied()
        .filter(|id| !active.contains(id))
        .collect()
}

/// Regions whose cross-region listings can be pulled in alongside the
/// current world's own region. Shared between the cross-region toggle's
/// resource (in `AnalyzerWorldView`) and the per-region opt-out checkboxes
/// rendered in the Columns popover (in `AnalyzerTable`) — both need the same
/// list, and only one of them may query it.
const CONNECTED_REGIONS: &[&str] = &["Europe", "Japan", "North-America", "Oceania"];

/// One sortable column header.
///
/// Clicking an inactive column sorts by it descending; clicking the column
/// already in effect flips the direction. The arrow reflects the direction
/// actually applied — the three call sites this replaces each hardcoded a
/// down arrow, so `?dir=asc` rendered ascending rows under a descending
/// glyph, and nothing in the UI could reach `?dir=` at all.
///
/// `dir` is omitted from the href when it would be the default, so the
/// common case stays a clean `?sort=…` and bookmarks don't accumulate a
/// redundant param.
#[component]
fn SortHeader(
    mode: SortMode,
    #[prop(into)] label: String,
    sort_mode: Memo<Option<SortMode>>,
    sort_dir: Memo<Option<SortDir>>,
) -> impl IntoView {
    let Location {
        pathname, query, ..
    } = use_location();
    let is_active = Signal::derive(move || sort_mode().unwrap_or(SortMode::ProfitPerDay) == mode);
    let dir = Signal::derive(move || sort_dir().unwrap_or_default());
    view! {
        <a
            class=move || {
                if is_active() {
                    "!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                } else {
                    "!text-brand-300 hover:text-brand-200"
                }
            }
            aria-current=move || if is_active() { "true" } else { "false" }
            href=move || {
                let mut q = query();
                q.remove("sort");
                q.remove("dir");
                q.insert("sort".to_string(), mode.to_string());
                let next = if is_active() {
                    match dir() {
                        SortDir::Desc => SortDir::Asc,
                        SortDir::Asc => SortDir::Desc,
                    }
                } else {
                    SortDir::Desc
                };
                if next != SortDir::default() {
                    q.insert("dir".to_string(), next.to_string());
                }
                format!("{}{}", pathname(), q.to_query_string())
            }
        >
            <div class="flex items-center gap-2">
                {label}
                {move || {
                    is_active()
                        .then(|| match dir() {
                            SortDir::Asc => view! { <Icon icon=i::BiSortUpRegular /> },
                            SortDir::Desc => view! { <Icon icon=i::BiSortDownRegular /> },
                        })
                }}
            </div>
        </a>
    }
    .into_any()
}

#[component]
fn AnalyzerTable(
    sales: RecentSales,
    global_cheapest_listings: CheapestListings,
    world_cheapest_listings: CheapestListings,
    cross_region: Vec<CheapestListings>,
    worlds: Arc<WorldHelper>,
    world: Signal<String>,
    filter_outliers: bool,
    /// Current world's region name, if resolvable. Only used to exclude the
    /// current region from the cross-region opt-out list in the Columns
    /// popover — a plain value like `filter_outliers`, not a reactive prop,
    /// since this component remounts whenever the caller's region changes.
    region: Option<String>,
    /// Current state of the cross-region toggle, mirroring `filter_outliers`.
    cross_region_enabled: bool,
    /// The caller's own `query_signal` setters for `?cross=` / `?filter-outliers=`.
    /// Threaded through as props rather than re-derived here so there is a
    /// single `query_signal` per URL key instead of two independent ones
    /// drifting in and out of the router's query-mutation queue.
    set_cross_region_enabled: SignalSetter<Option<bool>>,
    set_filter_outliers: SignalSetter<Option<bool>>,
    on_market_update: Callback<()>,
    /// Keeps the name chip mounted (in edit state) between "picked from the
    /// + Filter menu" and "first committed value" — an empty ?name= URL
    /// param is not relied on to round-trip. Owned by `AnalyzerWorldView`:
    /// this component lives inside the Suspense closure and remounts on
    /// every realtime market tick, so a signal declared here would be
    /// destroyed mid-keystroke along with the chip being typed into.
    name_chip_pending: RwSignal<bool>,
    /// True once client hydration has finished (Effect-set by the caller).
    /// Also owned by `AnalyzerWorldView` — declared here it would reset to
    /// false on every market-tick remount, rendering one full unfiltered
    /// pass per tick whenever `?name=` is active.
    hydrated: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let realtime = use_realtime();
    let realtime_for_market = realtime.clone();
    let rt_status = realtime.clone();
    let realtime_status = Signal::derive(move || {
        rt_status
            .as_ref()
            .map(|r| r.status.get())
            .unwrap_or_else(|| "offline".to_string())
    });
    let rt_update = realtime.clone();
    let last_update = Signal::derive(move || rt_update.as_ref().and_then(|r| r.last_update.get()));
    let profits = ProfitTable::new(
        sales,
        global_cheapest_listings,
        world_cheapest_listings,
        cross_region,
        filter_outliers,
    );

    let items = &tracked_data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    // Filter params use `filter_query_signal` (replace: true, scroll: false):
    // every keystroke in a chip writes the URL, and `query_signal`'s defaults
    // would push a history entry and yank the window to the top each time.
    // `sort`/`dir`/`cols` stay on plain `query_signal` — those are deliberate,
    // discrete actions where a history entry is wanted.
    let (minimum_profit, set_minimum_profit) = filter_query_signal::<i32>("profit");
    let (minimum_profit_per_day, set_minimum_profit_per_day) = filter_query_signal::<i32>("ppd");
    let (minimum_roi, set_minimum_roi) = filter_query_signal::<i32>("roi");
    // Seeded to 1d by AnalyzerWorldView so a first-time visitor isn't shown
    // items that sell once a month. The field sits in the primary toolbar and
    // the chip has an X, so the default is visible and one click from gone.
    let (max_predicted_time, set_max_predicted_time) = filter_query_signal::<String>("next-sale");
    let (world_filter, set_world_filter) = filter_query_signal::<String>("world");
    let (datacenter_filter, set_datacenter_filter) = filter_query_signal::<String>("datacenter");
    let (tax_enabled, set_tax_enabled) = filter_query_signal::<bool>("tax");
    let (minimum_sales, set_minimum_sales) = filter_query_signal::<usize>("sales");
    let (min_velocity, set_min_velocity) = filter_query_signal::<f32>("vel");
    // Single normalization point for the floor — the filter, the summary
    // chip and the toolbar input all read this, never `min_velocity` raw.
    let velocity_floor = Memo::new(move |_| normalize_velocity_floor(min_velocity()));
    let (category_filter, set_category_filter) = filter_query_signal::<i32>("category");
    let (max_purchase_price, set_max_purchase_price) = filter_query_signal::<i32>("max-price");
    let (min_buy_price, set_min_buy_price) = filter_query_signal::<i32>("min-buy");
    let (show_suspicious, set_show_suspicious) = filter_query_signal::<bool>("show-suspicious");
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    // The five column filters use `filter_query_signal` (replace: true,
    // scroll: false) — editing a filter must not push a history entry per
    // keystroke or yank the window back to the top.
    let (quality_filter, set_quality_filter) = filter_query_signal::<QualityFilter>("quality");
    let (name_filter, set_name_filter) = filter_query_signal::<String>("name");
    let (min_drift, set_min_drift) = filter_query_signal::<f32>("drift");
    // Same NaN guard as ?vel= — "NaN".parse::<f32>() succeeds and would
    // silently empty the table (every comparison with NaN is false).
    let drift_floor = Memo::new(move |_| normalize_velocity_floor(min_drift()));
    let (min_confidence, set_min_confidence) = filter_query_signal::<ConfidenceFloor>("confidence");
    let (min_volume, set_min_volume) = filter_query_signal::<u32>("min-volume");
    let visible_cols = Memo::new(move |_| parse_visible_cols(cols_param().as_deref()));
    let show_suspicious_active = Memo::new(move |_| show_suspicious().unwrap_or(false));
    let show_columns_picker = RwSignal::new(false);
    let show_filter_menu = RwSignal::new(false);
    // Route change, click outside, Escape. Both popovers and both trigger
    // buttons live inside the sticky bar, so one container covers both;
    // the triggers keep their own mutual exclusivity.
    let sticky_bar_ref = NodeRef::<leptos::html::Div>::new();
    use_dismissable(sticky_bar_ref, move || {
        show_columns_picker.set(false);
        show_filter_menu.set(false);
    });

    let world_clone = worlds.clone();
    let world_filter_list = Memo::new(move |_| {
        let world = world_filter().or_else(datacenter_filter)?;
        let filter = world_clone
            .lookup_world_by_name(&world)?
            .all_worlds()
            .map(|w| w.id)
            .collect::<Vec<_>>();
        Some(filter)
    });

    let world_clone = worlds.clone();
    let buy_filter = Memo::new(move |_| {
        let world = world_filter().or_else(datacenter_filter)?;
        world_clone
            .lookup_world_by_name(&world)
            .map(|world| AnySelector::from(&world))
    });

    let world_clone = worlds.clone();
    let lookup_world = Memo::new(move |_| {
        Some(AnySelector::from(
            &world_clone.lookup_world_by_name(&world())?,
        ))
    });

    let predicted_time =
        Memo::new(move |_| max_predicted_time().and_then(|d| parse_duration(d.as_str()).ok()));

    let (last_sold_within, set_last_sold_within) = filter_query_signal::<String>("last-sold");
    let last_sold_duration =
        Memo::new(move |_| last_sold_within().and_then(|d| parse_duration(d.as_str()).ok()));

    // Filters currently drawn as a chip. Drives the "no active filters"
    // hint and keeps `+ Filter` from offering a second copy of something
    // the user can already see.
    let active_filters = Memo::new(move |_| {
        let mut active: Vec<&'static str> = Vec::new();
        let mut push_if = |set: bool, id: &'static str| {
            if set {
                active.push(id);
            }
        };
        push_if(minimum_profit().is_some(), FILTER_PROFIT);
        push_if(minimum_profit_per_day().is_some(), FILTER_PROFIT_PER_DAY);
        push_if(minimum_roi().is_some(), FILTER_ROI);
        push_if(minimum_sales().is_some(), FILTER_SALES);
        push_if(velocity_floor().is_some(), FILTER_VELOCITY);
        push_if(min_buy_price().is_some(), FILTER_MIN_BUY);
        push_if(max_purchase_price().is_some(), FILTER_MAX_PRICE);
        push_if(max_predicted_time().is_some(), FILTER_NEXT_SALE);
        push_if(last_sold_within().is_some(), FILTER_LAST_SOLD);
        push_if(tax_enabled() == Some(false), FILTER_PRE_TAX);
        push_if(show_suspicious_active(), FILTER_SHOW_SUSPICIOUS);
        push_if(category_filter().is_some(), FILTER_CATEGORY);
        push_if(world_filter().is_some(), FILTER_WORLD);
        push_if(datacenter_filter().is_some(), FILTER_DATACENTER);
        push_if(quality_filter().is_some(), FILTER_QUALITY);
        push_if(
            name_filter().is_some() || name_chip_pending.get(),
            FILTER_NAME,
        );
        push_if(drift_floor().is_some(), FILTER_MIN_DRIFT);
        push_if(min_confidence().is_some(), FILTER_MIN_CONFIDENCE);
        push_if(min_volume().is_some(), FILTER_MIN_VOLUME);
        active
    });

    // Menu label for a filter. Reuses the labels the old toolbar fields
    // carried, which are longer and more explanatory than the chip labels —
    // the menu is where a filter has to be recognized, not just recalled.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, analyzer_filter_profit_min_label).to_string(),
            FILTER_PROFIT_PER_DAY => {
                t_string!(i18n, analyzer_filter_profit_per_day_min_label).to_string()
            }
            FILTER_ROI => t_string!(i18n, analyzer_filter_roi_min_label).to_string(),
            FILTER_SALES => t_string!(i18n, analyzer_filter_sales_min_label).to_string(),
            FILTER_VELOCITY => t_string!(i18n, analyzer_filter_velocity_min_label).to_string(),
            FILTER_MIN_BUY => t_string!(i18n, analyzer_filter_min_buy_label).to_string(),
            FILTER_MAX_PRICE => t_string!(i18n, analyzer_filter_buy_max_label).to_string(),
            FILTER_NEXT_SALE => t_string!(i18n, analyzer_filter_max_sale_time_label).to_string(),
            FILTER_LAST_SOLD => t_string!(i18n, analyzer_last_sold_within).to_string(),
            FILTER_PRE_TAX => t_string!(i18n, analyzer_pre_tax).to_string(),
            FILTER_SHOW_SUSPICIOUS => t_string!(i18n, analyzer_show_suspicious).to_string(),
            FILTER_QUALITY => t_string!(i18n, analyzer_filter_quality_label).to_string(),
            FILTER_NAME => t_string!(i18n, analyzer_filter_name_label).to_string(),
            FILTER_MIN_DRIFT => t_string!(i18n, analyzer_filter_drift_min_label).to_string(),
            FILTER_MIN_CONFIDENCE => {
                t_string!(i18n, analyzer_filter_confidence_min_label).to_string()
            }
            FILTER_MIN_VOLUME => t_string!(i18n, analyzer_filter_volume_min_label).to_string(),
            _ => String::new(),
        }
    };

    // Adding a filter seeds it with `default_filter_value` so the chip has
    // something to show; the user edits it in place from there.
    let add_filter = move |id: &str| {
        let value = default_filter_value(id);
        match id {
            FILTER_PROFIT => set_minimum_profit(value.parse().ok()),
            FILTER_PROFIT_PER_DAY => set_minimum_profit_per_day(value.parse().ok()),
            FILTER_ROI => set_minimum_roi(value.parse().ok()),
            FILTER_SALES => set_minimum_sales(value.parse().ok()),
            FILTER_VELOCITY => set_min_velocity(value.parse().ok()),
            FILTER_MIN_BUY => set_min_buy_price(value.parse().ok()),
            FILTER_MAX_PRICE => set_max_purchase_price(value.parse().ok()),
            FILTER_NEXT_SALE => set_max_predicted_time(Some(value.to_string())),
            FILTER_LAST_SOLD => set_last_sold_within(Some(value.to_string())),
            FILTER_PRE_TAX => set_tax_enabled(Some(false)),
            FILTER_SHOW_SUSPICIOUS => set_show_suspicious(Some(true)),
            FILTER_QUALITY => set_quality_filter(value.parse().ok()),
            FILTER_NAME => name_chip_pending.set(true),
            FILTER_MIN_DRIFT => set_min_drift(value.parse().ok()),
            FILTER_MIN_CONFIDENCE => set_min_confidence(value.parse().ok()),
            FILTER_MIN_VOLUME => set_min_volume(value.parse().ok()),
            _ => {}
        }
    };

    // --- Horizontal scroll sync ---------------------------------------------
    // Two sibling scrollports: one on the sticky header, one on the row area
    // (the list's own row container, which already computes to
    // `overflow-x: auto`). A single scrollport wrapping the list is not an
    // option — it would become the nearest scrollport for the sticky header
    // and stop it sticking to the viewport — and no scrollport at all leaves
    // the right-hand columns clipped by `html { overflow-x: hidden }` with no
    // way to reach them.
    let header_scroll = NodeRef::<leptos::html::Div>::new();
    let list_scroll = NodeRef::<leptos::html::Div>::new();
    // Client-only: gated out of the SSR build entirely. A `LocalStorage`
    // StoredValue created during SSR is a `SendWrapper` living on one tokio
    // worker thread, but the Suspense rendering this component re-runs (and
    // eventually disposes) across `.await` points, so the `on_cleanup` below
    // can fire on a *different* worker thread — a guaranteed SendWrapper
    // panic that aborts the response stream mid-body and leaves the client
    // hydrating a truncated document (no `__INCOMPLETE_CHUNKS` bootstrap).
    #[cfg(feature = "hydrate")]
    {
        // Parked here rather than `Closure::forget`-ed: a forgotten listener keeps
        // firing after the component is disposed.
        let hscroll_listeners =
            StoredValue::new_local(Vec::<(web_sys::HtmlDivElement, Closure<dyn FnMut()>)>::new());
        on_cleanup(move || {
            hscroll_listeners.update_value(|listeners| {
                for (el, cb) in listeners.drain(..) {
                    let _ = el
                        .remove_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
                }
            });
        });
        Effect::new(move |_| {
            // Re-runs when the refs are populated; the guard keeps a second run
            // from double-registering.
            let (Some(head), Some(body)) = (header_scroll.get(), list_scroll.get()) else {
                return;
            };
            if hscroll_listeners.with_value(|l| !l.is_empty()) {
                return;
            }
            // Mirroring writes `scrollLeft` on the other element, which fires its
            // scroll event in turn; the equality check is what keeps that from
            // ping-ponging.
            let mirror = |from: web_sys::HtmlDivElement, to: web_sys::HtmlDivElement| {
                Closure::wrap(Box::new(move || {
                    let x = from.scroll_left();
                    if to.scroll_left() != x {
                        to.set_scroll_left(x);
                    }
                }) as Box<dyn FnMut()>)
            };
            let head_cb = mirror(head.clone(), body.clone());
            let body_cb = mirror(body.clone(), head.clone());
            let _ =
                head.add_event_listener_with_callback("scroll", head_cb.as_ref().unchecked_ref());
            let _ =
                body.add_event_listener_with_callback("scroll", body_cb.as_ref().unchecked_ref());
            hscroll_listeners.set_value(vec![(head, head_cb), (body, body_cb)]);
        });
    }

    let clear_all_filters = move || {
        set_minimum_profit(None);
        set_minimum_profit_per_day(None);
        set_minimum_roi(None);
        set_max_predicted_time(None);
        set_world_filter(None);
        set_datacenter_filter(None);
        set_minimum_sales(None);
        set_min_velocity(None);
        set_category_filter(None);
        set_max_purchase_price(None);
        set_min_buy_price(None);
        set_last_sold_within(None);
        set_show_suspicious(None);
        set_tax_enabled(None);
        set_quality_filter(None);
        set_name_filter(None);
        name_chip_pending.set(false);
        set_min_drift(None);
        set_min_confidence(None);
        set_min_volume(None);
    };

    // Accumulating CH enrichment (quality + sparkline + settled), grown by the
    // visible-window fetch effect below; never wholesale-replaced (except on a
    // world change). Cells + the suspicious filter read it reactively.
    let enrichment = RwSignal::new(EnrichmentMaps::default());

    let filtered_rows = Memo::new(move |_| {
        let include_tax = tax_enabled().unwrap_or(true);
        // Normalized (trimmed + lowercased) once per recompute — the rows
        // loop below runs 20k+ times, and lowercasing the query per row
        // was an allocation per row. `None` when the filter is off, blank,
        // or the hydration gate is still down.
        let name_query: Option<String> = name_filter().and_then(|raw| {
            // SSR renders SSR_FALLBACK_ROWS rows with *English* item names;
            // the client hydrates localized ones. Localized-name matching
            // therefore must not run until after hydration or an active
            // ?name= produces different row sets and trips the tachys
            // hydration panic. Same Effect-driven gate as item_explorer.rs /
            // job_set_card.rs. Checked only when a name filter is active so
            // an idle page never subscribes this memo to `hydrated`.
            if !hydrated.get() {
                return None;
            }
            normalize_name_query(&raw)
        });
        // See `FilteredRows::rows_lacking_data`. Counted by the combined
        // drift/confidence/volume closure below.
        let mut rows_lacking_data = 0usize;
        let mut sorted_data = profits
            .0
            .iter()
            .map(|data| {
                let estimated = if include_tax {
                    (data.estimated_sale_price as f32 * 0.95) as i32
                } else {
                    data.estimated_sale_price
                };
                let profit = estimated - data.cheapest_price;
                let return_on_investment = return_on_investment(profit, data.cheapest_price);
                let profit_per_day = data
                    .sale_summary
                    .avg_sale_duration
                    .map(|d| {
                        let days = d.num_seconds() as f32 / 86400.0;
                        let days = days.max(1.0);
                        (profit as f32 / days) as i32
                    })
                    .unwrap_or(0);
                CalculatedProfitData {
                    inner: data.clone(),
                    profit,
                    return_on_investment,
                    profit_per_day,
                }
            })
            .filter(move |data| {
                minimum_profit()
                    .map(|min| data.profit > min)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_profit_per_day()
                    .map(|min| data.profit_per_day > min)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_roi()
                    .map(|roi| data.return_on_investment > roi)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_sales()
                    .map(|sales| data.inner.sale_summary.num_sold >= sales)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                // Velocity floor. Mirrors the Velocity column's preference —
                // ClickHouse rate first, derived rate as fallback — so the
                // number shown is the number evaluated. Reading `enrichment`
                // here is the same pattern the suspicious filter below uses;
                // the non-reactive `requested` dedupe breaks the recompute ->
                // refetch loop.
                velocity_floor()
                    .map(|min| {
                        let key = (data.inner.sale_summary.item_id, data.inner.sale_summary.hq);
                        let ch =
                            enrichment.with(|maps| maps.quality_for(&key).map(|q| q.sales_per_day));
                        passes_velocity_floor(min, ch, velocity_per_day(&data.inner.sale_summary))
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                category_filter()
                    .map(|cat_id| {
                        items
                            .get(&ItemId(data.inner.sale_summary.item_id))
                            .map(|item| item.item_search_category == cat_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                quality_filter()
                    .map(|q| passes_quality(q, data.inner.sale_summary.hq))
                    .unwrap_or(true)
            })
            .filter(|data| {
                let Some(query) = name_query.as_deref() else {
                    return true;
                };
                items
                    .get(&ItemId(data.inner.sale_summary.item_id))
                    .map(|item| matches_normalized_name(query, &item.name))
                    .unwrap_or(false)
            })
            .filter(|data| {
                // Drift, confidence and volume floors in one pass: they share
                // the "row may lack data" problem (drift needs >= 4 buffered
                // sales; the other two need CH enrichment, ~7% coverage), so
                // this is where `rows_lacking_data` is counted — and the two
                // enrichment-backed floors share a single map lookup.
                //
                // CH band first, derived fallback — the same preference the
                // Confidence column renders, so the label shown is the label
                // filtered. Reading `enrichment` here follows the velocity
                // filter's pattern; the non-reactive `requested` dedupe is
                // what keeps recompute -> refetch from looping.
                let drift_min = drift_floor();
                let confidence_min = min_confidence();
                let volume_min = min_volume();
                if drift_min.is_none() && confidence_min.is_none() && volume_min.is_none() {
                    return true;
                }
                let mut lacks_data = false;
                let mut pass = true;
                if let Some(min) = drift_min {
                    let drift = price_drift_pct(&data.inner.prices);
                    lacks_data |= drift.is_none();
                    pass &= passes_drift_floor(min, drift);
                }
                if confidence_min.is_some() || volume_min.is_some() {
                    let key = (data.inner.sale_summary.item_id, data.inner.sale_summary.hq);
                    // One lookup serves both floors.
                    let ch = enrichment.with(|maps| {
                        maps.quality_for(&key)
                            .map(|q| (q.confidence_band, q.sample_size))
                    });
                    if let Some(floor) = confidence_min {
                        let band = ch.map(|(band, _)| band);
                        // CH `Unknown` is "no deep scan yet": the floor is
                        // then judged on the derived band, so that row also
                        // counts as lacking real data.
                        lacks_data |= matches!(band, None | Some(ConfidenceBand::Unknown));
                        pass &= passes_confidence_floor(
                            floor,
                            band,
                            derived_confidence(&data.inner.sale_summary),
                        );
                    }
                    if let Some(min) = volume_min {
                        let volume = ch.map(|(_, sample_size)| sample_size);
                        lacks_data |= volume.is_none();
                        pass &= passes_volume_floor(min, volume);
                    }
                }
                if lacks_data {
                    rows_lacking_data += 1;
                }
                pass
            })
            .filter(move |data| {
                max_purchase_price()
                    .map(|max| data.inner.cheapest_price <= max)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                min_buy_price()
                    .map(|min| data.inner.cheapest_price >= min)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                predicted_time()
                    .map(|time| {
                        data.inner
                            .sale_summary
                            .avg_sale_duration
                            .map(|dur| dur.to_std().ok().map(|dur| dur < time).unwrap_or(false))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                last_sold_duration()
                    .map(|max_age| {
                        data.inner
                            .sale_summary
                            .days_since_last_sale
                            .and_then(|d| d.to_std().ok())
                            .map(|d| d <= max_age)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                world_filter_list()
                    .map(|world_filter| world_filter.contains(&data.inner.cheapest_world_id))
                    .unwrap_or(true)
            })
            .filter(move |data| {
                data.inner.cheapest_world_id
                    != lookup_world()
                        .and_then(|w| w.as_world_id())
                        .unwrap_or_default()
            })
            .filter(move |data| {
                // Suspicious filter: hide Unusable + high-launder unless
                // the user explicitly opted in via the show-suspicious
                // toggle. Rows without enrichment (no CH coverage yet, or
                // CH outage) are kept — Pass-1 sales data is still useful.
                if show_suspicious_active() {
                    return true;
                }
                let maps = enrichment.get();
                let key = (data.inner.sale_summary.item_id, data.inner.sale_summary.hq);
                let Some(q) = maps.quality_for(&key) else {
                    return true;
                };
                !(matches!(q.confidence_band, ConfidenceBand::Unusable)
                    || q.launder_suspicion > 0.7)
            })
            .collect::<Vec<_>>();

        sort_rows(
            &mut sorted_data,
            sort_mode().unwrap_or(SortMode::ProfitPerDay),
            sort_dir().unwrap_or_default(),
        );
        FilteredRows {
            rows: sorted_data.into_iter().enumerate().collect(),
            rows_lacking_data,
        }
    });
    // Split views over `filtered_rows`. The clone in `sorted_data` runs once
    // per filter recompute (Memo caches it), not per access, and each element
    // is an Arc bump — the VirtualScroller needs a `Signal<Vec<_>>` and this
    // keeps its `each` wiring unchanged.
    let sorted_data = Memo::new(move |_| filtered_rows.with(|f| f.rows.clone()));
    let rows_lacking_data = Memo::new(move |_| filtered_rows.with(|f| f.rows_lacking_data));

    // --- Visible-window lazy enrichment -------------------------------------
    // Dedupe / loop-breaker: keys we've already scheduled a fetch for. Non-
    // reactive (StoredValue) on purpose — claiming a key must not retrigger the
    // fetch effect.
    let requested = StoredValue::new(std::collections::HashSet::<(i32, bool)>::new());
    // Rendered row range published by the VirtualScroller (see view! below).
    let visible_range = RwSignal::new((0usize, 0usize));
    // Generation counter for debounce-with-cancellation (RwSignal, mirroring
    // components/search_box.rs). `gen` is a reserved keyword in edition 2024.
    let fetch_id = RwSignal::new(0u64);
    let analyzer_market_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let worlds_for_market = worlds.clone();

    Effect::new(move |_| {
        analyzer_market_subscription.update_value(|sub| *sub = None);

        let Some(realtime) = realtime_for_market.clone() else {
            return;
        };
        let Some(sell_world_id) = lookup_world().and_then(|world| world.as_world_id()) else {
            return;
        };
        let buy_filter = buy_filter();
        let range = visible_range.get();
        let mut item_ids = sorted_data.with(|data| {
            let (start, end) = range;
            let lo = start.saturating_sub(PREFETCH_MARGIN);
            let hi = (end + PREFETCH_MARGIN).min(data.len());
            data.get(lo..hi)
                .unwrap_or(&[])
                .iter()
                .map(|(_, data)| data.inner.sale_summary.item_id)
                .collect::<Vec<_>>()
        });
        item_ids.sort_unstable();
        item_ids.dedup();
        if item_ids.is_empty() {
            return;
        }

        let sell_selector = AnySelector::World(sell_world_id);
        let mut world_filter = FilterPredicate::World(sell_selector);
        if let Some(filter) = buy_filter
            && filter != sell_selector
        {
            world_filter = world_filter.or(FilterPredicate::World(filter));
        }
        let filter = world_filter.and(FilterPredicate::Items(item_ids.clone()));
        let worlds = worlds_for_market.clone();
        let subscribed_item_ids = item_ids.clone();
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            if is_analyzer_market_update_relevant(
                &message,
                &subscribed_item_ids,
                sell_world_id,
                buy_filter,
                &worlds,
            ) {
                on_market_update.run(());
            }
        });
        analyzer_market_subscription.set_value(Some(sub));
    });

    on_cleanup(move || {
        analyzer_market_subscription.update_value(|sub| *sub = None);
    });

    // Reset accumulated enrichment when the world changes. Defense-in-depth: if
    // the component is updated in place rather than remounted, another world's
    // data must not leak.
    Effect::new(move |_| {
        let _ = world.get(); // subscribe: re-run on world change
        enrichment.set(EnrichmentMaps::default());
        requested.update_value(|s| s.clear());
        // Invalidate any in-flight fetch from the previous world: bumping the
        // generation makes it bail at the guard below before it claims keys,
        // so a stale batch can't repopulate `requested` (which would strand
        // those rows on the skeleton) or merge another world's data.
        fetch_id.update(|n| *n += 1);
    });

    // Select the visible-window keys (honoring the active sort/filter via
    // sorted_data), debounce, fetch both batches, and merge — accumulating.
    Effect::new(move |_| {
        let range = visible_range.get(); // reactive: scroll
        let keys = sorted_data.with(|data| {
            requested.with_value(|seen| {
                visible_keys(data, range, PREFETCH_MARGIN, seen, |(_, d)| {
                    (d.inner.sale_summary.item_id, d.inner.sale_summary.hq)
                })
            })
        });
        if keys.is_empty() {
            return;
        }
        fetch_id.update(|n| *n += 1);
        let current_id = fetch_id.get_untracked();
        let world_name = world.get_untracked();
        leptos::task::spawn_local(async move {
            TimeoutFuture::new(DEBOUNCE_MS).await; // debounce
            // Past this await the component can be disposed (user navigated away
            // / changed world), which disposes these signals. Every access here
            // uses a `try_*` variant so touching a disposed signal returns
            // quietly instead of panicking (RustWasmPanic / "unreachable").
            if fetch_id.try_get_untracked() != Some(current_id) {
                return; // superseded by a newer range, or component disposed
            }
            // Claim post-debounce so superseded generations never claim.
            if requested
                .try_update_value(|s| s.extend(keys.iter().copied()))
                .is_none()
            {
                return; // component disposed
            }
            // window <= ~86 keys << 200 cap -> single batch, no chunking.
            let (quality, sparklines) = futures::join!(
                get_resale_quality(&world_name, keys.clone(), 30),
                post_sparklines(
                    &world_name,
                    SparklinesRequest {
                        items: keys.clone(),
                        hours: Some(168),
                    },
                ),
            );
            // The join above awaits the network, so the world may have changed
            // (or the component been disposed) while this batch was in flight.
            // Don't merge one world's enrichment into another's map (the
            // world-change reset already cleared `requested`, so the new world
            // refetches these keys). A disposed `world` signal yields None here,
            // which also bails.
            if world.try_get_untracked().as_deref() != Some(world_name.as_str()) {
                return;
            }
            // Merge whatever succeeded and mark every fetched key settled
            // (success OR error) so cells switch loading -> value / "—". On a CH
            // blip the rows degrade to "—" (same as today) — no retry loop; a
            // world change resets everything.
            let _ = enrichment.try_update(|m| {
                if let Ok(q) = &quality {
                    m.quality
                        .extend(q.rows.iter().map(|r| ((r.item_id, r.hq), r.clone())));
                }
                if let Ok(s) = &sparklines {
                    m.sparkline.extend(
                        s.series
                            .iter()
                            .map(|r| ((r.item_id, r.hq), r.points.clone())),
                    );
                }
                m.settled.extend(keys.iter().copied());
            });
        });
    });

    view! {
        <div class="flex flex-col gap-4">
            // Sticky control bar. Fixed at STICKY_BAR_HEIGHT (76px): the table
            // header sticks directly beneath it at that offset, so a bar that
            // grew with its content would cover its own column headers.
            <div
                class="sticky-bar h-[76px] px-2 py-1 flex flex-col gap-1"
                node_ref=sticky_bar_ref
            >
                // Row 1 — result count and view-level controls.
                //
                // The row cannot wrap (the bar is height-locked) and cannot
                // scroll (it holds the popovers, and `html` is `overflow-x:
                // hidden`), so it has to *fit*, at every width and in every
                // locale. It did not: every control is a `.sticky-bar-button`
                // — `flex: 0 0 auto` — so the row could only grow, and at
                // 375px it ran ~210px past the viewport with the last button
                // stranded off-screen (#1055).
                //
                // Three things keep it inside now, in the order they give up
                // space: the count group is `flex-1` and truncates first;
                // labels are hidden below `md` and ellipsize above it
                // (`.sticky-bar-button-shrink`); icons never shrink. A
                // breakpoint alone would not do — the side nav takes 240px at
                // `lg`, so the row is no wider at 1024px than at 768px.
                //
                // Anything added here needs to be able to yield too.
                <div class="h-8 flex items-center gap-2 md:gap-3 min-w-0">
                    // The one item allowed to give up space. `overflow-hidden`
                    // is safe on this wrapper specifically: it holds two spans
                    // and nothing sticky or absolutely positioned, so it does
                    // not become a scrollport for anything that matters.
                    <div class="flex-1 min-w-0 flex items-baseline gap-2 overflow-hidden">
                        <span class="text-sm text-[color:var(--brand-fg)] font-semibold truncate min-w-0">
                            {move || {
                                t_string!(i18n, analyzer_rows_count)
                                    .to_string()
                                    .replace("%count%", &sorted_data.with(|d| d.len()).to_string())
                            }}
                        </span>
                        // Data-transparency note: the drift / confidence / volume
                        // floors each meet rows with no underlying data (drift
                        // needs >= 4 buffered sales; the other two need CH
                        // enrichment) and resolve it differently — drop, judge on
                        // a derived band, pass. Say how many rows that was rather
                        // than letting the row count move for invisible reasons.
                        // Zero (and absent) whenever none of those floors is set.
                        {move || {
                            let n = rows_lacking_data();
                            (n > 0)
                                .then(|| {
                                    view! {
                                        <span class="text-xs text-[color:var(--color-text-muted)] truncate min-w-0">
                                            {t_string!(i18n, analyzer_rows_lacking_data)
                                                .to_string()
                                                .replace("%count%", &n.to_string())}
                                        </span>
                                    }
                                })
                        }}
                    </div>
                    // Live-market indicator, carried over from the realtime work on
                    // main. It sat in the results-summary panel this bar replaced.
                    <RealtimeStatus
                        status=realtime_status
                        last_update=last_update
                        compact=true
                    />
                    <SavedViewsMenu current_world=world />
                    <button
                        class="sticky-bar-button sticky-bar-button-shrink"
                        aria-label=t_string!(i18n, analyzer_columns_button)
                        aria-expanded=move || show_columns_picker.get().to_string()
                        on:click=move |_| {
                            show_filter_menu.set(false);
                            show_columns_picker.update(|v| *v = !*v);
                        }
                    >
                        <Icon icon=i::FaTableColumnsSolid />
                        <span class="hidden md:inline sticky-bar-button-label">
                            {t!(i18n, analyzer_columns_button)}
                        </span>
                    </button>
                    <button
                        class="sticky-bar-button sticky-bar-button-shrink"
                        aria-label=t_string!(i18n, aria_clear_all_filters)
                        on:click=move |_| clear_all_filters()
                    >
                        <Icon icon=icondata::MdiFilterRemove />
                        <span class="hidden md:inline sticky-bar-button-label">
                            {t!(i18n, analyzer_clear_all)}
                        </span>
                    </button>
                </div>

                // Row 2 — the filters themselves. One chip per active filter,
                // and nothing at all for the ones that are not in use.
                <div class="h-8 flex items-center gap-2 min-w-0">
                    <div class="filter-chip-row">
                        {move || {
                            active_filters()
                                .is_empty()
                                .then(|| {
                                    view! {
                                        <span class="text-sm text-[color:var(--color-text-muted)] whitespace-nowrap">
                                            {t!(i18n, analyzer_no_active_filters)}
                                        </span>
                                    }
                                })
                        }}
                        {move || {
                            minimum_profit()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_profit_gte).to_string()
                                            value=Signal::derive(move || minimum_profit().map(|v| v.to_string()))
                                            numeric=true
                                            min="0"
                                            step="1000"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_minimum_profit(
                                                    commit_numeric(minimum_profit.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            minimum_profit_per_day()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_profit_per_day_gte).to_string()
                                            value=Signal::derive(move || {
                                                minimum_profit_per_day().map(|v| v.to_string())
                                            })
                                            numeric=true
                                            min="0"
                                            step="1000"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_minimum_profit_per_day(
                                                    commit_numeric(minimum_profit_per_day.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            minimum_roi()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_roi_gte).to_string()
                                            value=Signal::derive(move || minimum_roi().map(|v| v.to_string()))
                                            numeric=true
                                            min="0"
                                            step="10"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_minimum_roi(commit_numeric(minimum_roi.get_untracked(), v));
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            minimum_sales()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_sales_gte).to_string()
                                            value=Signal::derive(move || minimum_sales().map(|v| v.to_string()))
                                            numeric=true
                                            min="0"
                                            max="6"
                                            step="1"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                // Only 6 sales ship per item, so a larger floor
                                                // silently empties the table.
                                                set_minimum_sales(
                                                    commit_numeric(minimum_sales.get_untracked(), v)
                                                        .map(|s: usize| s.min(6)),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            velocity_floor()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_velocity_gte).to_string()
                                            value=Signal::derive(move || {
                                                velocity_floor().map(format_velocity_floor)
                                            })
                                            numeric=true
                                            min="0"
                                            step="0.5"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_velocity(
                                                    commit_numeric(velocity_floor.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            min_buy_price()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_min_buy_gte).to_string()
                                            value=Signal::derive(move || min_buy_price().map(|v| v.to_string()))
                                            numeric=true
                                            min="0"
                                            step="1000"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_buy_price(
                                                    commit_numeric(min_buy_price.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            max_purchase_price()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_budget_lte).to_string()
                                            value=Signal::derive(move || max_purchase_price().map(|v| v.to_string()))
                                            numeric=true
                                            min="0"
                                            step="1000"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_max_purchase_price(
                                                    commit_numeric(max_purchase_price.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            max_predicted_time()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_next_sale_lte).to_string()
                                            value=Signal::derive(max_predicted_time)
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_max_predicted_time(v);
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            last_sold_within()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_last_sold_lte).to_string()
                                            value=Signal::derive(last_sold_within)
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_last_sold_within(v);
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            category_filter()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_category_label).to_string()
                                            readonly=true
                                            value=Signal::derive(move || {
                                                let cat_id = category_filter()?;
                                                Some(
                                                    tracked_data()
                                                        .item_search_categorys
                                                        .get(&xiv_gen::ItemSearchCategoryId(cat_id))
                                                        .map(|c| c.name.clone())
                                                        .unwrap_or_else(|| cat_id.to_string()),
                                                )
                                            })
                                            on_commit=Callback::new(move |_| set_category_filter(None))
                                        />
                                    }
                                })
                        }}
                        {move || {
                            world_filter()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_world_label).to_string()
                                            readonly=true
                                            value=Signal::derive(world_filter)
                                            on_commit=Callback::new(move |_| set_world_filter(None))
                                        />
                                    }
                                })
                        }}
                        {move || {
                            datacenter_filter()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_datacenter_label).to_string()
                                            readonly=true
                                            value=Signal::derive(datacenter_filter)
                                            on_commit=Callback::new(move |_| set_datacenter_filter(None))
                                        />
                                    }
                                })
                        }}
                        // Post-tax is the default, so only the opt-out is a chip.
                        {move || {
                            (tax_enabled() == Some(false))
                                .then(|| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_pre_tax).to_string()
                                            readonly=true
                                            value=Signal::derive(|| None::<String>)
                                            on_commit=Callback::new(move |_| set_tax_enabled(None))
                                        />
                                    }
                                })
                        }}
                        {move || {
                            show_suspicious_active()
                                .then(|| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_show_suspicious).to_string()
                                            readonly=true
                                            value=Signal::derive(|| None::<String>)
                                            on_commit=Callback::new(move |_| set_show_suspicious(None))
                                        />
                                    }
                                })
                        }}
                        {move || {
                            quality_filter()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_quality_label).to_string()
                                            value=Signal::derive(move || {
                                                quality_filter().map(|q| q.to_string())
                                            })
                                            options=vec![
                                                ("hq", t_string!(i18n, analyzer_col_hq).to_string()),
                                                ("nq", t_string!(i18n, analyzer_quality_nq).to_string()),
                                            ]
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_quality_filter(v.and_then(|s| s.parse().ok()));
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            (name_filter().is_some() || name_chip_pending.get())
                                .then(|| {
                                    // Fresh from the menu (no committed value yet) the
                                    // chip mounts editing so the user can type at once.
                                    let start_editing = name_filter().is_none();
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_name_contains).to_string()
                                            value=Signal::derive(name_filter)
                                            start_editing=start_editing
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_name_filter(v);
                                                name_chip_pending.set(false);
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            drift_floor()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_drift_gte).to_string()
                                            value=Signal::derive(move || {
                                                drift_floor().map(format_velocity_floor)
                                            })
                                            numeric=true
                                            step="1"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_drift(
                                                    commit_numeric(drift_floor.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            min_confidence()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_confidence_gte).to_string()
                                            value=Signal::derive(move || {
                                                min_confidence().map(|c| c.to_string())
                                            })
                                            options=vec![
                                                ("low", t_string!(i18n, analyzer_confidence_low).to_string()),
                                                ("medium", t_string!(i18n, analyzer_confidence_medium).to_string()),
                                                ("high", t_string!(i18n, analyzer_confidence_high).to_string()),
                                            ]
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_confidence(v.and_then(|s| s.parse().ok()));
                                            })
                                        />
                                    }
                                })
                        }}
                        {move || {
                            min_volume()
                                .map(|_| {
                                    view! {
                                        <FilterChip
                                            label=t_string!(i18n, analyzer_volume_gte).to_string()
                                            value=Signal::derive(move || {
                                                min_volume().map(|v| v.to_string())
                                            })
                                            numeric=true
                                            min="0"
                                            step="10"
                                            on_commit=Callback::new(move |v: Option<String>| {
                                                set_min_volume(
                                                    commit_numeric(min_volume.get_untracked(), v),
                                                );
                                            })
                                        />
                                    }
                                })
                        }}
                    </div>
                    <button
                        class="sticky-bar-button"
                        aria-expanded=move || show_filter_menu.get().to_string()
                        on:click=move |_| {
                            show_columns_picker.set(false);
                            show_filter_menu.update(|v| *v = !*v);
                        }
                    >
                        <Icon icon=i::FaFilterSolid />
                        {t!(i18n, analyzer_add_filter)}
                    </button>
                </div>

                // `+ Filter` menu. Unset filters live here, so the bar's height
                // tracks the filters in use rather than the filters that exist.
                {move || {
                    show_filter_menu
                        .get()
                        .then(|| {
                            view! {
                                <div class="sticky-bar-popover p-3 w-[min(92vw,20rem)] flex flex-col gap-2 text-sm">
                                    {move || {
                                        available_filters(&active_filters())
                                            .into_iter()
                                            .map(|id| {
                                                let label = filter_label(id);
                                                view! {
                                                    <button
                                                        class="text-left px-2 py-1 rounded-sm text-[color:var(--color-text)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]"
                                                        on:click=move |_| {
                                                            add_filter(id);
                                                            show_filter_menu.set(false);
                                                        }
                                                    >
                                                        {label}
                                                    </button>
                                                }
                                            })
                                            .collect_view()
                                    }}
                                    // Category is chosen from a list rather than
                                    // typed, so its chip is read-only and this is
                                    // where it is picked. Hidden once a category
                                    // is set: leaving it up would echo the chip,
                                    // which is the duplication this bar deletes.
                                    {move || category_filter().is_none().then(|| view! {
                                    <label class="flex flex-col gap-1 pt-1 border-t border-[color:var(--color-outline)]">
                                        <span class="text-[color:var(--color-text-muted)]">
                                            {t!(i18n, analyzer_filter_category_label)}
                                        </span>
                                        <select
                                            class="input input-sm"
                                            on:change=move |ev| {
                                                let val = event_target_value(&ev);
                                                if let Ok(id) = val.parse::<i32>() {
                                                    set_category_filter(Some(id));
                                                } else {
                                                    set_category_filter(None);
                                                }
                                                show_filter_menu.set(false);
                                            }
                                            prop:value=move || {
                                                category_filter().map(|c| c.to_string()).unwrap_or_default()
                                            }
                                        >
                                            <option value="">{t!(i18n, analyzer_all_categories)}</option>
                                            {
                                                let mut categories = tracked_data()
                                                    .item_search_categorys
                                                    .iter()
                                                    .filter(|(_, cat)| !cat.name.is_empty())
                                                    .map(|(id, cat)| (id.0, cat.name.clone()))
                                                    .collect::<Vec<_>>();
                                                categories.sort_by(|a, b| a.1.cmp(&b.1));
                                                categories
                                                    .into_iter()
                                                    .map(|(id, name)| {
                                                        view! {
                                                            <option
                                                                value=id.to_string()
                                                                selected=move || category_filter() == Some(id)
                                                            >
                                                                {name}
                                                            </option>
                                                        }
                                                    })
                                                    .collect_view()
                                            }
                                        </select>
                                    </label>
                                    })}
                                </div>
                            }
                        })
                }}

                // Columns picker (URL-persisted via ?cols=). A popover rather
                // than a panel so opening it cannot change the bar's height.
                {move || {
                    show_columns_picker
                        .get()
                        .then(|| {
                            let make_toggle = move |col: &'static str| {
                                move |_| {
                                    let mut set = visible_cols.get_untracked();
                                    if set.contains(col) {
                                        set.remove(col);
                                    } else {
                                        set.insert(col);
                                    }
                                    set_cols_param.set(Some(serialize_visible_cols(&set)));
                                }
                            };
                            let col_label = move |col: &'static str| -> String {
                                match col {
                                    c if c == COL_PROFIT_PER_DAY => {
                                        t_string!(i18n, analyzer_col_profit_per_day).to_string()
                                    }
                                    c if c == COL_VELOCITY => t_string!(i18n, analyzer_col_velocity).to_string(),
                                    c if c == COL_DRIFT => t_string!(i18n, analyzer_col_drift).to_string(),
                                    c if c == COL_CONFIDENCE => {
                                        t_string!(i18n, analyzer_col_confidence).to_string()
                                    }
                                    c if c == COL_ROI => t_string!(i18n, analyzer_col_roi).to_string(),
                                    c if c == COL_WORLD => t_string!(i18n, analyzer_col_world).to_string(),
                                    c if c == COL_DATACENTER => {
                                        t_string!(i18n, analyzer_col_datacenter).to_string()
                                    }
                                    c if c == COL_TREND => t_string!(i18n, analyzer_col_spark).to_string(),
                                    c if c == COL_SALES_PER_DAY => {
                                        t_string!(i18n, analyzer_col_sales_per_day).to_string()
                                    }
                                    c if c == COL_VOLUME_30D => {
                                        t_string!(i18n, analyzer_col_volume_30d).to_string()
                                    }
                                    c if c == COL_LAST_SOLD => {
                                        t_string!(i18n, analyzer_col_last_sold).to_string()
                                    }
                                    _ => String::new(),
                                }
                            };
                            view! {
                                <div class="sticky-bar-popover p-3 w-[min(92vw,32rem)] flex flex-row flex-wrap items-center gap-x-5 gap-y-2 text-sm">
                                    <span class="font-semibold text-[color:var(--brand-fg)]">
                                        {t!(i18n, analyzer_columns_picker_label)}
                                    </span>
                                    {ALL_OPTIONAL_COLS
                                        .iter()
                                        .map(|col| {
                                            let col = *col;
                                            let label = col_label(col);
                                            let on_change = make_toggle(col);
                                            view! {
                                                <label class="inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]">
                                                    <input
                                                        type="checkbox"
                                                        class="accent-brand-300"
                                                        prop:checked=move || visible_cols().contains(col)
                                                        on:change=on_change
                                                    />
                                                    <span>{label}</span>
                                                    {col_hidden_note_class(col)
                                                        .map(|hide_at| view! {
                                                            <span class=format!(
                                                                "text-xs text-[color:var(--color-text-muted)] {hide_at}",
                                                            )>
                                                                {t!(i18n, analyzer_columns_picker_desktop_only)}
                                                            </span>
                                                        })}
                                                </label>
                                            }
                                        })
                                        .collect_view()}
                                    <button
                                        class="ml-auto text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                        on:click=move |_| set_cols_param.set(None)
                                    >
                                        {t!(i18n, analyzer_columns_picker_reset)}
                                    </button>

                                    // Cross-region + outlier-filtering, formerly the controls
                                    // panel above the table. `w-full` forces its own row inside
                                    // the wrapping flex container above.
                                    <div class="w-full flex flex-col gap-2 pt-2 mt-1 border-t border-[color:var(--color-outline)]">
                                        <Toggle
                                            checked=Signal::derive(move || cross_region_enabled)
                                            set_checked=SignalSetter::map(move |val: bool| set_cross_region_enabled(
                                                val.then_some(true),
                                            ))
                                            checked_label=Oco::Owned(t_string!(i18n, analyzer_cross_region_enabled).to_string())
                                            unchecked_label=Oco::Owned(t_string!(i18n, analyzer_cross_region_disabled).to_string())
                                        />
                                        <Toggle
                                            checked=Signal::derive(move || filter_outliers)
                                            set_checked=SignalSetter::map(move |val: bool| set_filter_outliers(
                                                val.then_some(true),
                                            ))
                                            checked_label=Oco::Owned(t_string!(i18n, analyzer_filter_outliers_enabled).to_string())
                                            unchecked_label=Oco::Owned(t_string!(i18n, analyzer_filter_outliers_disabled).to_string())
                                        />
                                        <div
                                            class="flex flex-wrap gap-2"
                                            class:hidden=move || !cross_region_enabled
                                        >
                                            {
                                                let region = region.clone();
                                                move || {
                                                    let region = region.clone();
                                                    region
                                                        .map(|region| {
                                                            CONNECTED_REGIONS
                                                                .iter()
                                                                .filter(move |r| **r != region.as_str())
                                                                .map(|region_name| {
                                                                    let (enabled, set_enabled) = query_signal::<
                                                                        bool,
                                                                    >(region_name.to_string());
                                                                    view! {
                                                                        <Toggle
                                                                            checked=Signal::derive(move || enabled().unwrap_or(true))
                                                                            set_checked=SignalSetter::map(move |checked: bool| {
                                                                                set_enabled(Some(checked));
                                                                            })
                                                                            checked_label=t_string!(i18n, analyzer_region_enabled).to_string().replace("%region%", region_name)
                                                                            unchecked_label=t_string!(i18n, analyzer_region_disabled).to_string().replace("%region%", region_name)
                                                                        />
                                                                    }
                                                                })
                                                                .collect::<Vec<_>>()
                                                        })
                                                }
                                            }
                                        </div>
                                    </div>
                                </div>
                            }
                        })
                }}
            </div>

            // Results table. Deliberately no `overflow` on this wrapper: in
            // window mode an overflow on any ancestor of the sticky table
            // header re-parents its scrollport away from the viewport, which
            // silently defeats `sticky_offset`.
            <div
                class="analyzer-table border border-[color:var(--color-outline)]"
                style=move || {
                    let widths = extra_column_widths_px(&visible_cols());
                    format!(
                        "--analyzer-extra-cols-base: {}px; --analyzer-extra-cols-md: {}px; --analyzer-extra-cols-xl: {}px;",
                        widths.base,
                        widths.md,
                        widths.xl,
                    )
                }
            >
                <VirtualScroller
                        scroll_source=ScrollSource::Window { sticky_offset: STICKY_BAR_HEIGHT }
                        viewport_height=720.0
                        row_height=40.0
                        overscan=8
                        // The header row's own height. The rendered element is
                        // up to ~15px taller, because `.analyzer-hscroll`
                        // reserves a horizontal scrollbar, but that height
                        // depends on the platform's scrollbar and on whether
                        // the grid currently overflows — neither of which is
                        // knowable here. The row math only uses this to offset
                        // the scroll position, and `overscan=8` (320px) covers
                        // the error many times over, so the content height is
                        // deliberately the value passed.
                        header_height=56.0
                        variable_height=false
                        visible_range=visible_range
                        list_ref=list_scroll
                        row_min_width="var(--analyzer-row-min-width, 0px)"
                        header=view! {
                            <div class="analyzer-hscroll" node_ref=header_scroll>
                            <div class="analyzer-grid-row flex flex-row items-center h-14 text-xs font-semibold uppercase tracking-wider text-[color:var(--color-text-muted)] border-b border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]" role="rowgroup">
                                <div role="columnheader" class="w-[44px] shrink-0 px-2 text-center">
                                    {t!(i18n, analyzer_col_hq)}
                                </div>
                                <div role="columnheader" class="flex-1 min-w-[14rem] px-3">
                                    {t!(i18n, analyzer_col_item)}
                                </div>
                                <div role="columnheader" class="w-28 shrink-0 px-3 text-right">
                                    <SortHeader
                                        mode=SortMode::Profit
                                        label=t_string!(i18n, analyzer_col_profit).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                </div>
                                {move || visible_cols().contains(COL_PROFIT_PER_DAY).then(|| view! {
                                    <div role="columnheader" class="w-28 shrink-0 px-3 py-2" title=t_string!(i18n, analyzer_tooltip_profit_per_day)>
                                        <SortHeader
                                            mode=SortMode::ProfitPerDay
                                            label=t_string!(i18n, analyzer_col_profit_per_day).to_string()
                                            sort_mode
                                            sort_dir
                                        />
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_VELOCITY).then(|| view! {
                                    <div role="columnheader" class="w-[88px] shrink-0 px-3 py-2 hidden md:flex items-center justify-end" title=t_string!(i18n, analyzer_tooltip_velocity)>
                                        {t!(i18n, analyzer_col_velocity)}
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_DRIFT).then(|| view! {
                                    <div role="columnheader" class="w-[88px] shrink-0 px-3 py-2 hidden md:flex items-center justify-end" title=t_string!(i18n, analyzer_tooltip_drift)>
                                        {t!(i18n, analyzer_col_drift)}
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_CONFIDENCE).then(|| view! {
                                    <div role="columnheader" class="w-[72px] shrink-0 px-3 py-2 hidden md:flex items-center justify-center" title=t_string!(i18n, analyzer_tooltip_confidence)>
                                        {t!(i18n, analyzer_col_confidence)}
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_ROI).then(|| view! {
                                    <div role="columnheader" class="w-28 shrink-0 px-3 py-2">
                                        <SortHeader
                                            mode=SortMode::Roi
                                            label=t_string!(i18n, analyzer_col_roi).to_string()
                                            sort_mode
                                            sort_dir
                                        />
                                    </div>
                                })}
                                <div role="columnheader" class="w-28 shrink-0 px-3 py-2">
                                    {t!(i18n, analyzer_col_buy_price)}
                                </div>
                                {move || visible_cols().contains(COL_WORLD).then(|| view! {
                                    <div role="columnheader" class="w-28 shrink-0 px-3 py-2 flex flex-row gap-2 hidden lg:flex">
                                        {t!(i18n, analyzer_col_world)}
                                        <div>
                                            {move || {
                                                world_filter()
                                                    .map(|_filter| {
                                                        view! {
                                                            <div
                                                                class="hover:text-brand-200 transition-colors rounded-sm p-2 text-brand-300 cursor-pointer"
                                                                on:click=move |_| {
                                                                    set_world_filter(None);
                                                                }
                                                            >
                                                                <Icon icon=icondata::MdiFilterRemove />
                                                            </div>
                                                        }
                                                    })
                                            }}
                                        </div>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_DATACENTER).then(|| view! {
                                    <div role="columnheader" class="w-28 shrink-0 px-3 py-2 flex flex-row gap-2 hidden xl:flex">
                                        {t!(i18n, analyzer_col_datacenter)}
                                        <div>
                                            {move || {
                                                datacenter_filter()
                                                    .map(|_filter| {
                                                        view! {
                                                            <div
                                                                class="hover:text-brand-200 transition-colors rounded-sm p-2 text-brand-300 cursor-pointer"
                                                                on:click=move |_| {
                                                                    set_datacenter_filter(None);
                                                                }
                                                            >
                                                                <Icon icon=icondata::MdiFilterRemove />
                                                            </div>
                                                        }
                                                    })
                                            }}
                                        </div>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_TREND).then(|| view! {
                                    <div role="columnheader" class="w-[100px] shrink-0 px-3 py-2 hidden md:flex flex-col items-center text-center leading-tight" title=t_string!(i18n, analyzer_tooltip_trend)>
                                        <span>{t!(i18n, analyzer_col_spark)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_SALES_PER_DAY).then(|| view! {
                                    <div role="columnheader" class="w-[140px] shrink-0 px-3 py-2 hidden md:flex flex-col items-center text-center leading-tight" title=t_string!(i18n, analyzer_tooltip_sales_per_day)>

                                        <span>{t!(i18n, analyzer_col_sales_per_day)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_VOLUME_30D).then(|| view! {
                                    <div role="columnheader" class="w-[88px] shrink-0 px-3 py-2 hidden md:flex flex-col items-end text-right leading-tight" title=t_string!(i18n, analyzer_tooltip_volume_30d)>
                                        <span>{t!(i18n, analyzer_col_volume_30d)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_LAST_SOLD).then(|| view! {
                                    <div role="columnheader" class="w-28 shrink-0 px-3 py-2 hidden md:flex flex-col leading-tight">
                                        <span>{t!(i18n, analyzer_col_last_sold)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
                                })}
                            </div>
                            </div>
                        }.into_any()
                        each=sorted_data.into()
                        key=move |(index, data): &(usize, CalculatedProfitData)| (
                            *index,
                            data.inner.sale_summary.item_id,
                            data.inner.cheapest_world_id,
                            data.inner.sale_summary.hq,
                            data.profit,
                        )
                        view=move |(index, data): (usize, CalculatedProfitData)| {
                            // Hoist the Copy scalars out so each per-column `move ||`
                            // closure can capture them without contending for
                            // `data.inner` (an Arc, and not Copy). `row_key` is bound
                            // below alongside `item_id`/`hq`.
                            let row_cheapest_price = data.inner.cheapest_price;
                            let row_days_since = data.inner.sale_summary.days_since_last_sale;
                            let row_roi = data.return_on_investment;
                            let row_velocity = velocity_per_day(&data.inner.sale_summary);
                            let row_num_sold = data.inner.sale_summary.num_sold;
                            let row_drift = price_drift_pct(&data.inner.prices);
                            let row_confidence = derived_confidence(&data.inner.sale_summary);
                            let world = worlds
                                .lookup_selector(AnySelector::World(data.inner.cheapest_world_id));
                            let datacenter = world
                                .as_ref()
                                .and_then(|world| {
                                    let datacenters = worlds.get_datacenters(world);
                                    datacenters.first().map(|dc| dc.name.as_str())
                                })
                                .unwrap_or_default()
                                .to_string();
                            let datacenter = Signal::derive(move || datacenter.clone());
                            let world = world
                                .as_ref()
                                .map(|r| r.get_name())
                                .unwrap_or_default()
                                .to_string();
                            let world = Signal::derive(move || world.clone());
                            let item_id = data.inner.sale_summary.item_id;
                            let hq = data.inner.sale_summary.hq;
                            let row_key = (item_id, hq);
                            let item = items
                                .get(&ItemId(item_id))
                                .map(|item| item.name.as_str())
                                .unwrap_or_default();
                            let icon_loading = if index < 20 { "eager" } else { "" };
                            let classes = if (index % 2) == 0 {
                                "analyzer-grid-row flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                            } else {
                                "analyzer-grid-row flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                            };
                            view! {
                                <div class=classes role="row-group">
                                    <div role="cell" class="px-2 py-2 w-[44px] shrink-0 flex items-center justify-center">
                                        {if data.inner.sale_summary.hq {
                                            Some(view! { <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">{t!(i18n, analyzer_col_hq)}</span> })
                                        } else {
                                            None
                                        }}
                                    </div>
                                    <div role="cell" class="px-4 py-2 flex flex-row flex-1 min-w-[14rem] items-center gap-2">
                                        <a
                                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip min-w-0"
                                            href=format!("/item/{}/{item_id}", world())
                                        >
                                            <div class="shrink-0">
                                                <ItemIcon item_id icon_size=IconSize::Small loading=icon_loading />
                                            </div>
                                            {item}
                                            {move || {
                                                let maps = enrichment.get();
                                                maps.quality_for(&row_key).map(|q| {
                                                    view! {
                                                        <ConfidenceBadge
                                                            band=q.confidence_band
                                                            sample_size=q.sample_size
                                                        />
                                                    }
                                                })
                                            }}
                                        </a>
                                        <Clipboard clipboard_text=item.to_string() />
                                        <AddToList item_id />
                                    </div>
                                    <div role="cell" class="px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end">
                                        <Gil amount=data.profit />
                                    </div>
                                    {move || visible_cols().contains(COL_PROFIT_PER_DAY).then(|| view! {
                                        <div role="cell" class="px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end">
                                            <Gil amount=data.profit_per_day />
                                        </div>
                                    })}
                                    {move || visible_cols().contains(COL_VELOCITY).then(|| {
                                        // Prefer the ClickHouse 30d rate where the rollup
                                        // covers the row; otherwise the derived rate off the
                                        // 6-sale buffer, which every row has.
                                        let maps = enrichment.get();
                                        let v = maps
                                            .quality_for(&row_key)
                                            .map(|q| q.sales_per_day)
                                            .or(row_velocity);
                                        let text = match v {
                                            Some(v) => t_string!(i18n, analyzer_velocity_per_day)
                                                .to_string()
                                                .replace("%count%", &format!("{v:.1}")),
                                            None => "—".to_string(),
                                        };
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-[88px] shrink-0 hidden md:flex items-center justify-end font-mono tabular-nums">
                                                {text}
                                            </div>
                                        }
                                    })}
                                    {move || visible_cols().contains(COL_DRIFT).then(|| {
                                        // +/- 1% is inside the noise floor of a 6-sale window,
                                        // so it renders neutral rather than green/red.
                                        let (text, class, title) = match row_drift {
                                            Some(d) if d > 1.0 => (format!("+{d:.0}%"), "text-emerald-300", None),
                                            Some(d) if d < -1.0 => (format!("{d:.0}%"), "text-red-300", None),
                                            Some(d) => (format!("{d:+.0}%"), "text-[color:var(--color-text-muted)]", None),
                                            None => (
                                                "—".to_string(),
                                                "text-[color:var(--color-text-muted)]",
                                                Some(t_string!(i18n, analyzer_drift_unavailable).to_string()),
                                            ),
                                        };
                                        view! {
                                            <div
                                                role="cell"
                                                title=title
                                                class=format!("px-3 py-2 w-[88px] shrink-0 hidden md:flex items-center justify-end font-mono tabular-nums {class}")
                                            >
                                                {text}
                                            </div>
                                        }
                                    })}
                                    {move || visible_cols().contains(COL_CONFIDENCE).then(|| {
                                        // ClickHouse band where it exists, else the band derived
                                        // from buffer depth + velocity.
                                        let maps = enrichment.get();
                                        let (label, class) = match maps.quality_for(&row_key).map(|q| q.confidence_band) {
                                            Some(ConfidenceBand::High) => (t_string!(i18n, analyzer_confidence_high).to_string(), "text-emerald-300"),
                                            Some(ConfidenceBand::Medium) => (t_string!(i18n, analyzer_confidence_medium).to_string(), "text-amber-300"),
                                            Some(ConfidenceBand::Low) | Some(ConfidenceBand::Unusable) => (t_string!(i18n, analyzer_confidence_low).to_string(), "text-red-300"),
                                            Some(ConfidenceBand::Unknown) | None => match row_confidence {
                                                DerivedConfidence::High => (t_string!(i18n, analyzer_confidence_high).to_string(), "text-emerald-300"),
                                                DerivedConfidence::Medium => (t_string!(i18n, analyzer_confidence_medium).to_string(), "text-amber-300"),
                                                DerivedConfidence::Low => (t_string!(i18n, analyzer_confidence_low).to_string(), "text-red-300"),
                                            },
                                        };
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-[72px] shrink-0 hidden md:flex items-center justify-center">
                                                <span class=format!("text-xs font-semibold {class}")>{label}</span>
                                            </div>
                                        }
                                    })}
                                    {move || visible_cols().contains(COL_ROI).then(|| view! {
                                        <div role="cell" class="px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end">
                                            <span class=roi_badge_class(row_roi)>
                                                {format!("{row_roi}%")}
                                            </span>
                                        </div>
                                    })}
                                    <div role="cell" class="px-3 py-2 w-28 shrink-0 text-right flex items-center justify-end">
                                        <Gil amount=data.inner.cheapest_price />
                                    </div>
                                    {move || visible_cols().contains(COL_WORLD).then(|| view! {
                                        <div role="cell" class="px-3 py-2 w-28 shrink-0 hidden lg:block flex items-center">
                                            <Tooltip tooltip_text=Signal::derive(move || {
                                                t_string!(i18n, analyzer_only_show_world).to_string().replace("%world%", &world())
                                            })>
                                                <QueryButton
                                                    key="world"
                                                    value=world
                                                    class="!text-brand-300 hover:text-brand-200"
                                                    active_classes="!text-neutral-300 hover:text-neutral-200"
                                                    remove_queries=&["datacenter"]
                                                >
                                                    {world}
                                                </QueryButton>
                                            </Tooltip>
                                        </div>
                                    })}
                                    {move || visible_cols().contains(COL_DATACENTER).then(|| view! {
                                        <div role="cell" class="px-3 py-2 w-28 shrink-0 hidden xl:block flex items-center">
                                            <Tooltip tooltip_text=Signal::derive(move || {
                                                t_string!(i18n, analyzer_only_show_world).to_string().replace("%world%", &datacenter())
                                            })>
                                                <QueryButton
                                                    key="datacenter"
                                                    value=datacenter
                                                    class="!text-brand-300 hover:text-brand-200"
                                                    active_classes="!text-neutral-300 hover:text-neutral-200"
                                                    remove_queries=&["world"]
                                                >
                                                    {datacenter}
                                                </QueryButton>
                                            </Tooltip>
                                        </div>
                                    })}
                                    {move || visible_cols().contains(COL_TREND).then(|| {
                                        let maps = enrichment.get();
                                        let inner = if let Some(pts) = maps.sparkline_for(&row_key) {
                                            let pct = maps.quality_for(&row_key)
                                                .map(|q| {
                                                    let vwap = q.vwap as f32;
                                                    if vwap <= 0.0 {
                                                        0.0
                                                    } else {
                                                        (row_cheapest_price as f32 - vwap) / vwap * 100.0
                                                    }
                                                })
                                                .unwrap_or(0.0);
                                            view! { <Sparkline points=pts.clone() pct_change=pct /> }.into_any()
                                        } else if maps.is_settled(&row_key) {
                                            // fetched, no series -> empty sparkline (prior behavior)
                                            view! { <Sparkline points=Vec::new() pct_change=0.0 /> }.into_any()
                                        } else {
                                            view! { <SingleLineSkeleton /> }.into_any()
                                        };
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-[100px] hidden md:flex items-center justify-center">
                                                {inner}
                                            </div>
                                        }
                                    })}
                                    {move || visible_cols().contains(COL_SALES_PER_DAY).then(|| {
                                        // Cadence badge carried over from main. Where the
                                        // rollup has no row this falls back to the same
                                        // derived rate the Velocity column uses, so the two
                                        // columns never contradict each other.
                                        let maps = enrichment.get();
                                        let inner = match (maps.quality_for(&row_key), maps.is_settled(&row_key)) {
                                            (Some(q), _) => {
                                                let cadence = get_sales_cadence(q.sales_per_day, q.sample_size as usize);
                                                view! { <SalesCadenceBadge cadence sales_per_day=q.sales_per_day compact=true /> }.into_any()
                                            }
                                            (None, true) => match row_velocity {
                                                Some(spd) => {
                                                    let cadence = get_sales_cadence(spd, row_num_sold);
                                                    view! { <SalesCadenceBadge cadence sales_per_day=spd compact=true /> }.into_any()
                                                }
                                                None => view! { "—" }.into_any(),
                                            },
                                            (None, false) => view! { <SingleLineSkeleton /> }.into_any(),
                                        };
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-[140px] shrink-0 hidden md:flex items-center justify-center">
                                                {inner}
                                            </div>
                                        }
                                    })}
                                    {move || visible_cols().contains(COL_VOLUME_30D).then(|| {
                                        let maps = enrichment.get();
                                        let inner = match (maps.quality_for(&row_key), maps.is_settled(&row_key)) {
                                            (Some(q), _) => view! { {q.sample_size.to_string()} }.into_any(),
                                            (None, true) => view! { "—" }.into_any(),
                                            (None, false) => view! { <SingleLineSkeleton /> }.into_any(),
                                        };
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums">
                                                {inner}
                                            </div>
                                        }
                                    })}
                                    {move || visible_cols().contains(COL_LAST_SOLD).then(|| {
                                        let last = row_days_since
                                            .and_then(|d| d.to_std().ok())
                                            .map(|d| {
                                                let secs = d.as_secs();
                                                let days = secs / 86_400;
                                                let hours = (secs % 86_400) / 3_600;
                                                if days > 0 {
                                                    t_string!(i18n, analyzer_last_sold_days_ago)
                                                        .replace("%count%", &days.to_string())
                                                } else if hours > 0 {
                                                    t_string!(i18n, analyzer_last_sold_hours_ago)
                                                        .replace("%count%", &hours.to_string())
                                                } else {
                                                    t_string!(i18n, analyzer_last_sold_just_now).to_string()
                                                }
                                            })
                                            .unwrap_or_else(|| t_string!(i18n, analyzer_last_sold_never).to_string());
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-28 truncate hidden md:block flex items-center">
                                                {last}
                                            </div>
                                        }
                                    })}
                                </div>
                            }
                                .into_any()
                        }
                    />
            </div>

            // Empty state. A *sibling* of the table container, never a
            // replacement for it: the VirtualScroller (and the `list_scroll`
            // node the horizontal scroll-sync effect above registered its
            // listeners on) must stay mounted, or the listeners die with the
            // node and never re-register. With zero rows the table renders
            // just its header, which doubles as context for this panel.
            //
            // `sorted_data` is computed synchronously from props that only
            // exist once the route's resources resolved (AnalyzerTable mounts
            // inside `<Suspense>`), so an empty list here really means "every
            // row was filtered out" — there is no pending state to flash
            // through. Both SSR and CSR filter the same serialized data, so
            // the two sides agree on emptiness at hydration time.
            {move || {
                sorted_data.with(|data| data.is_empty()).then(|| view! {
                    <ActionableEmptyState
                        title=t_string!(i18n, analyzer_empty_title).to_string()
                        body=t_string!(i18n, analyzer_empty_all_filtered).to_string()
                        action_label=t_string!(i18n, analyzer_clear_all).to_string()
                        on_action=Callback::new(move |_: ()| clear_all_filters())
                    />
                })
            }}
        </div>
    }.into_any()
}

#[component]
pub fn AnalyzerWorldView() -> impl IntoView {
    let i18n = use_i18n();
    // Seeded here rather than in AnalyzerTable: that lives inside the Suspense
    // closure and remounts on every market refetch, which would keep undoing a
    // filter the user had cleared.
    //
    // A bare URL is a first visit with nothing to honor, so it gets a whole
    // view — the user's saved default, or "Realistic flips". Anything else is
    // a filter the visitor chose (a link, a preset, a back-navigation), and
    // only the single `next-sale` param is filled in. The two are exclusive:
    // the view already filters on recency, and adding `next-sale` on top would
    // narrow a view the user picked verbatim.
    if !seed_flip_finder_default_view() {
        seed_query_default("next-sale", DEFAULT_MAX_SALE_TIME.to_string());
    }
    // Owned here for the same reason as the seed above — AnalyzerTable
    // remounts on every realtime market tick, and this state must survive
    // those remounts. `name_chip_pending` keeps a not-yet-committed name
    // chip alive while the user is still typing into it; `hydrated` is the
    // one-shot hydration gate for localized-name matching (see the name
    // filter inside AnalyzerTable), which must not flip back to false and
    // re-render an unfiltered pass on every tick.
    let name_chip_pending = RwSignal::new(false);
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });
    let params = use_params_map();
    let world = Signal::derive(move || params.with(|p| p.get("world").clone()).unwrap_or_default());
    let (market_refresh_version, set_market_refresh_version) = signal(0_u64);
    let sales = ArcResource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            get_recent_sales_for_world(&world.ok_or(AppError::ParamMissing)?).await
        },
    );

    let world_cheapest_listings = ArcResource::new(
        move || {
            (
                params.with(|p| p.get("world").clone()),
                market_refresh_version.get(),
            )
        },
        move |(world, refresh_version)| async move {
            let world = world.ok_or(AppError::ParamMissing)?;
            get_cheapest_listings_live(&world, refresh_version).await
        },
    );

    let region = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        let world = params.with(|p| p.get("world").clone());
        let world = world.ok_or(AppError::ParamMissing)?;
        let region = worlds
            .lookup_world_by_name(&world)
            .map(|world| {
                let region = worlds.get_region(world);
                AnyResult::Region(region).get_name().to_string()
            })
            .ok_or(AppError::ParamMissing)?;
        Result::<_, AppError>::Ok(region)
    });

    let global_cheapest_listings = ArcResource::new(
        move || (region(), market_refresh_version.get()),
        move |(region, refresh_version)| async move {
            get_cheapest_listings_live(region?.as_str(), refresh_version).await
        },
    );

    let (cross_region_enabled, set_cross_region_enabled) = query_signal::<bool>("cross");
    let (filter_outliers, set_filter_outliers) = query_signal::<bool>("filter-outliers");
    let connected_regions = CONNECTED_REGIONS;
    let query = use_query_map();

    let enabled_regions = move || {
        let map = query();
        connected_regions
            .iter()
            .filter(|region| map.get(region).map(|value| value == "true").unwrap_or(true))
            .collect::<Vec<_>>()
    };

    let cross_region = ArcResource::new(
        move || {
            (
                cross_region_enabled(),
                region(),
                enabled_regions(),
                market_refresh_version.get(),
            )
        },
        move |(enabled, region, enabled_regions, refresh_version)| async move {
            let region = region?;
            if enabled.unwrap_or_default() && connected_regions.contains(&region.as_str()) {
                Ok(futures::future::join_all(
                    connected_regions
                        .iter()
                        .filter(|r| **r != region.as_str())
                        .filter(|r| enabled_regions.contains(r))
                        .map(|region| get_cheapest_listings_live(region, refresh_version)),
                )
                .await
                .into_iter()
                .filter_map(|l| l.ok())
                .collect())
            } else {
                Ok(vec![])
            }
        },
    );

    let refetch_market_data = Callback::new(move |_| {
        set_market_refresh_version.update(|version| {
            *version = version.wrapping_add(1);
        });
    });

    view! {
        <div class="main-content p-2 sm:p-6">
            <MetaTitle title=move || t_string!(i18n, analyzer_meta_title).to_string().replace("%world%", &world()) />
            <MetaDescription text=move || {
                t_string!(i18n, analyzer_meta_desc).to_string().replace("%world%", &world())
            } />
            <div class="flex flex-col gap-3">
                    // Header + world picker. Deliberately kept OUTSIDE the
                    // `<Suspense>` below: `AnalyzerTable` (and the sticky bar
                    // it renders) only exists once every resource has
                    // resolved, so a control placed there vanishes behind
                    // the skeleton on every load — including a world change,
                    // which is exactly when a user most needs to be able to
                    // change worlds again. Keeping it here means it is always
                    // on screen, load or no load.
                    //
                    // ToolHeader carries the tool's h1 plus the expandable
                    // "About this tool" summary and the link to
                    // `/help/flip-finder`, matching every other analyzer
                    // (see vendor_resale.rs).
                    <ToolHeader
                        title=t_string!(i18n, flip_finder).to_string()
                        summary=t_string!(i18n, flip_finder_tool_summary).to_string()
                        context=t_string!(i18n, flip_finder_tool_context).to_string()
                        help_href="/help/flip-finder"
                        help_body=t_string!(i18n, flip_finder_tool_help).to_string()
                    />
                    <div class="flex flex-wrap items-center justify-end gap-3">
                        <AnalyzerWorldNavigator />
                    </div>

                    // Main Content. No `min-h-screen` and no scroll container:
                    // the table virtualizes against the window, so the page
                    // itself is what scrolls.
                    <div>
                        <Suspense fallback=AnalyzerTableSkeleton>
                            {move || {
                                let world_cheapest = world_cheapest_listings.get();
                                let sales = sales.get();
                                let global_cheapest_listings = global_cheapest_listings.get();
                                let cross_region = cross_region
                                    .get()
                                    .and_then(|r: Result<_, AppError>| r.ok())
                                    .unwrap_or_default();
                                let worlds = use_context::<LocalWorldData>()
                                    .expect("Worlds should always be populated here")
                                    .0
                                    .unwrap();
                                match (world_cheapest, sales, global_cheapest_listings) {
                                    (Some(Ok(w)), Some(Ok(s)), Some(Ok(g))) => {
                                        Either::Left(

                                            view! {
                                                <AnalyzerTable
                                                    sales=s
                                                    global_cheapest_listings=g
                                                    world_cheapest_listings=w
                                                    cross_region
                                                    worlds
                                                    world=world
                                                    filter_outliers=filter_outliers().unwrap_or(false)
                                                    region=region().ok()
                                                    cross_region_enabled=cross_region_enabled().unwrap_or_default()
                                                    set_cross_region_enabled=set_cross_region_enabled
                                                    set_filter_outliers=set_filter_outliers
                                                    on_market_update=refetch_market_data
                                                    name_chip_pending=name_chip_pending
                                                    hydrated=hydrated
                                                />
                                            },
                                        )
                                    }
                                    _ => {
                                        Either::Right(
                                            view! {
                                                <div class="text-xl text-[color:var(--color-text)] text-center p-8
                                                bg-brand-900/20 rounded-2xl border border-white/10">
                                                    {t!(i18n, analyzer_failed_to_load)}
                                                </div>
                                            },
                                        )
                                    }
                                }
                            }}
                        </Suspense>
                    </div>
                </div>
        </div>
    }
}

#[component]
fn AnalyzerWorldNavigator() -> impl IntoView {
    let i18n = use_i18n();
    let nav = use_navigate();
    let params = use_params_map();
    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0
        .unwrap();

    let initial_world = params.with_untracked(|p| {
        let world = p.get_str("world").unwrap_or_default();
        worlds
            .lookup_world_by_name(world)
            .and_then(|w| w.as_world().cloned())
    });

    let (current_world, set_current_world) = signal(initial_world);
    let query = use_query_map();
    let location = use_location();

    Effect::new(move |_| {
        if let Some(world) = current_world() {
            let url = world_nav_url(
                "/flip-finder",
                &world.name,
                &location.pathname.get_untracked(),
                &query.get_untracked(),
            );
            if let Some(url) = url {
                nav(
                    &url,
                    NavigateOptions {
                        scroll: false,
                        ..Default::default()
                    },
                );
            }
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, select_world)}</label>
            <div class="w-full md:w-auto">
                <WorldOnlyPicker
                    current_world=current_world.into()
                    set_current_world=set_current_world.into()
                />
            </div>
        </div>
    }
}

#[component]
pub fn Analyzer() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <MetaTitle title=t_string!(i18n, analyzer_index_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, analyzer_index_meta_desc).to_string() />

        <div class="main-content p-2 sm:p-6">
            <div class="flex flex-col gap-8">
                    // Hero Section
                    <div class="panel p-4 sm:p-8 rounded-2xl">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                            {t!(i18n, analyzer_index_title)}
                        </h1>
                        <p class="text-xl text-[color:var(--color-text)] leading-relaxed mb-6">
                            {t!(i18n, analyzer_index_desc_1)}
                        </p>
                        <p class="text-lg text-[color:var(--color-text)]/90 mb-8">
                            {t!(i18n, analyzer_index_desc_2)}
                        </p>

                        // World Selection
                        <div class="panel p-6 rounded-xl">
                            <h2 class="text-xl font-semibold text-[color:var(--brand-fg)] mb-4">
                                {t!(i18n, analyzer_index_choose_world)}
                            </h2>
                            <AnalyzerWorldNavigator />
                        </div>
                    </div>
                </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::recent_sales::{SaleData, Sales};

    fn sale(price: i32, days_ago: i64) -> Sales {
        let date = Utc::now()
            .naive_utc()
            .checked_sub_signed(Duration::days(days_ago))
            .unwrap();
        Sales {
            price_per_unit: price,
            sale_date: date,
        }
    }

    fn sales_row(item_id: i32, hq: bool, prices_and_days: &[(i32, i64)]) -> SaleData {
        SaleData {
            item_id,
            hq,
            sales: prices_and_days.iter().map(|(p, d)| sale(*p, *d)).collect(),
        }
    }

    #[test]
    fn median_price_is_middle_of_clamped_sales() {
        let row = sales_row(
            1,
            false,
            &[(100, 0), (110, 1), (120, 2), (130, 3), (140, 4), (150, 5)],
        );
        let summary = compute_summary(row, false);
        // Six even-length sample: median = (third + fourth) / 2 = (120 + 130) / 2 = 125
        assert_eq!(summary.median_price, 125);
    }

    #[test]
    fn sniper_sale_below_10pct_of_median_is_dropped() {
        // Raw median of [1, 100, 110, 120, 130, 140] sorted = (110+120)/2 = 115.
        // The "1" is well below 10% of 115 (=11), so it's dropped.
        let row = sales_row(
            2,
            false,
            &[(1, 0), (100, 1), (110, 2), (120, 3), (130, 4), (140, 5)],
        );
        let summary = compute_summary(row, false);
        // Median of remaining [100, 110, 120, 130, 140] = 120.
        assert_eq!(summary.median_price, 120);
        // min_price should also reflect the clamp, not the sniper.
        assert_eq!(summary.min_price, 100);
    }

    #[test]
    fn hq_prices_do_not_contaminate_nq_summary() {
        // An NQ row with normal prices. compute_summary no longer takes HQ context.
        let row = sales_row(
            3,
            false,
            &[(500, 0), (510, 1), (520, 2), (530, 3), (540, 4), (550, 5)],
        );
        let summary = compute_summary(row, false);
        assert_eq!(summary.min_price, 500);
        assert_eq!(summary.median_price, 525);
    }

    #[test]
    fn troll_region_floor_drops_row_entirely() {
        use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
        use ultros_api_types::recent_sales::RecentSales;

        let sales = RecentSales {
            sales: vec![sales_row(
                100,
                false,
                &[
                    (1000, 0),
                    (1000, 1),
                    (1100, 2),
                    (1000, 3),
                    (1050, 4),
                    (1000, 5),
                ],
            )],
        };
        // Region cheapest = a troll 999,999,999 listing on a foreign world.
        let region = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 100,
                hq: false,
                cheapest_price: 999_999_999,
                world_id: 42,
            }],
        };
        // Our own world has a sane cheapest at 1100.
        let world = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 100,
                hq: false,
                cheapest_price: 1100,
                world_id: 1,
            }],
        };

        let table = ProfitTable::new(sales, region, world, vec![], false);
        // The troll 999M region listing should cause the row to be dropped entirely
        // (the displayed "deal" would be fictional). table.0 should be empty.
        assert_eq!(table.0.len(), 0);
    }

    #[test]
    fn troll_world_floor_falls_through_to_median() {
        use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
        use ultros_api_types::recent_sales::RecentSales;

        // Sales settle at a stable median of 1000.
        let sales = RecentSales {
            sales: vec![sales_row(
                300,
                false,
                &[
                    (1000, 0),
                    (1000, 1),
                    (1000, 2),
                    (1000, 3),
                    (1000, 4),
                    (1000, 5),
                ],
            )],
        };
        // Region floor is sane (500 — below median, a real deal).
        let region = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 300,
                hq: false,
                cheapest_price: 500,
                world_id: 42,
            }],
        };
        // Local world floor is a troll listing.
        let world = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 300,
                hq: false,
                cheapest_price: 999_999_999,
                world_id: 1,
            }],
        };

        let table = ProfitTable::new(sales, region, world, vec![], false);
        // Row is kept (region floor is sane), but the troll world floor is ignored —
        // estimated_sale_price falls through to median, not the troll value.
        assert_eq!(table.0.len(), 1);
        assert_eq!(table.0[0].estimated_sale_price, 1000);
    }

    #[test]
    fn median_i32_odd_length() {
        // Direct unit test on the helper — exercises the n % 2 == 1 branch.
        assert_eq!(median_in_place_i32(&mut [100, 200, 300, 400, 500]), 300);
        assert_eq!(median_in_place_i32(&mut [100, 110, 120, 130, 140]), 120);
    }

    #[test]
    fn estimated_sale_price_uses_median_not_min() {
        use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
        use ultros_api_types::recent_sales::RecentSales;

        let sales = RecentSales {
            sales: vec![sales_row(
                200,
                false,
                &[
                    (800, 0),
                    (1000, 1),
                    (1000, 2),
                    (1000, 3),
                    (1000, 4),
                    (1200, 5),
                ],
            )],
        };
        // Region floor is below median (a sane off-world deal).
        let region = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 200,
                hq: false,
                cheapest_price: 700,
                world_id: 42,
            }],
        };
        // Local world floor is well above the median — the estimate should pin to median (=1000),
        // not min (=800) and not the world floor (=5000).
        let world = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 200,
                hq: false,
                cheapest_price: 5000,
                world_id: 1,
            }],
        };

        let table = ProfitTable::new(sales, region, world, vec![], false);
        assert_eq!(table.0.len(), 1);
        let row = &table.0[0];
        assert_eq!(row.sale_summary.median_price, 1000);
        assert_eq!(row.estimated_sale_price, 1000);
    }

    #[test]
    fn visible_keys_includes_window_and_margin() {
        let data: Vec<(i32, bool)> = (0..100).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // rendered rows [40, 50), margin 5 => slice [35, 55)
        let keys = visible_keys(&data, (40, 50), 5, &seen, |k| *k);
        assert_eq!(keys.len(), 20);
        assert_eq!(keys.first(), Some(&(35, false)));
        assert_eq!(keys.last(), Some(&(54, false)));
    }

    #[test]
    fn visible_keys_clamps_at_start_and_end() {
        let data: Vec<(i32, bool)> = (0..10).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // start clamp: lo = 2.saturating_sub(5) = 0
        // end clamp: hi = (8 + 5).min(10) = 10 (would be 13 unclamped) => slice [0, 10)
        let keys = visible_keys(&data, (2, 8), 5, &seen, |k| *k);
        assert_eq!(keys.len(), 10);
        assert_eq!(keys.first(), Some(&(0, false)));
        assert_eq!(keys.last(), Some(&(9, false)));
    }

    #[test]
    fn visible_keys_excludes_already_seen() {
        let data: Vec<(i32, bool)> = (0..10).map(|i| (i, false)).collect();
        let mut seen = std::collections::HashSet::new();
        seen.insert((3, false));
        seen.insert((5, false));
        let keys = visible_keys(&data, (0, 10), 0, &seen, |k| *k);
        assert_eq!(keys.len(), 8);
        assert!(!keys.contains(&(3, false)));
        assert!(!keys.contains(&(5, false)));
    }

    #[test]
    fn visible_keys_empty_data_yields_empty() {
        let data: Vec<(i32, bool)> = Vec::new();
        let seen = std::collections::HashSet::new();
        let keys = visible_keys(&data, (0, 0), 30, &seen, |k| *k);
        assert!(keys.is_empty());
    }

    #[test]
    fn visible_keys_out_of_range_yields_empty() {
        let data: Vec<(i32, bool)> = (0..5).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // lo = 95, hi = (110 + 5).min(5) = 5 => get(95..5) is an invalid range => &[]
        let keys = visible_keys(&data, (100, 110), 5, &seen, |k| *k);
        assert!(keys.is_empty());
    }

    fn calc(profit: i32, roi: i32, ppd: i32) -> CalculatedProfitData {
        CalculatedProfitData {
            inner: Arc::new(ProfitData {
                estimated_sale_price: 0,
                cheapest_price: 0,
                cheapest_world_id: 0,
                prices: Vec::new(),
                sale_summary: SaleSummary {
                    item_id: 1,
                    hq: false,
                    num_sold: 6,
                    avg_sale_duration: None,
                    days_since_last_sale: None,
                    max_price: 0,
                    avg_price: 0,
                    median_price: 0,
                    min_price: 0,
                },
            }),
            profit,
            return_on_investment: roi,
            profit_per_day: ppd,
        }
    }

    #[test]
    fn sort_desc_puts_largest_first() {
        let mut rows = vec![calc(10, 0, 0), calc(30, 0, 0), calc(20, 0, 0)];
        sort_rows(&mut rows, SortMode::Profit, SortDir::Desc);
        assert_eq!(
            rows.iter().map(|r| r.profit).collect::<Vec<_>>(),
            vec![30, 20, 10]
        );
    }

    #[test]
    fn sort_asc_puts_smallest_first() {
        let mut rows = vec![calc(10, 0, 0), calc(30, 0, 0), calc(20, 0, 0)];
        sort_rows(&mut rows, SortMode::Profit, SortDir::Asc);
        assert_eq!(
            rows.iter().map(|r| r.profit).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn sort_by_profit_per_day_is_independent_of_profit() {
        let mut rows = vec![calc(100, 0, 1), calc(10, 0, 99)];
        sort_rows(&mut rows, SortMode::ProfitPerDay, SortDir::Desc);
        assert_eq!(rows[0].profit_per_day, 99);
    }

    #[test]
    fn velocity_floor_prefers_clickhouse_rate_over_derived() {
        // The Velocity column shows the ClickHouse rate whenever the rollup
        // covers a row, so the filter has to evaluate that same number.
        // Otherwise a row displays "0.3/day", survives a floor of 5 on a
        // derived 6/day, and the filter looks broken.
        assert!(!passes_velocity_floor(5.0, Some(0.3), Some(6.0)));
        assert!(passes_velocity_floor(5.0, Some(6.0), Some(0.3)));
    }

    #[test]
    fn velocity_floor_falls_back_to_derived_without_clickhouse() {
        // ~93% of rows have no rollup entry; those must still be filterable.
        assert!(passes_velocity_floor(1.0, None, Some(2.0)));
        assert!(!passes_velocity_floor(1.0, None, Some(0.5)));
    }

    #[test]
    fn velocity_floor_is_inclusive_and_drops_rateless_rows() {
        assert!(passes_velocity_floor(2.0, None, Some(2.0)));
        // No rate at all cannot clear an explicit floor, even a floor of zero.
        assert!(!passes_velocity_floor(0.0, None, None));
    }

    #[test]
    fn non_finite_velocity_floor_is_ignored() {
        // `"NaN".parse::<f32>()` succeeds, and `v >= NaN` is false for every
        // row, so honoring `?vel=NaN` would silently empty the table.
        assert_eq!(normalize_velocity_floor(Some(f32::NAN)), None);
        assert_eq!(normalize_velocity_floor(Some(f32::INFINITY)), None);
        assert_eq!(normalize_velocity_floor(Some(2.5)), Some(2.5));
        assert_eq!(normalize_velocity_floor(None), None);
    }

    #[test]
    fn roi_is_optional_and_off_by_default() {
        assert!(ALL_OPTIONAL_COLS.contains(&COL_ROI));
        assert!(!DEFAULT_VISIBLE_COLS.contains(&COL_ROI));
    }

    #[test]
    fn new_columns_are_on_by_default() {
        for col in [COL_VELOCITY, COL_DRIFT, COL_CONFIDENCE] {
            assert!(ALL_OPTIONAL_COLS.contains(&col), "{col} missing from ALL");
            assert!(DEFAULT_VISIBLE_COLS.contains(&col), "{col} not default-on");
        }
    }

    #[test]
    fn ch_only_columns_are_off_by_default() {
        for col in [COL_TREND, COL_VOLUME_30D, COL_SALES_PER_DAY, COL_DATACENTER] {
            assert!(
                !DEFAULT_VISIBLE_COLS.contains(&col),
                "{col} should be opt-in (ClickHouse covers ~7% of items)"
            );
        }
    }

    #[test]
    fn visible_cols_round_trip_with_new_ids() {
        let set = parse_visible_cols(Some("velocity,drift,confidence"));
        assert_eq!(set.len(), 3);
        let s = serialize_visible_cols(&set);
        assert_eq!(parse_visible_cols(Some(&s)), set);
    }

    #[test]
    fn explicit_empty_cols_param_is_respected() {
        // Regression guard: an explicit "" must mean "no optional columns",
        // not "fall back to defaults".
        assert!(parse_visible_cols(Some("")).is_empty());
        assert!(!parse_visible_cols(None).is_empty());
    }

    #[test]
    fn profit_table_keeps_raw_prices_newest_first() {
        use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
        use ultros_api_types::recent_sales::RecentSales;

        // `sales_row` takes (price, days_ago), so this is newest-first — the
        // wire order `price_drift_pct` expects. The captured vector must not be
        // the price-sorted one `compute_summary` builds internally, or the
        // Drift column would read a monotonic ramp for every row.
        let sales = RecentSales {
            sales: vec![sales_row(
                400,
                false,
                &[(90, 0), (95, 1), (100, 2), (300, 3), (110, 4), (105, 5)],
            )],
        };
        let region = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 400,
                hq: false,
                cheapest_price: 50,
                world_id: 42,
            }],
        };
        let world = CheapestListings {
            cheapest_listings: vec![],
        };

        let table = ProfitTable::new(sales, region, world, vec![], false);
        assert_eq!(table.0.len(), 1);
        assert_eq!(table.0[0].prices, vec![90, 95, 100, 300, 110, 105]);
    }

    #[test]
    fn the_default_column_set_adds_no_extra_width() {
        // The stylesheet's per-breakpoint baseline already covers these, so
        // counting them here would reserve the width twice and leave the grid
        // scrolling into empty space.
        let defaults: std::collections::HashSet<&'static str> =
            DEFAULT_VISIBLE_COLS.iter().copied().collect();
        assert_eq!(
            extra_column_widths_px(&defaults),
            ExtraColumnWidths::default()
        );
        assert_eq!(
            extra_column_widths_px(&std::collections::HashSet::new()),
            ExtraColumnWidths::default()
        );
    }

    #[test]
    fn every_opt_in_column_reserves_width() {
        // A column that neither the CSS baseline nor this function accounts
        // for is one the scrollports stop short of — the column renders and
        // cannot be reached, which is the bug this whole mechanism exists to
        // prevent.
        for col in ALL_OPTIONAL_COLS {
            if DEFAULT_VISIBLE_COLS.contains(col) {
                continue;
            }
            let set: std::collections::HashSet<&'static str> = [*col].into_iter().collect();
            let widths = extra_column_widths_px(&set);
            assert!(
                widths.base + widths.md + widths.xl > 0,
                "{col} reserves no width, so the grid would stop short of it"
            );
        }
    }

    #[test]
    fn breakpoint_hidden_columns_reserve_no_width_below_their_breakpoint() {
        // The other half of the reservation contract: a `hidden md:flex` /
        // `hidden xl:flex` column must not widen the scroll range of a
        // viewport that never renders it, or a phone scrolls into blank
        // space. `base` is the only bucket a phone-width stylesheet applies,
        // and `md` is the widest bucket applied below `xl`.
        let md_gated: std::collections::HashSet<&'static str> =
            [COL_TREND, COL_SALES_PER_DAY, COL_VOLUME_30D]
                .into_iter()
                .collect();
        let widths = extra_column_widths_px(&md_gated);
        assert_eq!(widths.base, 0);
        assert!(widths.md > 0);
        assert_eq!(widths.xl, 0);

        let xl_gated: std::collections::HashSet<&'static str> =
            [COL_DATACENTER].into_iter().collect();
        let widths = extra_column_widths_px(&xl_gated);
        assert_eq!(widths.base, 0);
        assert_eq!(widths.md, 0);
        assert!(widths.xl > 0);

        // ROI renders at every width, so its reservation must too.
        let always: std::collections::HashSet<&'static str> = [COL_ROI].into_iter().collect();
        assert!(extra_column_widths_px(&always).base > 0);
    }

    #[test]
    fn hidden_note_matches_the_width_buckets() {
        // Every optional column that is breakpoint-hidden gets a "desktop
        // only" note in the Columns picker; the two always-visible ones must
        // not, or the note would be a lie.
        for col in ALL_OPTIONAL_COLS {
            let note = col_hidden_note_class(col);
            if *col == COL_PROFIT_PER_DAY || *col == COL_ROI {
                assert!(note.is_none(), "{col} is always visible");
            } else {
                assert!(note.is_some(), "{col} is breakpoint-hidden");
            }
        }
    }

    #[test]
    fn a_bad_entry_leaves_the_filter_alone() {
        // The prod-facing bug this guards: `set(raw.parse().ok())` deletes the
        // filter the user is editing the moment the target type rejects what
        // they typed. `-5` is a legal number, so `type=number` hands it over
        // intact; `usize` then refuses it.
        assert_eq!(
            commit_numeric(Some(3usize), Some("-5".to_string())),
            Some(3)
        );
        assert_eq!(
            commit_numeric(Some(100_000i32), Some("abc".to_string())),
            Some(100_000)
        );
    }

    #[test]
    fn an_explicit_clear_removes_the_filter() {
        // `None` is the `x` button, and `committed_value` has already mapped
        // blank input to `None` by this point.
        assert_eq!(commit_numeric(Some(3usize), None), None);
    }

    #[test]
    fn a_valid_entry_replaces_the_value() {
        assert_eq!(commit_numeric(Some(3usize), Some("6".to_string())), Some(6));
        assert_eq!(
            commit_numeric(None, Some("0.25".to_string())),
            Some(0.25f32)
        );
    }

    #[test]
    fn a_bad_entry_with_nothing_to_keep_stays_unset() {
        assert_eq!(commit_numeric(None::<i32>, Some("abc".to_string())), None);
    }

    #[test]
    fn velocity_floor_renders_without_float_noise() {
        // `?vel=0.2` round-trips through f32 as 0.20000000298023224.
        assert_eq!(format_velocity_floor(0.2), "0.2");
        assert_eq!(format_velocity_floor(1.0), "1");
        assert_eq!(format_velocity_floor(2.5), "2.5");
        // Guards the naive `trim_end_matches('0')`, which reads "10.00" as "1".
        assert_eq!(format_velocity_floor(10.0), "10");
    }

    #[test]
    fn a_rendered_velocity_floor_parses_back_to_itself() {
        // The chip shows this string *and* edits it, so it has to survive a
        // round trip or opening the chip would change the filter.
        for v in [0.2f32, 0.25, 1.0, 10.0, 0.05] {
            let rendered = format_velocity_floor(v);
            assert_eq!(
                rendered.parse::<f32>(),
                Ok(v),
                "{v} rendered as {rendered:?} did not parse back"
            );
        }
    }

    #[test]
    fn filter_menu_omits_filters_that_already_have_a_chip() {
        // Offering a filter that is already a chip would put two editable
        // representations of one value back on the page — the exact thing
        // the sticky bar exists to delete.
        let available = available_filters(&[FILTER_PROFIT, FILTER_VELOCITY]);
        assert!(!available.contains(&FILTER_PROFIT));
        assert!(!available.contains(&FILTER_VELOCITY));
        let expected = ADDABLE_FILTERS
            .iter()
            .copied()
            .filter(|id| *id != FILTER_PROFIT && *id != FILTER_VELOCITY)
            .collect::<Vec<_>>();
        assert_eq!(available, expected, "menu order must be stable");
    }

    #[test]
    fn filter_menu_offers_everything_when_nothing_is_set() {
        assert_eq!(available_filters(&[]), ADDABLE_FILTERS.to_vec());
    }

    #[test]
    fn addable_filter_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for id in ADDABLE_FILTERS {
            assert!(seen.insert(*id), "{id} is listed twice in ADDABLE_FILTERS");
        }
    }

    #[test]
    fn every_addable_filter_has_a_starting_value() {
        for id in ADDABLE_FILTERS {
            // FILTER_NAME is the deliberate exception: its chip mounts in
            // edit state instead of seeding a resting value (see
            // `default_filter_value`'s doc comment).
            if *id == FILTER_NAME {
                continue;
            }
            assert!(
                !default_filter_value(id).is_empty(),
                "{id} has no default, so the + Filter menu would add an empty chip"
            );
        }
    }

    #[test]
    fn numeric_filter_defaults_are_parseable() {
        // These feed `"...".parse::<i32/usize/f32>()`; an unparseable default
        // silently adds nothing at all when picked from the menu.
        for id in [
            FILTER_PROFIT,
            FILTER_PROFIT_PER_DAY,
            FILTER_ROI,
            FILTER_SALES,
            FILTER_VELOCITY,
            FILTER_MIN_BUY,
            FILTER_MAX_PRICE,
        ] {
            let raw = default_filter_value(id);
            assert!(
                raw.parse::<f64>().is_ok(),
                "{id} default {raw:?} does not parse as a number"
            );
        }
    }

    #[test]
    fn duration_filter_defaults_parse_as_durations() {
        for id in [FILTER_NEXT_SALE, FILTER_LAST_SOLD] {
            let raw = default_filter_value(id);
            assert!(
                parse_duration(raw).is_ok(),
                "{id} default {raw:?} is not a duration humantime accepts"
            );
        }
    }

    /// The header's flip rule, extracted from the href closure so it can be
    /// pinned without a router. Clicking the column already in effect flips;
    /// clicking any other column starts descending.
    fn next_sort_dir(is_active: bool, current: SortDir) -> SortDir {
        if is_active {
            match current {
                SortDir::Desc => SortDir::Asc,
                SortDir::Asc => SortDir::Desc,
            }
        } else {
            SortDir::Desc
        }
    }

    #[test]
    fn clicking_the_active_column_flips_direction() {
        assert_eq!(next_sort_dir(true, SortDir::Desc), SortDir::Asc);
        assert_eq!(next_sort_dir(true, SortDir::Asc), SortDir::Desc);
    }

    #[test]
    fn clicking_a_different_column_starts_descending() {
        // Arriving at a new column ascending would bury the best rows, which
        // is the opposite of what every one of these columns is sorted for.
        assert_eq!(next_sort_dir(false, SortDir::Asc), SortDir::Desc);
        assert_eq!(next_sort_dir(false, SortDir::Desc), SortDir::Desc);
    }

    #[test]
    fn descending_is_the_default_so_it_stays_out_of_the_url() {
        // The header omits `dir` whenever it equals the default; if that
        // default ever changed, every bookmarked `?sort=` would silently
        // flip meaning.
        assert_eq!(SortDir::default(), SortDir::Desc);
    }

    #[test]
    fn sort_dir_round_trips_through_string() {
        assert_eq!("asc".parse::<SortDir>(), Ok(SortDir::Asc));
        assert_eq!("desc".parse::<SortDir>(), Ok(SortDir::Desc));
        assert_eq!(SortDir::Asc.to_string(), "asc");
        assert_eq!(SortDir::Desc.to_string(), "desc");
        assert!("sideways".parse::<SortDir>().is_err());
    }

    #[test]
    fn quality_filter_round_trips_its_url_tokens() {
        assert_eq!("hq".parse::<QualityFilter>(), Ok(QualityFilter::Hq));
        assert_eq!("nq".parse::<QualityFilter>(), Ok(QualityFilter::Nq));
        assert!("HQ".parse::<QualityFilter>().is_err());
        assert_eq!(QualityFilter::Hq.to_string(), "hq");
        assert_eq!(QualityFilter::Nq.to_string(), "nq");
    }

    #[test]
    fn quality_filter_selects_matching_rows_only() {
        assert!(passes_quality(QualityFilter::Hq, true));
        assert!(!passes_quality(QualityFilter::Hq, false));
        assert!(passes_quality(QualityFilter::Nq, false));
        assert!(!passes_quality(QualityFilter::Nq, true));
    }

    /// The production pairing: normalize once (per recompute), match many
    /// (per row). Composed here so the tests exercise the same path.
    fn name_matches(raw_query: &str, item_name: &str) -> bool {
        match normalize_name_query(raw_query) {
            Some(q) => matches_normalized_name(&q, item_name),
            None => true,
        }
    }

    #[test]
    fn name_match_is_case_insensitive_substring() {
        assert!(name_matches("grade", "Grade 8 Tincture of Strength"));
        assert!(name_matches("TINCTURE", "Grade 8 Tincture of Strength"));
        assert!(!name_matches("potion", "Grade 8 Tincture of Strength"));
    }

    #[test]
    fn blank_or_whitespace_name_query_matches_everything() {
        // The chip seeds empty and the user may commit whitespace; neither
        // should silently empty the table.
        assert!(name_matches("", "Anything"));
        assert!(name_matches("   ", "Anything"));
    }

    #[test]
    fn name_query_normalizes_once_to_trimmed_lowercase() {
        // The per-row side must be able to assume a pre-lowercased,
        // pre-trimmed query — that is the whole point of hoisting the
        // normalization out of the 20k-row loop.
        assert_eq!(
            normalize_name_query("  TiNcTuRe "),
            Some("tincture".to_string())
        );
        assert_eq!(normalize_name_query("   "), None);
        assert_eq!(normalize_name_query(""), None);
    }

    #[test]
    fn filter_param_keys_are_pinned_wire_format() {
        // `?drift=` / `?confidence=` deliberately share their *values* with
        // the COL_DRIFT / COL_CONFIDENCE `?cols=` tokens — different
        // namespaces. The values are public wire format (bookmarks, saved
        // views); only the Rust const names may change.
        assert_eq!(FILTER_MIN_DRIFT, COL_DRIFT);
        assert_eq!(FILTER_MIN_DRIFT, "drift");
        assert_eq!(FILTER_MIN_CONFIDENCE, COL_CONFIDENCE);
        assert_eq!(FILTER_MIN_CONFIDENCE, "confidence");
        assert_eq!(FILTER_MIN_VOLUME, "min-volume");
        assert_eq!(FILTER_NAME, "name");
        assert_eq!(FILTER_QUALITY, "quality");
    }

    #[test]
    fn drift_floor_keeps_rows_at_or_above_the_floor() {
        assert!(passes_drift_floor(-10.0, Some(-5.0)));
        assert!(passes_drift_floor(-10.0, Some(-10.0)));
        assert!(!passes_drift_floor(-10.0, Some(-25.0)));
    }

    #[test]
    fn drift_floor_drops_rows_with_uncomputable_drift() {
        // Universal-coverage metric: same unknown-fails rule as the
        // velocity floor (spec: Unknown-data semantics).
        assert!(!passes_drift_floor(-10.0, None));
    }

    #[test]
    fn confidence_floor_prefers_the_clickhouse_band() {
        // CH says Low; the derived band saying High must not override it.
        assert!(!passes_confidence_floor(
            ConfidenceFloor::Medium,
            Some(ConfidenceBand::Low),
            DerivedConfidence::High,
        ));
        assert!(passes_confidence_floor(
            ConfidenceFloor::Medium,
            Some(ConfidenceBand::High),
            DerivedConfidence::Low,
        ));
    }

    #[test]
    fn confidence_unknown_band_falls_back_to_derived() {
        // CH `Unknown` is "no deep-scan yet", not a verdict.
        assert!(passes_confidence_floor(
            ConfidenceFloor::Medium,
            Some(ConfidenceBand::Unknown),
            DerivedConfidence::Medium,
        ));
        assert!(passes_confidence_floor(
            ConfidenceFloor::High,
            None,
            DerivedConfidence::High,
        ));
        assert!(!passes_confidence_floor(
            ConfidenceFloor::High,
            None,
            DerivedConfidence::Medium,
        ));
    }

    #[test]
    fn confidence_unusable_fails_any_floor() {
        assert!(!passes_confidence_floor(
            ConfidenceFloor::Low,
            Some(ConfidenceBand::Unusable),
            DerivedConfidence::High,
        ));
    }

    #[test]
    fn confidence_floor_round_trips_its_url_tokens() {
        assert_eq!(
            "medium".parse::<ConfidenceFloor>(),
            Ok(ConfidenceFloor::Medium)
        );
        assert!("Medium".parse::<ConfidenceFloor>().is_err());
        assert_eq!(ConfidenceFloor::High.to_string(), "high");
    }

    #[test]
    fn volume_floor_keeps_rows_without_clickhouse_coverage() {
        // CH-only metric (~7% coverage, lazily enriched): unknown-fails
        // would empty the un-enriched table and deadlock the lazy fetch
        // (spec: Unknown-data semantics). Unknown rows pass.
        assert!(passes_volume_floor(10, None));
    }

    #[test]
    fn volume_floor_drops_rows_with_known_volume_below_it() {
        assert!(!passes_volume_floor(10, Some(3)));
        assert!(passes_volume_floor(10, Some(10)));
        assert!(passes_volume_floor(10, Some(250)));
    }
}
