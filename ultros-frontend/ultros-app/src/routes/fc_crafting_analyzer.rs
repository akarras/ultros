use crate::analysis::{SalesStats, analyze_sales, roi_badge_class};
use crate::components::crafting_cost::{
    CRYSTAL_SEARCH_CATEGORY, CraftingCostOptions, EmptyOnHand, OnHand, ShardsMode,
    compute_ingredient_cost,
};
use crate::components::on_hand_input::{ActiveListBanner, LocalOnHand, OnHandMap};
use crate::global_state::cookies::Cookies;
use crate::global_state::craft_options::{self, CraftOptions};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::{DEFAULT_MIN_DAILY_SALES, filter_query_signal, seed_query_default};
use crate::ws::realtime::use_realtime;
use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        control_bar::{ControlBar, FilterOption},
        filter_chip::FilterChip,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortableHeaderCell, sort_and_truncate},
        tool_help::*,
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::{home_world::use_home_world, region_for_world::use_region_for_world},
};
use leptos::prelude::*;
use leptos_meta::{Meta, Title};
use leptos_router::hooks::{query_signal, use_params_map};
use std::{cmp::Ordering, collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
};
use xiv_gen::{
    CompanyCraftPartId, CompanyCraftProcessId, CompanyCraftSequence, CompanyCraftSupplyItemId,
    ItemId,
};

#[derive(Clone, Debug, PartialEq)]
struct MaterialInfo {
    item_id: ItemId,
    total_quantity: i32,
    unit_cost: i32,
}

#[derive(Clone, Debug, PartialEq)]
struct FCCraftProfitData {
    sequence: &'static CompanyCraftSequence,
    profit: i32,
    return_on_investment: i32,
    cost: i32,
    market_price: i32,
    cheapest_world_id: i32,
    materials: Vec<MaterialInfo>,
    daily_sales: f32,
    avg_price: i32,
    total_sales: usize,
    shard_cost: i32,
    on_hand_savings: i32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
    Velocity,
    TotalCost,
    MarketPrice,
}

impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            "velocity" => Ok(SortMode::Velocity),
            "cost" => Ok(SortMode::TotalCost),
            "price" => Ok(SortMode::MarketPrice),
            _ => Err(()),
        }
    }
}

impl Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Roi => "roi",
            SortMode::Profit => "profit",
            SortMode::Velocity => "velocity",
            SortMode::TotalCost => "cost",
            SortMode::MarketPrice => "price",
        };
        f.write_str(val)
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }

    /// Total cost reads best-first ascending — the cheapest project is the
    /// interesting one. Everything else is a biggest-first metric.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::TotalCost => SortDir::Asc,
            _ => SortDir::Desc,
        }
    }
}

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_PROFIT: &str = "profit";
const FILTER_ROI: &str = "roi";
const FILTER_MIN_SALES: &str = "min-sales";
const FILTER_EXCLUDE_SHARDS: &str = "shards-exclude";
const FILTER_USE_ON_HAND: &str = "on-hand";

/// Filters the `+ Filter` menu can add, in menu order.
const ADDABLE_FILTERS: &[&str] = &[
    FILTER_PROFIT,
    FILTER_ROI,
    FILTER_MIN_SALES,
    FILTER_EXCLUDE_SHARDS,
    FILTER_USE_ON_HAND,
];

fn compare_fc_crafts(mode: SortMode, a: &FCCraftProfitData, b: &FCCraftProfitData) -> Ordering {
    match mode {
        SortMode::Roi => a.return_on_investment.cmp(&b.return_on_investment),
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Velocity => a
            .daily_sales
            .partial_cmp(&b.daily_sales)
            .unwrap_or(Ordering::Equal),
        SortMode::TotalCost => a.cost.cmp(&b.cost),
        SortMode::MarketPrice => a.market_price.cmp(&b.market_price),
    }
}

fn calculate_fc_project_cost(
    sequence: &'static CompanyCraftSequence,
    prices: &CheapestListingsMap,
    data: &'static xiv_gen::Data,
    opts: &CraftingCostOptions<'_>,
) -> (
    i32,
    Vec<MaterialInfo>,
    i32, /* shard_cost */
    i32, /* on_hand_savings */
) {
    let mut materials_map: HashMap<ItemId, i32> = HashMap::new();

    for part_id in sequence.company_craft_part {
        if let Some(part) = data.company_craft_parts.get(&CompanyCraftPartId(part_id)) {
            for process_link in part.company_craft_process {
                if let Some(process) = data
                    .company_craft_processs
                    .get(&CompanyCraftProcessId(process_link))
                {
                    for i in 0..12 {
                        let supply_item_link = process.supply_item[i];
                        let quantity_per_set = process.set_quantity[i];
                        let sets_required = process.sets_required[i];
                        if quantity_per_set == 0 || sets_required == 0 {
                            continue;
                        }
                        if let Some(supply_item) = data
                            .company_craft_supply_items
                            .get(&CompanyCraftSupplyItemId(supply_item_link))
                        {
                            if supply_item.item == 0 {
                                continue;
                            }
                            let total_quantity = quantity_per_set * sets_required;
                            *materials_map.entry(ItemId(supply_item.item)).or_default() +=
                                total_quantity;
                        }
                    }
                }
            }
        }
    }

    let mut total_cost: i64 = 0;
    let mut shard_cost: i64 = 0;
    let mut on_hand_savings: i64 = 0;
    let mut material_infos = Vec::new();

    for (item_id, quantity) in materials_map {
        let line = compute_ingredient_cost(item_id, quantity, prices, opts);
        let is_shard = data
            .items
            .get(&item_id)
            .map(|i| i.item_search_category == CRYSTAL_SEARCH_CATEGORY)
            .unwrap_or(false);

        let line_market = (line.used_from_market as i64) * (line.unit_price as i64);
        let line_on_hand = (line.used_from_on_hand as i64) * (line.unit_price as i64);

        if is_shard {
            shard_cost = shard_cost.saturating_add(line_market + line_on_hand);
            if matches!(opts.shards, ShardsMode::IncludeMarket) {
                total_cost = total_cost.saturating_add(line_market);
                on_hand_savings = on_hand_savings.saturating_add(line_on_hand);
            }
        } else {
            total_cost = total_cost.saturating_add(line_market);
            on_hand_savings = on_hand_savings.saturating_add(line_on_hand);
        }

        material_infos.push(MaterialInfo {
            item_id,
            total_quantity: quantity,
            unit_cost: line.unit_price,
        });
    }

    let clamp = |v: i64| -> i32 {
        if v > i32::MAX as i64 {
            i32::MAX
        } else if v < 0 {
            0
        } else {
            v as i32
        }
    };

    (
        clamp(total_cost),
        material_infos,
        clamp(shard_cost),
        clamp(on_hand_savings),
    )
}

#[component]
fn FCCraftingAnalyzerTable(
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
    let sequences = &data.company_craft_sequences;

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    // Filter params use `filter_query_signal` (replace: true, scroll: false):
    // editing a chip writes the URL on every keystroke, and plain
    // `query_signal`'s defaults would push a history entry and yank the
    // window to the top each time.
    let (minimum_profit, set_minimum_profit) = filter_query_signal::<i32>(FILTER_PROFIT);
    let (minimum_roi, set_minimum_roi) = filter_query_signal::<i32>(FILTER_ROI);
    // Seeded by FCCraftingAnalyzer so a first-time visitor isn't shown recipes
    // whose output sells once a month. Same velocity floor as the analyzer's
    // 1d default.
    let (min_daily_sales, set_min_daily_sales) = filter_query_signal::<f32>(FILTER_MIN_SALES);
    let (exclude_shards_url, set_exclude_shards) =
        filter_query_signal::<bool>(FILTER_EXCLUDE_SHARDS);
    let (use_on_hand_url, set_use_on_hand) = filter_query_signal::<bool>(FILTER_USE_ON_HAND);
    let cookies = use_context::<Cookies>().unwrap();
    let (craft_options, _) =
        cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME);
    let exclude_shards_enabled = move || {
        exclude_shards_url()
            .unwrap_or_else(|| craft_options.get().unwrap_or_default().exclude_shards)
    };
    let use_on_hand_enabled = move || {
        use_on_hand_url().unwrap_or_else(|| craft_options.get().unwrap_or_default().use_on_hand)
    };

    // A filter picked from the `+ Filter` menu but not yet committed — its
    // chip mounts in edit state with an empty input (see currency_exchange.rs
    // for the same pattern). The two on/off toggles commit immediately on add
    // instead, so this only ever holds a numeric filter id.
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

    let computed_data = Memo::new(move |_| {
        let sales_map: HashMap<i32, Vec<&SaleData>> = if let Some(ref sales) = recent_sales {
            let mut map: HashMap<i32, Vec<&SaleData>> = HashMap::new();
            for sale in &sales.sales {
                map.entry(sale.item_id).or_default().push(sale);
            }
            map
        } else {
            HashMap::new()
        };

        // Hoist context lookups ONCE; the on-hand SNAPSHOT is rebuilt
        // per sequence inside the loop because compute_ingredient_cost consumes it.
        let opts_value = craft_options.get().unwrap_or_default();
        let shards = if exclude_shards_enabled() {
            ShardsMode::ExcludeShards
        } else {
            ShardsMode::IncludeMarket
        };
        let on_hand_map = use_context::<OnHandMap>();
        let use_on_hand = use_on_hand_enabled();

        let mut results = Vec::new();

        for sequence in sequences.values() {
            // result_item can be 0 for some incomplete data, skip those
            if sequence.result_item == 0 {
                continue;
            }

            let sales_stats = if let Some(item_sales) = sales_map.get(&{ sequence.result_item }) {
                analyze_sales(item_sales, false)
            } else {
                SalesStats {
                    daily_sales: 0.0,
                    avg_price: 0,
                    total_sales: 0,
                }
            };

            let market_price_summary = prices.find_matching_listings(sequence.result_item);
            let market_price = market_price_summary.lowest_gil().unwrap_or(0);

            if market_price == 0 {
                continue;
            }

            let cheapest_world_id = market_price_summary
                .lq
                .map(|d| d.world_id)
                .or(market_price_summary.hq.map(|d| d.world_id))
                .unwrap_or(0);

            // Fresh on-hand snapshot per sequence — compute_ingredient_cost consumes
            // from the snapshot, and reusing one across sequences would wrongly deplete
            // the user's stockpile after the first sequence.
            let local = on_hand_map
                .map(|m: OnHandMap| LocalOnHand::from_map(m.0.get_untracked()))
                .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
            let empty = EmptyOnHand;
            // TODO(follow-up): when active_craft_list is Some, fetch the list resource
            // and construct ListOnHand from its items instead of falling through to LocalOnHand.
            // The type (ListOnHand) is in place; the async resource fetch is the missing piece.
            let active: Box<dyn OnHand> = match opts_value.active_craft_list {
                Some(_list_id) if use_on_hand => {
                    // List fetch is async-resourced separately; for the first cut,
                    // fall through to LocalOnHand if the resource isn't ready yet.
                    // (Plumbing the resource in is left for a follow-up — flagged
                    //  in the roadmap section of the spec.)
                    Box::new(local)
                }
                _ if use_on_hand => Box::new(local),
                _ => Box::new(empty),
            };
            let opts = CraftingCostOptions {
                require_hq: false,
                max_subcraft_depth: 0,
                shards,
                on_hand: active.as_ref(),
            };

            let (cost, materials, shard_cost, on_hand_savings) =
                calculate_fc_project_cost(sequence, &prices, data, &opts);

            if cost == 0 {
                // Cost 0 means probably missing data or no materials required (unlikely for valid projects)
                continue;
            }

            if cost >= market_price {
                continue;
            }

            let profit = market_price - cost;
            let roi = if cost > 0 {
                (profit as f64 / cost as f64 * 100.0) as i32
            } else {
                0
            };

            results.push(FCCraftProfitData {
                sequence,
                profit,
                return_on_investment: roi,
                cost,
                market_price,
                cheapest_world_id,
                materials,
                daily_sales: sales_stats.daily_sales,
                avg_price: sales_stats.avg_price,
                total_sales: sales_stats.total_sales,
                shard_cost,
                on_hand_savings,
            });
        }

        // Filter
        if let Some(min) = minimum_profit() {
            results.retain(|d| d.profit >= min);
        }
        if let Some(min) = minimum_roi() {
            results.retain(|d| d.return_on_investment >= min);
        }
        if let Some(min_sales) = min_daily_sales() {
            results.retain(|d| d.daily_sales >= min_sales);
        }

        // Sort
        // ⚡ Bolt: Optimization: In-place filtering and truncation for Top N lists using select_nth_unstable.
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        sort_and_truncate(&mut results, dir, 100, |a, b| compare_fc_crafts(mode, a, b));

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
        if minimum_roi().is_some() || pending_filter.get() == Some(FILTER_ROI) {
            active.push(FILTER_ROI);
        }
        if min_daily_sales().is_some() || pending_filter.get() == Some(FILTER_MIN_SALES) {
            active.push(FILTER_MIN_SALES);
        }
        // These two only show a chip once the URL explicitly overrides the
        // cookie default — otherwise the page is silently using the user's
        // saved crafting-cost preference, not filtering anything.
        if exclude_shards_url().is_some() {
            active.push(FILTER_EXCLUDE_SHARDS);
        }
        if use_on_hand_url().is_some() {
            active.push(FILTER_USE_ON_HAND);
        }
        active
    });

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, fc_crafting_filter_profit_min_label).to_string(),
            FILTER_ROI => t_string!(i18n, fc_crafting_filter_roi_min_label).to_string(),
            FILTER_MIN_SALES => {
                t_string!(i18n, fc_crafting_filter_daily_sales_min_label).to_string()
            }
            FILTER_EXCLUDE_SHARDS => {
                t_string!(i18n, fc_crafting_filter_exclude_shards_label).to_string()
            }
            FILTER_USE_ON_HAND => t_string!(i18n, fc_crafting_filter_use_on_hand_label).to_string(),
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

    let on_off_options = move || {
        vec![
            ("true", t_string!(i18n, toolbar_pill_on).to_string()),
            ("false", t_string!(i18n, toolbar_pill_off).to_string()),
        ]
    };

    let add_filter = Callback::new(move |id: &'static str| match id {
        FILTER_PROFIT => pending_filter.set(Some(FILTER_PROFIT)),
        FILTER_ROI => pending_filter.set(Some(FILTER_ROI)),
        FILTER_MIN_SALES => pending_filter.set(Some(FILTER_MIN_SALES)),
        // On/off toggles: seed to the "On" state, same as the pill's
        // affirmative side — the user flips or clears it from there.
        FILTER_EXCLUDE_SHARDS => set_exclude_shards(Some(true)),
        FILTER_USE_ON_HAND => set_use_on_hand(Some(true)),
        _ => {}
    });

    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        set_minimum_profit(None);
        set_minimum_roi(None);
        set_min_daily_sales(None);
        set_exclude_shards(None);
        set_use_on_hand(None);
    });

    view! {
        <div class="flex flex-col gap-6">
            <ActiveListBanner />
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, fc_crafting_result_count, n = move || computed_data().len())}
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
                    t_string!(i18n, fc_crafting_no_filters_hint).to_string()
                })
                is_empty=Signal::derive(move || active_filters().is_empty())
            >
                {move || {
                    (minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_PROFIT);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, fc_crafting_chip_profit_min).to_string()
                                    value=Signal::derive(move || minimum_profit().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    step="100000"
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
                                    label=t_string!(i18n, fc_crafting_chip_roi_min).to_string()
                                    value=Signal::derive(move || minimum_roi().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
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
                    (min_daily_sales().is_some() || pending_filter.get() == Some(FILTER_MIN_SALES))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked()
                                == Some(FILTER_MIN_SALES);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, fc_crafting_chip_daily_sales_min).to_string()
                                    value=Signal::derive(move || min_daily_sales().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    step="0.1"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_min_daily_sales(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_MIN_SALES) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    exclude_shards_url()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, fc_crafting_filter_exclude_shards_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=on_off_options()
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_exclude_shards(v.and_then(|v| v.parse().ok()));
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    use_on_hand_url()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, fc_crafting_filter_use_on_hand_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=on_off_options()
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_use_on_hand(v.and_then(|v| v.parse().ok()));
                                    })
                                />
                            }
                        })
                }}
            </ControlBar>

            <div class="rounded-2xl panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                 <VirtualScroller
                    viewport_height=720.0
                    row_height=60.0
                    overscan=8
                    header_height=64.0
                    variable_height=true
                     header=view! {
                        <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                             <div role="columnheader" class="w-84 shrink-0 p-4">{t!(i18n, fc_crafting_analyzer_col_project_result)}</div>
                             <SortableHeaderCell
                                mode=SortMode::Profit
                                label=t_string!(i18n, fc_crafting_analyzer_col_profit).to_string()
                                class="w-30 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Roi
                                label=t_string!(i18n, fc_crafting_analyzer_col_roi).to_string()
                                class="w-30 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::TotalCost
                                label=t_string!(i18n, fc_crafting_analyzer_col_total_cost).to_string()
                                class="w-30 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::MarketPrice
                                label=t_string!(i18n, fc_crafting_analyzer_col_market_price).to_string()
                                class="w-30 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Velocity
                                label=t_string!(i18n, fc_crafting_analyzer_col_daily_sales).to_string()
                                class="w-30 shrink-0 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<FCCraftProfitData>)| (*index, data.sequence.key_id)
                    view=move |(index, data): (usize, Arc<FCCraftProfitData>)| {
                        let item_id = ItemId(data.sequence.result_item);
                        let item = items.get(&item_id).map(|i| i.name.as_str().to_string()).unwrap_or_else(|| t_string!(i18n, unknown).to_string());
                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-start flex-nowrap min-h-[60px] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-start flex-nowrap min-h-[60px] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };
                         let sales_tooltip = format!(
                            "Based on {} sales over {:.1} days",
                            data.total_sales,
                            (data.total_sales as f32 / data.daily_sales.max(0.001))
                        );
                        let material_rows = data
                            .materials
                            .iter()
                            .take(6)
                            .map(|material| {
                                let material_name = items
                                    .get(&material.item_id)
                                    .map(|item| item.name.as_str().to_string())
                                    .unwrap_or_else(|| "Unknown material".to_string());
                                (
                                    material_name,
                                    material.total_quantity,
                                    material.unit_cost,
                                )
                            })
                            .collect::<Vec<_>>();

                        view! {
                            <div class=classes role="row-group">
                                <div role="cell" class="px-4 py-2 flex flex-row w-84 shrink-0 items-center gap-2">
                                    <div class="flex flex-row items-center gap-2 min-w-0 w-full">
                                        <a
                                            class="shrink-0 hover:text-brand-300 transition-colors"
                                            href=format!("/item/{}/{}", world(), item_id.0)
                                        >
                                            <ItemIcon item_id=item_id.0 icon_size=IconSize::Small />
                                        </a>
                                        <div class="flex flex-col min-w-0">
                                            <a
                                                class="truncate hover:text-brand-300 transition-colors"
                                                href=format!("/item/{}/{}", world(), item_id.0)
                                            >
                                                {item}
                                            </a>
                                            <ResultBreakdownDisclosure title=t_string!(i18n, fc_crafting_disclosure_material_breakdown).to_string()>
                                                <div class="flex flex-col gap-1">
                                                    {material_rows.into_iter().map(|(name, qty, unit_cost)| view! {
                                                        <div class="flex justify-between gap-3">
                                                            <span class="truncate">{qty} "x " {name}</span>
                                                            <Gil amount=unit_cost />
                                                        </div>
                                                    }).collect_view()}
                                                </div>
                                            </ResultBreakdownDisclosure>
                                        </div>
                                    </div>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 shrink-0 text-right">
                                    <Gil amount=data.profit />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 shrink-0 text-right">
                                    <span class={roi_badge_class(data.return_on_investment)}>
                                        {format!("{}%", data.return_on_investment)}
                                    </span>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 shrink-0 text-right">
                                    <Gil amount=data.cost />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 shrink-0 text-right">
                                    <Gil amount=data.market_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 shrink-0 text-right hidden md:block">
                                    <div class="flex flex-col items-end gap-1" title=sales_tooltip>
                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                            {t!(i18n, fc_crafting_analyzer_sales_per_day, sales = format!("{:.1}", data.daily_sales))}
                                        </span>
                                        <ConfidenceBadge total_sales=data.total_sales daily_sales=data.daily_sales />
                                    </div>
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
pub fn FCCraftingAnalyzer() -> impl IntoView {
    let i18n = use_i18n();
    // Seeded here rather than in FCCraftingAnalyzerTable: that lives inside the
    // Suspense closure and remounts whenever its resources change, which would
    // keep undoing a filter the user had cleared.
    seed_query_default("min-sales", DEFAULT_MIN_DAILY_SALES);
    let params = use_params_map();
    let (home_world, _) = use_home_world();

    let region = use_region_for_world(move || params.with(|p| p.get("world").clone()));

    let global_cheapest_listings = ArcResource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

    let (selected_world, set_selected_world) = signal(None);
    Effect::new(move |_| {
        if selected_world.get_untracked().is_none()
            && let Some(home) = home_world.get()
        {
            set_selected_world(Some(home));
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
            <Title text=t_string!(i18n, fc_crafting_analyzer_meta_title).to_string() />
            <Meta name="description" content=t_string!(i18n, fc_crafting_analyzer_meta_desc).to_string() />

             <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, fc_crafting_analyzer_title).to_string()
                    summary=t_string!(i18n, fc_crafting_tool_summary).to_string()
                    context=t_string!(i18n, fc_crafting_tool_context).to_string()
                    help_href="/help/fc-crafting"
                    help_body=t_string!(i18n, fc_crafting_tool_help).to_string()
                />
                 <div class="flex flex-row justify-end items-center">
                    <div class="flex flex-row gap-2 items-center">
                        <Suspense fallback=move || view! { <div class="text-brand-300 text-sm animate-pulse">{t!(i18n, fc_crafting_analyzer_loading_sales)}</div> }>
                            {move || {
                                recent_sales_clone
                                    .get()
                                    .and_then(|r| r.err())
                                    .map(|_| view! { <div class="text-red-400 text-sm">{t!(i18n, fc_crafting_analyzer_error_sales)}</div> })
                            }}
                        </Suspense>
                    </div>
                </div>

                <Show when=move || selected_world.get().is_some()>
                    <div class="flex flex-col md:flex-row items-center gap-2">
                        <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, fc_crafting_analyzer_select_world)}</label>
                        <div class="w-full md:w-auto">
                            <WorldOnlyPicker
                                current_world=selected_world.into()
                                set_current_world=set_selected_world.into()
                            />
                        </div>
                    </div>
                </Show>
                <CalculationSummary
                    title=t_string!(i18n, fc_crafting_calc_title).to_string()
                    formula=t_string!(i18n, fc_crafting_calc_formula).to_string()
                    details=t_string!(i18n, fc_crafting_calc_details).to_string()
                />
                <div class="flex flex-wrap gap-2">
                    <AssumptionBadge text=t_string!(i18n, fc_crafting_assumption_market_prices).to_string() />
                    <AssumptionBadge text=t_string!(i18n, fc_crafting_assumption_sparse_sales).to_string() />
                    <AssumptionBadge text=t_string!(i18n, fc_crafting_assumption_labor_not_priced).to_string() />
                </div>

                 <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        let sales = recent_sales.get();
                        match (listings, sales) {
                            (Some(Ok(listings)), Some(Ok(sales))) => {
                                view! {
                                    <FCCraftingAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=Some(sales)
                                        world=Signal::derive(region)
                                    />
                                }.into_any()
                            }
                             (Some(Ok(listings)), _) => {
                                view! {
                                    <FCCraftingAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=None
                                        world=Signal::derive(region)
                                    />
                                }.into_any()
                            }
                            (Some(Err(e)), _) => {
                                view! {
                                    <div class="text-red-400">
                                        {t!(i18n, fc_crafting_analyzer_error_listings)} {e.to_string()}
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
            SortMode::Roi,
            SortMode::Profit,
            SortMode::Velocity,
            SortMode::TotalCost,
            SortMode::MarketPrice,
        ] {
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert!("bogus".parse::<SortMode>().is_err());
    }
}
