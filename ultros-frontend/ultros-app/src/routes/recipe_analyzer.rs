use crate::components::crafting_cost::{
    CraftingCostOptions, EmptyOnHand, ShardsMode, compute_cost,
};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::on_hand_input::{ActiveListBanner, LocalOnHand, OnHandMap};
use crate::components::related_items::is_shard_item;
use crate::global_state::craft_options::{self, CraftOptions};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::{DEFAULT_MIN_DAILY_SALES, filter_query_signal, seed_query_default};
use crate::ws::realtime::use_realtime;
use crate::{
    analysis::{SalesStats, analyze_sales, roi_badge_class},
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_recipe_to_list::AddRecipeToList,
        crafter_settings::CrafterSettings,
        gil::*,
        icon::Icon,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortableHeaderCell, sort_and_truncate},
        tool_help::*,
        toolbar::{Toolbar, ToolbarField, ToolbarPills, ToolbarSpacer},
        tooltip::Tooltip,
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::{
        LocalWorldData, cookies::Cookies, crafter_levels::CrafterLevels,
        home_world::use_home_world, region_for_world::use_region_for_world,
    },
};
use icondata as i;
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_query_map},
};
use std::{cmp::Ordering, collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
};
use xiv_gen::{ItemId, Recipe, RecipeLevelTableId};

use crate::components::crafting_cost::SubcraftInfo;

#[derive(Clone, Debug, PartialEq)]
struct RecipeProfitData {
    recipe: &'static Recipe,
    profit: i32,
    return_on_investment: i32,
    cost: i32,
    market_price: i32,
    cheapest_world_id: i32,
    sub_crafts: Vec<SubcraftInfo>,
    daily_sales: f32,
    avg_price: i32,
    total_sales: usize,
    required_level: i32,
}

/// Acronym for a `Recipe::craft_type`, matching the `CraftType` sheet order.
/// Empty for anything outside the eight crafters.
fn craft_type_acronym(craft_type: i32) -> &'static str {
    match craft_type {
        0 => "CRP",
        1 => "BSM",
        2 => "ARM",
        3 => "GSM",
        4 => "LTW",
        5 => "WVR",
        6 => "ALC",
        7 => "CUL",
        _ => "",
    }
}

/// The user's level for a job acronym, or `None` if the acronym isn't a
/// crafter. Shares one table with the recipe filter so the per-job empty state
/// can never disagree with the rows the filter actually kept.
fn level_for_job_code(levels: &CrafterLevels, code: &str) -> Option<i32> {
    Some(match code {
        "CRP" => levels.carpenter,
        "BSM" => levels.blacksmith,
        "ARM" => levels.armorer,
        "GSM" => levels.goldsmith,
        "LTW" => levels.leatherworker,
        "WVR" => levels.weaver,
        "ALC" => levels.alchemist,
        "CUL" => levels.culinarian,
        _ => return None,
    })
}

/// Every crafter acronym, in `CraftType` order.
const JOB_CODES: [&str; 8] = ["CRP", "BSM", "ARM", "GSM", "LTW", "WVR", "ALC", "CUL"];

/// Whether any crafter is above level 0. A user with all-zero levels can't
/// craft anything, so the analyzer has nothing to rank.
fn has_any_level(levels: &CrafterLevels) -> bool {
    JOB_CODES
        .iter()
        .any(|code| level_for_job_code(levels, code).unwrap_or(0) > 0)
}

/// Why the results table has nothing in it. Each variant maps to a distinct
/// empty state — a blank table with no explanation is what made #1063 read as
/// "BSM is broken" rather than "this filter combination excludes everything".
#[derive(Debug, Clone, PartialEq, Eq)]
enum EmptyReason {
    /// Every crafter level is 0.
    NoLevels,
    /// A job filter is active and that one job's level is 0.
    JobLevelZero(String),
    /// Levels are set and recipes exist, but the filters removed all of them.
    FiltersExcludeAll,
}

/// Classify an empty results table. `results_empty` is the outcome of the full
/// filter pipeline; the other arguments are the inputs that most often explain
/// it. Returns `None` whenever there are rows to show, so an explanation can
/// never render above a populated table.
fn empty_reason(
    results_empty: bool,
    levels: &CrafterLevels,
    job_filter: Option<&str>,
) -> Option<EmptyReason> {
    if !results_empty {
        return None;
    }
    // A zeroed job is named ahead of the all-zero case: it's the filter the
    // user is actually looking at, and it's the one thing they can act on.
    if let Some(job) = job_filter
        && level_for_job_code(levels, job) == Some(0)
    {
        return Some(EmptyReason::JobLevelZero(job.to_string()));
    }
    if !has_any_level(levels) {
        return Some(EmptyReason::NoLevels);
    }
    Some(EmptyReason::FiltersExcludeAll)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
    Velocity,
    CostPerUnit,
    Price,
    AvgPrice,
}

impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            "velocity" => Ok(SortMode::Velocity),
            "cost" => Ok(SortMode::CostPerUnit),
            "price" => Ok(SortMode::Price),
            "avg-price" => Ok(SortMode::AvgPrice),
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
            SortMode::CostPerUnit => "cost",
            SortMode::Price => "price",
            SortMode::AvgPrice => "avg-price",
        };
        f.write_str(val)
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }

    /// Cost per unit reads best-first ascending — the cheapest craft is the
    /// interesting one. Everything else is a biggest-first metric.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::CostPerUnit => SortDir::Asc,
            _ => SortDir::Desc,
        }
    }
}

fn compare_recipes(mode: SortMode, a: &RecipeProfitData, b: &RecipeProfitData) -> Ordering {
    match mode {
        SortMode::Roi => a.return_on_investment.cmp(&b.return_on_investment),
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Velocity => a
            .daily_sales
            .partial_cmp(&b.daily_sales)
            .unwrap_or(Ordering::Equal),
        SortMode::CostPerUnit => a.cost.cmp(&b.cost),
        SortMode::Price => a.market_price.cmp(&b.market_price),
        SortMode::AvgPrice => a.avg_price.cmp(&b.avg_price),
    }
}

#[component]
fn RecipeAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    recent_sales: Option<RecentSales>,

    world: Signal<String>,
) -> impl IntoView {
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
    let recipes = &data.recipes;
    let recipe_level_tables = &data.recipe_level_tables;
    let i18n = use_i18n();

    // Index recipes by output item for subcraft lookup
    let recipes_by_output = Memo::new(move |_| {
        let mut map: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        for recipe in recipes.values() {
            map.entry(ItemId(recipe.item_result))
                .or_default()
                .push(recipe);
        }
        map
    });

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (job_filter, set_job_filter) = query_signal::<String>("job");
    let (use_subcrafts, set_use_subcrafts) = query_signal::<bool>("subcrafts");
    // Seeded by RecipeAnalyzer so a first-time visitor isn't shown recipes
    // whose output sells once a month. Same velocity floor as the analyzer's
    // 1d default.
    let (min_daily_sales, set_min_daily_sales) = filter_query_signal::<f32>("min-sales");
    let (require_hq, set_require_hq) = query_signal::<bool>("require-hq");
    let (filter_outliers, set_filter_outliers) = query_signal::<bool>("filter-outliers");
    let (exclude_shards_url, set_exclude_shards) = query_signal::<bool>("shards-exclude");
    let (use_on_hand_url, set_use_on_hand) = query_signal::<bool>("on-hand");

    let cookies = use_context::<Cookies>().unwrap();
    let (crafter_levels, _) = cookies.use_cookie_typed::<_, CrafterLevels>("CRAFTER_LEVELS");
    let (craft_options, _) =
        cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME);
    let exclude_shards_enabled = move || {
        exclude_shards_url()
            .unwrap_or_else(|| craft_options.get().unwrap_or_default().exclude_shards)
    };
    let use_on_hand_enabled = move || {
        use_on_hand_url().unwrap_or_else(|| craft_options.get().unwrap_or_default().use_on_hand)
    };

    let has_levels = Memo::new(move |_| has_any_level(&crafter_levels.get().unwrap_or_default()));

    let computed_data = Memo::new(move |_| {
        let recipes_by_output = recipes_by_output();
        let levels = crafter_levels.get().unwrap_or_default();
        let use_sub = use_subcrafts().unwrap_or(false);
        let require_hq_flag = require_hq().unwrap_or(false);
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

        let mut results = Vec::new();

        // If no levels set, return empty (but we'll show a message)
        if !has_levels() {
            return vec![];
        }

        // Hoist context lookups ONCE; the on-hand SNAPSHOT is rebuilt
        // per recipe inside the loop because compute_cost consumes it.
        let opts_value = craft_options.get().unwrap_or_default();
        let shards = if exclude_shards_enabled() {
            ShardsMode::ExcludeShards
        } else {
            ShardsMode::IncludeMarket
        };
        let on_hand_map = use_context::<OnHandMap>();
        let use_on_hand = use_on_hand_enabled();

        for recipe in recipes.values() {
            // Filter by job and level
            let required_level = recipe_level_tables
                .get(&RecipeLevelTableId(recipe.recipe_level_table))
                .map(|t| t.class_job_level as i32)
                .unwrap_or(0);

            let job_code = craft_type_acronym(recipe.craft_type);
            let user_level = level_for_job_code(&levels, job_code).unwrap_or(0);

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

            let sales_stats = if let Some(item_sales) = sales_map.get(&{ recipe.item_result }) {
                analyze_sales(item_sales, filter_outliers)
            } else {
                SalesStats {
                    daily_sales: 0.0,
                    avg_price: 0,
                    total_sales: 0,
                }
            };

            let market_price_summary = prices.find_matching_listings(recipe.item_result);
            let market_price = market_price_summary.lowest_gil().unwrap_or(0);

            if market_price == 0 {
                continue;
            }

            let cheapest_world_id = market_price_summary
                .lq
                .map(|d| d.world_id)
                .or(market_price_summary.hq.map(|d| d.world_id))
                .unwrap_or(0);

            // Fresh on-hand snapshot per recipe — compute_cost consumes
            // from the snapshot, and reusing one across recipes would
            // wrongly deplete the user's stockpile after the first recipe.
            let local = on_hand_map
                .map(|m: OnHandMap| LocalOnHand::from_map(m.0.get_untracked()))
                .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
            let empty = EmptyOnHand;
            // TODO(follow-up): when active_craft_list is Some, fetch the list resource
            // and construct ListOnHand from its items instead of falling through to LocalOnHand.
            // The type (ListOnHand) is in place; the async resource fetch is the missing piece.
            let active: Box<dyn crate::components::crafting_cost::OnHand> =
                match opts_value.active_craft_list {
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
                require_hq: require_hq_flag,
                max_subcraft_depth: if use_sub { 2 } else { 0 },
                shards,
                on_hand: active.as_ref(),
            };
            let breakdown =
                compute_cost(recipe, &prices, &recipes_by_output, &opts, &is_shard_item);
            let craft_cost = breakdown.cost;
            let sub_crafts = breakdown.sub_crafts.clone();

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
                sub_crafts,
                daily_sales: sales_stats.daily_sales,
                avg_price: sales_stats.avg_price,
                total_sales: sales_stats.total_sales,
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
        if let Some(min_sales) = min_daily_sales() {
            results.retain(|d| d.daily_sales >= min_sales);
        }

        // Sort
        // ⚡ Bolt: Optimization: In-place filtering and truncation for Top N lists using select_nth_unstable.
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        sort_and_truncate(&mut results, dir, 100, |a, b| compare_recipes(mode, a, b));

        results
            .into_iter()
            .map(Arc::new)
            .enumerate()
            .collect::<Vec<_>>()
    });

    let empty_state = Memo::new(move |_| {
        empty_reason(
            computed_data.with(|d| d.is_empty()),
            &crafter_levels.get().unwrap_or_default(),
            job_filter().as_deref(),
        )
    });

    // Localized display name for a job acronym, for the per-job empty state.
    let job_name = move |code: &str| -> String {
        match code {
            "CRP" => t_string!(i18n, carpenter).to_string(),
            "BSM" => t_string!(i18n, blacksmith).to_string(),
            "ARM" => t_string!(i18n, armorer).to_string(),
            "GSM" => t_string!(i18n, goldsmith).to_string(),
            "LTW" => t_string!(i18n, leatherworker).to_string(),
            "WVR" => t_string!(i18n, weaver).to_string(),
            "ALC" => t_string!(i18n, alchemist).to_string(),
            "CUL" => t_string!(i18n, culinarian).to_string(),
            other => other.to_string(),
        }
    };

    let clear_filters = Callback::new(move |()| {
        set_minimum_profit(None);
        set_minimum_roi(None);
        set_min_daily_sales(None);
    });
    let clear_job_filter = Callback::new(move |()| set_job_filter(None));

    view! {
        <div class="flex flex-col gap-6">
            <ActiveListBanner />
            // Primary filter toolbar
            <Toolbar>
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_profit_min_label).to_string()>
                    <input
                        class="input input-sm w-32"
                        min=0
                        step=1000
                        placeholder=t_string!(i18n, placeholder_eg_10000)
                        type="number"
                        prop:value=minimum_profit
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(profit) = value.parse::<i32>() {
                                set_minimum_profit(Some(profit));
                            } else if value.is_empty() {
                                set_minimum_profit(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_roi_min_label).to_string()>
                    <input
                        class="input input-sm w-28"
                        min=0
                        step=10
                        placeholder=t_string!(i18n, placeholder_eg_200)
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
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_daily_sales_min_label).to_string()>
                    <input
                        class="input input-sm w-24"
                        type="number"
                        min="0"
                        step="0.1"
                        placeholder="e.g. 1.0"
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
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_job_label).to_string()>
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
                        <option value="CRP" selected=move || job_filter() == Some("CRP".to_string())>{t!(i18n, carpenter)}</option>
                        <option value="BSM" selected=move || job_filter() == Some("BSM".to_string())>{t!(i18n, blacksmith)}</option>
                        <option value="ARM" selected=move || job_filter() == Some("ARM".to_string())>{t!(i18n, armorer)}</option>
                        <option value="GSM" selected=move || job_filter() == Some("GSM".to_string())>{t!(i18n, goldsmith)}</option>
                        <option value="LTW" selected=move || job_filter() == Some("LTW".to_string())>{t!(i18n, leatherworker)}</option>
                        <option value="WVR" selected=move || job_filter() == Some("WVR".to_string())>{t!(i18n, weaver)}</option>
                        <option value="ALC" selected=move || job_filter() == Some("ALC".to_string())>{t!(i18n, alchemist)}</option>
                        <option value="CUL" selected=move || job_filter() == Some("CUL".to_string())>{t!(i18n, culinarian)}</option>
                    </select>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_subcrafts_label).to_string()>
                    <ToolbarPills>
                        <button
                            aria-pressed=move || if use_subcrafts().unwrap_or(false) { "false" } else { "true" }
                            title=t_string!(i18n, recipe_analyzer_subcrafts_tooltip)
                            on:click=move |_| set_use_subcrafts(Some(!use_subcrafts().unwrap_or(false)))
                        >
                            "Off"
                        </button>
                        <button
                            aria-pressed=move || if use_subcrafts().unwrap_or(false) { "true" } else { "false" }
                            title=t_string!(i18n, recipe_analyzer_subcrafts_tooltip)
                            on:click=move |_| set_use_subcrafts(Some(!use_subcrafts().unwrap_or(false)))
                        >
                            "On"
                        </button>
                    </ToolbarPills>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_require_hq_label).to_string()>
                    <ToolbarPills>
                        <button
                            aria-pressed=move || if require_hq().unwrap_or(false) { "false" } else { "true" }
                            title=t_string!(i18n, recipe_analyzer_require_hq_tooltip)
                            on:click=move |_| set_require_hq(Some(!require_hq().unwrap_or(false)))
                        >
                            "Off"
                        </button>
                        <button
                            aria-pressed=move || if require_hq().unwrap_or(false) { "true" } else { "false" }
                            title=t_string!(i18n, recipe_analyzer_require_hq_tooltip)
                            on:click=move |_| set_require_hq(Some(!require_hq().unwrap_or(false)))
                        >
                            "On"
                        </button>
                    </ToolbarPills>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, filter_outliers).to_string()>
                    <ToolbarPills>
                        <button
                            aria-pressed=move || if filter_outliers().unwrap_or(false) { "false" } else { "true" }
                            title=t_string!(i18n, venture_analyzer_filter_outliers_tooltip)
                            on:click=move |_| set_filter_outliers(Some(!filter_outliers().unwrap_or(false)))
                        >
                            "Off"
                        </button>
                        <button
                            aria-pressed=move || if filter_outliers().unwrap_or(false) { "true" } else { "false" }
                            title=t_string!(i18n, venture_analyzer_filter_outliers_tooltip)
                            on:click=move |_| set_filter_outliers(Some(!filter_outliers().unwrap_or(false)))
                        >
                            "On"
                        </button>
                    </ToolbarPills>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_exclude_shards_label).to_string()>
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
                <ToolbarField label=t_string!(i18n, recipe_analyzer_filter_use_on_hand_label).to_string()>
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
                        last_update=last_update
                    />
            </Toolbar>

            {move || match empty_state.get() {
                None => ().into_any(),
                Some(EmptyReason::NoLevels) => view! {
                    <ActionableEmptyState
                        title=t_string!(i18n, recipe_analyzer_empty_set_levels_title).to_string()
                        body=t_string!(i18n, recipe_analyzer_empty_set_levels_body).to_string()
                        action_href="/help/recipe-analyzer"
                        action_label=t_string!(i18n, recipe_analyzer_empty_read_help).to_string()
                    />
                }.into_any(),
                Some(EmptyReason::JobLevelZero(job)) => {
                    let job = job_name(&job);
                    view! {
                        <ActionableEmptyState
                            title=t_string!(i18n, recipe_analyzer_empty_job_level_zero_title, job = job.clone()).to_string()
                            body=t_string!(i18n, recipe_analyzer_empty_job_level_zero_body, job = job).to_string()
                            on_action=clear_job_filter
                            action_label=t_string!(i18n, recipe_analyzer_empty_clear_job_filter).to_string()
                            secondary_action_href="/help/recipe-analyzer"
                            secondary_action_label=t_string!(i18n, recipe_analyzer_empty_read_help).to_string()
                        />
                    }.into_any()
                }
                Some(EmptyReason::FiltersExcludeAll) => view! {
                    <ActionableEmptyState
                        title=t_string!(i18n, recipe_analyzer_empty_filters_title).to_string()
                        body=t_string!(i18n, recipe_analyzer_empty_filters_body).to_string()
                        on_action=clear_filters
                        action_label=t_string!(i18n, recipe_analyzer_empty_clear_filters).to_string()
                        secondary_action_href="/help/recipe-analyzer"
                        secondary_action_label=t_string!(i18n, recipe_analyzer_empty_read_help).to_string()
                    />
                }.into_any(),
            }}

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
                             <div role="columnheader" class="w-64 md:w-80 shrink-0 p-4">{t!(i18n, item)}</div>
                             <SortableHeaderCell
                                mode=SortMode::Profit
                                label=t_string!(i18n, profit).to_string()
                                class="w-32 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Roi
                                label=t_string!(i18n, roi).to_string()
                                class="w-32 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::CostPerUnit
                                label=t_string!(i18n, recipe_analyzer_col_cost_per_unit).to_string()
                                class="w-32 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Price
                                label=t_string!(i18n, price).to_string()
                                class="w-32 shrink-0 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Velocity
                                label=t_string!(i18n, daily_sales).to_string()
                                class="w-32 shrink-0 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::AvgPrice
                                label=t_string!(i18n, avg_price).to_string()
                                class="w-32 shrink-0 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                             <div role="columnheader" class="w-20 shrink-0 p-4">{t!(i18n, actions)}</div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<RecipeProfitData>)| (*index, data.recipe.key_id)
                    view=move |(index, data): (usize, Arc<RecipeProfitData>)| {
                        let item_id = ItemId(data.recipe.item_result);
                        let item = items.get(&item_id).map(|i| i.name.as_str()).unwrap_or("Unknown");
                        let item_level = items.get(&item_id).map(|i| i.level_item).unwrap_or(0);
                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };

                        let job_abbrev = craft_type_acronym(data.recipe.craft_type);

                        let sales_tooltip = format!(
                            "Based on {} sales over {:.1} days",
                            data.total_sales,
                            (data.total_sales as f32 / data.daily_sales.max(0.001)) // approximate duration back
                        );

                        view! {
                            <div class=classes role="row-group">
                                <div role="cell" class="px-4 py-2 flex flex-row w-64 md:w-80 shrink-0 items-center gap-2">
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
                                <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right">
                                    <Gil amount=data.profit />
                                </div>
                                <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right">
                                     <span class={roi_badge_class(data.return_on_investment)}>
                                        {format!("{}%", data.return_on_investment)}
                                    </span>
                                </div>
                                <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right">
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
                                <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right">
                                    <Gil amount=data.market_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]" title=sales_tooltip>
                                        {format!("{:.1} / day", data.daily_sales)}
                                    </span>
                                </div>
                                <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right hidden md:block">
                                    <Gil amount=data.avg_price />
                                </div>
                                 <div role="cell" class="px-4 py-2 w-20 shrink-0">
                                     <AddRecipeToList recipe=data.recipe />
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
    let i18n = use_i18n();
    // Seeded here rather than in RecipeAnalyzerTable: that lives inside the
    // Suspense closure and remounts whenever its resources change, which would
    // keep undoing a filter the user had cleared.
    seed_query_default("min-sales", DEFAULT_MIN_DAILY_SALES);
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let nav = use_navigate();

    // The route has no `:world` path segment, so shared links carry the world
    // in the query string (`?world=Gilgamesh`), same as the leve analyzer.
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
            <MetaTitle title="Recipe Analyzer - Ultros" />
            <MetaDescription text=t_string!(i18n, recipe_analyzer_meta_desc) />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, recipe_analyzer).to_string()
                    summary=t_string!(i18n, recipe_analyzer_tool_summary).to_string()
                    context=t_string!(i18n, recipe_analyzer_tool_context).to_string()
                    help_href="/help/recipe-analyzer"
                    help_body=t_string!(i18n, recipe_analyzer_tool_help).to_string()
                />
                <div class="flex flex-row justify-end items-center">
                    <div class="flex flex-row gap-2 items-center">
                        <Suspense fallback=move || view! { <div class="text-brand-300 text-sm animate-pulse">{t!(i18n, loading_sales_data)}</div> }>
                            {move || {
                                recent_sales_clone
                                    .get()
                                    .and_then(|r| r.err())
                                    .map(|_| view! { <div class="text-red-400 text-sm">{t!(i18n, error_loading_sales_data)}</div> })
                            }}
                        </Suspense>
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
                                {t!(i18n, recipe_analyzer_adjust_levels)}
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
                <CalculationSummary
                    title=t_string!(i18n, recipe_analyzer_calc_title).to_string()
                    formula=t_string!(i18n, recipe_analyzer_calc_formula).to_string()
                    details=t_string!(i18n, recipe_analyzer_calc_details).to_string()
                />
                <div class="flex flex-wrap gap-2">
                    <AssumptionBadge text=t_string!(i18n, recipe_analyzer_assumption_crafter_levels).to_string() />
                    <AssumptionBadge text=t_string!(i18n, recipe_analyzer_assumption_subcraft_recursion).to_string() />
                    <AssumptionBadge text=t_string!(i18n, recipe_analyzer_assumption_sales_velocity).to_string() />
                </div>

                // Rendered unconditionally: gating on `selected_world.is_some()`
                // hid the only control that can set a world from a visitor who
                // has neither a home-world cookie nor `?world=` in the URL.
                <div class="flex flex-col md:flex-row items-center gap-2">
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, select_world_for_sales_data)}</label>
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
                        let sales = recent_sales.get();
                        match (listings, sales) {
                            (Some(Ok(listings)), Some(Ok(sales))) => {
                                view! {
                                    <RecipeAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=Some(sales)
                                        world=Signal::derive(region)
                                    />
                                }.into_any()
                            }
                            (Some(Ok(listings)), _) => {
                                view! {
                                    <RecipeAnalyzerTable
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

#[cfg(test)]
mod test {
    use super::*;
    use xiv_gen::ClassJobId;

    /// Display must produce exactly the token FromStr parses back — the
    /// shared SortHeader's hrefs depend on that round trip.
    #[test]
    fn sort_mode_round_trips_through_the_url() {
        for mode in [
            SortMode::Roi,
            SortMode::Profit,
            SortMode::Velocity,
            SortMode::CostPerUnit,
            SortMode::Price,
            SortMode::AvgPrice,
        ] {
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert!("bogus".parse::<SortMode>().is_err());
    }

    /// `Recipe::craft_type` is a row index into the `CraftType` sheet, which
    /// xiv-gen doesn't load. The eight Disciple of the Hand jobs are
    /// consecutive `ClassJob` rows starting at carpenter, in the same order
    /// `CraftType` uses, so walk those rows and pin both the spelling and the
    /// ordering against real game data instead of a second copy of the same
    /// hand-written list.
    ///
    /// `doh_dol_job_index` alone can't anchor this: it restarts at 0 for the
    /// gatherers, so miner also reports index 0. It's checked here as a second
    /// signal once the row is known to be a crafter.
    #[test]
    fn craft_type_acronyms_match_the_crafter_class_jobs() {
        let data = xiv_gen_db::data();
        let carpenter = data
            .class_jobs
            .iter()
            .find(|(_, j)| j.abbreviation == "CRP")
            .map(|(id, _)| id.0)
            .expect("game data should have a carpenter");

        for (offset, expected) in JOB_CODES.iter().enumerate() {
            let row = ClassJobId(carpenter + offset as i32);
            let class_job = data
                .class_jobs
                .get(&row)
                .unwrap_or_else(|| panic!("no ClassJob at row {}", row.0));
            assert_eq!(
                &class_job.abbreviation, expected,
                "ClassJob row {} is {:?}, not {expected}",
                row.0, class_job.abbreviation
            );
            assert_eq!(
                class_job.doh_dol_job_index as usize, offset,
                "{expected} should be crafter #{offset}"
            );
            assert_eq!(craft_type_acronym(offset as i32), *expected);
        }
    }

    #[test]
    fn craft_type_acronym_is_empty_outside_the_crafters() {
        assert_eq!(craft_type_acronym(8), "");
        assert_eq!(craft_type_acronym(-1), "");
    }

    /// Every acronym the job-filter dropdown can emit must resolve to a level.
    /// A missing arm here is what turns "filter by BSM" into a silent zero.
    #[test]
    fn every_job_code_resolves_to_a_level() {
        let levels = CrafterLevels::default();
        for code in JOB_CODES {
            assert_eq!(
                level_for_job_code(&levels, code),
                Some(100),
                "{code} should read back the default level"
            );
        }
        assert_eq!(level_for_job_code(&levels, "MIN"), None);
        assert_eq!(level_for_job_code(&levels, ""), None);
    }

    #[test]
    fn level_for_job_code_reads_the_matching_field() {
        let levels = CrafterLevels {
            blacksmith: 0,
            ..Default::default()
        };
        assert_eq!(level_for_job_code(&levels, "BSM"), Some(0));
        assert_eq!(level_for_job_code(&levels, "CRP"), Some(100));
    }

    /// An explanation must never render above a populated table, whatever the
    /// levels and filter say.
    #[test]
    fn no_empty_state_when_there_are_results() {
        assert_eq!(
            empty_reason(false, &CrafterLevels::default(), None),
            None,
            "a populated table needs no explanation"
        );
        let zeroed = CrafterLevels {
            blacksmith: 0,
            ..Default::default()
        };
        assert_eq!(empty_reason(false, &zeroed, Some("BSM")), None);
    }

    #[test]
    fn all_zero_levels_reports_no_levels() {
        let levels = CrafterLevels {
            carpenter: 0,
            blacksmith: 0,
            armorer: 0,
            goldsmith: 0,
            leatherworker: 0,
            weaver: 0,
            alchemist: 0,
            culinarian: 0,
        };
        assert!(!has_any_level(&levels));
        assert_eq!(
            empty_reason(true, &levels, None),
            Some(EmptyReason::NoLevels)
        );
    }

    /// The #1063 report: filtering to a job whose level is 0 produced a blank
    /// table that read as "this job is broken".
    #[test]
    fn zeroed_job_under_its_own_filter_is_called_out() {
        let levels = CrafterLevels {
            blacksmith: 0,
            ..Default::default()
        };
        assert!(
            has_any_level(&levels),
            "the other seven jobs are still leveled"
        );
        assert_eq!(
            empty_reason(true, &levels, Some("BSM")),
            Some(EmptyReason::JobLevelZero("BSM".to_string()))
        );
    }

    /// A zeroed job is worth naming even when the *other* jobs are also zero:
    /// it's the filter the user is actually looking at.
    #[test]
    fn job_level_zero_outranks_no_levels() {
        let levels = CrafterLevels {
            carpenter: 0,
            blacksmith: 0,
            armorer: 0,
            goldsmith: 0,
            leatherworker: 0,
            weaver: 0,
            alchemist: 0,
            culinarian: 0,
        };
        assert_eq!(
            empty_reason(true, &levels, Some("BSM")),
            Some(EmptyReason::JobLevelZero("BSM".to_string()))
        );
    }

    /// A leveled job with no rows left is the filters' doing, not the level's.
    #[test]
    fn leveled_job_with_no_rows_blames_the_filters() {
        assert_eq!(
            empty_reason(true, &CrafterLevels::default(), Some("BSM")),
            Some(EmptyReason::FiltersExcludeAll)
        );
        assert_eq!(
            empty_reason(true, &CrafterLevels::default(), None),
            Some(EmptyReason::FiltersExcludeAll)
        );
    }

    /// An unknown job filter can't be blamed on a level of 0.
    #[test]
    fn unknown_job_filter_falls_through_to_the_filter_message() {
        assert_eq!(
            empty_reason(true, &CrafterLevels::default(), Some("MIN")),
            Some(EmptyReason::FiltersExcludeAll)
        );
    }
}
