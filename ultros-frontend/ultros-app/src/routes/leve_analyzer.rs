use super::world_nav::use_analyzer_world;
use crate::analyzer_kit::filters::{price_control, register_filters, toggle_control};
use crate::analyzer_kit::window::MarketWindowControl;
use crate::analyzer_kit::{
    formula::PriceSignal,
    market::{MarketGrid, MarketSubject, resolve_price, use_market_data},
};
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
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use std::{cmp::Ordering, collections::HashMap, sync::Arc};
use thousands::Separable;
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
};
use xiv_gen::{
    ClassJobCategoryId, CraftLeve, ItemId, Leve, LeveId, LeveRewardItemGroupId, LeveRewardItemId,
};

#[derive(Clone, Debug, PartialEq)]
struct LeveProfitData {
    leve: &'static Leve,
    craft_leve: &'static CraftLeve,
    profit: i32,
    cost: i32,
    revenue: i32,
    reward_fallback: bool,
    market_price: i32,
    hq: bool,
    listing_price: Option<i32>,
    price_fallback: bool,
    cost_pending: bool,
    revenue_pending: bool,
    cheapest_world_id: i32,
    item_id: ItemId,
    item_count: u32,
    class_job_level: u16,
    job_category_name: String,
    avg_price: i32,
    daily_sales: f32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Level,
    Revenue,
    Cost,
    AvgPrice,
    DailySales,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "level" => Ok(SortMode::Level),
            "revenue" => Ok(SortMode::Revenue),
            "cost" => Ok(SortMode::Cost),
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
            SortMode::Revenue => "revenue",
            SortMode::Cost => "cost",
            SortMode::AvgPrice => "avg-price",
            SortMode::DailySales => "daily-sales",
        };
        f.write_str(val)
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }

    /// Cost reads best-first ascending — the cheapest turn-in is the
    /// interesting one. Everything else is a biggest-first metric.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::Cost => SortDir::Asc,
            _ => SortDir::Desc,
        }
    }
}

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_PROFIT: &str = "profit";
const FILTER_JOB: &str = "job";
const FILTER_OUTLIERS: &str = "filter-outliers";

/// The page's built-in views, offered above the reader's own saved ones.
///
/// Queries only: the labels live in [`leve_analyzer_presets`] because `t_string!`
/// needs a literal key. Every key used here is pinned by a test below.
const PRESET_QUERIES: [&str; 3] = [
    "?filter-outliers=true&sort=profit",
    "?filter-outliers=true&sort=daily-sales",
    "?filter-outliers=true&sort=cost",
];

fn leve_analyzer_presets(i18n: I18nContext<Locale, I18nKeys>) -> Vec<GridPresetView> {
    [
        t_string!(i18n, leve_analyzer_preset_best_profit).to_string(),
        t_string!(i18n, leve_analyzer_preset_steady_sellers).to_string(),
        t_string!(i18n, leve_analyzer_preset_cheapest).to_string(),
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
const LEGACY_PRESET_FILTER_KEYS: &[&str] = &[FILTER_PROFIT, FILTER_JOB, FILTER_OUTLIERS];

/// The job-select's values, in menu order. Values are the class-job-category
/// name substrings the old `<select>` matched against — kept verbatim so
/// `?job=` deep links survive the conversion.
const JOB_VALUES: &[&str] = &[
    "Carpenter",
    "Blacksmith",
    "Armorer",
    "Goldsmith",
    "Leatherworker",
    "Weaver",
    "Alchemist",
    "Culinarian",
];

fn compare_leves(mode: SortMode, a: &LeveProfitData, b: &LeveProfitData) -> Ordering {
    match mode {
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Level => a.class_job_level.cmp(&b.class_job_level),
        SortMode::Revenue => a.revenue.cmp(&b.revenue),
        SortMode::Cost => a.cost.cmp(&b.cost),
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

fn leve_metrics() -> Vec<GridMetric<(usize, Arc<LeveProfitData>)>> {
    vec![
        GridMetric::text("item", |(_, row): &(usize, Arc<LeveProfitData>)| {
            GridValue::Text(format!(
                "{} {}",
                row.leve.name,
                tracked_data()
                    .items
                    .get(&row.item_id)
                    .map(|item| item.name.as_str())
                    .unwrap_or_default()
            ))
        }),
        GridMetric::number("profit", |(_, row): &(usize, Arc<LeveProfitData>)| {
            financial_value(row.profit, row.cost_pending || row.revenue_pending)
        }),
        GridMetric::number("revenue", |(_, row): &(usize, Arc<LeveProfitData>)| {
            financial_value(row.revenue, row.revenue_pending)
        }),
        GridMetric::number("cost", |(_, row): &(usize, Arc<LeveProfitData>)| {
            financial_value(row.cost, row.cost_pending)
        }),
        GridMetric::number("avg-price", |(_, row): &(usize, Arc<LeveProfitData>)| {
            GridValue::Number(row.avg_price as f64)
        }),
        GridMetric::number("daily-sales", |(_, row): &(usize, Arc<LeveProfitData>)| {
            GridValue::Number(row.daily_sales as f64)
        }),
        GridMetric::number("level", |(_, row): &(usize, Arc<LeveProfitData>)| {
            GridValue::Number(row.class_job_level as f64)
        }),
    ]
}

#[component]
fn LeveAnalyzerTable(
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
    let (cost_basis, _set_cost_basis) = filter_query_signal::<PriceSignal>("cost-basis");
    market.require_price_basis(Signal::derive(move || cost_basis.get().unwrap_or_default()));
    let (revenue_basis, _set_revenue_basis) = filter_query_signal::<PriceSignal>("revenue");
    market.require_price_basis(Signal::derive(move || {
        revenue_basis.get().unwrap_or_default()
    }));
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = tracked_data();
    let items = &data.items;
    let leves = &data.leves;
    let craft_leves = &data.craft_leves;
    let leve_reward_items = &data.leve_reward_items;
    let leve_reward_item_groups = &data.leve_reward_item_groups;
    let class_job_categories = &data.class_job_categorys;

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    let (job_filter, _set_job_filter) = filter_query_signal::<String>(FILTER_JOB);
    let (filter_outliers, _set_filter_outliers) = filter_query_signal::<bool>(FILTER_OUTLIERS);

    let computed_data = Memo::new(move |_| {
        let mut results = Vec::new();
        let stats = market.selected_stats();
        let cost_pending =
            stats.is_none() && cost_basis.get().unwrap_or_default().sale_stat().is_some();
        let reward_signal_pending = stats.is_none()
            && revenue_basis
                .get()
                .unwrap_or_default()
                .sale_stat()
                .is_some();
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

        for craft_leve in craft_leves.values() {
            let leve_id = craft_leve.leve;
            // Some CraftLeves might point to invalid Leve IDs or placeholder 0
            if leve_id == 0 {
                continue;
            }
            let leve = match leves.get(&LeveId(leve_id)) {
                Some(l) => l,
                None => continue,
            };

            // Only consider levels with items
            let item_id = craft_leve.item_0;
            if item_id == 0 {
                continue;
            }
            let item_count = craft_leve.item_count_0 as u32;
            if item_count == 0 {
                continue;
            }

            // Job Category (for filtering)
            let job_category =
                class_job_categories.get(&ClassJobCategoryId(leve.class_job_category as i32));
            let job_category_name = job_category
                .map(|cj| cj.name.to_string())
                .unwrap_or_default();

            // Filter by Job
            if let Some(filter) = job_filter()
                && !filter.is_empty()
                && !job_category_name.contains(&filter)
            {
                continue;
            }

            // Calculate Cost
            let Some(resolved) = resolve_price(
                &prices,
                stats.as_deref(),
                item_id,
                None,
                cost_basis.get().unwrap_or_default(),
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
                // Can't calculate profit without market price
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

            // Cost is price * count.
            // Note: If you turn in HQ, rewards are double. But let's assume NQ for baseline safety.
            // Or maybe add a toggle for HQ later. For now, assume NQ cost for NQ rewards.
            let cost = market_price as i64 * item_count as i64;

            // Calculate Revenue
            let gil_reward = leve.gil_reward as i64;

            // Calculate Item Rewards Expected Value
            let mut expected_item_value = 0.0;
            let mut revenue_pending = false;
            let mut reward_fallback = false;
            let reward_item_id = leve.leve_reward_item;

            if let Some(reward_item_entry) =
                leve_reward_items.get(&LeveRewardItemId(reward_item_id as i32))
            {
                // Iterate over the 8 groups
                let groups = [
                    (
                        reward_item_entry.leve_reward_item_group_0,
                        reward_item_entry.probability_percent_0,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_1,
                        reward_item_entry.probability_percent_1,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_2,
                        reward_item_entry.probability_percent_2,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_3,
                        reward_item_entry.probability_percent_3,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_4,
                        reward_item_entry.probability_percent_4,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_5,
                        reward_item_entry.probability_percent_5,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_6,
                        reward_item_entry.probability_percent_6,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_7,
                        reward_item_entry.probability_percent_7,
                    ),
                ];

                for (group_id, probability) in groups {
                    if group_id == 0 || probability == 0 {
                        continue;
                    }

                    if let Some(group) =
                        leve_reward_item_groups.get(&LeveRewardItemGroupId(group_id as i32))
                    {
                        // A group can give ONE of the items listed? Or all?
                        // LeveRewardItemGroup usually picks one.
                        // But usually these groups have 1 item with 100% chance relative to the group selection?
                        // Let's assume average value of the items in the group?
                        // Actually, looking at the CSV structure from `head`:
                        // LeveRewardItemGroup has Item[0]..Item[8].
                        // Usually it's just one item per group for Leves.
                        // Let's sum up value of all possible items in the group?
                        // Wait, a LeveRewardItemGroup is a list of possible items.
                        // But standard Leve data usually maps probability to a specific item reward "slot".
                        // Let's iterate items in the group.

                        // For simplicity, let's take the first item in the group if it exists.
                        // Or sum them all?
                        // Most Leve reward groups for crafting seem to have just one item type (crystals, or the item itself).

                        let group_items = [
                            (group.item_0, group.count_0),
                            (group.item_1, group.count_1),
                            (group.item_2, group.count_2),
                            (group.item_3, group.count_3),
                            (group.item_4, group.count_4),
                            (group.item_5, group.count_5),
                            (group.item_6, group.count_6),
                            (group.item_7, group.count_7),
                            (group.item_8, group.count_8),
                        ];

                        for (g_item_id, g_count) in group_items {
                            if g_item_id == 0 || g_count == 0 {
                                continue;
                            }

                            revenue_pending |= reward_signal_pending;
                            let resolved_reward = resolve_price(
                                &prices,
                                stats.as_deref(),
                                g_item_id as i32,
                                None,
                                revenue_basis.get().unwrap_or_default(),
                            );
                            reward_fallback |=
                                resolved_reward.as_ref().is_some_and(|price| price.fallback);
                            let reward_price =
                                resolved_reward.map(|price| price.price).unwrap_or(0);

                            // Probability is for the GROUP.
                            // If the group has multiple items, it picks one?
                            // For now, let's assume it's additive value * (Probability / 100).
                            // This is an estimation.
                            let value = reward_price as f64 * g_count as f64;
                            expected_item_value += value * (probability as f64 / 100.0);
                        }
                    }
                }
            }

            let revenue = gil_reward + expected_item_value as i64;
            let profit = revenue - cost;

            results.push(LeveProfitData {
                leve,
                craft_leve,
                profit: profit as i32,
                cost: cost as i32,
                revenue: revenue as i32,
                reward_fallback,
                market_price,
                hq,
                listing_price,
                price_fallback,
                cost_pending,
                revenue_pending,
                cheapest_world_id,
                item_id: ItemId(item_id),
                item_count,
                class_job_level: leve.class_job_level as u16,
                job_category_name,
                avg_price: sales_stats.avg_price,
                daily_sales: sales_stats.daily_sales,
            });
        }

        // Keep every eligible row; the grid virtualizes rendering, not the result set.
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        results.sort_by(|a, b| {
            let order = compare_leves(mode, a, b);
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
            FILTER_PROFIT => t_string!(i18n, leve_analyzer_filter_profit_min_label).to_string(),
            FILTER_JOB => t_string!(i18n, leve_analyzer_filter_job_label).to_string(),
            FILTER_OUTLIERS => t_string!(i18n, leve_analyzer_filter_outliers).to_string(),
            _ => String::new(),
        }
    };

    // Localized label for one job-select value.
    let job_label = move |value: &str| -> String {
        match value {
            "Carpenter" => t_string!(i18n, carpenter).to_string(),
            "Blacksmith" => t_string!(i18n, blacksmith).to_string(),
            "Armorer" => t_string!(i18n, armorer).to_string(),
            "Goldsmith" => t_string!(i18n, goldsmith).to_string(),
            "Leatherworker" => t_string!(i18n, leatherworker).to_string(),
            "Weaver" => t_string!(i18n, weaver).to_string(),
            "Alchemist" => t_string!(i18n, alchemist).to_string(),
            "Culinarian" => t_string!(i18n, culinarian).to_string(),
            other => other.to_string(),
        }
    };
    let job_chip_options = Memo::new(move |_| {
        JOB_VALUES
            .iter()
            .map(|v| (*v, job_label(v)))
            .collect::<Vec<_>>()
    });

    let filters = register_filters(
        vec![FilterAlias::integer("profit", "profit", FilterOp::Gte)],
        Signal::derive(move || {
            vec![
                price_control(
                    "cost-basis",
                    t_string!(i18n, market_turn_in_cost).to_string(),
                    market.window,
                    t_string!(i18n, market_listing_basis).to_string(),
                ),
                price_control(
                    "revenue",
                    t_string!(i18n, market_reward_value).to_string(),
                    market.window,
                    t_string!(i18n, market_listing_basis).to_string(),
                ),
                toggle_control(FILTER_OUTLIERS, filter_label(FILTER_OUTLIERS)),
                {
                    let mut f = ColumnFilter::new(FILTER_JOB, filter_label(FILTER_JOB), false);
                    f.options = job_chip_options.get();
                    f
                },
            ]
        }),
    );

    let presets = Signal::derive(move || leve_analyzer_presets(i18n));

    view! {
            <div class="flex flex-col gap-6">
                <div class="flex flex-wrap gap-3">
                    <MarketWindowControl window=market.window />

                </div>

                <ControlBar sticky=false
                    summary=move || {
                        view! {
                            <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                                {move || t!(i18n, leve_analyzer_result_count, n = move || filters.row_count())}
                            </span>
                        }
                        .into_any()
                    }
                    actions=move || {
                        view! {
                            <RealtimeStatus status=realtime_status last_update=last_update />
                            <GridSavedViews id="leve-analyzer-grid" presets=presets />
                        }
                            .into_any()
                    }

                    empty_label=Signal::derive(move || {
                        t_string!(i18n, leve_analyzer_no_filters_hint).to_string()
                    })
                />

                <div>
                    <MarketGrid show_saved_views=false market subject=Arc::new(move |(_, row): &(usize, Arc<LeveProfitData>)| {
        let mut subject = MarketSubject::new(row.item_id.0, row.hq, row.cheapest_world_id);
        subject.listing_price = row.listing_price;
        subject.label = t_string!(i18n, market_turn_in_item).to_string();
        subject
     })
     metrics=leve_metrics()
     id="leve-analyzer-grid" label=t_string!(i18n, leve_analyzer_col_leve_item).to_string()
     row_height=60.0
     columns=Signal::derive(move || vec![GridColumn::new("item",t_string!(i18n, leve_analyzer_col_leve_item).to_string(), 320.0, false, true),
    { let mut col = GridColumn::new("profit",t_string!(i18n, leve_analyzer_col_profit).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::Profit, sort_dir.get().unwrap_or_else(||SortMode::Profit.default_dir()) == SortDir::Asc); col.filters.push(ColumnFilter::new("profit", filter_label("profit"), true)); col },
    GridColumn::new("revenue",t_string!(i18n, leve_analyzer_col_revenue).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::Revenue, sort_dir.get().unwrap_or_else(||SortMode::Revenue.default_dir()) == SortDir::Asc),
    GridColumn::new("cost",t_string!(i18n, leve_analyzer_col_cost).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::Cost, sort_dir.get().unwrap_or_else(||SortMode::Cost.default_dir()) == SortDir::Asc),
    GridColumn::new("avg-price",t_string!(i18n, leve_analyzer_col_avg_price).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::AvgPrice, sort_dir.get().unwrap_or_else(||SortMode::AvgPrice.default_dir()) == SortDir::Asc),
    GridColumn::new("daily-sales",t_string!(i18n, leve_analyzer_col_daily_sales).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::DailySales, sort_dir.get().unwrap_or_else(||SortMode::DailySales.default_dir()) == SortDir::Asc),
    { let mut col = GridColumn::new("level",t_string!(i18n, leve_analyzer_col_level).to_string(), 130.0, true, true).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::Level, sort_dir.get().unwrap_or_else(||SortMode::Level.default_dir()) == SortDir::Asc); let mut filter = ColumnFilter::new("job", filter_label("job"), false); filter.options = job_chip_options.get(); col.filters.push(filter); col }])
     header=move |id| {match id {"item" => view! {<div  class="w-full min-w-0">{t!(i18n, leve_analyzer_col_leve_item)}</div>}.into_any(),
    "profit" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::Profit
                                    label=t_string!(i18n, leve_analyzer_col_profit).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "revenue" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::Revenue
                                    label=t_string!(i18n, leve_analyzer_col_revenue).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "cost" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::Cost
                                    label=t_string!(i18n, leve_analyzer_col_cost).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "avg-price" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::AvgPrice
                                    label=t_string!(i18n, leve_analyzer_col_avg_price).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "daily-sales" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::DailySales
                                    label=t_string!(i18n, leve_analyzer_col_daily_sales).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(),
    "level" => view! {<SortableHeaderCell embedded=true
                                    mode=SortMode::Level
                                    label=t_string!(i18n, leve_analyzer_col_level).to_string()
                                    class="w-full min-w-0"
                                    sort_mode
                                    sort_dir
                                 />}.into_any(), _ => ().into_any()}}
     each=computed_data
                        key=move |(_, data): &(usize, Arc<LeveProfitData>)| data.leve.key_id

     measure=move |(_, data): &(usize, Arc<LeveProfitData>), id| {match id {"item" => (data.leve.name.as_str().to_string(), 110.0),
    "profit" => (data.profit.separate_with_commas(), 42.0),
    "revenue" => (data.revenue.separate_with_commas(), 42.0),
    "cost" => (data.cost.separate_with_commas(), 42.0),
    "avg-price" => (data.avg_price.separate_with_commas(), 42.0),
    "daily-sales" => (format!("{:.1}",data.daily_sales), 42.0),
    "level" => (format!("{} {}",data.class_job_level,data.job_category_name), 42.0), _ => (String::new(), 0.0)}}
     view=move |(index, data): (usize, Arc<LeveProfitData>), id| {
                            let item_id = data.item_id;
                            let item = items.get(&item_id).map(|i| i.name.as_str().to_string()).unwrap_or_else(|| t_string!(i18n, unknown).to_string());
                            let leve_name = data.leve.name.as_str();

     let _ = index;
     match id {"item" => view! {<div  class="flex flex-row items-center gap-2 w-full min-w-0">
                                         <a
                                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                            href=format!("/item/{}/{}", world(), item_id.0)
                                        >
                                            <div class="shrink-0">
                                                <ItemIcon item_id=item_id.0 icon_size=IconSize::Small />
                                            </div>
                                            <div class="flex flex-col truncate">
                                                <span class="font-semibold">{leve_name}</span>
                                                <span class="text-xs text-[color:var(--color-text-muted)] truncate">
                                                    {item} {t!(i18n, leve_analyzer_quantity_x)} {data.item_count}
                                                </span>
                                            </div>
                                        </a>
                                    </div>}.into_any(),
    "profit" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.profit />
                                    </div>}.into_any(),
    "revenue" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.revenue />
                                        {data.reward_fallback.then(|| view! { <span class="block text-xs text-amber-300">{t!(i18n, market_listing_fallback)}</span> })}
                                    </div>}.into_any(),
    "cost" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.cost />
                                        {data.price_fallback.then(|| view! { <span class="block text-xs text-amber-300">{t!(i18n, market_listing_fallback)}</span> })}
                                    </div>}.into_any(),
    "avg-price" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.avg_price />
                                    </div>}.into_any(),
    "daily-sales" => view! {<div  class="text-right w-full min-w-0">
                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                            {t!(i18n, leve_analyzer_sales_per_day, sales = format!("{:.1}", data.daily_sales))}
                                        </span>
                                    </div>}.into_any(),
    "level" => view! {<div  class="text-right w-full min-w-0">
                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                            {t!(i18n, leve_analyzer_lv)} {data.class_job_level} " " {data.job_category_name.clone()}
                                        </span>
                                    </div>}.into_any(), _ => ().into_any()}}
     />
                 </div>
            </div>
        }
}

#[component]
pub fn LeveAnalyzer() -> impl IntoView {
    provide_grid_saved_views("leve-analyzer-grid");
    let i18n = use_i18n();
    let (selected_world, set_selected_world) = use_analyzer_world("/leve-analyzer");
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
            <MetaTitle title=move || t_string!(i18n, leve_analyzer_meta_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, leve_analyzer_meta_desc).to_string() />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, leve_analyzer).to_string()
                    summary=t_string!(i18n, leve_analyzer_tool_summary).to_string()
                    context=t_string!(i18n, leve_analyzer_tool_context).to_string()
                    help_href="/help/leve-analyzer"
                    help_body=t_string!(i18n, leve_analyzer_tool_help).to_string()
                    calculation=ToolCalculation::new(
                        t_string!(i18n, leve_analyzer_calc_title).to_string(),
                        t_string!(i18n, leve_analyzer_calc_formula).to_string(),
                        t_string!(i18n, leve_analyzer_calc_details).to_string(),
                    )
                    assumptions=vec![
                        t_string!(i18n, leve_analyzer_assumption_baseline_nq).to_string(),
                        t_string!(i18n, leve_analyzer_assumption_expected_value).to_string(),
                        t_string!(i18n, leve_analyzer_assumption_recent_sales).to_string(),
                    ]
                >
                    <Suspense fallback=InlineStatusSkeleton>
                        {move || {
                            recent_sales_clone
                                .get()
                                .and_then(|r| r.err())
                                .map(|_| view! { <div class="text-red-400 text-sm">{t!(i18n, leve_analyzer_error_sales)}</div> })
                        }}
                    </Suspense>
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, leve_analyzer_select_world)}</label>
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
                                    <LeveAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=Some(sales)
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            (Some(Ok(listings)), _) => {
                                view! {
                                    <LeveAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=None
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            (Some(Err(e)), _) => {
                                view! {
                                    <div class="text-red-400">
                                        {t!(i18n, leve_analyzer_error_listings)} {e.to_string()}
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
            SortMode::Revenue,
            SortMode::Cost,
            SortMode::AvgPrice,
            SortMode::DailySales,
        ] {
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert!("bogus".parse::<SortMode>().is_err());
    }
}
