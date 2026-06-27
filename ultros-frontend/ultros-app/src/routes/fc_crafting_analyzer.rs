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
use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        gil::*,
        item_icon::*,
        query_button::QueryButton,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        tool_help::*,
        toolbar::{Toolbar, ToolbarField, ToolbarPills, ToolbarSpacer},
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    error::AppResult,
    global_state::{home_world::use_home_world, region_for_world::use_region_for_world},
    ws::realtime::{RealtimeSubscription, use_realtime},
};
use leptos::prelude::*;
use leptos_meta::{Meta, Title};
use leptos_router::hooks::{query_signal, use_params_map};
use std::{cmp::Reverse, collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
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
}

impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            "velocity" => Ok(SortMode::Velocity),
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
        };
        f.write_str(val)
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
    global_cheapest_listings_resource: ArcResource<AppResult<CheapestListings>>,
    recent_sales_resource: ArcResource<AppResult<RecentSales>>,
    world: Signal<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let global_res_for_memo = global_cheapest_listings_resource.clone();
    let prices = Memo::new(move |_| {
        global_res_for_memo
            .get()
            .and_then(|r| r.ok())
            .map(|listings| CheapestListingsMap::from(listings.clone()))
    });
    let data = tracked_data();
    let items = &data.items;
    let sequences = &data.company_craft_sequences;
    let (realtime_status, set_realtime_status) = signal("connecting".to_string());
    let (last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);

    let realtime = use_realtime();
    let market_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let global_res_capture = global_cheapest_listings_resource.clone();
    let recent_res_capture = recent_sales_resource.clone();
    Effect::new(move |_| {
        market_subscription.update_value(|sub| *sub = None);
        let world_name = world.get();
        let Some(realtime) = realtime.clone() else {
            set_realtime_status.set("offline".to_string());
            return;
        };
        let worlds = use_context::<crate::global_state::LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        let Some(selector) = worlds
            .lookup_world_by_name(&world_name)
            .map(|world| ultros_api_types::world_helper::AnySelector::from(&world))
        else {
            return;
        };

        let filter = ultros_api_types::websocket::FilterPredicate::World(selector);
        let recent_res = recent_res_capture.clone();
        let global_res = global_res_capture.clone();
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
                        recent_res.refetch();
                    }
                    ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                        set_realtime_status.set("reconnecting".to_string());
                        set_last_update_at.set(Some(chrono::Utc::now()));
                        recent_res.refetch();
                        global_res.refetch();
                    }
                    _ => {}
                }
            },
        );
        market_subscription.set_value(Some(sub));
    });

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (min_daily_sales, set_min_daily_sales) = query_signal::<f32>("min-sales");
    let (exclude_shards_url, set_exclude_shards) = query_signal::<bool>("shards-exclude");
    let (use_on_hand_url, set_use_on_hand) = query_signal::<bool>("on-hand");
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

    let computed_data = Memo::new(move |_| {
        let prices = match prices.get() {
            Some(p) => p,
            None => return vec![],
        };
        let recent_sales = match recent_sales_resource.get().and_then(|r| r.ok()) {
            Some(s) => s,
            None => return vec![],
        };

        let mut sales_map: HashMap<i32, Vec<&SaleData>> = HashMap::new();
        for sale in &recent_sales.sales {
            sales_map
                .entry(sale.item_id)
                .or_insert_with(Vec::new)
                .push(sale);
        }

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
        match sort_mode().unwrap_or(SortMode::Profit) {
            SortMode::Roi => results.sort_by_key(|d| Reverse(d.return_on_investment)),
            SortMode::Profit => results.sort_by_key(|d| Reverse(d.profit)),
            SortMode::Velocity => results.sort_by(|a, b| {
                b.daily_sales
                    .partial_cmp(&a.daily_sales)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }

        results
            .into_iter()
            .take(100)
            .map(Arc::new)
            .enumerate()
            .collect::<Vec<_>>()
    });

    view! {
        <div class="flex flex-col gap-6">
            <ActiveListBanner />
            <Toolbar>
                <ToolbarField label=t_string!(i18n, fc_crafting_filter_profit_min_label).to_string()>
                    <input
                        class="input input-sm w-32"
                        min=0
                        step=100000
                        type="number"
                        placeholder=t_string!(i18n, placeholder_eg_100000)
                        prop:value=minimum_profit
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(profit) = value.parse::<i32>() {
                                set_minimum_profit(Some(profit))
                            } else if value.is_empty() {
                                set_minimum_profit(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, fc_crafting_filter_roi_min_label).to_string()>
                    <input
                        class="input input-sm w-28"
                        min=0
                        step=10
                        type="number"
                        placeholder=t_string!(i18n, placeholder_eg_50)
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
                <ToolbarField label=t_string!(i18n, fc_crafting_filter_daily_sales_min_label).to_string()>
                    <input
                        class="input input-sm w-28"
                        type="number"
                        min="0"
                        step="0.1"
                        placeholder=t_string!(i18n, fc_crafting_placeholder_0_1)
                        prop:value=min_daily_sales
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(s) = value.parse::<f32>() {
                                set_min_daily_sales(Some(s));
                            } else if value.is_empty() {
                                set_min_daily_sales(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, fc_crafting_filter_exclude_shards_label).to_string()>
                    <ToolbarPills>
                        <button
                            aria-pressed=move || if exclude_shards_enabled() { "false" } else { "true" }
                            title=t_string!(i18n, tooltip_exclude_shards)
                            on:click=move |_| set_exclude_shards(Some(!exclude_shards_enabled()))
                        >
                            "Off"
                        </button>
                        <button
                            aria-pressed=move || if exclude_shards_enabled() { "true" } else { "false" }
                            title=t_string!(i18n, tooltip_exclude_shards)
                            on:click=move |_| set_exclude_shards(Some(!exclude_shards_enabled()))
                        >
                            "On"
                        </button>
                    </ToolbarPills>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, fc_crafting_filter_use_on_hand_label).to_string()>
                    <ToolbarPills>
                        <button
                            aria-pressed=move || if use_on_hand_enabled() { "false" } else { "true" }
                            title=t_string!(i18n, tooltip_use_on_hand)
                            on:click=move |_| set_use_on_hand(Some(!use_on_hand_enabled()))
                        >
                            "Off"
                        </button>
                        <button
                            aria-pressed=move || if use_on_hand_enabled() { "true" } else { "false" }
                            title=t_string!(i18n, tooltip_use_on_hand)
                            on:click=move |_| set_use_on_hand(Some(!use_on_hand_enabled()))
                        >
                            "On"
                        </button>
                    </ToolbarPills>
                </ToolbarField>
                <ToolbarSpacer />
                <RealtimeStatus
                    status=realtime_status
                    last_update=last_update_at
                />
            </Toolbar>

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
                             <div role="columnheader" class="w-30 shrink-0 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="profit"
                                >
                                    {t!(i18n, fc_crafting_analyzer_col_profit)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 shrink-0 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="roi"
                                >
                                    {t!(i18n, fc_crafting_analyzer_col_roi)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 shrink-0 p-4">{t!(i18n, fc_crafting_analyzer_col_total_cost)}</div>
                             <div role="columnheader" class="w-30 shrink-0 p-4">{t!(i18n, fc_crafting_analyzer_col_market_price)}</div>
                             <div role="columnheader" class="w-30 shrink-0 p-4 hidden md:block">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="velocity"
                                >
                                    {t!(i18n, fc_crafting_analyzer_col_daily_sales)}
                                </QueryButton>
                             </div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<FCCraftProfitData>)| (*index, data.sequence.key_id)
                    view=move |(index, data): (usize, Arc<FCCraftProfitData>)| {
                        let data_clone = data.clone();
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
                                    <span class={
                                        let data = data_clone.clone();
                                        move || roi_badge_class(data.return_on_investment)
                                    }>
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
                        let global_resource = global_cheapest_listings.clone();
                        let recent_resource = recent_sales.clone();
                        match (global_resource.get(), recent_resource.get()) {
                            (Some(_), Some(_)) => {
                                view! {
                                    <FCCraftingAnalyzerTable
                                        global_cheapest_listings_resource=global_resource
                                        recent_sales_resource=recent_resource
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
