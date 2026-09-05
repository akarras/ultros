use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::ws::realtime::use_realtime;
use crate::{
    analysis::{SalesStats, analyze_sales},
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        control_bar::{ControlBar, FilterOption},
        filter_chip::FilterChip,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::{BoxSkeleton, InlineStatusSkeleton},
        sort_header::{SortColumn, SortDir, SortableHeaderCell, sort_and_truncate},
        tool_help::*,
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::{
        LocalWorldData, home_world::use_home_world, region_for_world::use_region_for_world,
    },
    query_defaults::filter_query_signal,
};
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_query_map},
};
use std::{cmp::Ordering, collections::HashMap, sync::Arc};
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
    market_price: i32,
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

/// Filters the `+ Filter` menu can add, in menu order.
const ADDABLE_FILTERS: &[&str] = &[FILTER_PROFIT, FILTER_JOB, FILTER_OUTLIERS];

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
    // Filter params use `filter_query_signal` (replace: true, scroll: false):
    // editing a chip writes the URL on every keystroke, and plain
    // `query_signal`'s defaults would push a history entry and yank the
    // window to the top each time.
    let (minimum_profit, set_minimum_profit) = filter_query_signal::<i32>(FILTER_PROFIT);
    let (job_filter, set_job_filter) = filter_query_signal::<String>(FILTER_JOB);
    let (filter_outliers, set_filter_outliers) = filter_query_signal::<bool>(FILTER_OUTLIERS);

    // A filter picked from the `+ Filter` menu but not yet committed — its
    // chip mounts in edit state with an empty input/selection (see
    // currency_exchange.rs for the same pattern). The boolean toggle commits
    // immediately on add instead, so this only ever holds `FILTER_PROFIT` or
    // `FILTER_JOB`.
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

    let computed_data = Memo::new(move |_| {
        let mut results = Vec::new();
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
            let market_price_summary = prices.find_matching_listings(item_id);
            // Default to high price if not found to discourage bad data
            let market_price = market_price_summary.lowest_gil().unwrap_or(0);

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

            let cheapest_world_id = market_price_summary
                .lq
                .map(|d| d.world_id)
                .or(market_price_summary.hq.map(|d| d.world_id))
                .unwrap_or(0);

            // Cost is price * count.
            // Note: If you turn in HQ, rewards are double. But let's assume NQ for baseline safety.
            // Or maybe add a toggle for HQ later. For now, assume NQ cost for NQ rewards.
            let cost = market_price as i64 * item_count as i64;

            // Calculate Revenue
            let gil_reward = leve.gil_reward as i64;

            // Calculate Item Rewards Expected Value
            let mut expected_item_value = 0.0;
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

                            let reward_price_summary =
                                prices.find_matching_listings(g_item_id as i32);
                            let reward_price = reward_price_summary.lowest_gil().unwrap_or(0);

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

            if let Some(min) = minimum_profit()
                && (profit as i32) < min
            {
                continue;
            }

            results.push(LeveProfitData {
                leve,
                craft_leve,
                profit: profit as i32,
                cost: cost as i32,
                revenue: revenue as i32,
                market_price,
                cheapest_world_id,
                item_id: ItemId(item_id),
                item_count,
                class_job_level: leve.class_job_level as u16,
                job_category_name,
                avg_price: sales_stats.avg_price,
                daily_sales: sales_stats.daily_sales,
            });
        }

        // Sort
        // ⚡ Bolt: Optimization: In-place filtering and truncation for Top N lists using select_nth_unstable.
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        sort_and_truncate(&mut results, dir, 100, |a, b| compare_leves(mode, a, b));

        results
            .into_iter()
            .map(Arc::new)
            .enumerate()
            .collect::<Vec<_>>()
    });

    // Filters currently drawn as a chip. Drives the "no active filters" hint
    // and keeps `+ Filter` from offering a second copy of something the user
    // can already see.
    let active_filters = Memo::new(move |_| {
        let mut active: Vec<&'static str> = Vec::new();
        if minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT) {
            active.push(FILTER_PROFIT);
        }
        if job_filter().is_some() || pending_filter.get() == Some(FILTER_JOB) {
            active.push(FILTER_JOB);
        }
        if filter_outliers().unwrap_or(false) {
            active.push(FILTER_OUTLIERS);
        }
        active
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

    let add_filter = Callback::new(move |id: &'static str| match id {
        FILTER_PROFIT => pending_filter.set(Some(FILTER_PROFIT)),
        FILTER_JOB => pending_filter.set(Some(FILTER_JOB)),
        // Boolean toggle: the chip's presence *is* the value, so it commits
        // straight to `true` rather than mounting an editable chip.
        FILTER_OUTLIERS => set_filter_outliers(Some(true)),
        _ => {}
    });

    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        set_minimum_profit(None);
        set_job_filter(None);
        set_filter_outliers(None);
    });

    view! {
        <div class="flex flex-col gap-6">
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, leve_analyzer_result_count, n = move || computed_data().len())}
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
                    t_string!(i18n, leve_analyzer_no_filters_hint).to_string()
                })
                is_empty=Signal::derive(move || active_filters().is_empty())
            >
                {move || {
                    (minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_PROFIT);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, leve_analyzer_chip_profit_min).to_string()
                                    value=Signal::derive(move || minimum_profit().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
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
                    (job_filter().is_some() || pending_filter.get() == Some(FILTER_JOB))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_JOB);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, leve_analyzer_filter_job_label).to_string()
                                    value=Signal::derive(job_filter)
                                    options=job_chip_options.get()
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_job_filter(v);
                                        if pending_filter.get_untracked() == Some(FILTER_JOB) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    filter_outliers()
                        .unwrap_or(false)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, leve_analyzer_filter_outliers).to_string()
                                    readonly=true
                                    value=Signal::derive(|| None::<String>)
                                    on_commit=Callback::new(move |_| set_filter_outliers(None))
                                />
                            }
                        })
                }}
            </ControlBar>

            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                    viewport_height=720.0
                    row_height=60.0
                    overscan=8
                    header_height=64.0
                    variable_height=false
                    header=view! {
                        <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                             <div role="columnheader" class="w-84 p-4">{t!(i18n, leve_analyzer_col_leve_item)}</div>
                             <SortableHeaderCell
                                mode=SortMode::Profit
                                label=t_string!(i18n, leve_analyzer_col_profit).to_string()
                                class="w-30 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Revenue
                                label=t_string!(i18n, leve_analyzer_col_revenue).to_string()
                                class="w-30 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Cost
                                label=t_string!(i18n, leve_analyzer_col_cost).to_string()
                                class="w-30 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::AvgPrice
                                label=t_string!(i18n, leve_analyzer_col_avg_price).to_string()
                                class="w-30 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::DailySales
                                label=t_string!(i18n, leve_analyzer_col_daily_sales).to_string()
                                class="w-30 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Level
                                label=t_string!(i18n, leve_analyzer_col_level).to_string()
                                class="w-40 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<LeveProfitData>)| (*index, data.leve.key_id)
                    view=move |(index, data): (usize, Arc<LeveProfitData>)| {
                        let item_id = data.item_id;
                        let item = items.get(&item_id).map(|i| i.name.as_str().to_string()).unwrap_or_else(|| t_string!(i18n, unknown).to_string());
                        let leve_name = data.leve.name.as_str();

                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };

                        view! {
                            <div class=classes role="row-group">
                                <div role="cell" class="px-4 py-2 flex flex-row w-84 items-center gap-2">
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
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.profit />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.revenue />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.cost />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right hidden md:block">
                                    <Gil amount=data.avg_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]">
                                        {t!(i18n, leve_analyzer_sales_per_day, sales = format!("{:.1}", data.daily_sales))}
                                    </span>
                                </div>
                                <div role="cell" class="px-4 py-2 w-40 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]">
                                        {t!(i18n, leve_analyzer_lv)} {data.class_job_level} " " {data.job_category_name.clone()}
                                    </span>
                                </div>
                            </div>
                        }.into_any()
                    }
                />
             </div>
        </div>
    }
}

#[component]
pub fn LeveAnalyzer() -> impl IntoView {
    let i18n = use_i18n();
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let nav = use_navigate();

    let region = use_region_for_world(move || query.with(|p| p.get("world").clone()));

    let global_cheapest_listings = ArcResource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0
        .unwrap();

    let initial_world = query.with_untracked(|p| {
        let binding = p.get("world");
        let world = binding.as_deref().unwrap_or_default();
        worlds
            .lookup_world_by_name(world)
            .and_then(|w| w.as_world().cloned())
    });

    let (selected_world, set_selected_world) = signal(initial_world);

    // If no world is selected initially, try to use home world
    Effect::new(move |_| {
        if selected_world.get_untracked().is_none()
            && let Some(home) = home_world.get()
        {
            set_selected_world(Some(home));
        }
    });

    // When selected world changes, update the URL
    Effect::new(move |_| {
        if let Some(world) = selected_world.get() {
            let world_name = world.name;
            let current_query = query.get_untracked();
            let world_matches = current_query
                .get("world")
                .map(|s| s == world_name)
                .unwrap_or(false);

            if !world_matches {
                let mut query_string = format!("?world={}", world_name);
                for (k, v) in current_query.into_iter() {
                    if k != "world" {
                        query_string.push_str(&format!("&{}={}", k, v));
                    }
                }
                nav(
                    &query_string,
                    NavigateOptions {
                        scroll: false,
                        ..Default::default()
                    },
                );
            }
        }
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
                    <WorldOnlyPicker
                        current_world=selected_world.into()
                        set_current_world=set_selected_world.into()
                    />
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
