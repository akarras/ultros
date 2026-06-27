use crate::components::crafting_cost::{
    CraftingCostOptions, EmptyOnHand, ShardsMode, compute_cost,
};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::on_hand_input::{ActiveListBanner, LocalOnHand, OnHandMap};
use crate::components::related_items::is_shard_item;
use crate::global_state::craft_options::{self, CraftOptions};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::{
    analysis::{SalesStats, analyze_sales, roi_badge_class},
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_recipe_to_list::AddRecipeToList,
        crafter_settings::CrafterSettings,
        gil::*,
        icon::Icon,
        item_icon::*,
        query_button::QueryButton,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        tool_help::*,
        toolbar::{Toolbar, ToolbarField, ToolbarPills, ToolbarSpacer},
        tooltip::Tooltip,
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    error::AppResult,
    global_state::{
        LocalWorldData, cookies::Cookies, crafter_levels::CrafterLevels,
        home_world::use_home_world, region_for_world::use_region_for_world,
    },
    ws::realtime::{RealtimeSubscription, use_realtime},
};
use icondata as i;
use leptos::prelude::*;
use leptos_router::hooks::{query_signal, use_params_map};
use std::{cmp::Reverse, collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
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

#[component]
fn RecipeAnalyzerTable(
    global_cheapest_listings_resource: ArcResource<AppResult<CheapestListings>>,
    recent_sales_resource: ArcResource<AppResult<RecentSales>>,

    world: Signal<String>,
) -> impl IntoView {
    let global_res_for_memo = global_cheapest_listings_resource.clone();
    let prices = Memo::new(move |_| {
        global_res_for_memo
            .get()
            .and_then(|r| r.ok())
            .map(|listings| CheapestListingsMap::from(listings.clone()))
    });
    let data = tracked_data();
    let items = &data.items;
    let recipes = &data.recipes;
    let recipe_level_tables = &data.recipe_level_tables;
    let i18n = use_i18n();
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
        let worlds = use_context::<LocalWorldData>()
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
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (job_filter, set_job_filter) = query_signal::<String>("job");
    let (use_subcrafts, set_use_subcrafts) = query_signal::<bool>("subcrafts");
    let (min_daily_sales, set_min_daily_sales) = query_signal::<f32>("min-sales");
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
        let prices = match prices.get() {
            Some(p) => p,
            None => return vec![],
        };
        let recent_sales = match recent_sales_resource.get().and_then(|r| r.ok()) {
            Some(s) => s,
            None => return vec![],
        };
        let levels = crafter_levels.get().unwrap_or_default();
        let use_sub = use_subcrafts().unwrap_or(false);
        let require_hq_flag = require_hq().unwrap_or(false);
        let filter_outliers = filter_outliers().unwrap_or(false);

        let mut sales_map: HashMap<i32, Vec<&SaleData>> = HashMap::new();
        for sale in &recent_sales.sales {
            sales_map
                .entry(sale.item_id)
                .or_insert_with(Vec::new)
                .push(sale);
        }

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

            // Job mapping: 0=CRP, 1=BSM, 2=ARM, 3=GSM, 4=LTW, 5=WVR, 6=ALC, 7=CUL
            let (user_level, job_code) = match recipe.craft_type {
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
                    last_update=last_update_at
                />
            </Toolbar>

            <Show when=move || !has_levels()>
                <ActionableEmptyState
                    title=t_string!(i18n, recipe_analyzer_empty_set_levels_title).to_string()
                    body="Recipe Analyzer filters to crafts your character can make. Open the crafting profile section above and enter at least one crafter level."
                    action_href="/help/recipe-analyzer"
                    action_label="Read recipe help"
                />
            </Show>

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
                             <div role="columnheader" class="w-32 shrink-0 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="profit"
                                >
                                    {t!(i18n, profit)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-32 shrink-0 p-4">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="roi"
                                >
                                    {t!(i18n, roi)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-32 shrink-0 p-4">{t!(i18n, recipe_analyzer_col_cost_per_unit)}</div>
                             <div role="columnheader" class="w-32 shrink-0 p-4">{t!(i18n, price)}</div>
                             <div role="columnheader" class="w-32 shrink-0 p-4 hidden md:block">
                                <QueryButton
                                    class="!text-brand-300 hover:text-brand-200"
                                    active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                    key="sort"
                                    value="velocity"
                                >
                                    {t!(i18n, daily_sales)}
                                </QueryButton>
                             </div>
                             <div role="columnheader" class="w-32 shrink-0 p-4 hidden md:block">{t!(i18n, avg_price)}</div>
                             <div role="columnheader" class="w-20 shrink-0 p-4">{t!(i18n, actions)}</div>
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<RecipeProfitData>)| (*index, data.recipe.key_id)
                    view=move |(index, data): (usize, Arc<RecipeProfitData>)| {
                        // Clone data for use in closures to avoid moving the Arc
                        let data_clone = data.clone();
                        let item_id = ItemId(data.recipe.item_result);
                        let item = items.get(&item_id).map(|i| i.name.as_str()).unwrap_or("Unknown");
                        let item_level = items.get(&item_id).map(|i| i.level_item).unwrap_or(0);
                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };

                        let job_abbrev = match data.recipe.craft_type {
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
                                     <span class={
                                        let data = data_clone.clone();
                                        move || roi_badge_class(data.return_on_investment)
                                    }>
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

                <Show when=move || selected_world.get().is_some()>
                    <div class="flex flex-col md:flex-row items-center gap-2">
                        <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, select_world_for_sales_data)}</label>
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
                        let global_resource = global_cheapest_listings.clone();
                        let recent_resource = recent_sales.clone();
                        match (global_resource.get(), recent_resource.get()) {
                            (Some(_), Some(_)) => {
                                view! {
                                    <RecipeAnalyzerTable
                                        global_cheapest_listings_resource=global_resource
                                        recent_sales_resource=recent_resource
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
