use crate::{
    api::get_cheapest_listings,
    components::{
        gil::*,
        item_icon::*,
        meta::{MetaDescription, MetaTitle},
        query_button::QueryButton,
        skeleton::BoxSkeleton,
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::{LocalWorldData, home_world::use_home_world},
};
use leptos::{either::Either, prelude::*};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_query_map},
};
use std::{cmp::Reverse, sync::Arc};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    world_helper::AnyResult,
};
use xiv_gen::{CraftLeve, ItemId, Leve};

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
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Level,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "level" => Ok(SortMode::Level),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Profit => "profit",
            SortMode::Level => "level",
        };
        f.write_str(val)
    }
}

#[component]
fn LeveAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    world: Signal<String>,
) -> impl IntoView {
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = xiv_gen_db::data();
    let items = &data.items;
    let leves = &data.leves;
    let craft_leves = &data.craft_leves;
    let leve_reward_items = &data.leve_reward_items;
    let leve_reward_item_groups = &data.leve_reward_item_groups;
    let class_job_categories = &data.class_job_categorys;

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (job_filter, set_job_filter) = query_signal::<String>("job");

    let computed_data = Memo::new(move |_| {
        let mut results = Vec::new();

        for craft_leve in craft_leves.values() {
            let leve_id = craft_leve.leve;
            // Some CraftLeves might point to invalid Leve IDs or placeholder 0
            if leve_id.0 == 0 {
                continue;
            }
            let leve = match leves.get(&leve_id) {
                Some(l) => l,
                None => continue,
            };

            // Only consider levels with items
            let item_id = craft_leve.item_0;
            if item_id.0 == 0 {
                continue;
            }
            let item_count = craft_leve.item_count_0 as u32;
            if item_count == 0 {
                continue;
            }

            // Job Category (for filtering)
            let job_category = class_job_categories.get(&leve.class_job_category);
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
            let market_price_summary = prices.find_matching_listings(item_id.0);
            // Default to high price if not found to discourage bad data
            let market_price = market_price_summary.lowest_gil().unwrap_or(0);

            if market_price == 0 {
                // Can't calculate profit without market price
                continue;
            }

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

            if let Some(reward_item_entry) = leve_reward_items.get(&reward_item_id) {
                // Iterate over the 8 groups
                let groups = [
                    (
                        reward_item_entry.leve_reward_item_group_0,
                        reward_item_entry.probability_0,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_1,
                        reward_item_entry.probability_1,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_2,
                        reward_item_entry.probability_2,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_3,
                        reward_item_entry.probability_3,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_4,
                        reward_item_entry.probability_4,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_5,
                        reward_item_entry.probability_5,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_6,
                        reward_item_entry.probability_6,
                    ),
                    (
                        reward_item_entry.leve_reward_item_group_7,
                        reward_item_entry.probability_7,
                    ),
                ];

                for (group_id, probability) in groups {
                    if group_id.0 == 0 || probability == 0 {
                        continue;
                    }

                    if let Some(group) = leve_reward_item_groups.get(&group_id) {
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
                            if g_item_id.0 == 0 || g_count == 0 {
                                continue;
                            }

                            let reward_price_summary = prices.find_matching_listings(g_item_id.0);
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
                item_id,
                item_count,
                class_job_level: leve.class_job_level,
                job_category_name,
            });
        }

        // Sort
        match sort_mode().unwrap_or(SortMode::Profit) {
            SortMode::Profit => results.sort_by_key(|d| Reverse(d.profit)),
            SortMode::Level => results.sort_by_key(|d| Reverse(d.class_job_level)),
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
            <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
                 <div class="panel p-6 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100 z-20">
                    <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">"Minimum Profit"</h3>
                    <p class="mb-4 text-[color:var(--color-text-muted)]">"Set the minimum profit margin"</p>
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
                            step=1000
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
                </div>

                <div class="panel p-6 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100 z-20">
                    <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">"Job Filter"</h3>
                    <p class="mb-4 text-[color:var(--color-text-muted)]">"Filter by Crafting Job"</p>
                     <select
                        class="input"
                        on:change=move |ev| {
                            let val = event_target_value(&ev);
                            if val.is_empty() {
                                set_job_filter(None);
                            } else {
                                set_job_filter(Some(val));
                            }
                        }
                    >
                        <option value="">"All Jobs"</option>
                        <option value="Carpenter" selected=move || job_filter() == Some("Carpenter".to_string())>"Carpenter"</option>
                        <option value="Blacksmith" selected=move || job_filter() == Some("Blacksmith".to_string())>"Blacksmith"</option>
                        <option value="Armorer" selected=move || job_filter() == Some("Armorer".to_string())>"Armorer"</option>
                        <option value="Goldsmith" selected=move || job_filter() == Some("Goldsmith".to_string())>"Goldsmith"</option>
                        <option value="Leatherworker" selected=move || job_filter() == Some("Leatherworker".to_string())>"Leatherworker"</option>
                        <option value="Weaver" selected=move || job_filter() == Some("Weaver".to_string())>"Weaver"</option>
                        <option value="Alchemist" selected=move || job_filter() == Some("Alchemist".to_string())>"Alchemist"</option>
                        <option value="Culinarian" selected=move || job_filter() == Some("Culinarian".to_string())>"Culinarian"</option>
                    </select>
                </div>
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
                             <div role="columnheader" class="w-84 p-4">"Leve / Item"</div>
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
                             <div role="columnheader" class="w-30 p-4">"Revenue"</div>
                             <div role="columnheader" class="w-30 p-4">"Cost"</div>
                             <div role="columnheader" class="w-40 p-4 hidden md:block">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="level"
                                >
                                    "Level"
                                </QueryButton>
                             </div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<LeveProfitData>)| (*index, data.leve.key_id)
                    view=move |(index, data): (usize, Arc<LeveProfitData>)| {
                        let item_id = data.item_id;
                        let item = items.get(&item_id).map(|i| i.name.as_str()).unwrap_or("Unknown");
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
                                                {item} " x" {data.item_count}
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
                                <div role="cell" class="px-4 py-2 w-40 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]">
                                        "Lv " {data.class_job_level} " " {data.job_category_name.clone()}
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
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let nav = use_navigate();

    let region = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        // Default to home world region or North-America
        let world_name = query
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

    let global_cheapest_listings = Resource::new(region, move |region: String| async move {
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
            // Preserve other query params if needed, or just overwrite for now.
            // LeveAnalyzerTable manages its own state via query_signal which reads/writes directly.
            // But we should try to preserve existing params if possible, or at least not wipe them out unnecessarily.
            // Since we are changing the main context (world), a full refresh of params might be acceptable or we can just append.
            // Actually, query_signal handles reading/writing. We just need to update `world`.

            // Note: navigate will replace the current query params if not carefully merged.
            // use_query_map is reactive.

            let current_query = query.get_untracked();
            let world_matches = current_query
                .get("world")
                .map(|s| s == world_name)
                .unwrap_or(false);

            if !world_matches {
                // Construct new query string
                // Let's try to preserve them.
                let mut query_string = format!("?world={}", world_name);
                for (k, v) in current_query.into_iter() {
                    if k != "world" {
                        query_string.push_str(&format!("&{}={}", k, v));
                    }
                }

                nav(&query_string, NavigateOptions::default());
            }
        }
    });

    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title="Leve Analyzer - Ultros" />
            <MetaDescription text="Analyze Crafting Levequests for profitability" />

            <div class="flex flex-col gap-4 p-4 bg-brand-900/50 rounded-lg border border-brand-800">
                <div class="flex flex-row justify-between items-center">
                    <h1 class="text-2xl font-bold text-brand-100">"Leve Analyzer"</h1>
                </div>

                <div class="flex flex-col md:flex-row items-center gap-2">
                    <label class="text-[color:var(--brand-fg)] font-semibold">"Select World for Prices:"</label>
                    <div class="w-full md:w-auto">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world.into()
                        />
                    </div>
                </div>

                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        match listings {
                            Some(Ok(listings)) => {
                                view! {
                                    <LeveAnalyzerTable
                                        global_cheapest_listings=listings
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            Some(Err(e)) => {
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
