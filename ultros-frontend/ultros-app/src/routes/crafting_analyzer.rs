use crate::{
    api::get_cheapest_listings,
    components::{
        add_to_list::AddToList, clipboard::*, gil::*, item_icon::*, meta::*,
        query_button::QueryButton, skeleton::BoxSkeleton, virtual_scroller::*, world_picker::*,
    },
    error::AppError,
    global_state::{
        cookies::Cookies, crafter_levels::CrafterLevels, home_world::use_home_world, LocalWorldData,
    },
};
use icondata as i;
use leptos::{either::Either, prelude::*, reactive::wrappers::write::SignalSetter};
use leptos_icons::*;
use leptos_meta::Title;
use leptos_router::{
    hooks::{query_signal, use_params_map},
};
use std::{
    cmp::Reverse,
    collections::HashMap,
    str::FromStr,
    sync::Arc,
};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    world_helper::{AnyResult, WorldHelper},
};
use xiv_gen::{ItemId, Recipe};

#[derive(Clone, Debug, PartialEq)]
struct CraftingProfitData {
    recipe: &'static Recipe,
    profit: i32,
    return_on_investment: i32,
    cost: i32,
    market_price: i32,
    cheapest_world_id: i32,
    ingredients_cost: Vec<(ItemId, i32)>, // ItemId, Cost
    sub_craft_count: i32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
}

impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            _ => Err(()),
        }
    }
}

impl ToString for SortMode {
    fn to_string(&self) -> String {
        match self {
            SortMode::Roi => "roi".to_string(),
            SortMode::Profit => "profit".to_string(),
        }
    }
}

fn calculate_crafting_cost(
    recipe: &'static Recipe,
    prices: &CheapestListingsMap,
    items: &HashMap<ItemId, xiv_gen::Item>,
    recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    depth: i32,
    max_depth: i32,
    use_subcrafts: bool,
) -> (i32, i32) {
    let mut cost = 0;
    let mut total_sub_crafts = 0;
    // Helper to iterate ingredients
    let ingredients = [
        (recipe.item_ingredient_0, recipe.amount_ingredient_0),
        (recipe.item_ingredient_1, recipe.amount_ingredient_1),
        (recipe.item_ingredient_2, recipe.amount_ingredient_2),
        (recipe.item_ingredient_3, recipe.amount_ingredient_3),
        (recipe.item_ingredient_4, recipe.amount_ingredient_4),
        (recipe.item_ingredient_5, recipe.amount_ingredient_5),
        (recipe.item_ingredient_6, recipe.amount_ingredient_6),
        (recipe.item_ingredient_7, recipe.amount_ingredient_7),
    ];

    for (item_id, amount) in ingredients {
        if item_id.0 == 0 || amount == 0 {
            continue;
        }

        let market_price = prices
            .find_matching_listings(item_id.0)
            .lowest_gil()
            .unwrap_or(999_999_999); // High cost if not found

        let mut item_cost = market_price;
        let mut best_sub_count = 0;

        if use_subcrafts && depth < max_depth {
            if let Some(sub_recipes) = recipes_by_output.get(&item_id) {
                // Find cheapest way to craft this ingredient
                for sub_recipe in sub_recipes {
                    let (craft_cost, sub_count) = calculate_crafting_cost(
                        sub_recipe,
                        prices,
                        items,
                        recipes_by_output,
                        depth + 1,
                        max_depth,
                        use_subcrafts,
                    );
                    if craft_cost < item_cost {
                        item_cost = craft_cost;
                        best_sub_count = sub_count + 1; // +1 for this sub-craft
                    }
                }
            }
        }
        
        if item_cost < market_price {
             total_sub_crafts += best_sub_count;
        }
        
        // Check if item can be bought from vendor (simple check, ideally check shop data)
        if let Some(item) = items.get(&item_id) {
             if item.price_mid > 0 {
                 // This is sell price, not buy price. 
                 // Ideally we check gil_shop_items but that's complex here.
                 // For now rely on market board data which should reflect vendor price if people are smart,
                 // or add a basic check if we had vendor data easily accessible.
             }
        }

        cost += item_cost * amount as i32;
    }
    (cost, total_sub_crafts)
}

#[component]
fn FilterCard<T>(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] description: Oco<'static, str>,
    children: TypedChildren<T>,
) -> impl IntoView
where
    T: IntoView,
{
    view! {
        <div class="panel p-6 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100 z-20" style="backdrop-filter: none; background-image: none;">
            <h3 class="font-bold text-xl mb-2 text-[color:var(--brand-fg)]">{title}</h3>
            <p class="mb-4 text-[color:var(--color-text-muted)]">{description}</p>
            {children.into_inner()().into_view()}
        </div>
    }
}

#[component]
fn CraftingAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    worlds: Arc<WorldHelper>,
    world: Signal<String>,
) -> impl IntoView {
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = xiv_gen_db::data();
    let items = &data.items;
    let recipes = &data.recipes;
    
    // Index recipes by output item for subcraft lookup
    let recipes_by_output = Memo::new(move |_| {
        let mut map: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        for recipe in recipes.values() {
            map.entry(recipe.item_result).or_default().push(recipe);
        }
        map
    });

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (job_filter, set_job_filter) = query_signal::<String>("job");
    let (use_subcrafts, set_use_subcrafts) = query_signal::<bool>("subcrafts");
    
    let cookies = use_context::<Cookies>().unwrap();
    let (crafter_levels, _) = cookies.use_cookie_typed::<_, CrafterLevels>("CRAFTER_LEVELS");

    let has_levels = Memo::new(move |_| {
        let levels = crafter_levels.get().unwrap_or_default();
        levels.carpenter > 0 || levels.blacksmith > 0 || levels.armorer > 0 || 
        levels.goldsmith > 0 || levels.leatherworker > 0 || levels.weaver > 0 || 
        levels.alchemist > 0 || levels.culinarian > 0
    });

    let computed_data = Memo::new(move |_| {
        let recipes_by_output = recipes_by_output();
        let levels = crafter_levels.get().unwrap_or_default();
        let use_sub = use_subcrafts().unwrap_or(false);
        
        let mut results = Vec::new();

        // If no levels set, return empty (but we'll show a message)
        if !has_levels() {
            return vec![];
        }

        for recipe in recipes.values() {
            // Filter by job and level
            let level = recipe.recipe_level_table.0 as i32; // Simplified, actual level logic might need mapping
            // Actually recipe.recipe_level_table points to RecipeLevelTable which has ClassJobLevel
            // But for now let's use a simplified check or just check if user can craft it
            
            // Job mapping: 0=CRP, 1=BSM, 2=ARM, 3=GSM, 4=LTW, 5=WVR, 6=ALC, 7=CUL
            let (user_level, job_code) = match recipe.craft_type.0 {
                0 => (levels.carpenter, "CRP"),
                1 => (levels.blacksmith, "BSM"),
                2 => (levels.armorer, "ARM"),
                3 => (levels.goldsmith, "GSM"),
                4 => (levels.leatherworker, "LTW"),
                5 => (levels.weaver, "WVR"),
                6 => (levels.alchemist, "ALC"),
                7 => (levels.culinarian, "CUL"),
                _ => (0, ""),
            };

            if let Some(filter) = job_filter() {
                if filter != job_code {
                    continue;
                }
            }

            // Simple level check (this is rough, RecipeLevelTable is complex)
            // We'll assume if user level is 0 they can't craft it.
            // Ideally we map RecipeLevelTable -> ClassJobLevel
            if user_level == 0 {
                continue;
            }

            let market_price_summary = prices.find_matching_listings(recipe.item_result.0);
            let market_price = market_price_summary.lowest_gil().unwrap_or(0);
            
            if market_price == 0 {
                continue;
            }
            
            let cheapest_world_id = market_price_summary.lq.map(|d| d.world_id)
                .or(market_price_summary.hq.map(|d| d.world_id))
                .unwrap_or(0);

            let (cost, sub_craft_count) = calculate_crafting_cost(
                recipe,
                &prices,
                items,
                &recipes_by_output,
                0,
                if use_sub { 2 } else { 0 }, // Limit recursion depth
                use_sub,
            );

            if cost >= market_price {
                continue;
            }

            let profit = market_price - cost;
            let roi = if cost > 0 {
                (profit as f64 / cost as f64 * 100.0) as i32
            } else {
                0
            };

            results.push(CraftingProfitData {
                recipe,
                profit,
                return_on_investment: roi,
                cost,
                market_price,
                cheapest_world_id,
                ingredients_cost: vec![], // Populate if needed for tooltip
                sub_craft_count,
            });
        }
        
        // Filter results
        if let Some(min) = minimum_profit() {
            results.retain(|d| d.profit >= min);
        }
        if let Some(min) = minimum_roi() {
            results.retain(|d| d.return_on_investment >= min);
        }

        // Sort
        match sort_mode().unwrap_or(SortMode::Profit) {
            SortMode::Roi => results.sort_by_key(|d| Reverse(d.return_on_investment)),
            SortMode::Profit => results.sort_by_key(|d| Reverse(d.profit)),
        }

        results.into_iter().take(100).enumerate().collect::<Vec<_>>()
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
                    title="Options"
                    description="Configure calculation options"
                >
                     <div class="flex flex-col gap-4">
                <Show when=move || !has_levels()>
                    <div class="alert alert-warning">
                        <Icon icon=i::AiWarningOutlined width="2em" height="2em" />
                        <div>
                            <h3 class="font-bold">"No Crafter Levels Set"</h3>
                            <div class="text-sm">
                                "Please configure your crafter levels in "
                                <a href="/settings" class="link">"Settings"</a>
                                " to see profitable recipes."
                            </div>
                        </div>
                    </div>
                </Show>

                <div class="flex flex-row gap-4 flex-wrap">
                            <input 
                                type="checkbox" 
                                id="subcrafts" 
                                class="checkbox"
                                prop:checked=move || use_subcrafts().unwrap_or(false)
                                on:change=move |ev| set_use_subcrafts(Some(event_target_checked(&ev)))
                            />
                            <label for="subcrafts">"Include Sub-crafts"</label>
                        </div>
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
                            <option value="CRP" selected=move || job_filter() == Some("CRP".to_string())>"Carpenter"</option>
                            <option value="BSM" selected=move || job_filter() == Some("BSM".to_string())>"Blacksmith"</option>
                            <option value="ARM" selected=move || job_filter() == Some("ARM".to_string())>"Armorer"</option>
                            <option value="GSM" selected=move || job_filter() == Some("GSM".to_string())>"Goldsmith"</option>
                            <option value="LTW" selected=move || job_filter() == Some("LTW".to_string())>"Leatherworker"</option>
                            <option value="WVR" selected=move || job_filter() == Some("WVR".to_string())>"Weaver"</option>
                            <option value="ALC" selected=move || job_filter() == Some("ALC".to_string())>"Alchemist"</option>
                            <option value="CUL" selected=move || job_filter() == Some("CUL".to_string())>"Culinarian"</option>
                        </select>
                     </div>
                </FilterCard>
            </div>

            // Results Table
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
                             <div role="columnheader" class="w-30 p-4">"Cost"</div>
                             <div role="columnheader" class="w-30 p-4">"Price"</div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, CraftingProfitData)| (*index, data.recipe.key_id)
                    view=move |(index, data): (usize, CraftingProfitData)| {
                        let item_id = data.recipe.item_result;
                        let item = items.get(&item_id).map(|i| i.name.as_str()).unwrap_or("Unknown");
                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };

                        let sub_craft_count = data.sub_craft_count;

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
                                        <div class="flex flex-col">
                                            <span>{item}</span>
                                            <span class="text-xs text-[color:var(--color-text-muted)]">
                                                "Lv " {data.recipe.recipe_level_table.0}
                                            </span>
                                        </div>
                                    </a>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.profit />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                     <span class=move || {
                                        let roi = data.return_on_investment;
                                        let tint = if roi >= 500 { "24%" } else if roi >= 200 { "20%" } else if roi >= 100 { "16%" } else if roi >= 50 { "12%" } else { "10%" };
                                        format!("inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_{tint},transparent)]")
                                    }>
                                        {format!("{}%", data.return_on_investment)}
                                    </span>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.cost />
                                    <Show when=move || { sub_craft_count > 0 }>
                                        <div class="text-xs text-brand-300 flex items-center justify-end gap-1" title="Includes sub-crafts">
                                            <Icon icon=i::FaHammerSolid width="0.8em" height="0.8em" />
                                            <span>{sub_craft_count} " sub"</span>
                                        </div>
                                    </Show>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.market_price />
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
pub fn CraftingAnalyzer() -> impl IntoView {
    let params = use_params_map();
    let (home_world, _) = use_home_world();
    
    let region = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        // Default to home world region or North-America
        let world_name = params.with(|p| p.get("world").clone())
            .or_else(|| home_world.get().map(|w| w.name))
            .unwrap_or_else(|| "North-America".to_string());
            
        let region = worlds
            .lookup_world_by_name(&world_name)
            .map(|world| {
                let region = worlds.get_region(world);
                AnyResult::Region(region).get_name().to_string()
            })
            .unwrap_or_else(|| "North-America".to_string());
        region
    });

    let global_cheapest_listings = Resource::new(
        move || region(),
        move |region| async move { get_cheapest_listings(region.as_str()).await },
    );
    
    let worlds = use_context::<LocalWorldData>().unwrap().0.unwrap();

    view! {
         <div class="main-content p-6">
            <MetaTitle title="Crafting Analyzer" />
            <div class="container mx-auto max-w-7xl space-y-6">
                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">"Crafting Analyzer"</h1>
                <p class="text-[color:var(--color-text-muted)]">
                    "Find profitable crafting recipes based on current market prices. Set your crafter levels in settings to see relevant recipes."
                </p>
                
                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        global_cheapest_listings.get().map(|listings| {
                             match listings {
                                Ok(listings) => {
                                    view! {
                                        <CraftingAnalyzerTable 
                                            global_cheapest_listings=listings 
                                            worlds=worlds.clone()
                                            world=Signal::derive(move || region())
                                        />
                                    }.into_any()
                                }
                                Err(e) => {
                                     view! {
                                        <div class="text-red-400">
                                            "Error loading listings: " {e.to_string()}
                                        </div>
                                    }.into_any()
                                }
                             }
                        })
                    }}
                </Suspense>
            </div>
        </div>
    }
}
