use super::world_nav::use_analyzer_world;
use crate::analyzer_kit::filters::{price_control, register_filters, toggle_control};
use crate::analyzer_kit::window::MarketWindowControl;
use crate::analyzer_kit::{
    formula::PriceSignal,
    market::{MarketGrid, MarketSubject, resolve_price, use_market_data},
};
use crate::components::app_link::use_query_map_or_default;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::virtual_grid::metrics::FilterOp;
use crate::components::virtual_grid::metrics::{GridMetric, GridValue};
use crate::components::virtual_grid::registry::FilterAlias;
use crate::components::virtual_grid::saved_views::{
    GridPresetView, GridSavedViews, provide_grid_saved_views,
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::query_signal;
use crate::ws::realtime::use_realtime;
use crate::{
    analysis::{SalesStats, analyze_sales},
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        control_bar::ControlBar,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::{BoxSkeleton, InlineStatusSkeleton},
        sort_header::{SortColumn, SortDir, SortableHeaderCell},
        tool_help::*,
        virtual_grid::{ColumnFilter, GridColumn},
        world_picker::WorldOnlyPicker,
    },
    global_state::region_for_world::use_region_for_world,
    query_defaults::filter_query_signal,
};
use itertools::Itertools;
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};
use thousands::Separable;
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
};

#[derive(Clone, Debug, PartialEq)]
struct VentureProfitData {
    task_id: i32,
    task_level: i32,
    item_id: i32,
    quantity: i32,
    market_price: i32,
    cheapest_world_id: i32,
    hq: bool,
    listing_price: Option<i32>,
    price_fallback: bool,
    pricing_pending: bool,
    profit: i32,
    avg_price: i32,
    daily_sales: f32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Level,
    UnitPrice,
    AvgPrice,
    DailySales,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "level" => Ok(SortMode::Level),
            "unit-price" => Ok(SortMode::UnitPrice),
            "avg-price" => Ok(SortMode::AvgPrice),
            "daily-sales" => Ok(SortMode::DailySales),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Profit => "profit",
            SortMode::Level => "level",
            SortMode::UnitPrice => "unit-price",
            SortMode::AvgPrice => "avg-price",
            SortMode::DailySales => "daily-sales",
        };
        f.write_str(val)
    }
}

/// Every column reads best-first descending — ventures cost venture coins,
/// not gil, so there is no cost-like column to default ascending.
impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }
}

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_PROFIT: &str = "profit";
const FILTER_OUTLIERS: &str = "filter-outliers";

/// The page's built-in views, offered above the reader's own saved ones.
///
/// Queries only: the labels live in [`venture_analyzer_presets`] because `t_string!`
/// needs a literal key. Every key used here is pinned by a test below.
const PRESET_QUERIES: [&str; 3] = [
    "?filter-outliers=true&sort=profit",
    "?filter-outliers=true&sort=daily-sales",
    "?filter-outliers=true&profit=50000&sort=profit",
];

fn venture_analyzer_presets(i18n: I18nContext<Locale, I18nKeys>) -> Vec<GridPresetView> {
    [
        t_string!(i18n, venture_analyzer_preset_best_profit).to_string(),
        t_string!(i18n, venture_analyzer_preset_fast_movers).to_string(),
        t_string!(i18n, venture_analyzer_preset_high_value).to_string(),
    ]
    .into_iter()
    .zip(PRESET_QUERIES)
    .map(|(label, query)| GridPresetView {
        label,
        query: query.to_string(),
    })
    .collect()
}

/// Historical preset keys: these must remain readable after migration.
#[cfg(test)]
const LEGACY_PRESET_FILTER_KEYS: &[&str] = &[FILTER_PROFIT, FILTER_OUTLIERS];

fn compare_ventures(mode: SortMode, a: &VentureProfitData, b: &VentureProfitData) -> Ordering {
    match mode {
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Level => a.task_level.cmp(&b.task_level),
        SortMode::UnitPrice => a.market_price.cmp(&b.market_price),
        SortMode::AvgPrice => a.avg_price.cmp(&b.avg_price),
        SortMode::DailySales => a
            .daily_sales
            .partial_cmp(&b.daily_sales)
            .unwrap_or(Ordering::Equal),
    }
}

/// A listing fallback is displayable while history loads, but it cannot decide
/// eligibility for a filter on the selected sale-based calculation.
fn financial_value(value: i32, pending: bool) -> GridValue {
    if pending {
        GridValue::Pending
    } else {
        GridValue::Number(value as f64)
    }
}

#[cfg(test)]
fn profit_meets_minimum(profit: i32, minimum: Option<i32>, pending: bool) -> bool {
    pending || minimum.is_none_or(|minimum| profit >= minimum)
}

fn venture_metrics() -> Vec<GridMetric<(usize, Arc<VentureProfitData>)>> {
    vec![
        GridMetric::text("item", |(_, row): &(usize, Arc<VentureProfitData>)| {
            GridValue::Text(
                tracked_data()
                    .items
                    .get(&xiv_gen::ItemId(row.item_id))
                    .map(|item| item.name.as_str().to_string())
                    .unwrap_or_default(),
            )
        }),
        GridMetric::number("profit", |(_, row): &(usize, Arc<VentureProfitData>)| {
            financial_value(row.profit, row.pricing_pending)
        }),
        GridMetric::number(
            "unit-price",
            |(_, row): &(usize, Arc<VentureProfitData>)| {
                financial_value(row.market_price, row.pricing_pending)
            },
        ),
        GridMetric::number("avg-price", |(_, row): &(usize, Arc<VentureProfitData>)| {
            GridValue::Number(row.avg_price as f64)
        }),
        GridMetric::number(
            "daily-sales",
            |(_, row): &(usize, Arc<VentureProfitData>)| GridValue::Number(row.daily_sales as f64),
        ),
        GridMetric::number("level", |(_, row): &(usize, Arc<VentureProfitData>)| {
            GridValue::Number(row.task_level as f64)
        }),
    ]
}

#[component]
fn VentureAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    recent_sales: Option<RecentSales>,
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
    let market = use_market_data(world);
    let (revenue_basis, _set_revenue_basis) = filter_query_signal::<PriceSignal>("revenue");
    market.require_price_basis(Signal::derive(move || {
        revenue_basis.get().unwrap_or_default()
    }));
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = tracked_data();
    let items = &data.items;
    let retainer_tasks = &data.retainer_tasks;
    let retainer_task_normals = &data.retainer_task_normals;

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    let (filter_outliers, _set_filter_outliers) = filter_query_signal::<bool>(FILTER_OUTLIERS);
    let query = use_query_map_or_default();

    let categories = Memo::new(move |_| {
        retainer_tasks
            .values()
            .filter(|t| !t.is_random)
            .map(|t| t.class_job_category)
            .unique()
            .filter_map(|id| {
                data.class_job_categorys
                    .get(&xiv_gen::ClassJobCategoryId(id))
                    .map(|c| (id, c.name.as_str().to_string()))
            })
            .sorted_by(|a, b| a.1.cmp(&b.1))
            .collect::<Vec<_>>()
    });

    let selected_jobs_set = Memo::new(move |_| {
        query.with(|q| {
            q.get("jobs")
                .map(|s| s.split(',').map(|s| s.to_string()).collect::<HashSet<_>>())
                .unwrap_or_default()
        })
    });

    let selected_category_ids = Memo::new(move |_| {
        let selected_names = selected_jobs_set.get();
        if selected_names.is_empty() {
            return None;
        }
        let ids: HashSet<_> = categories
            .get()
            .iter()
            .filter(|(_, name)| selected_names.contains(name))
            .map(|(id, _)| *id)
            .collect();
        Some(ids)
    });

    let computed_data = Memo::new(move |_| {
        let mut results = Vec::new();
        let stats = market.selected_stats();
        let pricing_pending = stats.is_none()
            && revenue_basis
                .get()
                .unwrap_or_default()
                .sale_stat()
                .is_some();
        let selected_ids = selected_category_ids.get();
        let filter_outliers = filter_outliers().unwrap_or(false);

        let sales_map: HashMap<i32, Vec<&SaleData>> = if let Some(ref sales) = recent_sales {
            let mut map: HashMap<i32, Vec<&SaleData>> = HashMap::new();
            for sale in &sales.sales {
                map.entry(sale.item_id).or_default().push(sale);
            }
            map
        } else {
            HashMap::new()
        };

        // Iterate over RetainerTasks to find normal ventures
        for (task_id, task) in retainer_tasks.iter() {
            if task.is_random {
                continue;
            }

            if let Some(ids) = &selected_ids
                && !ids.contains(&task.class_job_category)
            {
                continue;
            }

            // Check if `task.task` (RowId) corresponds to a RetainerTaskNormal
            // We need to cast RowId to RetainerTaskNormalId?
            // Since RowId is just u16 wrapper, and RetainerTaskNormalId is i32 wrapper.
            let normal_id = xiv_gen::RetainerTaskNormalId(task.task);

            if let Some(normal_task) = retainer_task_normals.get(&normal_id) {
                let item_id = normal_task.item;
                if item_id == 0 {
                    continue;
                }

                let quantity = normal_task.quantity_0; // taking base quantity
                if quantity == 0 {
                    continue;
                }

                let task_level = task.retainer_level as i32;

                // Market Price
                let Some(resolved) = resolve_price(
                    &prices,
                    stats.as_deref(),
                    item_id,
                    None,
                    revenue_basis.get().unwrap_or_default(),
                ) else {
                    continue;
                };
                let market_price = resolved.price;
                let hq = resolved.hq;
                let listing = prices.find_matching_listings(item_id);
                let listing = if hq { listing.hq } else { listing.lq };
                let listing_price = listing.map(|entry| entry.price);
                let cheapest_world_id = listing.map(|entry| entry.world_id).unwrap_or(0);
                let price_fallback = resolved.fallback;

                if market_price == 0 {
                    continue;
                }

                let sales_stats = if let Some(item_sales) = sales_map.get(&{ item_id }) {
                    analyze_sales(item_sales, filter_outliers)
                } else {
                    SalesStats {
                        daily_sales: 0.0,
                        avg_price: 0,
                        total_sales: 0,
                    }
                };

                // Ventures cost venture coins (not gil), so "profit" here is gross revenue.
                // If we ever convert ventures to a gil-equivalent cost, subtract it here.
                let revenue = market_price * quantity;
                let profit = revenue;

                results.push(VentureProfitData {
                    task_id: task_id.0,
                    task_level,
                    item_id,
                    quantity,
                    market_price,
                    cheapest_world_id,
                    hq,
                    listing_price,
                    price_fallback,
                    pricing_pending,
                    profit,
                    avg_price: sales_stats.avg_price,
                    daily_sales: sales_stats.daily_sales,
                });
            }
        }

        // Keep every eligible row; the grid virtualizes rendering, not the result set.
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        results.sort_by(|a, b| {
            let order = compare_ventures(mode, a, b);
            if dir == SortDir::Asc {
                order
            } else {
                order.reverse()
            }
        });

        results
            .into_iter()
            .map(Arc::new)
            .enumerate()
            .collect::<Vec<_>>()
    });

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, venture_analyzer_filter_profit_min_label).to_string(),
            FILTER_OUTLIERS => t_string!(i18n, venture_analyzer_filter_outliers).to_string(),
            _ => String::new(),
        }
    };

    let filters = register_filters(
        vec![FilterAlias::integer("profit", "profit", FilterOp::Gte)],
        Signal::derive(move || {
            vec![
                price_control(
                    "revenue",
                    t_string!(i18n, market_returned_value).to_string(),
                    market.window,
                    t_string!(i18n, market_listing_basis).to_string(),
                ),
                toggle_control(FILTER_OUTLIERS, filter_label(FILTER_OUTLIERS)),
                {
                    let mut control = ColumnFilter::new(
                        "jobs",
                        t_string!(i18n, venture_analyzer_filter_by_job).to_string(),
                        false,
                    );
                    control.multiple = true;
                    control.choices = categories
                        .get()
                        .into_iter()
                        .map(|(_, name)| (name.clone(), name))
                        .collect();
                    control
                },
            ]
        }),
    );

    let presets = Signal::derive(move || venture_analyzer_presets(i18n));

    view! {
            <div class="flex flex-col gap-6">
                <div class="flex flex-wrap items-start gap-3">
                    <MarketWindowControl window=market.window />

                </div>

                <ControlBar sticky=false
                    summary=move || {
                        view! {
                            <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                                {move || t!(i18n, venture_analyzer_result_count, n = move || filters.row_count())}
                            </span>
                        }
                        .into_any()
                    }
                    actions=move || {
                        view! {
                            <RealtimeStatus status=realtime_status last_update=last_update />
                            <GridSavedViews id="venture-analyzer-grid" presets=presets />
                        }
                            .into_any()
                    }

                    empty_label=Signal::derive(move || {
                        t_string!(i18n, venture_analyzer_no_filters_hint).to_string()
                    })
                />

                <div>
                    <MarketGrid show_saved_views=false market subject=Arc::new(move |(_, row): &(usize, Arc<VentureProfitData>)| {
        let mut subject = MarketSubject::new(row.item_id, row.hq, row.cheapest_world_id);
        subject.listing_price = row.listing_price;
        subject.label = t_string!(i18n, market_returned_item).to_string();
        subject
     })
     metrics=venture_metrics()
     id="venture-analyzer-grid" label=t_string!(i18n, venture_analyzer_col_venture_item).to_string()
     row_height=60.0
     columns=Signal::derive(move || vec![GridColumn::new("item",t_string!(i18n, venture_analyzer_col_venture_item).to_string(), 320.0, false, true),
    { let mut col = GridColumn::new("profit",t_string!(i18n, venture_analyzer_col_profit).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::Profit, sort_dir.get().unwrap_or_else(||SortMode::Profit.default_dir()) == SortDir::Asc); col.filters.push(ColumnFilter::new("profit", filter_label("profit"), true)); col },
    GridColumn::new("unit-price",t_string!(i18n, venture_analyzer_col_unit_price).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::UnitPrice, sort_dir.get().unwrap_or_else(||SortMode::UnitPrice.default_dir()) == SortDir::Asc),
    GridColumn::new("avg-price",t_string!(i18n, venture_analyzer_col_avg_price).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::AvgPrice, sort_dir.get().unwrap_or_else(||SortMode::AvgPrice.default_dir()) == SortDir::Asc),
    GridColumn::new("daily-sales",t_string!(i18n, venture_analyzer_col_daily_sales).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::DailySales, sort_dir.get().unwrap_or_else(||SortMode::DailySales.default_dir()) == SortDir::Asc),
    GridColumn::new("level",t_string!(i18n, venture_analyzer_col_level).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::Level, sort_dir.get().unwrap_or_else(||SortMode::Level.default_dir()) == SortDir::Asc)])
     header=move |id| {match id {"item" => view! {<div  class="w-full min-w-0">{t!(i18n, venture_analyzer_col_venture_item)}</div>}.into_any(),
    "profit" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::Profit
                                    label=t_string!(i18n, venture_analyzer_col_profit).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "unit-price" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::UnitPrice
                                    label=t_string!(i18n, venture_analyzer_col_unit_price).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "avg-price" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::AvgPrice
                                    label=t_string!(i18n, venture_analyzer_col_avg_price).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "daily-sales" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::DailySales
                                    label=t_string!(i18n, venture_analyzer_col_daily_sales).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "level" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::Level
                                    label=t_string!(i18n, venture_analyzer_col_level).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(), _ => ().into_any()}}
     each=computed_data
                        key=move |(_, data): &(usize, Arc<VentureProfitData>)| data.task_id

     measure=move |(_, data): &(usize, Arc<VentureProfitData>), id| {match id {"item" => (items.get(&xiv_gen::ItemId(data.item_id)).map(|i| i.name.as_str()).unwrap_or_default().to_string(), 110.0),
    "profit" => (data.profit.separate_with_commas(), 42.0),
    "unit-price" => (data.market_price.separate_with_commas(), 42.0),
    "avg-price" => (data.avg_price.separate_with_commas(), 42.0),
    "daily-sales" => (format!("{:.1}", data.daily_sales), 42.0),
    "level" => (data.task_level.to_string(), 42.0), _ => (String::new(), 0.0)}}
     view=move |(index, data): (usize, Arc<VentureProfitData>), id| {
                            let item_id = data.item_id;
                            let item = items.get(&xiv_gen::ItemId(item_id)).map(|i| i.name.as_str().to_string()).unwrap_or_else(|| t_string!(i18n, unknown).to_string());

     let _ = index;
     match id {"item" => view! {<div  class="flex flex-row items-center gap-2 w-full min-w-0">
                                         <a
                                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                            href=format!("/item/{}/{}", world(), item_id)
                                        >
                                            <div class="shrink-0">
                                                <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                            </div>
                                            <div class="flex flex-col truncate">
                                                <span class="font-semibold">{item}</span>
                                                <span class="text-xs text-[color:var(--color-text-muted)] truncate">
                                                    {t!(i18n, venture_analyzer_quantity_x)} " " {data.quantity}
                                                </span>
                                            </div>
                                        </a>
                                    </div>}.into_any(),
    "profit" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.profit />
                                    </div>}.into_any(),
    "unit-price" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.market_price />
                                        {data.price_fallback.then(|| view! { <span class="block text-xs text-amber-300">{t!(i18n, market_listing_fallback)}</span> })}
                                    </div>}.into_any(),
    "avg-price" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.avg_price />
                                    </div>}.into_any(),
    "daily-sales" => view! {<div  class="text-right w-full min-w-0">
                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                            {t!(i18n, venture_analyzer_sales_per_day, sales = format!("{:.1}", data.daily_sales))}
                                        </span>
                                    </div>}.into_any(),
    "level" => view! {<div  class="text-right w-full min-w-0">
                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                            {t!(i18n, venture_analyzer_lv)} " " {data.task_level}
                                        </span>
                                    </div>}.into_any(), _ => ().into_any()}}
     />
                 </div>
            </div>
        }
}

#[component]
pub fn VentureAnalyzer() -> impl IntoView {
    provide_grid_saved_views("venture-analyzer-grid");
    let i18n = use_i18n();
    let (selected_world, set_selected_world) = use_analyzer_world("/venture-analyzer");
    let region = use_region_for_world(move || selected_world.get().map(|world| world.name));

    let global_cheapest_listings = ArcResource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

    let recent_sales = ArcResource::new(selected_world, move |world| async move {
        if let Some(world) = world {
            get_recent_sales_for_world(&world.name).await
        } else {
            Ok(RecentSales { sales: vec![] })
        }
    });

    let recent_sales_clone = recent_sales.clone();
    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title=move || t_string!(i18n, venture_analyzer_meta_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, venture_analyzer_meta_desc).to_string() />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, venture_analyzer).to_string()
                    summary=t_string!(i18n, venture_analyzer_tool_summary).to_string()
                    context=t_string!(i18n, venture_analyzer_tool_context).to_string()
                    help_href="/help/venture-analyzer"
                    help_body=t_string!(i18n, venture_analyzer_tool_help).to_string()
                    calculation=ToolCalculation::new(
                        t_string!(i18n, venture_analyzer_calc_title).to_string(),
                        t_string!(i18n, venture_analyzer_calc_formula).to_string(),
                        t_string!(i18n, venture_analyzer_calc_details).to_string(),
                    )
                    assumptions=vec![
                        t_string!(i18n, venture_analyzer_assumption_gross_revenue).to_string(),
                        "Normal ventures only".to_string(),
                        "Recent sales affect confidence".to_string(),
                    ]
                >
                    <Suspense fallback=InlineStatusSkeleton>
                        {move || {
                            recent_sales_clone
                                .get()
                                .and_then(|r| r.err())
                                .map(|_| view! { <div class="text-red-400 text-sm">{t!(i18n, venture_analyzer_error_sales)}</div> })
                        }}
                    </Suspense>
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, world)}</label>
                    <div data-testid="analyzer-world-picker">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world
                        />
                    </div>
                    <span class="text-sm text-[color:var(--color-text-muted)]" data-testid="analyzer-market-scope">
                        {t!(i18n, market_scope)} ": " {move || region.get()}
                    </span>
                </ToolHeader>
                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        let sales = recent_sales.get();
                        match (listings, sales) {
                            (Some(Ok(listings)), Some(Ok(sales))) => {
                                view! {
                                    <VentureAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=Some(sales)
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            (Some(Ok(listings)), _) => {
                                view! {
                                    <VentureAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=None
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            (Some(Err(e)), _) => {
                                view! {
                                    <div class="text-red-400">
                                        {t!(i18n, venture_analyzer_error_listings)} {e.to_string()}
                                    </div>
                                }.into_any()
                            }
                            _ => {
                                view! { <BoxSkeleton /> }.into_any()
                            }
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// A preset is applied by rebuilding the URL from its query, so a stray
    /// separator or an empty pair would ship straight into the address bar.
    #[test]
    fn every_preset_query_is_a_clean_query_string() {
        for query in PRESET_QUERIES {
            assert!(query.starts_with('?'), "{query}");
            assert!(!query.ends_with('&'), "{query}");
            assert!(!query.contains("&&"), "{query}");
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                assert!(!key.is_empty(), "{query}");
                assert!(!value.is_empty(), "{query}");
            }
        }
    }

    /// Renaming a sort token or retiring a filter would otherwise leave a
    /// built-in view quietly pointing at nothing.
    #[test]
    fn preset_queries_only_use_keys_this_page_still_reads() {
        for query in PRESET_QUERIES {
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').expect("key=value");
                match key {
                    "sort" => assert!(
                        std::str::FromStr::from_str(value)
                            .map(|_: SortMode| ())
                            .is_ok(),
                        "{query}"
                    ),
                    other => assert!(LEGACY_PRESET_FILTER_KEYS.contains(&other), "{query}"),
                }
            }
        }
    }

    #[test]
    fn sale_price_filters_wait_for_selected_basis_instead_of_rejecting_listing_fallback() {
        use crate::components::virtual_grid::metrics::{FilterOp, MetricFilter};
        let minimum = MetricFilter {
            op: FilterOp::Gte,
            value: "100".into(),
        };
        // Every fallback is below the threshold, including candidates beyond the old cap.
        let candidates: Vec<_> = (0..150)
            .filter(|_| profit_meets_minimum(20, Some(100), true))
            .collect();
        assert_eq!(candidates.len(), 150);
        assert_eq!(minimum.matches(&financial_value(20, true), false), None);
        // Once the selected statistic arrives, both legacy and grid filters use it.
        assert!(profit_meets_minimum(140, Some(100), false));
        assert_eq!(
            minimum.matches(&financial_value(140, false), false),
            Some(true)
        );
        assert!(!profit_meets_minimum(20, Some(100), false));
        assert_eq!(
            minimum.matches(&financial_value(20, false), false),
            Some(false)
        );
        assert!(profit_meets_minimum(20, None, false));
    }

    /// Display must produce exactly the token FromStr parses back — the
    /// shared SortHeader's hrefs depend on that round trip.
    #[test]
    fn sort_mode_round_trips_through_the_url() {
        for mode in [
            SortMode::Profit,
            SortMode::Level,
            SortMode::UnitPrice,
            SortMode::AvgPrice,
            SortMode::DailySales,
        ] {
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert!("bogus".parse::<SortMode>().is_err());
    }

    #[test]
    fn compare_ventures_orders_ascending_by_column() {
        let row = |profit: i32, daily_sales: f32| VentureProfitData {
            task_id: 1,
            task_level: profit,
            item_id: 1,
            quantity: 1,
            market_price: profit,
            cheapest_world_id: 1,
            hq: false,
            listing_price: Some(profit),
            price_fallback: false,
            pricing_pending: false,
            profit,
            avg_price: profit,
            daily_sales,
        };
        let low = row(10, 0.5);
        let high = row(20, 2.0);
        for mode in [
            SortMode::Profit,
            SortMode::Level,
            SortMode::UnitPrice,
            SortMode::AvgPrice,
            SortMode::DailySales,
        ] {
            assert_eq!(
                compare_ventures(mode, &low, &high),
                Ordering::Less,
                "{mode:?}"
            );
        }
    }
}
