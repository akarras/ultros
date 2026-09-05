use crate::analysis::{SaleSummary, format_duration_short, roi_badge_class};
use crate::global_state::xiv_data::tracked_data;
use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_to_list::AddToList,
        clipboard::*,
        control_bar::{ControlBar, FilterOption},
        filter_chip::FilterChip,
        gil::*,
        icon::Icon,
        item_icon::*,
        meta::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortHeader, cmp_none_last},
        tool_help::*,
        virtual_scroller::*,
        world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
    i18n::*,
    query_defaults::{DEFAULT_MAX_SALE_TIME, filter_query_signal, seed_query_default},
    routes::world_nav::world_nav_url,
    ws::realtime::use_realtime,
};
use chrono::{Duration, Utc};
use humantime::parse_duration;
use icondata as i;
use leptos::{either::Either, prelude::*};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_location, use_navigate, use_params_map, use_query_map},
};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
};
use xiv_gen::ItemId;

/// Intern a category id as a `&'static str` token for
/// [`FilterChip`](crate::components::filter_chip::FilterChip)'s
/// `(&'static str, String)` options contract.
///
/// `item_search_categorys` is a small, fixed-size table read from the
/// process-lifetime game data (`xiv_gen_db::data()`), so the set of ids ever
/// asked for here is bounded — each one is leaked exactly once and cached,
/// never per-render, so this cannot grow unbounded over a long session.
fn category_id_token(id: i32) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<i32, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("category token cache poisoned");
    guard
        .entry(id)
        .or_insert_with(|| Box::leak(id.to_string().into_boxed_str()))
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
struct VendorProfitKey {
    item_id: i32,
    hq: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct VendorProfitData {
    item_id: i32,
    vendor_price: i32,
    market_price: i32,
    sale_summary: Option<SaleSummary>,
}

#[derive(Clone, Debug, PartialEq)]
struct CalculatedVendorProfitData {
    inner: Arc<VendorProfitData>,
    profit: i32,
    return_on_investment: i32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
    VendorPrice,
    MarketPrice,
    SaleTime,
}

#[derive(Clone, Debug)]
struct VendorProfitTable(Vec<Arc<VendorProfitData>>);

fn compute_summary(sale: SaleData) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;
    let min_price = sales
        .iter()
        .map(|price| price.price_per_unit)
        .min()
        .unwrap_or_default();
    let max_price = sales
        .iter()
        .map(|price| price.price_per_unit)
        .max()
        .unwrap_or_default();
    let avg_price = (sales
        .iter()
        .map(|price| price.price_per_unit as i64)
        .sum::<i64>()
        / sales.len() as i64) as i32;
    let t = sales
        .last()
        .map(|last| (last.sale_date - now).num_milliseconds().abs() / sales.len() as i64);
    let avg_sale_duration = t.map(Duration::milliseconds);
    let days_since_last_sale = sales
        .first()
        .map(|first| Duration::milliseconds((now - first.sale_date).num_milliseconds().max(0)));
    let mut prices = sales
        .iter()
        .map(|price| price.price_per_unit)
        .collect::<Vec<_>>();
    // ⚡ Bolt: Optimization: Use select_nth_unstable instead of sort_unstable for median calculation.
    let median_price = match prices.as_mut_slice() {
        [] => 0,
        values if values.len() % 2 == 1 => {
            let len = values.len();
            let (_, &mut median, _) = values.select_nth_unstable(len / 2);
            median
        }
        values => {
            let mid = values.len() / 2;
            let (left, &mut mid_val, _) = values.select_nth_unstable(mid);
            let mid_left_val = *left.iter().max().unwrap();
            ((mid_val as i64 + mid_left_val as i64) / 2) as i32
        }
    };
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

/// Ratio of a current market listing to the item's own median recent sale
/// price above which that listing is treated as unachievable.
///
/// This page ranks by ROI against the cheapest *listing*, but a listing only
/// pays out if somebody actually buys it. Gil-trader and troll listings sit
/// orders of magnitude above what an item really clears for, and because they
/// inflate ROI the most they sort straight to the top.
///
/// Measured against a live Gilgamesh snapshot (10,289 NQ items holding both a
/// cheapest listing and recent sales): the median listing sits at 0.99x the
/// item's median recent sale and the 95th percentile at 10.2x, but the 99th is
/// 433x and the 99.9th is 83,091x — a clearly separate population. 50x leaves
/// five times normal variation intact while dropping 2.1% of rows, and the
/// junk actually observed on the page ran 1,000x-150,000x.
const SUSPICIOUS_PRICE_MULTIPLE: i64 = 50;

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_PROFIT: &str = "profit";
const FILTER_ROI: &str = "roi";
const FILTER_SALES: &str = "sales";
const FILTER_NEXT_SALE: &str = "next-sale";
const FILTER_CATEGORY: &str = "category";
const FILTER_TAX: &str = "tax";
const FILTER_SUSPICIOUS: &str = "show-suspicious";

/// Filters the `+ Filter` menu can add, in the old toolbar's left-to-right
/// order.
const ADDABLE_FILTERS: &[&str] = &[
    FILTER_PROFIT,
    FILTER_ROI,
    FILTER_SALES,
    FILTER_NEXT_SALE,
    FILTER_CATEGORY,
    FILTER_TAX,
    FILTER_SUSPICIOUS,
];

/// Whether a row's market price is implausible relative to what the item
/// actually sells for.
///
/// Rows we cannot judge — no recent sales, or a degenerate median — are
/// *kept*, matching the server-side `ResaleQualityFilter` rule that a row
/// without enrichment is shown rather than penalized. The page's existing
/// "Sales (min)" filter is the control for requiring sale history outright.
fn is_suspicious_market_price(market_price: i32, sale_summary: Option<&SaleSummary>) -> bool {
    let Some(summary) = sale_summary else {
        return false;
    };
    if summary.num_sold == 0 || summary.median_price <= 0 {
        return false;
    }
    market_price as i64 > summary.median_price as i64 * SUSPICIOUS_PRICE_MULTIPLE
}

// Add FromStr and ToString implementations for SortMode
impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            "vendor-price" => Ok(SortMode::VendorPrice),
            "market-price" => Ok(SortMode::MarketPrice),
            "sale-time" => Ok(SortMode::SaleTime),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Roi => "roi",
            SortMode::Profit => "profit",
            SortMode::VendorPrice => "vendor-price",
            SortMode::MarketPrice => "market-price",
            SortMode::SaleTime => "sale-time",
        };
        f.write_str(val)
    }
}

/// Profit, ROI and market price read best-first descending — the biggest
/// margin, return, or payout. Vendor price is a cost and avg sale time a
/// wait, so a fresh click on those starts ascending: cheapest to buy in,
/// fastest to move out.
impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Roi
    }

    fn default_dir(self) -> SortDir {
        match self {
            SortMode::Roi | SortMode::Profit | SortMode::MarketPrice => SortDir::Desc,
            SortMode::VendorPrice | SortMode::SaleTime => SortDir::Asc,
        }
    }
}

/// Sort rows in place. Extracted from the `sorted_data` memo so the ordering
/// is unit-testable without a reactive runtime. Rows without sale history
/// sort last under Avg Sale Time in both directions.
fn sort_rows(rows: &mut [CalculatedVendorProfitData], mode: SortMode, dir: SortDir) {
    rows.sort_by(|a, b| {
        let ord = |x: i32, y: i32| match dir {
            SortDir::Asc => x.cmp(&y),
            SortDir::Desc => y.cmp(&x),
        };
        match mode {
            SortMode::Roi => ord(a.return_on_investment, b.return_on_investment),
            SortMode::Profit => ord(a.profit, b.profit),
            SortMode::VendorPrice => ord(a.inner.vendor_price, b.inner.vendor_price),
            SortMode::MarketPrice => ord(a.inner.market_price, b.inner.market_price),
            SortMode::SaleTime => {
                let dur = |d: &CalculatedVendorProfitData| {
                    d.inner
                        .sale_summary
                        .as_ref()
                        .and_then(|s| s.avg_sale_duration)
                };
                cmp_none_last(dur(a), dur(b), dir, Ord::cmp)
            }
        }
    });
}

impl VendorProfitTable {
    fn new(sales: RecentSales, world_cheapest_listings: CheapestListings) -> Self {
        let data = tracked_data();

        // Build map of vendor items: ItemId -> VendorPrice
        // We only care about base items, HQ doesn't exist for vendors usually (or is same price)
        let mut vendor_prices = HashMap::new();
        for items in data.gil_shop_items.values() {
            for shop_item in items {
                if let Some(item_def) = data.items.get(&ItemId(shop_item.item)) {
                    vendor_prices.insert(shop_item.item, item_def.price_mid as i32);
                }
            }
        }

        let mut sales_map: HashMap<VendorProfitKey, SaleData> = HashMap::new();
        for sale in sales.sales {
            sales_map.insert(
                VendorProfitKey {
                    item_id: sale.item_id,
                    hq: sale.hq,
                },
                sale,
            );
        }

        let mut table = Vec::new();

        for listing in world_cheapest_listings.cheapest_listings {
            if let Some(&vendor_price) = vendor_prices.get(&listing.item_id) {
                // If the item is sold by a vendor
                // Note: Vendor items are always NQ when bought, but can be sold as NQ.
                // If listing is HQ, we can compare, but usually vendor resale is NQ -> NQ.
                // However, sometimes people buy NQ from vendor and sell as HQ? No, that's crafting.
                // We strictly look for Vendor -> Market.
                // If the market listing is HQ, we shouldn't compare directly unless we want to compete with HQ?
                // Usually vendor resale competes with NQ.
                // Let's filter to only NQ listings for simplicity and correctness,
                // OR we can include HQ listings if the user wants to see if they can undercut HQ with NQ (unlikely to work well).
                // "Flip Finder" logic usually matches HQ to HQ.
                // Vendor items are NQ. So we should compare with NQ market prices.

                if listing.hq {
                    continue;
                }

                let sale_summary = sales_map
                    .remove(&VendorProfitKey {
                        item_id: listing.item_id,
                        hq: false,
                    })
                    .map(compute_summary);

                table.push(Arc::new(VendorProfitData {
                    item_id: listing.item_id,
                    vendor_price,
                    market_price: listing.cheapest_price,
                    sale_summary,
                }));
            }
        }

        VendorProfitTable(table)
    }
}

#[component]
fn VendorResaleTable(
    sales: RecentSales,
    world_cheapest_listings: CheapestListings,
    world: Signal<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let realtime = use_realtime();
    let rt_status = realtime.clone();
    let realtime_status = Signal::derive(move || {
        rt_status
            .as_ref()
            .map(|r| r.status.get())
            .unwrap_or_else(|| "offline".to_string())
    });
    let rt_update = realtime;
    let last_update = Signal::derive(move || rt_update.as_ref().and_then(|r| r.last_update.get()));
    let profits = VendorProfitTable::new(sales, world_cheapest_listings);

    let items = &tracked_data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    // Filter params use `filter_query_signal` (replace: true, scroll: false):
    // typing into a chip writes the URL on every keystroke, and plain
    // `query_signal`'s defaults would push a history entry and yank the
    // window to the top each time.
    let (minimum_profit, set_minimum_profit) = filter_query_signal::<i32>(FILTER_PROFIT);
    let (minimum_roi, set_minimum_roi) = filter_query_signal::<i32>(FILTER_ROI);
    // Seeded to 1d by VendorWorldView so a first-time visitor isn't shown items
    // that sell once a month. The field sits in the ControlBar and the
    // chip has an X, so the default is visible and one click from gone.
    let (max_predicted_time, set_max_predicted_time) =
        filter_query_signal::<String>(FILTER_NEXT_SALE);
    let (tax_enabled, set_tax_enabled) = filter_query_signal::<bool>(FILTER_TAX);
    let (minimum_sales, set_minimum_sales) = filter_query_signal::<usize>(FILTER_SALES);
    let (category_filter, set_category_filter) = filter_query_signal::<i32>(FILTER_CATEGORY);
    // Hidden by default, like the Flip Finder and Trends toggles of the same
    // name — an unachievable listing is worse than no row at all here, because
    // it inflates ROI and therefore sorts to the top.
    let (show_suspicious, set_show_suspicious) = filter_query_signal::<bool>(FILTER_SUSPICIOUS);
    let show_suspicious_active = Signal::derive(move || show_suspicious().unwrap_or(false));

    // A filter picked from the `+ Filter` menu but not yet committed — its
    // chip mounts in edit state with an empty input (see currency_exchange.rs
    // for the same pattern). Booleans and the category select commit a
    // sensible value immediately instead (see `add_filter` below).
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

    let predicted_time =
        Memo::new(move |_| max_predicted_time().and_then(|d| parse_duration(d.as_str()).ok()));

    let sorted_data = Memo::new(move |_| {
        let include_tax = tax_enabled().unwrap_or(true);
        let mut sorted_data = profits
            .0
            .iter()
            .map(|data| {
                let estimated_revenue = if include_tax {
                    (data.market_price as f32 * 0.95) as i32
                } else {
                    data.market_price
                };
                let profit = estimated_revenue - data.vendor_price;
                let return_on_investment = if data.vendor_price > 0 {
                    ((profit as f32 / data.vendor_price as f32) * 100.0) as i32
                } else {
                    0
                };
                CalculatedVendorProfitData {
                    inner: data.clone(),
                    profit,
                    return_on_investment,
                }
            })
            .filter(move |data| {
                minimum_profit()
                    .map(|min| data.profit > min)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_roi()
                    .map(|roi| data.return_on_investment > roi)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_sales()
                    .map(|sales| {
                        data.inner
                            .sale_summary
                            .as_ref()
                            .map(|s| s.num_sold >= sales)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                category_filter()
                    .map(|cat_id| {
                        items
                            .get(&ItemId(data.inner.item_id))
                            .map(|item| item.item_search_category == cat_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                show_suspicious_active()
                    || !is_suspicious_market_price(
                        data.inner.market_price,
                        data.inner.sale_summary.as_ref(),
                    )
            })
            .filter(move |data| {
                predicted_time()
                    .map(|time| {
                        data.inner
                            .sale_summary
                            .as_ref()
                            .and_then(|s| s.avg_sale_duration)
                            .map(|dur| dur.to_std().ok().map(|dur| dur < time).unwrap_or(false))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        // `?dir=` used to be ignored here while the header hardcoded a
        // descending arrow, so the one direction the table could produce was
        // also the only one it claimed. The shared header can now reach `asc`.
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        sort_rows(
            &mut sorted_data,
            mode,
            sort_dir().unwrap_or_else(|| mode.default_dir()),
        );
        sorted_data
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, CalculatedVendorProfitData)>>()
    });

    let category_options = move || {
        let mut categories = tracked_data()
            .item_search_categorys
            .iter()
            .filter(|(_, cat)| !cat.name.is_empty())
            .map(|(id, cat)| (id.0, cat.name.clone()))
            .collect::<Vec<_>>();
        categories.sort_by(|a, b| a.1.cmp(&b.1));
        categories
            .into_iter()
            .map(|(id, name)| (category_id_token(id), name))
            .collect::<Vec<_>>()
    };
    let on_off_options = move || {
        vec![
            ("true", t_string!(i18n, vendor_resale_tax_post).to_string()),
            ("false", t_string!(i18n, vendor_resale_tax_pre).to_string()),
        ]
    };

    // Filters currently drawn as a chip. Drives the "no active filters" hint
    // and keeps `+ Filter` from offering a second copy of something the user
    // can already see.
    let active_filters = Memo::new(move |_| {
        let mut active: Vec<&'static str> = Vec::new();
        if minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT) {
            active.push(FILTER_PROFIT);
        }
        if minimum_roi().is_some() || pending_filter.get() == Some(FILTER_ROI) {
            active.push(FILTER_ROI);
        }
        if minimum_sales().is_some() || pending_filter.get() == Some(FILTER_SALES) {
            active.push(FILTER_SALES);
        }
        if max_predicted_time().is_some() || pending_filter.get() == Some(FILTER_NEXT_SALE) {
            active.push(FILTER_NEXT_SALE);
        }
        if category_filter().is_some() || pending_filter.get() == Some(FILTER_CATEGORY) {
            active.push(FILTER_CATEGORY);
        }
        if tax_enabled().is_some() {
            active.push(FILTER_TAX);
        }
        if show_suspicious_active() {
            active.push(FILTER_SUSPICIOUS);
        }
        active
    });

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, vendor_resale_filter_profit_min_label).to_string(),
            FILTER_ROI => t_string!(i18n, vendor_resale_filter_roi_min_label).to_string(),
            FILTER_SALES => t_string!(i18n, vendor_resale_filter_sales_min_label).to_string(),
            FILTER_NEXT_SALE => {
                t_string!(i18n, vendor_resale_filter_max_sale_time_label).to_string()
            }
            FILTER_CATEGORY => t_string!(i18n, vendor_resale_filter_category_label).to_string(),
            FILTER_TAX => t_string!(i18n, vendor_resale_filter_prices_label).to_string(),
            FILTER_SUSPICIOUS => t_string!(i18n, vendor_resale_suspicious_label).to_string(),
            _ => String::new(),
        }
    };

    // What the `+ Filter` menu offers: everything addable that is not already
    // on screen as a chip.
    let filter_options = Memo::new(move |_| {
        ADDABLE_FILTERS
            .iter()
            .copied()
            .filter(|id| !active_filters().contains(id))
            .map(|id| FilterOption {
                id,
                label: filter_label(id),
            })
            .collect::<Vec<_>>()
    });

    // Adding a filter seeds it with a value the user can see and edit
    // straight away, rather than mounting a select with nothing chosen —
    // except `FILTER_CATEGORY`, where there is no "obviously correct"
    // default. That one mounts blank via `pending_filter`, same as the three
    // free-typed filters. `FILTER_TAX` seeds `false` (pre-tax) — the
    // non-default action, since post-tax is already the silent default.
    let add_filter = Callback::new(move |id: &'static str| match id {
        FILTER_PROFIT => pending_filter.set(Some(FILTER_PROFIT)),
        FILTER_ROI => pending_filter.set(Some(FILTER_ROI)),
        FILTER_SALES => pending_filter.set(Some(FILTER_SALES)),
        FILTER_NEXT_SALE => pending_filter.set(Some(FILTER_NEXT_SALE)),
        FILTER_CATEGORY => pending_filter.set(Some(FILTER_CATEGORY)),
        FILTER_TAX => set_tax_enabled(Some(false)),
        // Boolean toggle: the chip's presence *is* the value, so it commits
        // straight to `true` rather than mounting an editable chip.
        FILTER_SUSPICIOUS => set_show_suspicious(Some(true)),
        _ => {}
    });

    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        set_minimum_profit(None);
        set_minimum_roi(None);
        set_minimum_sales(None);
        set_max_predicted_time(None);
        set_category_filter(None);
        set_tax_enabled(None);
        set_show_suspicious(None);
    });

    view! {
        <div class="flex flex-col gap-6">
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, vendor_resale_results_count, n = move || sorted_data().len())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! { <RealtimeStatus status=realtime_status last_update=last_update /> }
                        .into_any()
                }
                available_filters=Signal::derive(filter_options)
                on_add_filter=add_filter
                on_clear_all=clear_all
                empty_label=Signal::derive(move || {
                    t_string!(i18n, vendor_resale_no_active_filters).to_string()
                })
                is_empty=Signal::derive(move || active_filters().is_empty())
            >
                {move || {
                    (minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_PROFIT);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, vendor_resale_filter_profit_min_label).to_string()
                                    value=Signal::derive(move || minimum_profit().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    max="100000"
                                    step="1000"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_minimum_profit(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_PROFIT) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    (minimum_roi().is_some() || pending_filter.get() == Some(FILTER_ROI))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_ROI);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, vendor_resale_filter_roi_min_label).to_string()
                                    value=Signal::derive(move || minimum_roi().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    max="100000"
                                    step="10"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_minimum_roi(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_ROI) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    (minimum_sales().is_some() || pending_filter.get() == Some(FILTER_SALES))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_SALES);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, vendor_resale_filter_sales_min_label).to_string()
                                    value=Signal::derive(move || minimum_sales().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    max="6"
                                    step="1"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_minimum_sales(
                                            v.and_then(|v| v.parse::<usize>().ok()).map(|s| s.min(6)),
                                        );
                                        if pending_filter.get_untracked() == Some(FILTER_SALES) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    (max_predicted_time().is_some() || pending_filter.get() == Some(FILTER_NEXT_SALE))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_NEXT_SALE);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, vendor_resale_filter_max_sale_time_label).to_string()
                                    value=Signal::derive(max_predicted_time)
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_max_predicted_time(v);
                                        if pending_filter.get_untracked() == Some(FILTER_NEXT_SALE) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    (category_filter().is_some() || pending_filter.get() == Some(FILTER_CATEGORY))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_CATEGORY);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, vendor_resale_filter_category_label).to_string()
                                    value=Signal::derive(move || category_filter().map(|c| category_id_token(c).to_string()))
                                    options=category_options()
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_category_filter(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_CATEGORY) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    tax_enabled()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, vendor_resale_filter_prices_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=on_off_options()
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_tax_enabled(v.and_then(|v| v.parse().ok()));
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    show_suspicious_active()
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, vendor_resale_suspicious_chip).to_string()
                                    readonly=true
                                    value=Signal::derive(|| None::<String>)
                                    on_commit=Callback::new(move |_| set_show_suspicious(None))
                                />
                            }
                        })
                }}
            </ControlBar>

            // Results table
            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                        viewport_height=720.0
                        row_height=40.0
                        overscan=8
                        header_height=64.0
                        variable_height=false
                        header=view! {
                            <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                                <div role="columnheader" class="w-[40px] p-4 text-center">
                                    {t!(i18n, vendor_resale_hq)}
                                </div>
                                <div role="columnheader" class="w-84 p-4">
                                    {t!(i18n, vendor_resale_item)}
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <SortHeader
                                        mode=SortMode::Profit
                                        label=t_string!(i18n, vendor_resale_profit).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <SortHeader
                                        mode=SortMode::Roi
                                        label=t_string!(i18n, vendor_resale_roi).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <SortHeader
                                        mode=SortMode::VendorPrice
                                        label=t_string!(i18n, vendor_resale_vendor_price).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <SortHeader
                                        mode=SortMode::MarketPrice
                                        label=t_string!(i18n, vendor_resale_market_price).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                </div>
                                <div role="columnheader" class="w-30 p-4 hidden md:block">
                                    <SortHeader
                                        mode=SortMode::SaleTime
                                        label=t_string!(i18n, vendor_resale_avg_sale_time).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                </div>
                            </div>
                        }.into_any()
                        each=sorted_data.into()
                        key=move |(index, data): &(usize, CalculatedVendorProfitData)| (
                            *index,
                            data.inner.item_id,
                            data.profit,
                        )
                        view=move |(index, data): (usize, CalculatedVendorProfitData)| {
                            let world = Signal::derive(move || world().to_string());
                            let item_id = data.inner.item_id;
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
                                        // Vendor items are always NQ effectively
                                    </div>
                                    <div role="cell" class="px-4 py-2 flex flex-row w-84 items-center gap-2">
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
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.profit />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <span class={roi_badge_class(data.return_on_investment)}>
                                            {format!("{}%", data.return_on_investment)}
                                        </span>
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.inner.vendor_price />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.inner.market_price />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 truncate hidden md:block flex items-center">
                                        {data.inner
                                            .sale_summary
                                            .as_ref()
                                            .and_then(|s| s.avg_sale_duration)
                                            .and_then(|duration| duration.to_std().ok())
                                            .map(|duration| format_duration_short(duration.as_secs()))
                                            .unwrap_or_else(|| "---".to_string())}
                                    </div>
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
pub fn VendorWorldView() -> impl IntoView {
    let i18n = use_i18n();
    // Seeded here rather than in VendorResaleTable: that lives inside the
    // Suspense closure and remounts on every market refetch, which would keep
    // undoing a filter the user had cleared.
    seed_query_default("next-sale", DEFAULT_MAX_SALE_TIME.to_string());
    let params = use_params_map();
    let world = Signal::derive(move || params.with(|p| p.get("world").clone()).unwrap_or_default());

    // We fetch sales for better estimation, even though we are comparing to vendor prices
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

    view! {
        <div class="main-content p-2 sm:p-6">
            <MetaTitle title=move || format!("{} - {}", t_string!(i18n, vendor_resale_title), world()) />
            <div class="flex flex-col gap-8">
                <ToolHeader
                    title=t_string!(i18n, vendor_resale).to_string()
                    summary=t_string!(i18n, vendor_resale_tool_summary_v2).to_string()
                    context=t_string!(i18n, vendor_resale_tool_context).to_string()
                    help_href="/help/vendor-resale"
                    help_body=t_string!(i18n, vendor_resale_tool_help).to_string()
                    calculation=ToolCalculation::new(
                        t_string!(i18n, vendor_resale_calc_title).to_string(),
                        t_string!(i18n, vendor_resale_calc_formula).to_string(),
                        t_string!(i18n, vendor_resale_calc_details).to_string(),
                    )
                    assumptions=vec![
                        t_string!(i18n, vendor_resale_assumption_nq_purchase).to_string(),
                        t_string!(i18n, vendor_resale_assumption_hq_excluded).to_string(),
                        t_string!(i18n, vendor_resale_assumption_no_vendor_names).to_string(),
                    ]
                />

                // Controls Section
                <div class="panel p-4 sm:p-6 rounded-2xl">
                    <div class="flex flex-col gap-4">
                        <MetaDescription text=move || {
                            t_string!(i18n, vendor_resale_meta_desc).to_string().replace("%world%", &world())
                        } />

                        // World Navigator
                        <div class="flex flex-col md:flex-row gap-4 items-center">
                            <VendorWorldNavigator />
                        </div>

                        // Preset Filters
                        <div class="flex flex-wrap gap-4">
                            <PresetFilterButton
                                href="?next-sale=7d&roi=100&profit=1000&sort=profit&"
                                label=t_string!(i18n, vendor_resale_preset_100_roi).to_string()
                            />
                            <PresetFilterButton
                                href="?next-sale=1M&roi=500&profit=5000&"
                                label=t_string!(i18n, vendor_resale_preset_500_roi).to_string()
                            />
                            <PresetFilterButton href="?profit=50000" label=t_string!(i18n, vendor_resale_preset_50k_profit).to_string() />
                        </div>
                    </div>
                </div>

                // Main Content
                <div class="min-h-screen">
                    <Suspense fallback=move || view! { <BoxSkeleton /> }>
                        {move || {
                            let world_cheapest = world_cheapest_listings.get();
                            let sales = sales.get();
                            match (world_cheapest, sales) {
                                (Some(Ok(w)), Some(Ok(s))) => {
                                    Either::Left(
                                        view! {
                                            <VendorResaleTable
                                                sales=s
                                                world_cheapest_listings=w
                                                world=world
                                            />
                                        },
                                    )
                                }
                                _ => {
                                    Either::Right(
                                        view! {
                                            <div class="text-xl text-[color:var(--color-text)] text-center p-8
                                            bg-brand-900/20 rounded-2xl border border-white/10">
                                                {t!(i18n, vendor_resale_loading_data)}
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
fn PresetFilterButton(href: &'static str, label: String) -> impl IntoView {
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
fn VendorWorldNavigator() -> impl IntoView {
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
                "/vendor-resale",
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
pub fn VendorResale() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <MetaTitle title=t_string!(i18n, vendor_resale_meta_title_ultros) />
        <MetaDescription text=t_string!(i18n, vendor_resale_meta_desc_default) />

        <div class="main-content p-2 sm:p-6">
            <div class="flex flex-col gap-8">
                // Hero Section
                <div class="panel p-4 sm:p-8 rounded-2xl">
                    <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                        {t!(i18n, vendor_resale_tool_title)}
                    </h1>
                    <p class="text-xl text-[color:var(--color-text)] leading-relaxed mb-6">
                        {t!(i18n, vendor_resale_tool_desc)}
                    </p>
                    <p class="text-lg text-[color:var(--color-text)]/90 mb-8">
                        {t!(i18n, vendor_resale_tool_select_world)}
                    </p>

                    // World Selection
                    <div class="panel p-6 rounded-xl">
                        <h2 class="text-xl font-semibold text-[color:var(--brand-fg)] mb-4">
                            {t!(i18n, vendor_resale_choose_world)}
                        </h2>
                        <VendorWorldNavigator />
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
                        <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, vendor_resale_arbitrage)}</h3>
                        <p class="text-gray-300">
                            {t!(i18n, vendor_resale_arbitrage_desc)}
                        </p>
                    </div>

                    <div class="card p-6 rounded-lg transition-colors duration-200">
                        <Icon
                            attr:class="text-brand-300 mb-4"
                            width="2.5em"
                            height="2.5em"
                            icon=i::FaShopSolid
                        />
                        <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, vendor_resale_vendor_data)}</h3>
                        <p class="text-gray-300">
                            {t!(i18n, vendor_resale_vendor_data_desc)}
                        </p>
                    </div>

                    <div class="card p-6 rounded-lg transition-colors duration-200">
                        <Icon
                            attr:class="text-brand-300 mb-4"
                            width="2.5em"
                            height="2.5em"
                            icon=i::FaFilterSolid
                        />
                        <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, vendor_resale_filters)}</h3>
                        <p class="text-gray-300">
                            {t!(i18n, vendor_resale_filters_desc)}
                        </p>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_sort_mode_from_str() {
        // Valid states matching URL query parameters
        assert_eq!(SortMode::from_str("roi"), Ok(SortMode::Roi));
        assert_eq!(SortMode::from_str("profit"), Ok(SortMode::Profit));
        assert_eq!(
            SortMode::from_str("vendor-price"),
            Ok(SortMode::VendorPrice)
        );
        assert_eq!(
            SortMode::from_str("market-price"),
            Ok(SortMode::MarketPrice)
        );
        assert_eq!(SortMode::from_str("sale-time"), Ok(SortMode::SaleTime));
        // Verify invalid options trigger fallback/error paths instead of parsing panic
        assert_eq!(SortMode::from_str("unknown"), Err(()));
        assert_eq!(SortMode::from_str(""), Err(()));
    }

    #[test]
    fn every_sort_token_round_trips_through_display() {
        // Display must emit exactly the token FromStr parses — that round
        // trip through `?sort=` is the whole mechanism of the shared header.
        for mode in [
            SortMode::Roi,
            SortMode::Profit,
            SortMode::VendorPrice,
            SortMode::MarketPrice,
            SortMode::SaleTime,
        ] {
            assert_eq!(SortMode::from_str(&mode.to_string()), Ok(mode));
        }
    }

    #[test]
    fn sort_defaults_keep_old_links_meaning() {
        // The shared header omits `dir` when it matches the column's default,
        // so bookmarked `?sort=` links resolve through these. Flipping one
        // silently changes what old links mean.
        for mode in [SortMode::Roi, SortMode::Profit, SortMode::MarketPrice] {
            assert_eq!(mode.default_dir(), SortDir::Desc, "{mode}");
        }
        for mode in [SortMode::VendorPrice, SortMode::SaleTime] {
            assert_eq!(mode.default_dir(), SortDir::Asc, "{mode}");
        }
        assert_eq!(<SortMode as SortColumn>::fallback(), SortMode::Roi);
    }

    fn calc(
        vendor_price: i32,
        market_price: i32,
        avg_secs: Option<i64>,
    ) -> CalculatedVendorProfitData {
        CalculatedVendorProfitData {
            inner: Arc::new(VendorProfitData {
                item_id: 1,
                vendor_price,
                market_price,
                sale_summary: avg_secs.map(|secs| SaleSummary {
                    item_id: 1,
                    hq: false,
                    num_sold: 6,
                    avg_sale_duration: Some(Duration::seconds(secs)),
                    days_since_last_sale: None,
                    max_price: 0,
                    avg_price: 0,
                    median_price: 0,
                    min_price: 0,
                }),
            }),
            profit: market_price - vendor_price,
            return_on_investment: 0,
        }
    }

    #[test]
    fn vendor_price_sorts_cheapest_first_by_default() {
        let mut rows = vec![calc(30, 0, None), calc(10, 0, None), calc(20, 0, None)];
        sort_rows(
            &mut rows,
            SortMode::VendorPrice,
            SortMode::VendorPrice.default_dir(),
        );
        assert_eq!(
            rows.iter()
                .map(|r| r.inner.vendor_price)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn market_price_sorts_both_directions() {
        let mut rows = vec![calc(0, 10, None), calc(0, 30, None), calc(0, 20, None)];
        sort_rows(&mut rows, SortMode::MarketPrice, SortDir::Desc);
        assert_eq!(
            rows.iter()
                .map(|r| r.inner.market_price)
                .collect::<Vec<_>>(),
            vec![30, 20, 10]
        );
        sort_rows(&mut rows, SortMode::MarketPrice, SortDir::Asc);
        assert_eq!(
            rows.iter()
                .map(|r| r.inner.market_price)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn sale_time_sorts_rows_without_history_last_in_both_directions() {
        // A row with no sales has no avg sale time; whichever direction is
        // asked for, it must never displace a row that has real data.
        let mut rows = vec![
            calc(0, 0, Some(300)),
            calc(0, 0, None),
            calc(0, 0, Some(100)),
        ];
        sort_rows(&mut rows, SortMode::SaleTime, SortDir::Asc);
        let times = |rows: &[CalculatedVendorProfitData]| {
            rows.iter()
                .map(|r| {
                    r.inner
                        .sale_summary
                        .as_ref()
                        .and_then(|s| s.avg_sale_duration)
                        .map(|d| d.num_seconds())
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(times(&rows), vec![Some(100), Some(300), None]);
        sort_rows(&mut rows, SortMode::SaleTime, SortDir::Desc);
        assert_eq!(times(&rows), vec![Some(300), Some(100), None]);
    }

    /// Builds just the fields `is_suspicious_market_price` reads.
    fn summary(num_sold: usize, median_price: i32) -> SaleSummary {
        SaleSummary {
            item_id: 1,
            hq: false,
            num_sold,
            avg_sale_duration: None,
            days_since_last_sale: None,
            max_price: median_price,
            avg_price: median_price,
            median_price,
            min_price: median_price,
        }
    }

    /// The eight rows that filled the top of `/vendor-resale/Gilgamesh` on a
    /// live load, with each item's real median recent sale price. Every one is
    /// a listing nobody will ever buy, and because ROI is computed off the
    /// listing they sorted above every genuine opportunity.
    #[test]
    fn real_gilgamesh_top_rows_are_all_suspicious() {
        // (item name, listed market price, median of the item's recent sales
        // as `compute_summary` computes it from the live API response)
        let observed = [
            ("Sweet Rice Cake", 18_999_996, 6_249),
            ("Copper Wristlets", 150_000_000, 849),
            ("Hyuran Longboots", 9_999_999, 9_999),
            ("Steel Jig", 71_428_571, 295),
            ("Leather Crakows", 20_000_000, 9_000),
            ("Goatskin Eyepatch", 100_000_000, 11_504),
            ("Horn Ring", 150_000_000, 12_747),
        ];
        for (name, market_price, median) in observed {
            assert!(
                is_suspicious_market_price(market_price, Some(&summary(6, median))),
                "{name}: listing {market_price} vs median sale {median} should be suspicious",
            );
        }
    }

    /// A listing at or near what the item actually clears for is the whole
    /// point of the page and must survive the filter. 0.99x is the measured
    /// median listing/sale ratio and 10.2x the 95th percentile.
    #[test]
    fn ordinary_listings_are_kept() {
        for (market_price, median) in [(990, 1_000), (1_000, 1_000), (10_200, 1_000)] {
            assert!(
                !is_suspicious_market_price(market_price, Some(&summary(6, median))),
                "listing {market_price} vs median sale {median} must be kept",
            );
        }
    }

    /// Exactly at the multiple is kept; one gil past it is not.
    #[test]
    fn threshold_boundary_is_exclusive() {
        let median = 1_000;
        let at = (median as i64 * SUSPICIOUS_PRICE_MULTIPLE) as i32;
        assert!(!is_suspicious_market_price(at, Some(&summary(6, median))));
        assert!(is_suspicious_market_price(
            at + 1,
            Some(&summary(6, median))
        ));
    }

    /// Rows we cannot judge are kept rather than penalized — same rule the
    /// server-side quality filter uses for rows without ClickHouse coverage.
    /// Without this, every item lacking recent sales would silently vanish.
    #[test]
    fn unjudgeable_rows_are_kept() {
        assert!(
            !is_suspicious_market_price(150_000_000, None),
            "no sale summary at all must not be filtered"
        );
        assert!(
            !is_suspicious_market_price(150_000_000, Some(&summary(0, 0))),
            "zero recorded sales must not be filtered"
        );
        assert!(
            !is_suspicious_market_price(150_000_000, Some(&summary(6, 0))),
            "a degenerate zero median must not be filtered"
        );
    }

    /// The largest price FFXIV allows against the smallest possible median
    /// must not overflow the multiplication.
    #[test]
    fn extreme_prices_do_not_overflow() {
        assert!(is_suspicious_market_price(
            999_999_999,
            Some(&summary(6, 1))
        ));
        assert!(!is_suspicious_market_price(
            999_999_999,
            Some(&summary(6, i32::MAX))
        ));
    }
}
