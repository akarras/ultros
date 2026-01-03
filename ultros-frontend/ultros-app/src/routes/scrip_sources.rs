use crate::{
    api::get_cheapest_listings,
    components::{
        gil::*, item_icon::*, query_button::QueryButton, skeleton::BoxSkeleton,
        virtual_scroller::*, world_picker::WorldOnlyPicker,
    },
    global_state::{LocalWorldData, home_world::use_home_world},
};
use leptos::prelude::*;
use leptos_meta::{Meta, Title};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_query_map},
};
use std::{cmp::Reverse, sync::Arc};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    world_helper::AnyResult,
};
use xiv_gen::{ItemId, Recipe};

#[derive(Clone, Debug, PartialEq)]
struct ScripSourceData {
    item_id: ItemId,
    item_name: String,
    level: u16,
    job_category_name: String,
    scrip_type: ScripType,
    scrip_amount: u32,
    cost: i32,
    cost_per_scrip: f32,
    cheapest_world_id: i32,
    recipe: Option<&'static Recipe>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScripType {
    WhiteCrafters,
    PurpleCrafters,
    WhiteGatherers,
    PurpleGatherers,
    Other(u32),
}

impl ScripType {
    fn from_id(id: u32) -> Self {
        match id {
            25199 => ScripType::WhiteCrafters,
            33913 => ScripType::PurpleCrafters,
            25200 => ScripType::WhiteGatherers,
            33914 => ScripType::PurpleGatherers,
            _ => ScripType::Other(id),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            ScripType::WhiteCrafters => "White Crafters' Scrip",
            ScripType::PurpleCrafters => "Purple Crafters' Scrip",
            ScripType::WhiteGatherers => "White Gatherers' Scrip",
            ScripType::PurpleGatherers => "Purple Gatherers' Scrip",
            ScripType::Other(_) => "Other",
        }
    }

    fn color_class(&self) -> &'static str {
        match self {
            ScripType::WhiteCrafters | ScripType::WhiteGatherers => "text-gray-200",
            ScripType::PurpleCrafters | ScripType::PurpleGatherers => "text-purple-400",
            ScripType::Other(_) => "text-gray-400",
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    CostPerScrip,
    ScripAmount,
    Cost,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "efficiency" => Ok(SortMode::CostPerScrip),
            "amount" => Ok(SortMode::ScripAmount),
            "cost" => Ok(SortMode::Cost),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::CostPerScrip => "efficiency",
            SortMode::ScripAmount => "amount",
            SortMode::Cost => "cost",
        };
        f.write_str(val)
    }
}

#[component]
fn ScripSourceTable(
    global_cheapest_listings: CheapestListings,
    world: Signal<String>,
) -> impl IntoView {
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = xiv_gen_db::data();
    let items = &data.items;
    let recipes = &data.recipes;

    // Create a lookup for recipes by result item
    let recipes_by_output = Memo::new(move |_| {
        let mut map = std::collections::HashMap::new();
        for recipe in recipes.values() {
            map.insert(recipe.item_result, recipe);
        }
        map
    });

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (scrip_filter, set_scrip_filter) = query_signal::<String>("scrip");
    let (job_filter, set_job_filter) = query_signal::<String>("job");

    let computed_data = Memo::new(move |_| {
        let mut results = Vec::new();
        let recipes_lookup = recipes_by_output();

        let scrip_filter_val = scrip_filter();
        let job_filter_val = job_filter();

        for item_vec in data.collectables_shop_items.values() {
            for item_entry in item_vec {
                let reward_scrip_id = item_entry.collectables_shop_reward_scrip;
                if reward_scrip_id.0 == 0 {
                    continue;
                }

                let reward = match data.collectables_shop_reward_scrips.get(&reward_scrip_id) {
                    Some(r) => r,
                    None => continue,
                };

                // Reward has `currency` and `low/mid/high_reward`
                let currency_id = reward.currency;
                let scrip_type = ScripType::from_id(currency_id as u32);

                // Filter Scrip Type
                if let Some(ref s_filter) = scrip_filter_val {
                    if s_filter == "WhiteCrafters" && scrip_type != ScripType::WhiteCrafters {
                        continue;
                    }
                    if s_filter == "PurpleCrafters" && scrip_type != ScripType::PurpleCrafters {
                        continue;
                    }
                    if s_filter == "WhiteGatherers" && scrip_type != ScripType::WhiteGatherers {
                        continue;
                    }
                    if s_filter == "PurpleGatherers" && scrip_type != ScripType::PurpleGatherers {
                        continue;
                    }
                } else {
                    // Default to showing Crafters scrips if no filter
                    if matches!(scrip_type, ScripType::Other(_)) {
                        continue;
                    }
                }

                // Reward amount (High Reward for max collectability)
                let scrip_amount = reward.high_reward as u32;
                if scrip_amount == 0 {
                    continue;
                }

                let item_id = item_entry.item;
                let item_def = match items.get(&item_id) {
                    Some(i) => i,
                    None => continue,
                };

                // Recipe lookup
                let recipe = recipes_lookup.get(&item_id).copied();

                // Filter Job
                if let Some(ref j_filter) = job_filter_val {
                    if let Some(r) = recipe {
                        let job_abbrev = match r.craft_type.0 {
                            0 => "Carpenter",
                            1 => "Blacksmith",
                            2 => "Armorer",
                            3 => "Goldsmith",
                            4 => "Leatherworker",
                            5 => "Weaver",
                            6 => "Alchemist",
                            7 => "Culinarian",
                            _ => "",
                        };
                        if job_abbrev != j_filter {
                            continue;
                        }
                    } else if !j_filter.is_empty() {
                        // If no recipe (gathering?), skip if job filter is active for crafting jobs
                        // Unless we add gathering job filters later
                        continue;
                    }
                }

                // Cost Calculation
                let mut cost = 0;

                if let Some(r) = recipe {
                    // Sum ingredients
                    let ingredients = [
                        (r.item_ingredient_0, r.amount_ingredient_0),
                        (r.item_ingredient_1, r.amount_ingredient_1),
                        (r.item_ingredient_2, r.amount_ingredient_2),
                        (r.item_ingredient_3, r.amount_ingredient_3),
                        (r.item_ingredient_4, r.amount_ingredient_4),
                        (r.item_ingredient_5, r.amount_ingredient_5),
                        (r.item_ingredient_6, r.amount_ingredient_6),
                        (r.item_ingredient_7, r.amount_ingredient_7),
                    ];

                    for (ing_id, amount) in ingredients {
                        if ing_id.0 == 0 || amount == 0 {
                            continue;
                        }
                        let price_summary = prices.find_matching_listings(ing_id.0);
                        let price = price_summary.lowest_gil().unwrap_or(0); // If no price, assume 0? Or skip?
                        cost += price * amount as i32;
                    }
                } else {
                    // Skip non-craftables for now
                    continue;
                }

                if cost == 0 {
                    continue;
                } // Avoid division by zero or free items

                let cost_per_scrip = cost as f32 / scrip_amount as f32;

                results.push(ScripSourceData {
                    item_id,
                    item_name: item_def.name.to_string(),
                    level: item_def.level_item.0,
                    job_category_name: if let Some(r) = recipe {
                        match r.craft_type.0 {
                            0 => "Carpenter".to_string(),
                            1 => "Blacksmith".to_string(),
                            2 => "Armorer".to_string(),
                            3 => "Goldsmith".to_string(),
                            4 => "Leatherworker".to_string(),
                            5 => "Weaver".to_string(),
                            6 => "Alchemist".to_string(),
                            7 => "Culinarian".to_string(),
                            _ => "Unknown".to_string(),
                        }
                    } else {
                        "Gathering".to_string()
                    },
                    scrip_type,
                    scrip_amount,
                    cost,
                    cost_per_scrip,
                    cheapest_world_id: 0, // Not tracked per ingredient
                    recipe,
                });
            }
        }

        // Sort
        match sort_mode().unwrap_or(SortMode::CostPerScrip) {
            SortMode::CostPerScrip => {
                results.sort_by(|a, b| a.cost_per_scrip.partial_cmp(&b.cost_per_scrip).unwrap())
            }
            SortMode::ScripAmount => results.sort_by_key(|d| Reverse(d.scrip_amount)),
            SortMode::Cost => results.sort_by_key(|d| d.cost),
        }

        // Deduplicate by item_id (since item may appear in multiple shop lists)
        results.dedup_by_key(|r| r.item_id);

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
                    <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">"Scrip Type"</h3>
                    <select
                        class="input"
                        on:change=move |ev| {
                            let val = event_target_value(&ev);
                            if val.is_empty() {
                                set_scrip_filter(None);
                            } else {
                                set_scrip_filter(Some(val));
                            }
                        }
                    >
                        <option value="">"All Scrips"</option>
                        <option value="PurpleCrafters" selected=move || scrip_filter() == Some("PurpleCrafters".to_string())>"Purple Crafters' Scrip"</option>
                        <option value="WhiteCrafters" selected=move || scrip_filter() == Some("WhiteCrafters".to_string())>"White Crafters' Scrip"</option>
                        <option value="PurpleGatherers" selected=move || scrip_filter() == Some("PurpleGatherers".to_string())>"Purple Gatherers' Scrip"</option>
                        <option value="WhiteGatherers" selected=move || scrip_filter() == Some("WhiteGatherers".to_string())>"White Gatherers' Scrip"</option>
                    </select>
                </div>

                <div class="panel p-6 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100 z-20">
                    <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">"Job Filter"</h3>
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
                             <div role="columnheader" class="w-84 p-4">"Item"</div>
                             <div role="columnheader" class="w-40 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="efficiency"
                                >
                                    "Cost / Scrip"
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="amount"
                                >
                                    "Scrips"
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="cost"
                                >
                                    "Cost"
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-40 p-4 hidden md:block">"Scrip Type"</div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<ScripSourceData>)| (*index, data.item_id)
                    view=move |(index, data): (usize, Arc<ScripSourceData>)| {
                        let item_id = data.item_id;
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
                                            <span class="font-semibold">{data.item_name.clone()}</span>
                                            <span class="text-xs text-[color:var(--color-text-muted)] truncate">
                                                "Lv " {data.level} " " {data.job_category_name.clone()}
                                            </span>
                                        </div>
                                    </a>
                                </div>
                                <div role="cell" class="px-4 py-2 w-40 text-right font-bold text-brand-300">
                                    <Gil amount=data.cost_per_scrip as i32 />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    {data.scrip_amount}
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.cost />
                                </div>
                                <div role="cell" class="px-4 py-2 w-40 text-right hidden md:block">
                                    <span class={format!("text-xs {}", data.scrip_type.color_class())}>
                                        {data.scrip_type.name()}
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
pub fn ScripSources() -> impl IntoView {
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

    Effect::new(move |_| {
        if selected_world.get_untracked().is_none()
            && let Some(home) = home_world.get()
        {
            set_selected_world(Some(home));
        }
    });

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
                nav(&query_string, NavigateOptions::default());
            }
        }
    });

    view! {
        <div class="flex flex-col gap-4 h-full">
            <Title text="Scrip Source Analyzer - Ultros" />
            <Meta name="description" content="Analyze Collectable Scrip Sources for efficiency" />

            <div class="flex flex-col gap-4 p-4 bg-brand-900/50 rounded-lg border border-brand-800">
                <div class="flex flex-row justify-between items-center">
                    <h1 class="text-2xl font-bold text-brand-100">"Scrip Sources"</h1>
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

                <div class="text-sm text-[color:var(--color-text-muted)]">
                    "This tool finds the most efficient Collectables to craft for White/Purple Scrips based on current market board prices for materials."
                </div>

                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        match listings {
                            Some(Ok(listings)) => {
                                view! {
                                    <ScripSourceTable
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
