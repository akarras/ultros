use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        filter_card::*, gil::*, item_icon::*, query_button::QueryButton, skeleton::BoxSkeleton,
        virtual_scroller::*, world_picker::WorldOnlyPicker,
    },
    global_state::{LocalWorldData, home_world::use_home_world},
};
use chrono::Utc;
use leptos::{either::Either, prelude::*};
use leptos_meta::{Meta, Title};
use leptos_router::hooks::{query_signal, use_params_map};
use std::{cmp::Reverse, collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
    world_helper::AnyResult,
};
use xiv_gen::{CompanyCraftSequence, ItemId};

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
) -> (i32, Vec<MaterialInfo>) {
    let mut total_cost: i64 = 0;
    let mut materials_map: HashMap<ItemId, i32> = HashMap::new();

    let parts = [
        &sequence.company_craft_part_0,
        &sequence.company_craft_part_1,
        &sequence.company_craft_part_2,
        &sequence.company_craft_part_3,
        &sequence.company_craft_part_4,
        &sequence.company_craft_part_5,
        &sequence.company_craft_part_6,
        &sequence.company_craft_part_7,
    ];

    for part_link in parts {
        if let Some(part) = data.company_craft_parts.get(part_link) {
            let processes = [
                &part.company_craft_process_0,
                &part.company_craft_process_1,
                &part.company_craft_process_2,
            ];

            for process_link in processes {
                if let Some(process) = data.company_craft_processs.get(process_link) {
                    // Iterate through the 12 possible supply items
                    // Based on the JSON, these are flattened as supply_item_0, etc.
                    // I'll create a helper macro or just list them out.
                    // Listing them out is safer for now.

                    let items = [
                        (
                            &process.supply_item_0,
                            process.set_quantity_0,
                            process.sets_required_0,
                        ),
                        (
                            &process.supply_item_1,
                            process.set_quantity_1,
                            process.sets_required_1,
                        ),
                        (
                            &process.supply_item_2,
                            process.set_quantity_2,
                            process.sets_required_2,
                        ),
                        (
                            &process.supply_item_3,
                            process.set_quantity_3,
                            process.sets_required_3,
                        ),
                        (
                            &process.supply_item_4,
                            process.set_quantity_4,
                            process.sets_required_4,
                        ),
                        (
                            &process.supply_item_5,
                            process.set_quantity_5,
                            process.sets_required_5,
                        ),
                        (
                            &process.supply_item_6,
                            process.set_quantity_6,
                            process.sets_required_6,
                        ),
                        (
                            &process.supply_item_7,
                            process.set_quantity_7,
                            process.sets_required_7,
                        ),
                        (
                            &process.supply_item_8,
                            process.set_quantity_8,
                            process.sets_required_8,
                        ),
                        (
                            &process.supply_item_9,
                            process.set_quantity_9,
                            process.sets_required_9,
                        ),
                        (
                            &process.supply_item_10,
                            process.set_quantity_10,
                            process.sets_required_10,
                        ),
                        (
                            &process.supply_item_11,
                            process.set_quantity_11,
                            process.sets_required_11,
                        ),
                    ];

                    for (supply_item_link, quantity_per_set, sets_required) in items {
                        if quantity_per_set == 0 || sets_required == 0 {
                            continue;
                        }

                        if let Some(supply_item) =
                            data.company_craft_supply_items.get(supply_item_link)
                        {
                            if supply_item.item.0 == 0 {
                                continue;
                            }

                            let total_quantity = (quantity_per_set as i32) * (sets_required as i32);
                            *materials_map.entry(supply_item.item).or_default() += total_quantity;
                        }
                    }
                }
            }
        }
    }

    let mut material_infos = Vec::new();
    for (item_id, quantity) in materials_map {
        let price_summary = prices.find_matching_listings(item_id.0);
        let unit_cost = price_summary.lowest_gil().unwrap_or(999_999_999) as i64;

        // Cost calc
        total_cost = total_cost.saturating_add(unit_cost.saturating_mul(quantity as i64));

        material_infos.push(MaterialInfo {
            item_id,
            total_quantity: quantity,
            unit_cost: unit_cost as i32,
        });
    }

    let clamped_cost = if total_cost > i32::MAX as i64 {
        i32::MAX
    } else {
        total_cost as i32
    };

    (clamped_cost, material_infos)
}

#[derive(Clone, Copy, Debug)]
struct SalesStats {
    daily_sales: f32,
    avg_price: i32,
    total_sales: usize,
}

fn analyze_sales(sales_data: &[&SaleData]) -> SalesStats {
    let now = Utc::now().naive_utc();
    let mut total_sales = 0;
    let mut total_price: i64 = 0;
    let mut oldest_date = now;

    for data in sales_data {
        for sale in &data.sales {
            total_sales += 1;
            total_price += sale.price_per_unit as i64;
            if sale.sale_date < oldest_date {
                oldest_date = sale.sale_date;
            }
        }
    }

    if total_sales == 0 {
        return SalesStats {
            daily_sales: 0.0,
            avg_price: 0,
            total_sales: 0,
        };
    }

    let avg_price = (total_price / total_sales as i64) as i32;
    let duration_millis = (now - oldest_date).num_milliseconds().abs();
    let duration_hours = (duration_millis as f64 / 1000.0 / 3600.0).max(1.0);
    let days_in_sample = duration_hours / 24.0;
    let daily_sales = total_sales as f32 / days_in_sample as f32;

    SalesStats {
        daily_sales,
        avg_price,
        total_sales,
    }
}

#[component]
fn FCCraftingAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    recent_sales: Option<RecentSales>,
    world: Signal<String>,
) -> impl IntoView {
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = xiv_gen_db::data();
    let items = &data.items;
    let sequences = &data.company_craft_sequences;

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (min_daily_sales, set_min_daily_sales) = query_signal::<f32>("min-sales");

    let computed_data = Memo::new(move |_| {
        let sales_map: HashMap<i32, Vec<&SaleData>> = if let Some(ref sales) = recent_sales {
            let mut map = HashMap::new();
            for sale in &sales.sales {
                map.entry(sale.item_id).or_insert_with(Vec::new).push(sale);
            }
            map
        } else {
            HashMap::new()
        };

        let mut results = Vec::new();

        for sequence in sequences.values() {
            // result_item can be 0 for some incomplete data, skip those
            if sequence.result_item.0 == 0 {
                continue;
            }

            // Also some sequences might not have valid parts (e.g. unfinished content)
            // We can check if part 0 exists as a proxy
            // Wait, part is a link (key), so we check if the key is valid when fetching.
            // But we can pre-check if the key is 0/invalid?
            // The generated code uses keys, let's assume valid keys if present.

            let sales_stats = if let Some(item_sales) = sales_map.get(&sequence.result_item.0) {
                analyze_sales(item_sales)
            } else {
                SalesStats {
                    daily_sales: 0.0,
                    avg_price: 0,
                    total_sales: 0,
                }
            };

            let market_price_summary = prices.find_matching_listings(sequence.result_item.0);
            let market_price = market_price_summary.lowest_gil().unwrap_or(0);

            if market_price == 0 {
                continue;
            }

            let cheapest_world_id = market_price_summary
                .lq
                .map(|d| d.world_id)
                .or(market_price_summary.hq.map(|d| d.world_id))
                .unwrap_or(0);

            let (cost, materials) = calculate_fc_project_cost(sequence, &prices, data);

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
            <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
                <FilterCard
                    title="Minimum Profit"
                    description="Set the minimum profit margin"
                >
                     <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_profit()
                                    .map(|profit| Either::Left(view! { <Gil amount=profit /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            step=100000
                            type="number"
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
                    </div>
                </FilterCard>

                 <FilterCard
                    title="Minimum ROI"
                    description="Set the minimum return on investment %"
                >
                    <div class="flex flex-col gap-2">
                         <div class="text-brand-300">
                            {move || {
                                minimum_roi()
                                    .map(|roi| format!("{roi}%"))
                                    .unwrap_or("---".to_string())
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            step=10
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
                    </div>
                </FilterCard>

                <FilterCard
                    title="Minimum Daily Sales"
                    description="Filter items by sales velocity (sales/day)"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                             {move || {
                                min_daily_sales()
                                    .map(|s| format!("{:.1} / day", s))
                                    .unwrap_or("---".to_string())
                            }}
                        </div>
                        <input
                            class="input"
                            type="number"
                            min="0"
                            step="0.1"
                            placeholder="e.g. 0.1"
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
                    </div>
                </FilterCard>
            </div>

            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                 <VirtualScroller
                    viewport_height=720.0
                    row_height=60.0
                    overscan=8
                    header_height=64.0
                    variable_height=false
                     header=view! {
                        <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                             <div role="columnheader" class="w-84 p-4">"Project Result"</div>
                             <div role="columnheader" class="w-30 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="profit"
                                >
                                    "Profit"
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="roi"
                                >
                                    "ROI"
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 p-4">"Total Cost"</div>
                             <div role="columnheader" class="w-30 p-4">"Market Price"</div>
                             <div role="columnheader" class="w-30 p-4 hidden md:block">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="velocity"
                                >
                                    "Daily Sales"
                                </QueryButton>
                             </div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<FCCraftProfitData>)| (*index, data.sequence.key_id)
                    view=move |(index, data): (usize, Arc<FCCraftProfitData>)| {
                        let data_clone = data.clone();
                        let item_id = data.sequence.result_item;
                        let item = items.get(&item_id).map(|i| i.name.as_str()).unwrap_or("Unknown");
                         let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };
                         let sales_tooltip = format!(
                            "Based on {} sales over {:.1} days",
                            data.total_sales,
                            (data.total_sales as f32 / data.daily_sales.max(0.001))
                        );

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
                                        <span>{item}</span>
                                    </a>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.profit />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <span class={
                                        let data = data_clone.clone();
                                        move || {
                                            let roi = data.return_on_investment;
                                            let tint = if roi >= 500 { "24%" } else if roi >= 200 { "20%" } else if roi >= 100 { "16%" } else if roi >= 50 { "12%" } else { "10%" };
                                            format!("inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_{tint},transparent)]")
                                        }
                                    }>
                                        {format!("{}%", data.return_on_investment)}
                                    </span>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.cost />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.market_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]" title=sales_tooltip>
                                        {format!("{:.1} / day", data.daily_sales)}
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
pub fn FCCraftingAnalyzer() -> impl IntoView {
    let params = use_params_map();
    let (home_world, _) = use_home_world();

    let region = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        // Default to home world region or North-America
        let world_name = params
            .with(|p| p.get("world").clone())
            .or_else(|| home_world.get().map(|w| w.name))
            .unwrap_or_else(|| "North-America".to_string());

        worlds
            .lookup_world_by_name(&world_name)
            .map(|world| {
                let region = worlds.get_region(world);
                AnyResult::Region(region).get_name().to_string()
            })
            .unwrap_or_else(|| "North-America".to_string())
    });

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
            <Title text="FC Crafting Analyzer - Ultros" />
            <Meta name="description" content="Analyze Free Company crafting projects (Airships, Submersibles) for profitability" />

             <div class="flex flex-col gap-4 p-4 bg-brand-900/50 rounded-lg border border-brand-800">
                 <div class="flex flex-row justify-between items-center">
                    <h1 class="text-2xl font-bold text-brand-100">"FC Crafting Analyzer"</h1>
                    <div class="flex flex-row gap-2 items-center">
                        <Suspense fallback=|| view! { <div class="text-brand-300 text-sm animate-pulse">"Loading sales data..."</div> }>
                            {move || {
                                recent_sales_clone
                                    .get()
                                    .and_then(|r| r.err())
                                    .map(|_| view! { <div class="text-red-400 text-sm">"Error loading sales data"</div> })
                            }}
                        </Suspense>
                    </div>
                </div>

                <Show when=move || selected_world.get().is_some()>
                    <div class="flex flex-col md:flex-row items-center gap-2">
                        <label class="text-[color:var(--brand-fg)] font-semibold">"Select World for Sales Data:"</label>
                        <div class="w-full md:w-auto">
                            <WorldOnlyPicker
                                current_world=selected_world.into()
                                set_current_world=set_selected_world.into()
                            />
                        </div>
                    </div>
                </Show>

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
                                        "Error loading listings: " {e.to_string()}
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
