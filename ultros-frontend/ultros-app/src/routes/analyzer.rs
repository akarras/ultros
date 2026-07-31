use super::analyzer_columns::*;
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
        filter_chip::FilterChip,
        gil::*,
        icon::Icon,
        item_icon::*,
        meta::*,
        query_button::QueryButton,
        realtime_status::RealtimeStatus,
        sales_cadence_badge::SalesCadenceBadge,
        saved_views::SavedViewsMenu,
        skeleton::{BoxSkeleton, SingleLineSkeleton},
        sparkline::Sparkline,
        toggle::Toggle,
        tooltip::*,
        virtual_scroller::*,
        world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
    math::filter_outliers_iqr_in_place,
    query_defaults::{
        DEFAULT_MAX_SALE_TIME, filter_query_signal, seed_query_defaults_when_unfiltered,
    },
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

use chrono::{Duration, Utc};
use codee::string::JsonSerdeCodec;
use gloo_timers::future::TimeoutFuture;
use humantime::parse_duration;
use icondata as i;
use leptos::{either::Either, prelude::*, reactive::wrappers::write::SignalSetter};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_location, use_navigate, use_params_map, use_query_map},
    location::Location,
};
use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};
use leptos_use::{use_element_bounding, use_window_scroll, use_window_size};
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
];

/// Params whose presence means the visitor already chose filters, so the
/// Realistic-flips default must not be seeded on top. Everything in
/// [`ADDABLE_FILTERS`], the chip-only filters, and the sort params. View
/// configuration (`cols`, `cross`, `filter-outliers`, per-region toggles)
/// deliberately does not suppress: a columns bookmark still deserves the
/// default filters.
pub(crate) const SEED_SUPPRESSING_PARAMS: &[&str] = &[
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
    FILTER_CATEGORY,
    FILTER_WORLD,
    FILTER_DATACENTER,
    "sort",
    "dir",
];

/// The "Realistic flips" built-in view's params (saved_views.rs) plus the
/// long-standing `next-sale=1d` velocity default — what a first-time
/// visitor lands on, rendered as removable chips.
pub(crate) const REALISTIC_DEFAULT_PARAMS: &[(&str, &str)] = &[
    (FILTER_MIN_BUY, "5000"),
    (FILTER_LAST_SOLD, "1d"),
    (FILTER_ROI, "30"),
    ("sort", "profit-per-day"),
    (FILTER_NEXT_SALE, DEFAULT_MAX_SALE_TIME),
];

/// Value a filter takes when it is added from the `+ Filter` menu.
///
/// A filter with no starting value would render a chip with nothing in it,
/// so every entry in [`ADDABLE_FILTERS`] must have one. These mirror the
/// example values the old toolbar carried as input placeholders.
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

/// Drag handle on a header cell's right edge. Pointer events + pointer
/// capture give mouse and touch one code path. During the drag the new
/// width is written straight to the pane element's `--colw-*` property
/// (no reactive churn at 60fps); the signal — and through it localStorage
/// — commits once on release, and the pane's reactive `style` re-render
/// then agrees with what the drag already painted.
#[component]
fn ColResizeHandle(
    col: &'static str,
    pane: NodeRef<leptos::html::Div>,
    col_widths: Signal<HashMap<String, f64>>,
    set_col_widths: WriteSignal<HashMap<String, f64>>,
) -> impl IntoView {
    // (start_client_x, width_at_start)
    let drag = RwSignal::new(None::<(f64, f64)>);
    let spec = column_spec(col).expect("resize handle on unregistered column");

    let width_from = move |ev: &web_sys::PointerEvent| -> Option<f64> {
        let (start_x, start_w) = drag.get_untracked()?;
        Some((start_w + (ev.client_x() - start_x)).max(spec.min_width))
    };

    view! {
        <div
            class="analyzer-col-resize"
            on:pointerdown=move |ev: web_sys::PointerEvent| {
                ev.prevent_default();
                ev.stop_propagation();
                let target = event_target::<web_sys::HtmlElement>(&ev);
                let _ = target.set_pointer_capture(ev.pointer_id());
                let start_w = effective_width(spec, &col_widths.get_untracked());
                drag.set(Some((ev.client_x(), start_w)));
            }
            on:pointermove=move |ev: web_sys::PointerEvent| {
                if let (Some(w), Some(el)) = (width_from(&ev), pane.get_untracked()) {
                    // Fully qualified: leptos's `style` attribute-builder
                    // trait method otherwise shadows web_sys's inherent
                    // `HtmlElement::style()` on the deref chain.
                    let _ = web_sys::HtmlElement::style(&el)
                        .set_property(&format!("--colw-{col}"), &format!("{}px", w.round()));
                }
            }
            on:pointerup=move |ev: web_sys::PointerEvent| {
                if let Some(w) = width_from(&ev) {
                    set_col_widths.update(|m| {
                        m.insert(col.to_string(), w.round());
                    });
                }
                drag.set(None);
            }
            on:pointercancel=move |_| drag.set(None)
            on:dblclick=move |_| {
                // Double-click a handle = reset that column to its default.
                set_col_widths.update(|m| {
                    m.remove(col);
                });
            }
        ></div>
    }
    .into_any()
}

/// One header cell, sized by its column's `--colw-*` variable. Tasks
/// layered on top: the resize handle and the context-menu hookup.
#[component]
fn HeaderCell(
    col: &'static str,
    /// Extra classes: alignment (`justify-end`, `justify-center`) and
    /// anything cell-specific.
    #[prop(optional, into)]
    class: String,
    pane: NodeRef<leptos::html::Div>,
    col_widths: Signal<HashMap<String, f64>>,
    set_col_widths: WriteSignal<HashMap<String, f64>>,
    children: Children,
) -> impl IntoView {
    let resizable = column_spec(col).map(|s| s.resizable).unwrap_or(false);
    view! {
        <div
            role="columnheader"
            class=format!("relative shrink-0 px-3 py-2 flex items-center gap-2 min-w-0 {class}")
            style=format!("width:var(--colw-{col})")
        >
            {children()}
            {resizable.then(|| view! {
                <ColResizeHandle col pane col_widths set_col_widths />
            })}
        </div>
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
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_profit_per_day, set_minimum_profit_per_day) = query_signal::<i32>("ppd");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    // Defaults to 1d, seeded by AnalyzerWorldView as part of the
    // Realistic-flips defaults — but only for fully unfiltered URLs, so a
    // shared link with explicit filters is honored verbatim. The chip has an
    // X, so the default is visible and one click from gone.
    let (max_predicted_time, set_max_predicted_time) = filter_query_signal::<String>("next-sale");
    let (world_filter, set_world_filter) = query_signal::<String>("world");
    let (datacenter_filter, set_datacenter_filter) = query_signal::<String>("datacenter");
    let (tax_enabled, set_tax_enabled) = query_signal::<bool>("tax");
    let (minimum_sales, set_minimum_sales) = query_signal::<usize>("sales");
    let (min_velocity, set_min_velocity) = query_signal::<f32>("vel");
    // Single normalization point for the floor — the filter, the summary
    // chip and the toolbar input all read this, never `min_velocity` raw.
    let velocity_floor = Memo::new(move |_| normalize_velocity_floor(min_velocity()));
    let (category_filter, set_category_filter) = query_signal::<i32>("category");
    let (max_purchase_price, set_max_purchase_price) = query_signal::<i32>("max-price");
    let (min_buy_price, set_min_buy_price) = query_signal::<i32>("min-buy");
    let (show_suspicious, set_show_suspicious) = query_signal::<bool>("show-suspicious");
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    let visible_cols = Memo::new(move |_| parse_visible_cols(cols_param().as_deref()));
    // User column-width overrides, px, keyed by column id. Device-local
    // preference like saved views — deliberately NOT in the URL.
    // `delay_during_hydration` is load-bearing (see saved_views.rs).
    let (col_widths, set_col_widths, _) =
        use_local_storage_with_options::<HashMap<String, f64>, JsonSerdeCodec>(
            COL_WIDTHS_KEY,
            UseStorageOptions::default().delay_during_hydration(true),
        );
    // Target for the drag's direct `--colw-*` style writes: the pane div
    // that carries the column-width variables.
    let pane_ref = NodeRef::<leptos::html::Div>::new();
    let show_suspicious_active = Memo::new(move |_| show_suspicious().unwrap_or(false));
    let show_columns_picker = RwSignal::new(false);
    let show_filter_menu = RwSignal::new(false);

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

    let (last_sold_within, set_last_sold_within) = query_signal::<String>("last-sold");
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
            _ => {}
        }
    };

    // --- Pane height -------------------------------------------------------
    // The table is a contained pane filling the viewport below the control
    // bar: height = window height − the pane root's document-space top. Both
    // terms are reactive (resize, and any reflow above the pane); the
    // document-space top (viewport top + scroll y) is constant under page
    // scroll, so the pane does not jiggle while the user scrolls to the
    // footer.
    let pane_root = NodeRef::<leptos::html::Div>::new();
    let pane_bounds = use_element_bounding(pane_root);
    let (_, window_scroll_y) = use_window_scroll();
    let window_size = use_window_size();
    let pane_height = Memo::new(move |_| {
        let window_h = window_size.height.get();
        // `use_window_size` is INFINITY on the server (leptos-use's ssr
        // feature never measures), not 0.0 — without the finiteness check
        // SSR would ship `height:2147483647px` (i32 saturation).
        if !window_h.is_finite() || window_h <= 0.0 {
            return 640.0; // SSR / pre-hydration fallback
        }
        let doc_top = pane_bounds.top.get() + window_scroll_y.get();
        ((window_h - doc_top) - 8.0).max(320.0)
    });

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
    };

    // Accumulating CH enrichment (quality + sparkline + settled), grown by the
    // visible-window fetch effect below; never wholesale-replaced (except on a
    // world change). Cells + the suspicious filter read it reactively.
    let enrichment = RwSignal::new(EnrichmentMaps::default());

    let sorted_data = Memo::new(move |_| {
        let include_tax = tax_enabled().unwrap_or(true);
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
        sorted_data
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, CalculatedProfitData)>>()
    });

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
        <div
            node_ref=pane_root
            class="flex flex-col gap-2 min-h-0"
            style=move || format!("height:{}px;", pane_height().round() as i32)
        >
            // Control bar. Height still fixed so the pane-height measurement
            // is stable; no longer load-bearing for any sticky offset — the
            // table header now sticks inside the pane's own scrollport.
            <div class="sticky-bar h-[76px] px-2 py-1 flex flex-col gap-1">
                // Row 1 — result count and view-level controls.
                <div class="h-8 flex items-center gap-3 min-w-0">
                    <span class="text-sm text-[color:var(--brand-fg)] font-semibold whitespace-nowrap">
                        {move || {
                            t_string!(i18n, analyzer_rows_count)
                                .to_string()
                                .replace("%count%", &sorted_data().len().to_string())
                        }}
                    </span>
                    // Live-market indicator, carried over from the realtime work on
                    // main. It sat in the results-summary panel this bar replaced.
                    <RealtimeStatus status=realtime_status last_update=last_update />
                    <div class="flex-1" />
                    <SavedViewsMenu current_world=world />
                    <button
                        class="sticky-bar-button"
                        aria-expanded=move || show_columns_picker.get().to_string()
                        on:click=move |_| {
                            show_filter_menu.set(false);
                            show_columns_picker.update(|v| *v = !*v);
                        }
                    >
                        <Icon icon=i::FaTableColumnsSolid />
                        {t!(i18n, analyzer_columns_button)}
                    </button>
                    <button
                        class="sticky-bar-button"
                        aria-label=t_string!(i18n, aria_clear_all_filters)
                        on:click=move |_| clear_all_filters()
                    >
                        {t!(i18n, analyzer_clear_all)}
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

            // The pane: fills the rest of the root's fixed height; the
            // VirtualScroller inside it (fill mode) is the single scrollport
            // for both axes, with the column header sticky inside it.
            <div
                node_ref=pane_ref
                class="analyzer-table border border-[color:var(--color-outline)] flex-1 min-h-0"
                style=move || colw_style(&visible_cols(), &col_widths())
            >
                <VirtualScroller
                        viewport_height=640.0
                        fill=true
                        row_height=40.0
                        overscan=8
                        // The header row's own content height. In fill mode
                        // the header lives inside the single scrollport, so
                        // no scrollbar is reserved on it; `overscan=8`
                        // absorbs any residual off-by-a-few-px.
                        header_height=56.0
                        variable_height=false
                        visible_range=visible_range
                        row_min_width="var(--analyzer-row-min-width, 0px)"
                        header=view! {
                            <div class="analyzer-grid-row flex flex-row items-center h-14 text-xs font-semibold uppercase tracking-wider text-[color:var(--color-text-muted)] border-b border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]" role="rowgroup">
                                <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_HQ class="!px-2 justify-center">
                                    {t!(i18n, analyzer_col_hq)}
                                </HeaderCell>
                                <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_ITEM>
                                    {t!(i18n, analyzer_col_item)}
                                </HeaderCell>
                                <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_PROFIT class="justify-end">
                                    <SortHeader
                                        mode=SortMode::Profit
                                        label=t_string!(i18n, analyzer_col_profit).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                </HeaderCell>
                                {move || visible_cols().contains(COL_PROFIT_PER_DAY).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_PROFIT_PER_DAY class="justify-end">
                                        <SortHeader
                                            mode=SortMode::ProfitPerDay
                                            label=t_string!(i18n, analyzer_col_profit_per_day).to_string()
                                            sort_mode
                                            sort_dir
                                        />
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_VELOCITY).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_VELOCITY class="justify-end">
                                        {t!(i18n, analyzer_col_velocity)}
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_DRIFT).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_DRIFT class="justify-end">
                                        {t!(i18n, analyzer_col_drift)}
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_CONFIDENCE).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_CONFIDENCE class="justify-center">
                                        {t!(i18n, analyzer_col_confidence)}
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_ROI).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_ROI class="justify-end">
                                        <SortHeader
                                            mode=SortMode::Roi
                                            label=t_string!(i18n, analyzer_col_roi).to_string()
                                            sort_mode
                                            sort_dir
                                        />
                                    </HeaderCell>
                                })}
                                <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_BUY_PRICE class="justify-end">
                                    {t!(i18n, analyzer_col_buy_price)}
                                </HeaderCell>
                                {move || visible_cols().contains(COL_WORLD).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_WORLD>
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
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_DATACENTER).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_DATACENTER>
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
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_TREND).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_TREND class="flex-col justify-center text-center leading-tight !gap-0">
                                        <span>{t!(i18n, analyzer_col_spark)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_SALES_PER_DAY).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_SALES_PER_DAY class="flex-col justify-center text-center leading-tight !gap-0">
                                        <span>{t!(i18n, analyzer_col_sales_per_day)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_VOLUME_30D).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_VOLUME_30D class="flex-col !items-end text-right leading-tight !gap-0">
                                        <span>{t!(i18n, analyzer_col_volume_30d)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </HeaderCell>
                                })}
                                {move || visible_cols().contains(COL_LAST_SOLD).then(|| view! {
                                    <HeaderCell pane=pane_ref col_widths set_col_widths col=COL_LAST_SOLD class="flex-col !items-start leading-tight !gap-0">
                                        <span>{t!(i18n, analyzer_col_last_sold)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </HeaderCell>
                                })}
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
                                    <div role="cell" class="px-2 py-2 shrink-0 flex items-center justify-center" style="width:var(--colw-hq)">
                                        {if data.inner.sale_summary.hq {
                                            Some(view! { <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">{t!(i18n, analyzer_col_hq)}</span> })
                                        } else {
                                            None
                                        }}
                                    </div>
                                    <div role="cell" class="px-4 py-2 flex flex-row items-center gap-2 shrink-0 min-w-0" style="width:var(--colw-item)">
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
                                    <div role="cell" class="px-3 py-2 shrink-0 text-right flex items-center justify-end" style="width:var(--colw-profit)">
                                        <Gil amount=data.profit />
                                    </div>
                                    {move || visible_cols().contains(COL_PROFIT_PER_DAY).then(|| view! {
                                        <div role="cell" class="px-3 py-2 shrink-0 text-right flex items-center justify-end" style="width:var(--colw-profit_per_day)">
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
                                            <div role="cell" class="px-3 py-2 shrink-0 flex items-center justify-end font-mono tabular-nums" style="width:var(--colw-velocity)">
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
                                                class=format!("px-3 py-2 shrink-0 flex items-center justify-end font-mono tabular-nums {class}")
                                                style="width:var(--colw-drift)"
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
                                            <div role="cell" class="px-3 py-2 shrink-0 flex items-center justify-center" style="width:var(--colw-confidence)">
                                                <span class=format!("text-xs font-semibold {class}")>{label}</span>
                                            </div>
                                        }
                                    })}
                                    {move || visible_cols().contains(COL_ROI).then(|| view! {
                                        <div role="cell" class="px-3 py-2 shrink-0 text-right flex items-center justify-end" style="width:var(--colw-roi)">
                                            <span class=roi_badge_class(row_roi)>
                                                {format!("{row_roi}%")}
                                            </span>
                                        </div>
                                    })}
                                    <div role="cell" class="px-3 py-2 shrink-0 text-right flex items-center justify-end" style="width:var(--colw-buy_price)">
                                        <Gil amount=data.inner.cheapest_price />
                                    </div>
                                    {move || visible_cols().contains(COL_WORLD).then(|| view! {
                                        <div role="cell" class="px-3 py-2 shrink-0 flex items-center min-w-0" style="width:var(--colw-world)">
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
                                        <div role="cell" class="px-3 py-2 shrink-0 flex items-center min-w-0" style="width:var(--colw-datacenter)">
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
                                            <div role="cell" class="px-3 py-2 shrink-0 flex items-center justify-center" style="width:var(--colw-trend)">
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
                                            <div role="cell" class="px-3 py-2 shrink-0 flex items-center justify-center" style="width:var(--colw-sales_per_day)">
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
                                            <div role="cell" class="px-3 py-2 shrink-0 flex items-center justify-end font-mono tabular-nums" style="width:var(--colw-volume_30d)">
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
                                                if days > 0 { format!("{}d ago", days) }
                                                else if hours > 0 { format!("{}h ago", hours) }
                                                else { "just now".to_string() }
                                            })
                                            .unwrap_or_else(|| t_string!(i18n, analyzer_last_sold_never).to_string());
                                        view! {
                                            <div role="cell" class="px-3 py-2 shrink-0 truncate flex items-center" style="width:var(--colw-last_sold)">
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
        </div>
    }.into_any()
}

#[component]
pub fn AnalyzerWorldView() -> impl IntoView {
    let i18n = use_i18n();
    // Seeded here rather than in AnalyzerTable: that lives inside the Suspense
    // closure and remounts on every market refetch, which would keep undoing a
    // filter the user had cleared. A URL with no filter/sort params at all
    // gets the Realistic-flips defaults (as removable chips); a URL carrying
    // any explicit filter is honored verbatim — including no longer getting
    // `next-sale=1d` silently appended.
    seed_query_defaults_when_unfiltered(SEED_SUPPRESSING_PARAMS, REALISTIC_DEFAULT_PARAMS);
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
                    // Title + world picker. Deliberately kept OUTSIDE the
                    // `<Suspense>` below: `AnalyzerTable` (and the sticky bar
                    // it renders) only exists once every resource has
                    // resolved, so a control placed there vanishes behind
                    // `BoxSkeleton` on every load — including a world change,
                    // which is exactly when a user most needs to be able to
                    // change worlds again. Keeping it here means it is always
                    // on screen, load or no load.
                    <div class="flex flex-wrap items-center justify-between gap-3">
                        <h1 class="text-xl sm:text-2xl font-bold text-[color:var(--brand-fg)]">
                            {t!(i18n, flip_finder)}
                        </h1>
                        <AnalyzerWorldNavigator />
                    </div>

                    // Main Content. AnalyzerTable renders a fixed-height pane
                    // (measured against the viewport) whose scroller owns all
                    // scrolling; this wrapper adds no height or overflow.
                    <div>
                        <Suspense fallback=BoxSkeleton>
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

    Effect::new(move |_| {
        if let Some(world) = current_world() {
            let world = world.name;
            let query_map = query.get_untracked();
            // `to_query_string()` already includes the leading `?` when the map
            // is non-empty (and is "" when empty) — don't add another, or the
            // URL becomes `/flip-finder/World??cols=…`, which parses the query
            // key as `?cols` and silently drops the column selection on reload.
            let query = query_map.to_query_string();
            nav(
                &format!("/flip-finder/{world}{query}"),
                NavigateOptions {
                    scroll: false,
                    ..Default::default()
                },
            );
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, analyzer_select_world)}</label>
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
    fn realistic_defaults_match_the_realistic_preset_plus_next_sale() {
        // The seeded set must stay in lockstep with the "Realistic flips"
        // built-in view (saved_views.rs) — same values, plus next-sale.
        // Derived from `built_in_views()` rather than duplicated literals, so
        // editing the preset without editing the seeded defaults fails here.
        let realistic = crate::components::saved_views::built_in_views()
            .into_iter()
            .find(|v| v.name == "analyzer_preset_realistic")
            .expect("the Realistic flips built-in view must exist");
        let mut expected: std::collections::HashMap<String, String> = realistic
            .query
            .trim_start_matches('?')
            .split('&')
            .map(|pair| {
                let (k, v) = pair
                    .split_once('=')
                    .expect("every preset param must be key=value");
                (k.to_string(), v.to_string())
            })
            .collect();
        expected.insert("next-sale".to_string(), "1d".to_string());

        let params: std::collections::HashMap<String, String> = REALISTIC_DEFAULT_PARAMS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(
            params.len(),
            REALISTIC_DEFAULT_PARAMS.len(),
            "seeded params must not repeat a key"
        );
        assert_eq!(params, expected);
        // The humantime values must actually parse, or the filter silently
        // becomes a no-op.
        assert!(humantime::parse_duration("1d").is_ok());
    }

    #[test]
    fn seeding_is_idempotent_because_every_seeded_key_suppresses_seeding() {
        for (key, _) in REALISTIC_DEFAULT_PARAMS {
            assert!(
                SEED_SUPPRESSING_PARAMS.contains(key),
                "seeded key {key} must also suppress seeding, or a reload loops"
            );
        }
    }

    #[test]
    fn suppression_covers_every_filter_but_not_view_config() {
        // Every addable filter + the chip-only filters + sort/dir suppress.
        for id in ADDABLE_FILTERS {
            assert!(SEED_SUPPRESSING_PARAMS.contains(id), "{id} must suppress");
        }
        for id in [
            FILTER_CATEGORY,
            FILTER_WORLD,
            FILTER_DATACENTER,
            "sort",
            "dir",
        ] {
            assert!(SEED_SUPPRESSING_PARAMS.contains(&id), "{id} must suppress");
        }
        // View configuration is NOT a filter: a ?cols= bookmark or a region
        // toggle must still receive the default filters.
        for id in ["cols", "cross", "filter-outliers", "Europe", "Japan"] {
            assert!(
                !SEED_SUPPRESSING_PARAMS.contains(&id),
                "{id} must NOT suppress"
            );
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
}
