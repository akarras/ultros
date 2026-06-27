use crate::analysis::{SaleSummary, roi_badge_class};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world, get_resale_quality, post_sparklines},
    components::{
        add_to_list::AddToList,
        clipboard::*,
        gil::*,
        icon::Icon,
        item_icon::*,
        meta::*,
        query_button::QueryButton,
        realtime_status::RealtimeStatus,
        skeleton::{BoxSkeleton, SingleLineSkeleton},
        sparkline::Sparkline,
        toggle::Toggle,
        tool_help::*,
        toolbar::{Toolbar, ToolbarField, ToolbarPills, ToolbarSpacer},
        tooltip::*,
        virtual_scroller::*,
        world_picker::*,
    },
    error::{AppError, AppResult},
    global_state::LocalWorldData,
    math::filter_outliers_iqr_in_place,
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
/// Profit, ROI, Buy Price) are not in this list — they always render.
/// Order here is the canonical render + serialization order.
const COL_PROFIT_PER_DAY: &str = "profit_per_day";
const COL_WORLD: &str = "world";
const COL_DATACENTER: &str = "datacenter";
const COL_TREND: &str = "trend";
const COL_SALES_PER_DAY: &str = "sales_per_day";
const COL_VOLUME_30D: &str = "volume_30d";
const COL_LAST_SOLD: &str = "last_sold";

const ALL_OPTIONAL_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_WORLD,
    COL_DATACENTER,
    COL_TREND,
    COL_SALES_PER_DAY,
    COL_VOLUME_30D,
    COL_LAST_SOLD,
];

/// Default visible set when `?cols=` is absent from the URL. Once the
/// user explicitly sets the param (even to ""), we respect that exact
/// set instead of falling back to defaults.
const DEFAULT_VISIBLE_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_WORLD,
    COL_DATACENTER,
    COL_TREND,
    COL_SALES_PER_DAY,
    COL_VOLUME_30D,
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
use humantime::{format_duration, parse_duration};
use icondata as i;
use leptos::{either::Either, prelude::*, reactive::wrappers::write::SignalSetter};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_params_map, use_query_map},
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

#[derive(Clone, Debug, PartialEq)]
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

fn median_i32(sorted: &[i32]) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        ((sorted[n / 2 - 1] as i64 + sorted[n / 2] as i64) / 2) as i32
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
    raw.sort_unstable();
    let raw_median = median_i32(&raw);
    let floor = (raw_median as f64 * SNIPER_FRACTION) as i32;

    // 2. Build the clamped vector. If the clamp would remove everything, keep the raw set.
    let mut clamped: Vec<i32> = raw.iter().copied().filter(|p| *p >= floor).collect();
    if clamped.is_empty() {
        clamped = raw;
    }
    let median_price = median_i32(&clamped);
    let min_price = *clamped.first().unwrap_or(&0);
    let max_price = *clamped.last().unwrap_or(&0);

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

#[component]
fn PresetFilterButton(href: &'static str, #[prop(into)] label: String) -> impl IntoView {
    view! {
        <a
            href=href
            class="btn-secondary"
        >
            {label}
        </a>
    }
}

#[component]
fn AnalyzerTable(
    sales_resource: ArcResource<AppResult<RecentSales>>,
    global_cheapest_listings_resource: ArcResource<AppResult<CheapestListings>>,
    world_cheapest_listings_resource: ArcResource<AppResult<CheapestListings>>,
    cross_region: Vec<CheapestListings>,
    worlds: Arc<WorldHelper>,
    world: Signal<String>,
    filter_outliers: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let (realtime_status, set_realtime_status) = signal("connecting".to_string());
    let (last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);

    let profits = Memo::new(move |_| {
        let sales = sales_resource.get().and_then(|r| r.ok())?;
        let global = global_cheapest_listings_resource
            .get()
            .and_then(|r| r.ok())?;
        let world_cheapest = world_cheapest_listings_resource
            .get()
            .and_then(|r| r.ok())?;

        Some(ProfitTable::new(
            sales,
            global,
            world_cheapest,
            cross_region.clone(),
            filter_outliers,
        ))
    });

    let realtime = use_realtime();
    let world_data = worlds.clone();
    let market_subscription = StoredValue::new(None::<RealtimeSubscription>);
    Effect::new(move |_| {
        market_subscription.update_value(|sub| *sub = None);
        let world_name = world.get();
        let Some(realtime) = realtime.clone() else {
            set_realtime_status.set("offline".to_string());
            return;
        };
        let Some(selector) = world_data
            .lookup_world_by_name(&world_name)
            .map(|world| ultros_api_types::world_helper::AnySelector::from(&world))
        else {
            return;
        };

        let filter = ultros_api_types::websocket::FilterPredicate::World(selector);
        let sub = realtime.subscribe_market(
            filter,
            ultros_api_types::websocket::SocketMessageType::Sales,
            move |message| {
                use ultros_api_types::websocket::ServerClient;
                match message {
                    ServerClient::Subscribed { .. } => {
                        set_realtime_status.set("live".to_string());
                    }
                    ServerClient::Sales(_) => {
                        set_realtime_status.set("live".to_string());
                        set_last_update_at.set(Some(chrono::Utc::now()));
                        sales_resource.refetch();
                    }
                    ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                        set_realtime_status.set("reconnecting".to_string());
                        set_last_update_at.set(Some(chrono::Utc::now()));
                        sales_resource.refetch();
                        global_cheapest_listings_resource.refetch();
                        world_cheapest_listings_resource.refetch();
                    }
                    _ => {}
                }
            },
        );
        market_subscription.set_value(Some(sub));
    });

    let items = &tracked_data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_profit_per_day, set_minimum_profit_per_day) = query_signal::<i32>("ppd");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (max_predicted_time, set_max_predicted_time) = query_signal::<String>("next-sale");
    let (world_filter, set_world_filter) = query_signal::<String>("world");
    let (datacenter_filter, set_datacenter_filter) = query_signal::<String>("datacenter");
    let (tax_enabled, set_tax_enabled) = query_signal::<bool>("tax");
    let (minimum_sales, set_minimum_sales) = query_signal::<usize>("sales");
    let (category_filter, set_category_filter) = query_signal::<i32>("category");
    let (max_purchase_price, set_max_purchase_price) = query_signal::<i32>("max-price");
    let (min_buy_price, set_min_buy_price) = query_signal::<i32>("min-buy");
    let (show_suspicious, set_show_suspicious) = query_signal::<bool>("show-suspicious");
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    let visible_cols = Memo::new(move |_| parse_visible_cols(cols_param().as_deref()));
    let show_suspicious_active = Memo::new(move |_| show_suspicious().unwrap_or(false));
    let show_columns_picker = RwSignal::new(false);

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
    let lookup_world = Memo::new(move |_| {
        Some(AnySelector::from(
            &world_clone.lookup_world_by_name(&world())?,
        ))
    });

    let predicted_time =
        Memo::new(move |_| max_predicted_time().and_then(|d| parse_duration(d.as_str()).ok()));
    let predicted_time_string = Memo::new(move |_| {
        predicted_time()
            .map(|duration| format_duration(duration).to_string())
            .unwrap_or("---".to_string())
    });

    let (last_sold_within, set_last_sold_within) = query_signal::<String>("last-sold");
    let show_more = RwSignal::new(false);
    let last_sold_duration =
        Memo::new(move |_| last_sold_within().and_then(|d| parse_duration(d.as_str()).ok()));
    let last_sold_string = Memo::new(move |_| {
        last_sold_duration()
            .map(|d| format_duration(d).to_string())
            .unwrap_or("---".to_string())
    });

    // Accumulating CH enrichment (quality + sparkline + settled), grown by the
    // visible-window fetch effect below; never wholesale-replaced (except on a
    // world change). Cells + the suspicious filter read it reactively.
    let enrichment = RwSignal::new(EnrichmentMaps::default());

    let sorted_data = Memo::new(move |_| {
        let include_tax = tax_enabled().unwrap_or(true);
        let Some(profits) = profits.get() else {
            return vec![];
        };
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
                let return_on_investment = if data.cheapest_price > 0 {
                    ((profit as f32 / data.cheapest_price as f32) * 100.0) as i32
                } else {
                    0
                };
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

        match sort_mode().unwrap_or(SortMode::Roi) {
            SortMode::Roi => sorted_data.sort_by_key(|data| Reverse(data.return_on_investment)),
            SortMode::Profit => sorted_data.sort_by_key(|data| Reverse(data.profit)),
            SortMode::ProfitPerDay => sorted_data.sort_by_key(|data| Reverse(data.profit_per_day)),
        }
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
        <div class="flex flex-col gap-6">
            // Primary filter toolbar
            <Toolbar>
                <ToolbarField label=t_string!(i18n, analyzer_filter_profit_min_label).to_string()>
                    <input
                        class="input input-sm w-32"
                        min=0
                        max=100000
                        step=1000
                        placeholder=t_string!(i18n, analyzer_placeholder_100000)
                        type="number"
                        prop:value=minimum_profit
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(profit) = value.parse::<i32>() {
                                set_minimum_profit(Some(profit));
                            } else if value.is_empty() {
                                set_minimum_profit(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, analyzer_filter_roi_min_label).to_string()>
                    <input
                        class="input input-sm w-28"
                        min=0
                        max=100000
                        step=10
                        placeholder=t_string!(i18n, analyzer_placeholder_200)
                        type="number"
                        prop:value=minimum_roi
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(roi) = value.parse::<i32>() {
                                set_minimum_roi(Some(roi));
                            } else if value.is_empty() {
                                set_minimum_roi(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, analyzer_filter_sales_min_label).to_string()>
                    <input
                        class="input input-sm w-24"
                        min=0
                        max=6
                        step=1
                        placeholder=t_string!(i18n, analyzer_placeholder_0_to_6)
                        title=t_string!(i18n, analyzer_tooltip_sales_min)
                        type="number"
                        prop:value=minimum_sales
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(sales) = value.parse::<usize>() {
                                set_minimum_sales(Some(sales.min(6)));
                            } else if value.is_empty() {
                                set_minimum_sales(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, analyzer_filter_buy_max_label).to_string()>
                    <input
                        class="input input-sm w-32"
                        min=0
                        step=1000
                        placeholder=t_string!(i18n, analyzer_placeholder_500000)
                        type="number"
                        prop:value=max_purchase_price
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(p) = value.parse::<i32>() {
                                set_max_purchase_price(Some(p));
                            } else if value.is_empty() {
                                set_max_purchase_price(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, analyzer_filter_category_label).to_string()>
                    <select
                        class="input input-sm"
                        on:change=move |ev| {
                            let val = event_target_value(&ev);
                            if let Ok(id) = val.parse::<i32>() {
                                set_category_filter(Some(id));
                            } else {
                                set_category_filter(None);
                            }
                        }
                        prop:value=move || category_filter().map(|c| c.to_string()).unwrap_or_default()
                    >
                        <option value="">{t!(i18n, analyzer_all_categories)}</option>
                        {
                            let mut categories = tracked_data().item_search_categorys
                                .iter()
                                .filter(|(_, cat)| !cat.name.is_empty())
                                .map(|(id, cat)| (id.0, cat.name.clone()))
                                .collect::<Vec<_>>();
                            categories.sort_by(|a, b| a.1.cmp(&b.1));
                            categories.into_iter().map(|(id, name)| {
                                view! { <option value=id.to_string() selected=move || category_filter() == Some(id)>{name}</option> }
                            }).collect_view()
                        }
                    </select>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, analyzer_filter_prices_label).to_string()>
                    // tax_enabled semantics: Some(true) = post-tax (5% deducted), None/Some(false) = pre-tax
                    // Default unwrap_or(true) means post-tax is the default
                    <ToolbarPills>
                        <button
                            aria-pressed=move || if tax_enabled().unwrap_or(true) { "false" } else { "true" }
                            on:click=move |_| set_tax_enabled(Some(false))
                        >
                            "Pre-tax"
                        </button>
                        <button
                            aria-pressed=move || if tax_enabled().unwrap_or(true) { "true" } else { "false" }
                            on:click=move |_| set_tax_enabled(Some(true))
                        >
                            "Post-tax"
                        </button>
                    </ToolbarPills>
                </ToolbarField>
                <ToolbarSpacer />
                <button
                    class="btn-secondary flex items-center gap-2"
                    on:click=move |_| show_columns_picker.update(|v| *v = !*v)
                    aria-expanded=move || show_columns_picker.get().to_string()
                >
                    <Icon icon=i::FaTableColumnsSolid />
                    {t!(i18n, analyzer_columns_button)}
                </button>
                <button
                    class="btn-secondary flex items-center gap-2"
                    on:click=move |_| show_more.update(|v| *v = !*v)
                >
                    <Icon icon=i::FaFilterSolid />
                    {move || if show_more.get() { "Fewer Filters" } else { "More Filters" }}
                </button>
            </Toolbar>

            // Columns picker (URL-persisted via ?cols=)
            {move || show_columns_picker.get().then(|| {
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
                        c if c == COL_PROFIT_PER_DAY => t_string!(i18n, analyzer_col_profit_per_day).to_string(),
                        c if c == COL_WORLD => t_string!(i18n, analyzer_col_world).to_string(),
                        c if c == COL_DATACENTER => t_string!(i18n, analyzer_col_datacenter).to_string(),
                        c if c == COL_TREND => t_string!(i18n, analyzer_col_spark).to_string(),
                        c if c == COL_SALES_PER_DAY => t_string!(i18n, analyzer_col_sales_per_day).to_string(),
                        c if c == COL_VOLUME_30D => t_string!(i18n, analyzer_col_volume_30d).to_string(),
                        c if c == COL_LAST_SOLD => t_string!(i18n, analyzer_col_last_sold).to_string(),
                        _ => String::new(),
                    }
                };
                view! {
                    <div class="panel px-4 py-3 rounded-lg flex flex-row flex-wrap items-center gap-x-5 gap-y-2 text-sm">
                        <span class="font-semibold text-[color:var(--brand-fg)]">
                            {t!(i18n, analyzer_columns_picker_label)}
                        </span>
                        {ALL_OPTIONAL_COLS.iter().map(|col| {
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
                        }).collect_view()}
                        <button
                            class="ml-auto text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                            on:click=move |_| set_cols_param.set(None)
                        >
                            {t!(i18n, analyzer_columns_picker_reset)}
                        </button>
                    </div>
                }
            })}

            // Secondary filter toolbar (expanded)
            {move || show_more.get().then(|| view! {
                <Toolbar>
                    <ToolbarField label=t_string!(i18n, analyzer_filter_profit_per_day_min_label).to_string()>
                        <input
                            class="input input-sm w-32"
                            min=0
                            max=100000
                            step=1000
                            placeholder=t_string!(i18n, placeholder_eg_10000)
                            type="number"
                            prop:value=minimum_profit_per_day
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(profit) = value.parse::<i32>() {
                                    set_minimum_profit_per_day(Some(profit));
                                } else if value.is_empty() {
                                    set_minimum_profit_per_day(None);
                                }
                            }
                        />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, analyzer_filter_min_buy_label).to_string()>
                        <input
                            class="input input-sm w-32"
                            min=0
                            step=1000
                            placeholder=t_string!(i18n, analyzer_placeholder_5000)
                            type="number"
                            prop:value=min_buy_price
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(p) = value.parse::<i32>() {
                                    set_min_buy_price(Some(p));
                                } else if value.is_empty() {
                                    set_min_buy_price(None);
                                }
                            }
                        />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, analyzer_filter_max_sale_time_label).to_string()>
                        <input
                            class="input input-sm w-32"
                            placeholder=t_string!(i18n, analyzer_placeholder_7d_12h)
                            title=t_string!(i18n, analyzer_tooltip_duration_format)
                            prop:value=move || max_predicted_time().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_max_predicted_time(Some(value));
                            }
                        />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, analyzer_last_sold_within).to_string()>
                        <input
                            class="input input-sm w-32"
                            placeholder=t_string!(i18n, analyzer_placeholder_7d)
                            title=t_string!(i18n, analyzer_tooltip_duration_format)
                            prop:value=move || last_sold_within().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_last_sold_within(Some(value));
                            }
                        />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, analyzer_show_suspicious).to_string()>
                        <Toggle
                            checked=Signal::derive(move || show_suspicious_active.get())
                            set_checked=SignalSetter::map(move |v: bool| set_show_suspicious(v.then_some(true)))
                            checked_label=Oco::Owned(t_string!(i18n, analyzer_show_suspicious).to_string())
                            unchecked_label=Oco::Owned(t_string!(i18n, analyzer_show_suspicious).to_string())
                        />
                    </ToolbarField>
                </Toolbar>
            })}

            // Results summary
            <div class="panel px-4 py-3 flex flex-col md:flex-row md:items-center gap-3 md:gap-0 md:justify-between">
                <div class="text-sm text-[color:var(--color-text)] flex flex-wrap items-center gap-3">
                    <div>
                        <span class="text-brand-300 font-semibold">{move || sorted_data().len()}</span> {t!(i18n, analyzer_results)}
                    </div>
                    <RealtimeStatus
                        status=realtime_status
                        last_update=last_update_at
                    />
                </div>
                <div class="flex flex-wrap gap-2">
                    {move || {
                        let mut chips: Vec<_> = Vec::new();
                        if let Some(p) = minimum_profit() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_profit_gte)} <Gil amount=p />
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_profit(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(p) = minimum_profit_per_day() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_profit_per_day_gte)} <Gil amount=p />
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_profit_per_day(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(cat_id) = category_filter() {
                            let cat_name = tracked_data()
                                .item_search_categorys
                                .get(&xiv_gen::ItemSearchCategoryId(cat_id))
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| format!("Category {}", cat_id));
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_category_label)} {cat_name}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_category_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(sales) = minimum_sales() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_sales_gte)} {sales}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_sales(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(roi) = minimum_roi() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_roi_gte)} {format!("{roi}%")}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_roi(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(p) = max_purchase_price() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Budget ≤ " <Gil amount=p />
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_max_purchase_price(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(p) = min_buy_price() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_min_buy_gte)} <Gil amount=p />
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_min_buy_price(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(_ns) = max_predicted_time() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_next_sale_lte)} {predicted_time_string()}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_max_predicted_time(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if last_sold_within().is_some() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_last_sold_lte)} {last_sold_string()}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_last_sold_within(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(w) = world_filter() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_world_label)} {w.clone()}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_world_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(dc) = datacenter_filter() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_datacenter_label)} {dc.clone()}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_datacenter_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if chips.is_empty() {
                            Either::Left(view! { <span class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, analyzer_no_active_filters)}</span> })
                        } else {
                            Either::Right(view! { <>{chips}</> })
                        }
                    }}
                </div>
                <button aria-label=t_string!(i18n, aria_clear_all_filters) class="text-sm text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] self-start md:self-auto" on:click=move |_| {
                    set_minimum_profit(None);
                    set_minimum_profit_per_day(None);
                    set_minimum_roi(None);
                    set_max_predicted_time(None);
                    set_world_filter(None);
                    set_datacenter_filter(None);
                    set_minimum_sales(None);
                    set_category_filter(None);
                    set_max_purchase_price(None);
                    set_min_buy_price(None);
                    set_last_sold_within(None);
                    set_show_suspicious(None);
                }>
                    {t!(i18n, analyzer_clear_all)}
                </button>
            </div>

            // Results table
            <div class="rounded-lg overflow-x-auto border border-[color:var(--color-outline)] content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                        viewport_height=720.0
                        row_height=40.0
                        overscan=8
                        header_height=56.0
                        variable_height=false
                        visible_range=visible_range
                        header=view! {
                            <div class="flex flex-row items-center h-14 text-xs font-semibold uppercase tracking-wider text-[color:var(--color-text-muted)] border-b border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]" role="rowgroup">
                                <div role="columnheader" class="w-[44px] px-2 text-center">
                                    {t!(i18n, analyzer_col_hq)}
                                </div>
                                <div role="columnheader" class="flex-1 min-w-[14rem] px-3">
                                    {t!(i18n, analyzer_col_item)}
                                </div>
                                <div role="columnheader" class="w-28 px-3 text-right">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="profit"
                                    >
                                        <div class="flex items-center gap-2">
                                            {t!(i18n, analyzer_col_profit)}
                                            {move || {
                                                (sort_mode() == Some(SortMode::Profit))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                {move || visible_cols().contains(COL_PROFIT_PER_DAY).then(|| view! {
                                    <div role="columnheader" class="w-28 px-3 py-2">
                                        <QueryButton
                                            class="!text-brand-300 hover:text-brand-200"
                                            active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                            key="sort"
                                            value="profit-per-day"
                                        >
                                            <div class="flex items-center gap-2">
                                                {t!(i18n, analyzer_col_profit_per_day)}
                                                {move || {
                                                    (sort_mode() == Some(SortMode::ProfitPerDay))
                                                        .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                                }}
                                            </div>
                                        </QueryButton>
                                    </div>
                                })}
                                <div role="columnheader" class="w-28 px-3 py-2">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="roi"
                                        default=true
                                    >
                                        <div class="flex items-center gap-2">
                                            {t!(i18n, analyzer_col_roi)}
                                            {move || {
                                                (sort_mode() == Some(SortMode::Roi))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                <div role="columnheader" class="w-28 px-3 py-2">
                                    {t!(i18n, analyzer_col_buy_price)}
                                </div>
                                {move || visible_cols().contains(COL_WORLD).then(|| view! {
                                    <div role="columnheader" class="w-28 px-3 py-2 flex flex-row gap-2 hidden lg:flex">
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
                                    <div role="columnheader" class="w-28 px-3 py-2 flex flex-row gap-2 hidden xl:flex">
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
                                    <div role="columnheader" class="w-[100px] px-3 py-2 hidden md:flex flex-col items-center text-center leading-tight">
                                        <span>{t!(i18n, analyzer_col_spark)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_SALES_PER_DAY).then(|| view! {
                                    <div role="columnheader" class="w-[88px] px-3 py-2 hidden md:flex flex-col items-end text-right leading-tight">
                                        <span>{t!(i18n, analyzer_col_sales_per_day)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_VOLUME_30D).then(|| view! {
                                    <div role="columnheader" class="w-[88px] px-3 py-2 hidden md:flex flex-col items-end text-right leading-tight">
                                        <span>{t!(i18n, analyzer_col_volume_30d)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
                                })}
                                {move || visible_cols().contains(COL_LAST_SOLD).then(|| view! {
                                    <div role="columnheader" class="w-28 px-3 py-2 hidden md:flex flex-col leading-tight">
                                        <span>{t!(i18n, analyzer_col_last_sold)}</span>
                                        <span class="text-[10px] font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full">
                                            {move || world()}
                                        </span>
                                    </div>
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
                            let item = items
                                .get(&ItemId(item_id))
                                .map(|item| item.name.as_str())
                                .unwrap_or_default();
                            let icon_loading = if index < 20 { "eager" } else { "" };
                            let classes = if (index % 2) == 0 {
                                "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                            } else {
                                "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                            };
                            view! {
                                <div class=classes role="row-group">
                                    <div role="cell" class="px-2 py-2 w-[40px] flex items-center justify-center">
                                        {if data.inner.sale_summary.hq {
                                            Some(view! { <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">{t!(i18n, analyzer_col_hq)}</span> })
                                        } else {
                                            None
                                        }}
                                    </div>
                                    <div role="cell" class="px-4 py-2 flex flex-row flex-1 min-w-[14rem] items-center gap-2">
                                        <a
                                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                            href=format!("/item/{}/{item_id}", world())
                                        >
                                            <div class="shrink-0">
                                                <ItemIcon item_id icon_size=IconSize::Small loading=icon_loading />
                                            </div>
                                            {item}
                                        </a>
                                        <AddToList item_id />
                                        <Clipboard clipboard_text=item.to_string() />
                                    </div>
                                    <div role="cell" class="px-3 py-2 w-28 text-right flex items-center justify-end">
                                        <Gil amount=data.profit />
                                    </div>
                                    {move || visible_cols().contains(COL_PROFIT_PER_DAY).then(|| view! {
                                        <div role="cell" class="px-3 py-2 w-28 text-right flex items-center justify-end">
                                            <Gil amount=data.profit_per_day />
                                        </div>
                                    })}
                                    <div role="cell" class="px-3 py-2 w-28 text-right flex items-center justify-end">
                                        <span class={roi_badge_class(data.return_on_investment)}>
                                            {format!("{}%", data.return_on_investment)}
                                        </span>
                                    </div>
                                    <div role="cell" class="px-3 py-2 w-28 text-right flex items-center justify-end">
                                        <Gil amount=data.inner.cheapest_price />
                                    </div>
                                    {move || visible_cols().contains(COL_WORLD).then(|| view! {
                                        <div role="cell" class="px-3 py-2 w-28 hidden lg:block flex items-center">
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
                                        <div role="cell" class="px-3 py-2 w-28 hidden xl:block flex items-center">
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
                                    {
                                        // Hoist Copy values out so each per-column `move ||` closure
                                        // can capture them without contending for `data.inner` (Arc, not Copy).
                                        let row_key = (data.inner.sale_summary.item_id, data.inner.sale_summary.hq);
                                        let row_cheapest_price = data.inner.cheapest_price;
                                        let row_days_since = data.inner.sale_summary.days_since_last_sale;
                                        view! {
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
                                                let maps = enrichment.get();
                                                let inner = match (maps.quality_for(&row_key), maps.is_settled(&row_key)) {
                                                    (Some(q), _) => view! { {format!("{:.1}", q.sales_per_day)} }.into_any(),
                                                    (None, true) => view! { "—" }.into_any(),
                                                    (None, false) => view! { <SingleLineSkeleton /> }.into_any(),
                                                };
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums">
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
                                                        if days > 0 { format!("{}d ago", days) }
                                                        else if hours > 0 { format!("{}h ago", hours) }
                                                        else { "just now".to_string() }
                                                    })
                                                    .unwrap_or_else(|| t_string!(i18n, analyzer_last_sold_never).to_string());
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-28 truncate hidden md:block flex items-center">
                                                        {last}
                                                    </div>
                                                }
                                            })}
                                        }
                                    }
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
    let params = use_params_map();
    let world = Signal::derive(move || params.with(|p| p.get("world").clone()).unwrap_or_default());
    let sales = ArcResource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            get_recent_sales_for_world(&world.ok_or(AppError::ParamMissing)?).await
        },
    );

    let world_cheapest_listings = ArcResource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            let world = world.ok_or(AppError::ParamMissing)?;
            get_cheapest_listings(&world).await
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

    let global_cheapest_listings = ArcResource::new(region, move |region| async move {
        get_cheapest_listings(region?.as_str()).await
    });

    let (cross_region_enabled, set_cross_region_enabled) = query_signal::<bool>("cross");
    let (filter_outliers, set_filter_outliers) = query_signal::<bool>("filter-outliers");
    let connected_regions = &["Europe", "Japan", "North-America", "Oceania"];
    let query = use_query_map();

    let enabled_regions = move || {
        let map = query();
        connected_regions
            .iter()
            .filter(|region| map.get(region).map(|value| value == "true").unwrap_or(true))
            .collect::<Vec<_>>()
    };

    let cross_region = ArcResource::new(
        move || (cross_region_enabled(), region(), enabled_regions()),
        move |(enabled, region, enabled_regions)| async move {
            let region = region?;
            if enabled.unwrap_or_default() && connected_regions.contains(&region.as_str()) {
                Ok(futures::future::join_all(
                    connected_regions
                        .iter()
                        .filter(|r| **r != region.as_str())
                        .filter(|r| enabled_regions.contains(r))
                        .map(|region| get_cheapest_listings(region)),
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

    view! {
        <div class="main-content p-2 sm:p-6">
            <MetaTitle title=move || t_string!(i18n, analyzer_meta_title).to_string().replace("%world%", &world()) />
            <div class="flex flex-col gap-8">
                    <ToolHeader
                        title=t_string!(i18n, flip_finder).to_string()
                        summary=t_string!(i18n, analyzer_tool_summary).to_string()
                        context=t_string!(i18n, analyzer_tool_context).to_string()
                        help_href="/help/flip-finder"
                        help_body=t_string!(i18n, analyzer_tool_help).to_string()
                    />

                    // Controls Section
                    <div class="panel p-4 sm:p-6 rounded-2xl">
                        <div class="flex flex-col gap-4">
                            <MetaDescription text=move || {
                                t_string!(i18n, analyzer_meta_desc).to_string().replace("%world%", &world())
                            } />

                            // World Navigator
                            <div class="flex flex-col md:flex-row gap-4 items-center">
                                <AnalyzerWorldNavigator />
                                <div class="flex flex-col gap-2">
                                    <Toggle
                                        checked=Signal::derive(move || {
                                            cross_region_enabled().unwrap_or_default()
                                        })
                                        set_checked=SignalSetter::map(move |val: bool| set_cross_region_enabled(
                                            val.then_some(true),
                                        ))
                                        checked_label=Oco::Owned(t_string!(i18n, analyzer_cross_region_enabled).to_string())
                                        unchecked_label=Oco::Owned(t_string!(i18n, analyzer_cross_region_disabled).to_string())
                                    />
                                    <Toggle
                                        checked=Signal::derive(move || {
                                            filter_outliers().unwrap_or_default()
                                        })
                                        set_checked=SignalSetter::map(move |val: bool| set_filter_outliers(
                                            val.then_some(true),
                                        ))
                                        checked_label=Oco::Owned(t_string!(i18n, analyzer_filter_outliers_enabled).to_string())
                                        unchecked_label=Oco::Owned(t_string!(i18n, analyzer_filter_outliers_disabled).to_string())
                                    />

                                    <div
                                        class="flex flex-wrap gap-2"
                                        class:hidden=move || {
                                            !cross_region_enabled().unwrap_or_default()
                                        }
                                    >
                                        {move || {
                                            region()
                                                .map(|region| move || {
                                                    connected_regions
                                                        .iter()
                                                        .filter(|r| **r != region.as_str())
                                                        .map(|region| {
                                                            let (enabled, set_enabled) = query_signal::<
                                                                bool,
                                                            >(region.to_string());
                                                            view! {
                                                                <Toggle
                                                                    checked=Signal::derive(move || enabled().unwrap_or(true))
                                                                    set_checked=SignalSetter::map(move |checked: bool| {
                                                                        set_enabled(Some(checked));
                                                                    })
                                                                    checked_label=t_string!(i18n, analyzer_region_enabled).to_string().replace("%region%", region)
                                                                    unchecked_label=t_string!(i18n, analyzer_region_disabled).to_string().replace("%region%", region)
                                                                />
                                                            }
                                                        })
                                                        .collect::<Vec<_>>()
                                                })
                                                .ok()
                                        }}
                                    </div>
                                </div>
                            </div>

                            // Preset Filters
                            <div class="flex flex-wrap gap-4">
                                <PresetFilterButton
                                    href="?min-buy=5000&last-sold=7d&roi=30&sort=profit-per-day"
                                    label=t_string!(i18n, analyzer_preset_realistic).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=100000&last-sold=14d&roi=20&sort=profit"
                                    label=t_string!(i18n, analyzer_preset_big_ticket).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=1000&last-sold=3d&sort=profit-per-day"
                                    label=t_string!(i18n, analyzer_preset_volume).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=1000&last-sold=7d&roi=300&profit=0&sort=profit"
                                    label=t_string!(i18n, analyzer_preset_300_return).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=10000&last-sold=1M&roi=500&profit=200000"
                                    label=t_string!(i18n, analyzer_preset_500_return).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=1000&last-sold=30d&profit=100000"
                                    label=t_string!(i18n, analyzer_preset_100k_profit).to_string()
                                />
                            </div>
                            <details class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)] open:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]">
                                <summary class="cursor-pointer select-none px-3 py-2 text-sm font-semibold text-[color:var(--brand-fg)] hover:text-[color:var(--color-text)]">
                                    {t!(i18n, analyzer_calc_title)}
                                </summary>
                                <div class="px-3 pb-3 pt-1 flex flex-col gap-2">
                                    <code class="text-sm text-brand-300 whitespace-normal break-words">
                                        {t!(i18n, analyzer_calc_formula)}
                                    </code>
                                    <p class="text-sm text-[color:var(--color-text-muted)] leading-relaxed">
                                        {t!(i18n, analyzer_calc_details)}
                                    </p>
                                    <div class="flex flex-wrap gap-2 pt-1">
                                        <AssumptionBadge text=t_string!(i18n, analyzer_assumption_cross_region).to_string() />
                                        <AssumptionBadge text=t_string!(i18n, analyzer_assumption_hq_nq).to_string() />
                                    </div>
                                </div>
                            </details>
                        </div>
                    </div>

                    // Main Content
                    <div class="min-h-screen">
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
                                    (Some(_), Some(_), Some(_)) => {
                                        let sales = sales.clone();
                                        let global = global_cheapest_listings.clone();
                                        let world_cheapest = world_cheapest.clone();
                                        Either::Left(

                                            view! {
                                                <AnalyzerTable
                                                    sales_resource=sales
                                                    global_cheapest_listings_resource=global
                                                    world_cheapest_listings_resource=world_cheapest
                                                    cross_region
                                                    worlds
                                                    world=world
                                                    filter_outliers=filter_outliers().unwrap_or(false)
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

                    // Features Grid
                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaMoneyBillTrendUpSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, analyzer_feature_profit_tracking)}</h3>
                            <p class="text-gray-300">
                                {t!(i18n, analyzer_feature_profit_tracking_desc)}
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaChartLineSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, analyzer_feature_market_analysis)}</h3>
                            <p class="text-gray-300">
                                {t!(i18n, analyzer_feature_market_analysis_desc)}
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaFilterSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, analyzer_feature_custom_filters)}</h3>
                            <p class="text-gray-300">
                                {t!(i18n, analyzer_feature_custom_filters_desc)}
                            </p>
                        </div>
                    </div>

                    // Tips Section
                    <div class="panel p-6 rounded-2xl">
                        <h2 class="text-xl font-bold text-brand-300 mb-4">{t!(i18n, analyzer_tips_title)}</h2>
                        <ul class="list-disc list-inside text-gray-300 space-y-2">
                            <li>
                                {t!(i18n, analyzer_tip_1)}
                            </li>
                            <li>
                                {t!(i18n, analyzer_tip_2)}
                            </li>
                            <li>{t!(i18n, analyzer_tip_3)}</li>
                            <li>
                                {t!(i18n, analyzer_tip_4)}
                            </li>
                        </ul>
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
        assert_eq!(median_i32(&[100, 200, 300, 400, 500]), 300);
        assert_eq!(median_i32(&[100, 110, 120, 130, 140]), 120);
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
}
