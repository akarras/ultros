use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::xiv_data::tracked_data;
use crate::{
    api::get_cheapest_listings,
    components::{
        gil::*,
        item_icon::*,
        query_button::QueryButton,
        skeleton::BoxSkeleton,
        tool_help::*,
        toolbar::{Toolbar, ToolbarField},
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::{
        LocalWorldData, home_world::use_home_world, region_for_world::use_region_for_world,
    },
};
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_query_map},
};
use std::{cmp::Reverse, sync::Arc};
use ultros_api_types::cheapest_listings::{CheapestListings, CheapestListingsMap};
use xiv_gen::{CollectablesShopRewardScripId, ItemId, Recipe};

use crate::i18n::*;

#[derive(Clone, Debug, PartialEq)]
struct ScripSourceData {
    item_id: ItemId,
    item_name: String,
    level: u16,
    craft_type: Option<i32>,
    scrip_type: ScripType,
    scrip_amount: u32,
    cost: i32,
    cost_per_scrip: f32,
    cheapest_world_id: i32,
    recipe: Option<&'static Recipe>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScripType {
    OrangeCrafters,
    OrangeGatherers,
    WhiteCrafters,
    PurpleCrafters,
    WhiteGatherers,
    PurpleGatherers,
    Other(u32),
}

impl ScripType {
    fn from_id(id: u32) -> Self {
        match id {
            41784 => ScripType::OrangeCrafters,
            41785 => ScripType::OrangeGatherers,
            25199 => ScripType::WhiteCrafters,
            33913 => ScripType::PurpleCrafters,
            25200 => ScripType::WhiteGatherers,
            33914 => ScripType::PurpleGatherers,
            _ => ScripType::Other(id),
        }
    }

    fn color_class(&self) -> &'static str {
        match self {
            ScripType::OrangeCrafters | ScripType::OrangeGatherers => "text-orange-400",
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
    let i18n = use_i18n();
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = tracked_data();
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
                if reward_scrip_id == 0 {
                    continue;
                }

                let reward = match data
                    .collectables_shop_reward_scrips
                    .get(&CollectablesShopRewardScripId(reward_scrip_id))
                {
                    Some(r) => r,
                    None => continue,
                };

                // Reward has `currency` and `low/mid/high_reward`
                let currency_id = reward.currency;
                let scrip_type = ScripType::from_id(currency_id as u32);

                // Filter Scrip Type
                if let Some(ref s_filter) = scrip_filter_val {
                    if s_filter == "OrangeCrafters" && scrip_type != ScripType::OrangeCrafters {
                        continue;
                    }
                    if s_filter == "OrangeGatherers" && scrip_type != ScripType::OrangeGatherers {
                        continue;
                    }
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
                let item_def = match items.get(&ItemId(item_id)) {
                    Some(i) => i,
                    None => continue,
                };

                // Recipe lookup
                let recipe = recipes_lookup.get(&item_id).copied();

                // Filter Job
                if let Some(ref j_filter) = job_filter_val {
                    if let Some(r) = recipe {
                        let job_abbrev = match r.craft_type {
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
                    for i in 0..8 {
                        let ing_id = r.ingredient[i];
                        let amount = r.amount_ingredient[i];
                        if ing_id == 0 || amount == 0 {
                            continue;
                        }
                        let price_summary = prices.find_matching_listings(ing_id);
                        let price = price_summary.lowest_gil().unwrap_or(0); // If no price, assume 0? Or skip?
                        cost += price * amount;
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
                    item_id: ItemId(item_id),
                    item_name: item_def.name.to_string(),
                    level: item_def.level_item as u16,
                    craft_type: recipe.map(|r| r.craft_type),
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
            <Toolbar>
                <ToolbarField label=t_string!(i18n, scrip_sources_scrip_type).to_string()>
                    <select
                        class="input input-sm w-48"
                        on:change=move |ev| {
                            let val = event_target_value(&ev);
                            if val.is_empty() {
                                set_scrip_filter(None);
                            } else {
                                set_scrip_filter(Some(val));
                            }
                        }
                    >
                        <option value="">{t!(i18n, scrip_sources_all_scrips)}</option>
                        <option value="OrangeCrafters" selected=move || scrip_filter() == Some("OrangeCrafters".to_string())>{t!(i18n, scrip_sources_orange_crafters)}</option>
                        <option value="OrangeGatherers" selected=move || scrip_filter() == Some("OrangeGatherers".to_string())>{t!(i18n, scrip_sources_orange_gatherers)}</option>
                        <option value="PurpleCrafters" selected=move || scrip_filter() == Some("PurpleCrafters".to_string())>{t!(i18n, scrip_sources_purple_crafters)}</option>
                        <option value="WhiteCrafters" selected=move || scrip_filter() == Some("WhiteCrafters".to_string())>{t!(i18n, scrip_sources_white_crafters)}</option>
                        <option value="PurpleGatherers" selected=move || scrip_filter() == Some("PurpleGatherers".to_string())>{t!(i18n, scrip_sources_purple_gatherers)}</option>
                        <option value="WhiteGatherers" selected=move || scrip_filter() == Some("WhiteGatherers".to_string())>{t!(i18n, scrip_sources_white_gatherers)}</option>
                    </select>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, scrip_sources_job_filter).to_string()>
                    <select
                        class="input input-sm w-40"
                        on:change=move |ev| {
                            let val = event_target_value(&ev);
                            if val.is_empty() {
                                set_job_filter(None);
                            } else {
                                set_job_filter(Some(val));
                            }
                        }
                    >
                        <option value="">{t!(i18n, all_jobs)}</option>
                        <option value="Carpenter" selected=move || job_filter() == Some("Carpenter".to_string())>{t!(i18n, carpenter)}</option>
                        <option value="Blacksmith" selected=move || job_filter() == Some("Blacksmith".to_string())>{t!(i18n, blacksmith)}</option>
                        <option value="Armorer" selected=move || job_filter() == Some("Armorer".to_string())>{t!(i18n, armorer)}</option>
                        <option value="Goldsmith" selected=move || job_filter() == Some("Goldsmith".to_string())>{t!(i18n, goldsmith)}</option>
                        <option value="Leatherworker" selected=move || job_filter() == Some("Leatherworker".to_string())>{t!(i18n, leatherworker)}</option>
                        <option value="Weaver" selected=move || job_filter() == Some("Weaver".to_string())>{t!(i18n, weaver)}</option>
                        <option value="Alchemist" selected=move || job_filter() == Some("Alchemist".to_string())>{t!(i18n, alchemist)}</option>
                        <option value="Culinarian" selected=move || job_filter() == Some("Culinarian".to_string())>{t!(i18n, culinarian)}</option>
                    </select>
                </ToolbarField>
            </Toolbar>

            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                    viewport_height=720.0
                    row_height=60.0
                    overscan=8
                    header_height=64.0
                    variable_height=false
                    header=view! {
                        <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                             <div role="columnheader" class="w-84 p-4">{t!(i18n, scrip_sources_item)}</div>
                             <div role="columnheader" class="w-40 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="efficiency"
                                >
                                    {t!(i18n, scrip_sources_cost_per_scrip)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="amount"
                                >
                                    {t!(i18n, scrip_sources_scrips)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-30 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="cost"
                                >
                                    {t!(i18n, scrip_sources_cost)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-40 p-4 hidden md:block">{t!(i18n, scrip_sources_scrip_type_header)}</div>
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
                                                {t!(i18n, scrip_sources_lv_prefix)} " " {data.level} " " {match data.craft_type {
                                                    None => view! { {t!(i18n, gathering)} }.into_any(),
                                                    Some(0) => view! { {t!(i18n, carpenter)} }.into_any(),
                                                    Some(1) => view! { {t!(i18n, blacksmith)} }.into_any(),
                                                    Some(2) => view! { {t!(i18n, armorer)} }.into_any(),
                                                    Some(3) => view! { {t!(i18n, goldsmith)} }.into_any(),
                                                    Some(4) => view! { {t!(i18n, leatherworker)} }.into_any(),
                                                    Some(5) => view! { {t!(i18n, weaver)} }.into_any(),
                                                    Some(6) => view! { {t!(i18n, alchemist)} }.into_any(),
                                                    Some(7) => view! { {t!(i18n, culinarian)} }.into_any(),
                                                    _ => view! { {t!(i18n, unknown)} }.into_any(),
                                                }}
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
                                        {match data.scrip_type {
                                            ScripType::OrangeCrafters => t_string!(i18n, scrip_sources_orange_crafters).to_string(),
                                            ScripType::OrangeGatherers => t_string!(i18n, scrip_sources_orange_gatherers).to_string(),
                                            ScripType::WhiteCrafters => t_string!(i18n, scrip_sources_white_crafters).to_string(),
                                            ScripType::PurpleCrafters => t_string!(i18n, scrip_sources_purple_crafters).to_string(),
                                            ScripType::WhiteGatherers => t_string!(i18n, scrip_sources_white_gatherers).to_string(),
                                            ScripType::PurpleGatherers => t_string!(i18n, scrip_sources_purple_gatherers).to_string(),
                                            ScripType::Other(_) => t_string!(i18n, scrip_sources_other_name).to_string(),
                                        }}
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

    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title=t_string!(i18n, scrip_sources_meta_title).to_string() />
            <MetaDescription text=t_string!(i18n, scrip_sources_meta_desc).to_string() />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title="Scrip Sources"
                    summary="Find collectables with the lowest estimated gil cost per scrip."
                    context="The default sort optimizes efficiency: lower cost per scrip is better."
                    help_href="/help/scrip-sources"
                    help_body="Scrip Sources assumes high collectability rewards and calculates ingredient cost from market listings. Use scrip type and job filters to narrow to what you can actually turn in."
                />

                <Toolbar>
                    <ToolbarField label=t_string!(i18n, scrip_sources_select_world).to_string()>
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world.into()
                        />
                    </ToolbarField>
                </Toolbar>

                <div class="text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, scrip_sources_description)}
                </div>
                <CalculationSummary
                    title="Efficiency model"
                    formula="cost per scrip = ingredient cost / high collectability scrip reward"
                    details="The high reward is used as the max collectability target. Gathering and non-craftable sources are intentionally limited in this pass."
                />
                <div class="flex flex-wrap gap-2">
                    <AssumptionBadge text="High collectability reward" />
                    <AssumptionBadge text="Market ingredient cost" />
                    <AssumptionBadge text="Lower cost per scrip is better" />
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
                                        {t!(i18n, scrip_sources_error_loading)} {e.to_string()}
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
