use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        crafter_settings::CrafterSettings, gil::*, item_icon::*, query_button::QueryButton,
        skeleton::BoxSkeleton, tooltip::Tooltip, virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::{
        LocalWorldData, cookies::Cookies, crafter_levels::CrafterLevels, home_world::use_home_world,
    },
};
use chrono::Utc;
use humantime::{format_duration, parse_duration};
use icondata as i;
use leptos::{either::Either, prelude::*};
use leptos_icons::*;
use leptos_meta::{Meta, Title};
use leptos_router::{
    hooks::{query_signal, use_query_map},
    use_navigate,
};
use std::{cmp::Reverse, collections::HashMap, fmt::Display, str::FromStr};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
    world::World,
    world_helper::AnyResult,
};
use xiv_gen::{ItemId, Recipe};

#[derive(Clone, Debug, PartialEq)]
struct SubcraftInfo {
    item_id: ItemId,
    amount: i32,
    unit_cost: i32,
}

#[derive(Clone, Debug, PartialEq)]
struct RecipeProfitData {
    recipe: &'static Recipe,
    profit: i32,
    return_on_investment: i32,
    cost: i32,
    market_price: i32,
    cheapest_world_id: i32,
    ingredients_cost: Vec<(ItemId, i32)>, // ItemId, Cost
    sub_crafts: Vec<SubcraftInfo>,
    avg_sale_interval_secs: Option<u64>,
    num_sold: usize,
    required_level: i32,
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

impl Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Roi => "roi",
            SortMode::Profit => "profit",
        };
        f.write_str(val)
    }
}

fn calculate_crafting_cost(
    recipe: &'static Recipe,
    prices: &CheapestListingsMap,
    recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    depth: i32,
    max_depth: i32,
    use_subcrafts: bool,
    require_hq: bool,
) -> (i32, Vec<SubcraftInfo>) {
    // Use 64-bit intermediates with saturating math to avoid overflow in debug builds.
    let mut cost: i64 = 0;
    let mut sub_crafts = Vec::new();
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

        let price_summary = prices.find_matching_listings(item_id.0);
        let market_price = if require_hq {
            price_summary.price_preferring_hq().unwrap_or(999_999_999) as i64
        } else {
            price_summary.lowest_gil().unwrap_or(999_999_999) as i64
        };

        let mut item_cost = market_price;
        let mut best_sub_crafts = Vec::new();

        if use_subcrafts
            && depth < max_depth
            && let Some(sub_recipes) = recipes_by_output.get(&item_id)
        {
            // Find cheapest way to craft this ingredient
            for sub_recipe in sub_recipes {
                let (craft_cost, sub_details) = calculate_crafting_cost(
                    sub_recipe,
                    prices,
                    recipes_by_output,
                    depth + 1,
                    max_depth,
                    use_subcrafts,
                    require_hq,
                );
                let craft_cost = craft_cost as i64;
                if craft_cost < item_cost {
                    item_cost = craft_cost;
                    best_sub_crafts = sub_details;
                    // Add the direct subcraft itself
                    best_sub_crafts.push(SubcraftInfo {
                        item_id,
                        amount: 1, // Will be scaled by 'amount' later
                        unit_cost: craft_cost as i32,
                    });
                }
            }
        }

        if item_cost < market_price {
            // Scale subcrafts by the amount needed
            for mut sub in best_sub_crafts {
                sub.amount *= amount as i32;
                sub_crafts.push(sub);
            }
        }

        let amount = amount as i64;
        cost = cost.saturating_add(item_cost.saturating_mul(amount));
    }
    let clamped_cost = if cost < 0 {
        0
    } else if cost > i32::MAX as i64 {
        i32::MAX
    } else {
        cost as i32
    };

    (clamped_cost, sub_crafts)
}

fn compute_avg_interval_secs(sale: &SaleData) -> (usize, Option<u64>) {
    let now = Utc::now().naive_utc();
    let num_sold = sale.sales.len();
    if num_sold == 0 {
        return (0, None);
    }
    let last = match sale.sales.last() {
        Some(last) => last,
        None => return (0, None),
    };
    let ms = (last.sale_date - now).num_milliseconds().abs() / num_sold as i64;
    if ms < 0 {
        (num_sold, None)
    } else {
        (num_sold, Some((ms as u64) / 1_000))
    }
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
fn RecipeAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    recent_sales: Option<RecentSales>,

    world: Signal<String>,
) -> impl IntoView {
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = xiv_gen_db::data();
    let items = &data.items;
    let recipes = &data.recipes;
    let recipe_level_tables = &data.recipe_level_tables;

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
    let (max_sale_interval, set_max_sale_interval) = query_signal::<String>("next-sale");
    let (require_hq, set_require_hq) = query_signal::<bool>("require-hq");

    let sale_interval_limit =
        Memo::new(move |_| max_sale_interval().and_then(|d| parse_duration(d.as_str()).ok()));
    let sale_interval_string = Memo::new(move |_| {
        sale_interval_limit()
            .map(|duration| format_duration(duration).to_string())
            .unwrap_or("---".to_string())
    });

    let cookies = use_context::<Cookies>().unwrap();
    let (crafter_levels, _) = cookies.use_cookie_typed::<_, CrafterLevels>("CRAFTER_LEVELS");

    let has_levels = Memo::new(move |_| {
        let levels = crafter_levels.get().unwrap_or_default();
        levels.carpenter > 0
            || levels.blacksmith > 0
            || levels.armorer > 0
            || levels.goldsmith > 0
            || levels.leatherworker > 0
            || levels.weaver > 0
            || levels.alchemist > 0
            || levels.culinarian > 0
    });

    let computed_data = Memo::new(move |_| {
        let recipes_by_output = recipes_by_output();
        let levels = crafter_levels.get().unwrap_or_default();
        let use_sub = use_subcrafts().unwrap_or(false);
        let require_hq_flag = require_hq().unwrap_or(false);

        let sale_index: HashMap<i32, (usize, Option<u64>)> = if let Some(ref sales) = recent_sales {
            let mut idx = HashMap::new();
            for sale in &sales.sales {
                let (count, interval) = compute_avg_interval_secs(sale);
                idx.entry(sale.item_id)
                    .and_modify(|(total, existing)| {
                        *total += count;
                        if let Some(new) = interval {
                            match existing {
                                Some(old) if new < *old => *existing = Some(new),
                                None => *existing = Some(new),
                                _ => {}
                            }
                        }
                    })
                    .or_insert((count, interval));
            }
            idx
        } else {
            HashMap::new()
        };

        let mut results = Vec::new();

        // If no levels set, return empty (but we'll show a message)
        if !has_levels() {
            return vec![];
        }

        for recipe in recipes.values() {
            // Filter by job and level
            let required_level = recipe_level_tables
                .get(&recipe.recipe_level_table)
                .map(|t| t.class_job_level as i32)
                .unwrap_or(0);

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

            if let Some(filter) = job_filter()
                && filter != job_code
            {
                continue;
            }

            // Check if the user can realistically craft this recipe.
            // If we have a required_level from RecipeLevelTable, ensure user_level >= required_level.
            // If we don't, fall back to "any non-zero level can craft".
            if user_level == 0 {
                continue;
            }
            if required_level > 0 && user_level < required_level {
                continue;
            }

            let (num_sold, avg_interval_secs) = sale_index
                .get(&recipe.item_result.0)
                .cloned()
                .unwrap_or((0, None));

            let market_price_summary = prices.find_matching_listings(recipe.item_result.0);
            let market_price = market_price_summary.lowest_gil().unwrap_or(0);

            if market_price == 0 {
                continue;
            }

            let cheapest_world_id = market_price_summary
                .lq
                .map(|d| d.world_id)
                .or(market_price_summary.hq.map(|d| d.world_id))
                .unwrap_or(0);

            let (craft_cost, sub_crafts) = calculate_crafting_cost(
                recipe,
                &prices,
                &recipes_by_output,
                0,
                if use_sub { 2 } else { 0 }, // Limit recursion depth
                use_sub,
                require_hq_flag,
            );

            // craft_cost represents the cost to perform the recipe once.
            // This is effectively a per-result-unit cost for recipes that yield a single item.
            // If result quantities are exposed from xiv_gen in the future, divide by that quantity here.
            let cost_per_unit = craft_cost;

            if cost_per_unit >= market_price {
                continue;
            }

            let profit = market_price - cost_per_unit;
            let roi = if cost_per_unit > 0 {
                (profit as f64 / cost_per_unit as f64 * 100.0) as i32
            } else {
                0
            };

            results.push(RecipeProfitData {
                recipe,
                profit,
                return_on_investment: roi,
                cost: cost_per_unit,
                market_price,
                cheapest_world_id,
                ingredients_cost: vec![], // Populate if needed for tooltip
                sub_crafts,
                avg_sale_interval_secs: avg_interval_secs,
                num_sold,
                required_level,
            });
        }

        // Filter results
        if let Some(min) = minimum_profit() {
            results.retain(|d| d.profit >= min);
        }
        if let Some(min) = minimum_roi() {
            results.retain(|d| d.return_on_investment >= min);
        }
        if let Some(limit) = sale_interval_limit() {
            let limit_secs = limit.as_secs();
            results.retain(|d| {
                d.avg_sale_interval_secs
                    .map(|avg| avg <= limit_secs)
                    .unwrap_or(false)
            });
        }

        // Sort
        match sort_mode().unwrap_or(SortMode::Profit) {
            SortMode::Roi => results.sort_by_key(|d| Reverse(d.return_on_investment)),
            SortMode::Profit => results.sort_by_key(|d| Reverse(d.profit)),
        }

        results
            .into_iter()
            .take(100)
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
                    title="Sale Time Prediction"
                    description="Filter by predicted time to next sale (e.g., 1d 12h)"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">{sale_interval_string}</div>
                        <input
                            class="input"
                            placeholder="e.g. 1d 12h"
                            title="Accepts formats like 1h 30m, 7d, 1M (month), etc."
                            prop:value=move || max_sale_interval().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_max_sale_interval(Some(value))
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
                    <div class="text-center p-8 text-brand-300 bg-brand-900/20 rounded-lg border border-brand-800">
                    <h3 class="text-xl font-bold mb-2">"No Crafter Levels Configured"</h3>
                    <p>"Please configure your crafter levels above to see profitable recipes."</p>
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
                    <div class="text-brand-300 cursor-help" title="If enabled, the analyzer will check if it's cheaper to craft intermediate ingredients rather than buying them from the market board.">
                        <Icon icon=i::AiQuestionCircleOutlined />
                    </div>
                </div>
                <div class="flex flex-row gap-4 flex-wrap">
                    <input
                        type="checkbox"
                        id="require-hq"
                        class="checkbox"
                        prop:checked=move || require_hq().unwrap_or(false)
                        on:change=move |ev| set_require_hq(Some(event_target_checked(&ev)))
                    />
                    <label for="require-hq">"Require HQ Ingredients"</label>
                    <div class="text-brand-300 cursor-help" title="If enabled, ingredient costs will prefer HQ listings when available. Falls back to LQ if no HQ listing exists.">
                        <Icon icon=i::AiQuestionCircleOutlined />
                    </div>
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
                             <div role="columnheader" class="w-30 p-4">"Cost / unit"</div>
                             <div role="columnheader" class="w-30 p-4">"Price"</div>
                             <div role="columnheader" class="w-40 p-4 hidden md:block">"Avg sale"</div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, RecipeProfitData)| (*index, data.recipe.key_id)
                    view=move |(index, data): (usize, RecipeProfitData)| {
                        let item_id = data.recipe.item_result;
                        let item = items.get(&item_id).map(|i| i.name.as_str()).unwrap_or("Unknown");
                        let item_level = items.get(&item_id).map(|i| i.level_item.0).unwrap_or(0);
                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };

                        let job_abbrev = match data.recipe.craft_type.0 {
                            0 => "CRP",
                            1 => "BSM",
                            2 => "ARM",
                            3 => "GSM",
                            4 => "LTW",
                            5 => "WVR",
                            6 => "ALC",
                            7 => "CUL",
                            _ => "",
                        };



                        let (avg_label, avg_title) = if let Some(secs) = data.avg_sale_interval_secs {
                            let days = secs / 86_400;
                            let hours = (secs % 86_400) / 3_600;
                            let minutes = (secs % 3_600) / 60;
                            let mut parts = Vec::new();
                            if days > 0 {
                                parts.push(format!("{}d", days));
                            }
                            if hours > 0 && parts.len() < 2 {
                                parts.push(format!("{}h", hours));
                            }
                            if minutes > 0 && parts.len() < 2 {
                                parts.push(format!("{}m", minutes));
                            }
                            let label = if parts.is_empty() {
                                "<1m".to_string()
                            } else {
                                parts.join(" ")
                            };
                            let title = format!(
                                "Approximate time between sales based on recent history ({} sales in sample).",
                                data.num_sold
                            );
                            (label, title)
                        } else {
                            ("no data".to_string(), "No recent sales data for this item.".to_string())
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
                                        <div class="flex flex-col">
                                            <span>{item}</span>
                                            <span class="text-xs text-[color:var(--color-text-muted)]">
                                                "Lv " {data.required_level} " • iLv " {item_level} " " {job_abbrev}
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
                                    {
                                        let has_sub_crafts = !data.sub_crafts.is_empty();
                                        let sub_crafts = data.sub_crafts.clone();
                                        view! {
                                            <Show when=move || has_sub_crafts>
                                                {
                                                    let sub_crafts_for_text = sub_crafts.clone();
                                                    let count = sub_crafts.len();
                                                    view! {
                                                        <Tooltip
                                                            tooltip_text={
                                                                let sub_crafts_details: Vec<(String, i32, i32)> = sub_crafts_for_text.iter().map(|sub| {
                                                                    let name = items.get(&sub.item_id).map(|i| i.name.to_string()).unwrap_or("Unknown".to_string());
                                                                    (name, sub.amount, sub.unit_cost)
                                                                }).collect();
                                                                Signal::derive(move || {
                                                                    let mut tooltip = String::from("Includes sub-crafts:\n");
                                                                    for (name, amount, cost) in &sub_crafts_details {
                                                                        tooltip.push_str(&format!("• {}x {} ({} gil)\n", amount, name, cost));
                                                                    }
                                                                    tooltip
                                                                })
                                                            }
                                                        >
                                                            <div class="text-xs text-brand-300 flex items-center justify-end gap-1 cursor-help">
                                                                <Icon icon=i::FaHammerSolid width="0.8em" height="0.8em" />
                                                                <span>{count} " sub"</span>
                                                            </div>
                                                        </Tooltip>
                                                    }
                                                }
                                            </Show>
                                        }
                                    }
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.market_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-40 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]" title=avg_title>
                                        {avg_label}
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
fn CollapseIcon(collapsed: Signal<bool>) -> impl IntoView {
    view! {
        <Show
            when=collapsed
            fallback=|| view! { <div class="ml-auto"><Icon icon=i::BiChevronDownRegular /></div> }
        >
            <div class="ml-auto"><Icon icon=i::BiChevronUpRegular /></div>
        </Show>
    }
}

#[component]
pub fn RecipeAnalyzer() -> impl IntoView {
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let navigate = use_navigate();

    let selected_world = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        query
            .with(|p| p.get("world").cloned())
            .and_then(|name| worlds.lookup_world_by_name(&name))
            .and_then(|any_result| match any_result {
                AnyResult::World(w) => Some(w.clone()),
                _ => None,
            })
            .or_else(|| home_world.get())
    });

    let region = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        selected_world
            .get()
            .as_ref()
            .map(|world| {
                let region = worlds.get_region(AnyResult::World(world));
                AnyResult::Region(region).get_name().to_string()
            })
            .unwrap_or_else(|| "North-America".to_string())
    });

    let global_cheapest_listings = Resource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

<<<<<<< HEAD
=======
    let (selected_world, set_selected_world) = signal(None);
    Effect::new(move |_| {
        if selected_world.get_untracked().is_none()
            && let Some(home) = home_world.get()
        {
            set_selected_world(Some(home));
        }
    });

>>>>>>> main
    let recent_sales = Resource::new(selected_world, move |world| async move {
        if let Some(world) = world {
            leptos::logging::log!("Fetching sales for world: {}", &world.name);
            let res = get_recent_sales_for_world(&world.name).await;
            match &res {
                Ok(sales) => leptos::logging::log!("Sales result: {} items", sales.sales.len()),
                Err(e) => leptos::logging::log!("Sales error: {}", e),
            }
            res
        } else {
            leptos::logging::log!("No world selected for sales");
            Ok(RecentSales { sales: vec![] })
        }
    });

    let set_selected_world = Callback::new(move |world: Option<World>| {
        if let Some(world) = world {
            let query = format!("?world={}", world.name);
            let _ = navigate(&query, Default::default());
        }
    });

    view! {
        <div class="flex flex-col gap-4 h-full">
            <Title text="Recipe Analyzer - Ultros" />
            <Meta name="description" content="Analyze crafting recipes for profitability" />

            <div class="flex flex-col gap-4 p-4 bg-brand-900/50 rounded-lg border border-brand-800">
                <div class="flex flex-row justify-between items-center">
                    <h1 class="text-2xl font-bold text-brand-100">"Recipe Analyzer"</h1>
                    <div class="flex flex-row gap-2 items-center">
                        <Show when=move || recent_sales.get().is_none()>
                            <div class="text-brand-300 text-sm animate-pulse">"Loading sales data..."</div>
                        </Show>
                        <Show when=move || recent_sales.get().and_then(|r| r.err()).is_some()>
                            <div class="text-red-400 text-sm">"Error loading sales data"</div>
                        </Show>
<<<<<<< HEAD
                        <WorldOnlyPicker
                            current_world=selected_world
                            set_current_world=set_selected_world
                        />
=======
>>>>>>> main
                    </div>
                </div>
                {
                    let (show_settings, set_show_settings) = signal(false);
                    view! {
                        <div class="panel p-4 rounded-xl bg-brand-900/20 border border-white/10">
                            <button
                                class="flex items-center gap-2 text-brand-300 hover:text-brand-200 transition-colors font-medium w-full"
                                on:click=move |_| set_show_settings.update(|v| *v = !*v)
                            >
                                <Icon icon=i::AiSettingOutlined />
                                "Adjust Crafter Levels"
                                <CollapseIcon collapsed=show_settings.into() />
                            </button>
                            <div class=move || {
                                if show_settings() {
                                    "mt-4 block animate-in fade-in slide-in-from-top-2 duration-200"
                                } else {
                                    "hidden"
                                }
                            }>
                                <CrafterSettings />
                            </div>
                        </div>
                    }
                }

<<<<<<< HEAD
=======
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

>>>>>>> main
                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        let sales = recent_sales.get();
                        match (listings, sales) {
                            (Some(Ok(listings)), Some(Ok(sales))) => {
                                view! {
                                    <RecipeAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=Some(sales)
                                        world=Signal::derive(move || selected_world.get().map(|w| w.name.clone()).unwrap_or_default())
                                    />
                                }.into_any()
                            }
                            (Some(Ok(listings)), _) => {
                                view! {
                                    <RecipeAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=None
                                        world=Signal::derive(move || selected_world.get().map(|w| w.name.clone()).unwrap_or_default())
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
