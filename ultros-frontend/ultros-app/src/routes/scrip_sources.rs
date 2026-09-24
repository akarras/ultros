use super::world_nav::use_analyzer_world;
use crate::analyzer_kit::calculation::{Calculation, CalculationStrip, CalculationTerm};
use crate::analyzer_kit::filters::{price_control, register_filters, toggle_control};
use crate::analyzer_kit::{
    connected_regions::buy_listings,
    scope::{MarketScope, use_buy_market_scope},
};
use crate::analyzer_kit::{
    formula::PriceSignal,
    market::{MarketGrid, MarketSubject, resolve_price, use_market_data},
};
use crate::columnar_wire::columnar_resource;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::term_badge::TermRole;
use crate::components::virtual_grid::saved_views::{
    GridPresetView, GridSavedViews, provide_grid_saved_views,
};
use crate::components::virtual_grid::{metrics::FilterOp, registry::FilterAlias};
use crate::components::virtual_grid::{metrics::with_units, units::Unit};
use crate::global_state::xiv_data::tracked_data;
use crate::query_defaults::filter_query_signal;
use crate::ws::realtime::use_realtime;
use crate::{
    api::get_cheapest_listings,
    components::{
        control_bar::ControlBar,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortHeader},
        tool_help::*,
        virtual_grid::{
            ColumnFilter, GridColumn,
            metrics::{GridMetric, GridValue},
        },
        world_picker::WorldOnlyPicker,
    },
};
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use std::{collections::HashSet, sync::Arc};
use thousands::Separable;
use ultros_api_types::cheapest_listings::{CheapestListings, CheapestListingsMap};
use ultros_game_sources::{ScripTurnIn, ScripType, scrip_turn_ins};
use xiv_gen::{ItemId, Recipe};

use crate::i18n::*;
use crate::query_defaults::query_signal;

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
    /// Ingredients that had at least one market listing to price from.
    priced_ingredients: u32,
    /// Ingredients the recipe actually uses.
    total_ingredients: u32,
    cheapest_world_id: i32,
    market_item_id: i32,
    market_item_name: String,
    market_hq: bool,
    listing_price: Option<i32>,
    pricing_fallback: bool,
    pricing_pending: bool,
    recipe: Option<&'static Recipe>,
}

impl ScripSourceData {
    fn passes_complete_prices(&self, required: bool) -> bool {
        !required || self.pricing_pending || self.coverage_tier() == 0
    }
    /// `0` when every ingredient had a market price, `1` when some were
    /// missing. Used as the *primary* ranking key so rows with an understated
    /// cost can never float above fully-priced rows — an unlisted ingredient
    /// used to be counted as *free*, which pushed exactly the least
    /// trustworthy rows to the top of the best-efficiency sort.
    fn coverage_tier(&self) -> u8 {
        if self.priced_ingredients >= self.total_ingredients {
            0
        } else {
            1
        }
    }
}

fn scrip_color_class(scrip: ScripType) -> &'static str {
    match scrip {
        ScripType::OrangeCrafters | ScripType::OrangeGatherers => "text-orange-400",
        ScripType::WhiteCrafters | ScripType::WhiteGatherers => "text-gray-200",
        ScripType::PurpleCrafters | ScripType::PurpleGatherers => "text-purple-400",
        ScripType::Other(_) => "text-gray-400",
    }
}

fn scrip_alias() -> FilterAlias {
    FilterAlias {
        convert: |raw| ScripType::from_filter_key(raw).map(|_| raw.to_string()),
        ..FilterAlias::new("scrip", "scrip-type", FilterOp::Eq)
    }
}

/// Does a row awarding `scrip_type` survive the `?scrip=` filter?
///
/// A row whose currency we don't recognise stays *visible*. Dropping unknown
/// values is what turned a stale `Currency` mapping into a blank page rather
/// than a few oddly-labelled rows, and one new expansion adding `Currency = 8`
/// would do it again. An unrecognised `?scrip=` value is likewise treated as
/// "no filter" instead of emptying the table.
#[cfg(test)]
fn passes_scrip_filter(scrip_type: ScripType, filter: Option<&str>) -> bool {
    match filter.and_then(ScripType::from_filter_key) {
        Some(wanted) => scrip_type == wanted,
        None => true,
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
            "efficiency" | "grid:cost-per-scrip" => Ok(SortMode::CostPerScrip),
            "amount" | "grid:scrip-amount" => Ok(SortMode::ScripAmount),
            "cost" | "grid:cost" => Ok(SortMode::Cost),
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

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::CostPerScrip
    }

    /// Costs read best-first ascending; the scrip payout reads best-first
    /// descending.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::CostPerScrip | SortMode::Cost => SortDir::Asc,
            SortMode::ScripAmount => SortDir::Desc,
        }
    }
}

/// Choose the same direction as the grid before selecting one turn-in per
/// item. Canonical grid URLs historically default to descending; native cost
/// URLs default to ascending. Losing that distinction changes which offer
/// survives deduplication before the grid ever sees it.
fn candidate_sort_direction(
    raw_sort: Option<&str>,
    explicit: Option<SortDir>,
    mode: SortMode,
) -> SortDir {
    explicit.unwrap_or_else(|| {
        if raw_sort
            .is_some_and(|sort| sort.starts_with("grid:") && sort.parse::<SortMode>().is_ok())
        {
            SortDir::Desc
        } else {
            mode.default_dir()
        }
    })
}

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_SCRIP: &str = "scrip";
const FILTER_JOB: &str = "job";
const FILTER_COMPLETE_PRICES: &str = "complete-prices";

/// The page's built-in views, offered above the reader's own saved ones.
///
/// Queries only: the labels live in [`scrip_sources_presets`] because `t_string!`
/// needs a literal key. Every key used here is pinned by a test below.
const PRESET_QUERIES: [&str; 2] = ["?sort=efficiency", "?scrip=OrangeCrafters&sort=efficiency"];

fn scrip_sources_presets(i18n: I18nContext<Locale, I18nKeys>) -> Vec<GridPresetView> {
    [
        t_string!(i18n, scrip_sources_preset_best_value).to_string(),
        t_string!(i18n, scrip_sources_preset_orange_crafters).to_string(),
    ]
    .into_iter()
    .zip(PRESET_QUERIES)
    .map(|(label, query)| GridPresetView {
        label,
        query: query.to_string(),
    })
    .collect()
}

/// Filters the `+ Filter` menu can add, in the old toolbar's left-to-right
/// order.
#[cfg(test)]
const LEGACY_PRESET_FILTER_KEYS: &[&str] = &[FILTER_SCRIP, FILTER_JOB, FILTER_COMPLETE_PRICES];

/// Rank the collected rows and collapse repeated items without a result cap.
///
/// The ranking has to be a *total* order. Rows are collected by iterating
/// `collectables_shop_items`, a `std::collections::HashMap`, so they arrive
/// here in an order that `RandomState` randomizes per process. The SSR server
/// and the hydrating wasm client each build their own copy of the game data,
/// so ranking that leaves ties unresolved puts different rows in different
/// places on the two sides. That is the hydration-mismatch class fixed for
/// the item page in #960. Tie-breaking on the stable item id pins one order.
///
/// The composite key, in order:
///
/// 1. [`ScripSourceData::coverage_tier`] — rows whose cost is understated
///    because some ingredients had no market listing always rank *below*
///    fully-priced rows, in either direction. Direction never applies here:
///    flipping a column reorders values, it doesn't make incomplete data more
///    trustworthy.
/// 2. The active column's metric, reversed when `dir` is the non-default.
/// 3. The stable item id, always ascending, so ties resolve identically on
///    the server and the client regardless of direction.
fn rank_scrip_sources(
    mut results: Vec<ScripSourceData>,
    sort_mode: SortMode,
    dir: SortDir,
) -> Vec<ScripSourceData> {
    results.sort_unstable_by(|a, b| {
        // `total_cmp` rather than `partial_cmp().unwrap()`: the unwrap was a
        // latent panic if a cost ever produced a NaN ratio.
        let metric = match sort_mode {
            SortMode::CostPerScrip => a.cost_per_scrip.total_cmp(&b.cost_per_scrip),
            SortMode::ScripAmount => a.scrip_amount.cmp(&b.scrip_amount),
            SortMode::Cost => a.cost.cmp(&b.cost),
        };
        let metric = match dir {
            SortDir::Asc => metric,
            SortDir::Desc => metric.reverse(),
        };
        a.coverage_tier()
            .cmp(&b.coverage_tier())
            .then(metric)
            .then_with(|| a.item_id.0.cmp(&b.item_id.0))
    });

    // An item stocked by several collectables shops yields one row per shop.
    // After a metric sort those rows are not adjacent, so the previous
    // `dedup_by_key` — which only removes *consecutive* duplicates — left them
    // on screen. Keep the first, i.e. best-ranked, row for each item.
    let mut seen = HashSet::with_capacity(results.len());
    results.retain(|r| seen.insert(r.item_id));

    results
}

#[component]
fn ScripSourceTable(
    scope: MarketScope,
    global_cheapest_listings: CheapestListings,
    world: Signal<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let market = use_market_data(world);
    let (cost_basis, _set_cost_basis) = filter_query_signal::<PriceSignal>("cost-basis");
    market.require_price_basis(Signal::derive(move || cost_basis.get().unwrap_or_default()));
    let data = tracked_data();
    let items = &data.items;
    let recipes = &data.recipes;

    // Create a lookup for recipes by result item
    let recipes_by_output = Memo::new(move |_| {
        let mut map: std::collections::HashMap<i32, &'static Recipe> =
            std::collections::HashMap::new();
        for recipe in recipes.values() {
            // Choose consistently across the SSR and hydration data maps.
            map.entry(recipe.item_result)
                .and_modify(|current| {
                    if recipe.key_id.0 < current.key_id.0 {
                        *current = recipe;
                    }
                })
                .or_insert(recipe);
        }
        map
    });

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");

    // `?item=` is a navigation target from search, not a filter: plain
    // `query_signal`, and revealed once per value (see `RevealOnce`) so live
    // re-sorts don't keep yanking the scroll position.
    let (reveal_item, _set_reveal_item) = query_signal::<i32>("item");
    let shown_rows = RwSignal::new(Vec::<(usize, Arc<ScripSourceData>)>::new());
    let reveal_once = StoredValue::new(crate::components::reveal_once::RevealOnce::default());
    let reveal_index = RwSignal::new(None::<usize>);
    Effect::new(move |_| {
        let item = reveal_item.get();
        let mut hit = None;
        shown_rows.with(|rows| {
            reveal_once.update_value(|once| {
                hit = once.next(item, |item| {
                    rows.iter().position(|(_, row)| row.item_id.0 == item)
                });
            })
        });
        if hit.is_some() {
            reveal_index.set(hit);
        }
    });
    let query = crate::components::app_link::use_query_map_or_default();
    let scrip_filter = Memo::new(move |_| {
        let filters = crate::components::virtual_grid::registry::resolve_filters(
            &query.get(),
            &[scrip_alias()],
        );
        filters
            .get("scrip-type")
            .filter(|f| f.op == FilterOp::Eq)
            .map(|f| f.value.clone())
    });
    let (job_filter, _set_job_filter) = filter_query_signal::<String>(FILTER_JOB);
    let (complete_prices, _) = filter_query_signal::<bool>(FILTER_COMPLETE_PRICES);

    // Global websocket health, same wiring as the other sales-driven tools —
    // the prices here come from the realtime-fed cheapest-listings store.
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

    let ranked_rows = Memo::new(move |_| {
        let stats = market.selected_stats();
        let basis = cost_basis.get().unwrap_or_default();
        let pricing_pending = stats.is_none() && basis.sale_stat().is_some();
        let mut results = Vec::new();
        let recipes_lookup = recipes_by_output();

        let job_filter_val = job_filter();

        for turn_in in scrip_turn_ins(data) {
            let ScripTurnIn {
                item_id,
                scrip_type,
                scrip_amount,
            } = turn_in;

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

            // Cost Calculation. An ingredient with no market listing used
            // to be priced at zero, which *understated* the cost and
            // floated exactly the least trustworthy rows to the top of
            // the best-efficiency sort. Instead, track how many
            // ingredients could actually be priced: rows with partial
            // coverage stay visible (badged, ranked below fully-priced
            // rows), rows with *no* priced ingredient are dropped.
            let mut cost = 0;
            let mut priced_ingredients = 0u32;
            let mut total_ingredients = 0u32;
            let mut pricing_fallback = false;
            // The collectable cannot be sold. Market context belongs to its
            // largest-cost ingredient, named explicitly alongside the metrics.
            let mut market_ingredient = None;

            if let Some(r) = recipe {
                // Sum ingredients
                for i in 0..8 {
                    let ing_id = r.ingredient[i];
                    let amount = r.amount_ingredient[i];
                    if ing_id == 0 || amount == 0 {
                        continue;
                    }
                    total_ingredients += 1;
                    if let Some(price) =
                        resolve_price(&prices, stats.as_deref(), ing_id, None, basis)
                    {
                        priced_ingredients += 1;
                        pricing_fallback |= price.fallback;
                        let line_cost = price.price.saturating_mul(amount);
                        cost = i32::saturating_add(cost, line_cost);
                        if market_ingredient.as_ref().is_none_or(|(largest, id, _)| {
                            line_cost > *largest || (line_cost == *largest && ing_id < *id)
                        }) {
                            market_ingredient = Some((line_cost, ing_id, price));
                        }
                    }
                }
            } else {
                // Skip non-craftables for now
                continue;
            }

            if priced_ingredients == 0 || cost == 0 {
                continue;
            } // Nothing priceable, or free items: no cost to compare
            let cost_per_scrip = cost as f32 / scrip_amount as f32;
            let (_, market_item_id, market_price) =
                market_ingredient.expect("priced ingredients establish market context");
            let listings = prices.find_matching_listings(market_item_id);
            let listing = if market_price.hq {
                listings.hq
            } else {
                listings.lq
            };

            results.push(ScripSourceData {
                item_id: ItemId(item_id),
                item_name: item_def.name.to_string(),
                level: item_def.level_item as u16,
                craft_type: recipe.map(|r| r.craft_type),
                scrip_type,
                scrip_amount,
                cost,
                cost_per_scrip,
                priced_ingredients,
                total_ingredients,
                cheapest_world_id: listing.map(|entry| entry.world_id).unwrap_or(0),
                market_item_id,
                market_item_name: items
                    .get(&ItemId(market_item_id))
                    .map(|item| item.name.to_string())
                    .unwrap_or_else(|| market_item_id.to_string()),
                market_hq: market_price.hq,
                listing_price: listing.map(|entry| entry.price),
                pricing_fallback,
                pricing_pending,
                recipe,
            });
        }

        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = candidate_sort_direction(
            query.with(|query| query.get("sort")).as_deref(),
            sort_dir(),
            mode,
        );
        // Keep every eligible row; only the rendered cells are virtualized.
        results.retain(|row| row.passes_complete_prices(complete_prices().unwrap_or(false)));
        rank_scrip_sources(results, mode, dir)
    });

    let computed_data = Memo::new(move |_| {
        ranked_rows.with(|rows| {
            rows.iter()
                .cloned()
                .map(Arc::new)
                .enumerate()
                .collect::<Vec<_>>()
        })
    });

    // The three gatherer scrip options can never produce a row today: the
    // loop above prices *craft* costs and skips anything without a recipe,
    // and gatherer collectables are gathered, not crafted. Explain that
    // instead of showing a silently empty table.
    let gatherer_filter_selected = Memo::new(move |_| {
        scrip_filter()
            .as_deref()
            .and_then(ScripType::from_filter_key)
            .is_some_and(|s| s.is_gatherer())
    });

    let scrip_label = move |scrip_type| match scrip_type {
        ScripType::OrangeCrafters => t_string!(i18n, scrip_sources_orange_crafters).to_string(),
        ScripType::OrangeGatherers => t_string!(i18n, scrip_sources_orange_gatherers).to_string(),
        ScripType::WhiteCrafters => t_string!(i18n, scrip_sources_white_crafters).to_string(),
        ScripType::PurpleCrafters => t_string!(i18n, scrip_sources_purple_crafters).to_string(),
        ScripType::WhiteGatherers => t_string!(i18n, scrip_sources_white_gatherers).to_string(),
        ScripType::PurpleGatherers => t_string!(i18n, scrip_sources_purple_gatherers).to_string(),
        ScripType::Other(_) => t_string!(i18n, scrip_sources_other_name).to_string(),
    };
    let scrip_options = move || {
        vec![
            (
                "OrangeCrafters",
                t_string!(i18n, scrip_sources_orange_crafters).to_string(),
            ),
            (
                "OrangeGatherers",
                t_string!(i18n, scrip_sources_orange_gatherers).to_string(),
            ),
            (
                "PurpleCrafters",
                t_string!(i18n, scrip_sources_purple_crafters).to_string(),
            ),
            (
                "WhiteCrafters",
                t_string!(i18n, scrip_sources_white_crafters).to_string(),
            ),
            (
                "PurpleGatherers",
                t_string!(i18n, scrip_sources_purple_gatherers).to_string(),
            ),
            (
                "WhiteGatherers",
                t_string!(i18n, scrip_sources_white_gatherers).to_string(),
            ),
        ]
    };
    let job_options = move || {
        vec![
            ("Carpenter", t_string!(i18n, carpenter).to_string()),
            ("Blacksmith", t_string!(i18n, blacksmith).to_string()),
            ("Armorer", t_string!(i18n, armorer).to_string()),
            ("Goldsmith", t_string!(i18n, goldsmith).to_string()),
            ("Leatherworker", t_string!(i18n, leatherworker).to_string()),
            ("Weaver", t_string!(i18n, weaver).to_string()),
            ("Alchemist", t_string!(i18n, alchemist).to_string()),
            ("Culinarian", t_string!(i18n, culinarian).to_string()),
        ]
    };

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_SCRIP => t_string!(i18n, scrip_sources_scrip_type).to_string(),
            FILTER_JOB => t_string!(i18n, scrip_sources_job_filter).to_string(),
            _ => String::new(),
        }
    };

    let filters = register_filters(
        vec![scrip_alias()],
        Signal::derive(move || {
            vec![
                toggle_control(
                    FILTER_COMPLETE_PRICES,
                    t_string!(i18n, scrip_sources_complete_prices).to_string(),
                ),
                price_control(
                    "cost-basis",
                    t_string!(i18n, market_ingredient_price).to_string(),
                    market.window,
                    t_string!(i18n, market_listing_basis).to_string(),
                ),
                {
                    let mut f = ColumnFilter::new(FILTER_JOB, filter_label(FILTER_JOB), false);
                    f.options = job_options();
                    f
                },
            ]
        }),
    );

    let presets = Signal::derive(move || scrip_sources_presets(i18n));

    let calculation = Calculation::provide(
        filters,
        vec![
            CalculationTerm::fixed(
                TermRole::Result,
                t_string!(i18n, scrip_sources_cost_per_scrip).to_string(),
                Some("cost-per-scrip"),
            ),
            CalculationTerm::input(TermRole::Value, "cost-basis", "cost")
                .with_place_select(scope.place()),
            CalculationTerm::fixed(
                TermRole::Divide,
                t_string!(i18n, scrip_sources_scrips).to_string(),
                Some("scrip-amount"),
            ),
        ],
        Some("cost-basis"),
    );

    view! {
            <div class="flex flex-col gap-6">
                <div class="flex flex-wrap items-start gap-3">
                    <CalculationStrip calculation window=market.window />

                </div>
                <p class="text-xs text-[color:var(--color-text-muted)]">
                    {t!(i18n, calculation_collectable_subject)}
                </p>

                <ControlBar sticky=false
                    summary=move || {
                        view! {
                            <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                                {move || t!(i18n, scrip_sources_results_count, n = move || filters.row_count())}
                            </span>
                            <span class="text-xs text-[color:var(--color-text-muted)] whitespace-nowrap truncate">
                                {move || t!(i18n, scrip_sources_region_pricing, region = world())}
                            </span>
                        }
                        .into_any()
                    }
                    actions=move || {
                        view! {
                            <RealtimeStatus status=realtime_status last_update=last_update />
                            <GridSavedViews id="scrip-sources-grid" presets=presets />
                        }
                            .into_any()
                    }

                    empty_label=Signal::derive(move || {
                        t_string!(i18n, no_active_filters).to_string()
                    })
                />

                // Empty states render as *siblings* of the scroller container,
                // never by unmounting it in a <Show>: the VirtualScroller wires
                // scroll-sync effects to node refs and remounting breaks them.
                <Show when=move || gatherer_filter_selected() && filters.row_count() == 0>
                    <ActionableEmptyState
                        title=t_string!(i18n, scrip_sources_gatherers_unsupported_title).to_string()
                        body=t_string!(i18n, scrip_sources_gatherers_unsupported_body).to_string()
                    />
                </Show>
                <Show when=move || !gatherer_filter_selected() && filters.row_count() == 0>
                    <ActionableEmptyState
                        title=t_string!(i18n, scrip_sources_no_results_title).to_string()
                        body=t_string!(i18n, scrip_sources_no_results_body).to_string()
                    />
                </Show>

                <div>
                    <MarketGrid show_saved_views=false on_rows=Callback::new(move |rows| shown_rows.set(rows)) reveal_index id="scrip-sources-grid" label=t_string!(i18n, scrip_sources_item).to_string()
     market=market
     subject=Arc::new(move |(_, row): &(usize, Arc<ScripSourceData>)| {
         let mut subject = MarketSubject::new(row.market_item_id, row.market_hq, row.cheapest_world_id);
         subject.label = t_string!(i18n, market_ingredient_label, item = row.market_item_name.clone()).to_string();
         subject.listing_price = row.listing_price;
         subject
     })
     metrics=with_units(vec![
         GridMetric::text("item", |(_, row): &(usize, Arc<ScripSourceData>)| GridValue::Text(row.item_name.clone())).tier(|(_, row): &(usize, Arc<ScripSourceData>)| row.coverage_tier()),
         GridMetric::text("market-ingredient", |(_, row): &(usize, Arc<ScripSourceData>)| GridValue::Text(row.market_item_name.clone())).tier(|(_, row): &(usize, Arc<ScripSourceData>)| row.coverage_tier()),
         GridMetric::number("cost-per-scrip", |(_, row): &(usize, Arc<ScripSourceData>)| if row.pricing_pending { GridValue::Pending } else { GridValue::Number(row.cost_per_scrip as f64) }).tier(|(_, row): &(usize, Arc<ScripSourceData>)| row.coverage_tier()),
         GridMetric::number("scrip-amount", |(_, row): &(usize, Arc<ScripSourceData>)| GridValue::Number(row.scrip_amount as f64)).tier(|(_, row): &(usize, Arc<ScripSourceData>)| row.coverage_tier()),
         GridMetric::number("cost", |(_, row): &(usize, Arc<ScripSourceData>)| if row.pricing_pending { GridValue::Pending } else { GridValue::Number(row.cost as f64) }).tier(|(_, row): &(usize, Arc<ScripSourceData>)| row.coverage_tier()),
         GridMetric::text("scrip-type", move |(_, row): &(usize, Arc<ScripSourceData>)| GridValue::Set(vec![scrip_label(row.scrip_type), format!("{:?}", row.scrip_type)])).tier(|(_, row): &(usize, Arc<ScripSourceData>)| row.coverage_tier()),
     ], &[("cost-per-scrip", Unit::Gil), ("cost", Unit::Gil)])
     row_height=60.0
     columns=Signal::derive(move || vec![GridColumn::new("item",t_string!(i18n, scrip_sources_item).to_string(), 320.0, false, true).fixed_width(),
    GridColumn::new("market-ingredient", t_string!(i18n, market_ingredient).to_string(), 240.0, false, true),
    GridColumn::new("cost-per-scrip",t_string!(i18n, scrip_sources_cost_per_scrip).to_string(), 130.0, true, true).native_sort("efficiency", SortMode::CostPerScrip.default_dir() == SortDir::Asc).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::CostPerScrip, sort_dir.get().unwrap_or_else(||SortMode::CostPerScrip.default_dir()) == SortDir::Asc),
    GridColumn::new("scrip-amount",t_string!(i18n, scrip_sources_scrips).to_string(), 130.0, true, true).native_sort("amount", SortMode::ScripAmount.default_dir() == SortDir::Asc).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::ScripAmount, sort_dir.get().unwrap_or_else(||SortMode::ScripAmount.default_dir()) == SortDir::Asc),
    GridColumn::new("cost",t_string!(i18n, scrip_sources_cost).to_string(), 130.0, true, true).native_sort("cost", SortMode::Cost.default_dir() == SortDir::Asc).sorted(sort_mode.get().unwrap_or_else(SortMode::fallback) == SortMode::Cost, sort_dir.get().unwrap_or_else(||SortMode::Cost.default_dir()) == SortDir::Asc),
    { let mut col = GridColumn::new("scrip-type",t_string!(i18n, scrip_sources_scrip_type_header).to_string(), 130.0, true, true); let mut filter = ColumnFilter::new("scrip", filter_label("scrip"), false); filter.options = scrip_options(); col.filters.push(filter); col }])
     header=move |id| {match id {"item" => view! {<div  class="w-full min-w-0">{t!(i18n, scrip_sources_item)}</div>}.into_any(),
    "market-ingredient" => view! { <span title=t_string!(i18n, market_ingredient_stats_title).to_string()>{t!(i18n, market_ingredient)}</span> }.into_any(),
    "cost-per-scrip" => view! {<div  class="w-full min-w-0">
                                    <SortHeader
                                        mode=SortMode::CostPerScrip
                                        label=t_string!(i18n, scrip_sources_cost_per_scrip).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                 </div>}.into_any(),
    "scrip-amount" => view! {<div  class="w-full min-w-0">
                                    <SortHeader
                                        mode=SortMode::ScripAmount
                                        label=t_string!(i18n, scrip_sources_scrips).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                 </div>}.into_any(),
    "cost" => view! {<div  class="w-full min-w-0">
                                    <SortHeader
                                        mode=SortMode::Cost
                                        label=t_string!(i18n, scrip_sources_cost).to_string()
                                        sort_mode
                                        sort_dir
                                    />
                                 </div>}.into_any(),
    "scrip-type" => view! {<div  class="w-full min-w-0">{t!(i18n, scrip_sources_scrip_type_header)}</div>}.into_any(), _ => ().into_any()}}
     each=computed_data
                        key=move |(_, data): &(usize, Arc<ScripSourceData>)| data.item_id.0

     measure=move |(_, data): &(usize, Arc<ScripSourceData>), id| {match id {"item" => (data.item_name.clone(), 110.0),
    "market-ingredient" => (data.market_item_name.clone(), 30.0),
    "cost-per-scrip" => (format!("{:.1}",data.cost_per_scrip), 42.0),
    "scrip-amount" => (data.scrip_amount.to_string(), 42.0),
    "cost" => (data.cost.separate_with_commas(), 42.0),
    "scrip-type" => (scrip_label(data.scrip_type), 42.0), _ => (String::new(), 0.0)}}
     view=move |(index, data): (usize, Arc<ScripSourceData>), id| {
                            let item_id = data.item_id;

     let _ = index;
     match id {
    "market-ingredient" => view! { <a class="truncate hover:text-brand-300" href=format!("/item/{}/{}", world(), data.market_item_id) title=t_string!(i18n, market_ingredient_cost_title).to_string()>{data.market_item_name.clone()}</a> }.into_any(),
    "item" => view! {<div  class="flex flex-row items-center gap-2 w-full min-w-0">
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
                                    </div>}.into_any(),
    "cost-per-scrip" => view! {<div  class="text-right font-bold text-brand-300 w-full min-w-0">
                                        // One decimal below 10 gil/scrip: whole-gil
                                        // truncation collapsed the interesting end
                                        // of the efficiency scale (2.4 and 2.9
                                        // both showed as 2).
                                        <div class="flex flex-row items-center">
                                            <GilIcon />
                                            <div>
                                                {if data.cost_per_scrip < 10.0 {
                                                    format!("{:.1}", data.cost_per_scrip)
                                                } else {
                                                    (data.cost_per_scrip as i32).separate_with_commas()
                                                }}
                                            </div>
                                        </div>
                                    </div>}.into_any(),
    "scrip-amount" => view! {<div  class="text-right w-full min-w-0">
                                        {data.scrip_amount}
                                    </div>}.into_any(),
    "cost" => view! {<div  class="text-right w-full min-w-0">
                                        <Gil amount=data.cost />
                                        {data.pricing_pending.then(|| view! { <span class="block text-xs text-amber-400">{t!(i18n, market_loading_prices)}</span> })}
                                        {(!data.pricing_pending && data.pricing_fallback).then(|| view! { <span class="block text-xs text-amber-400">{t!(i18n, market_listing_fallback)}</span> })}
                                        {(data.coverage_tier() != 0)
                                            .then(|| {
                                                view! {
                                                    <span
                                                        class="block text-[10px] leading-tight text-amber-400"
                                                        title=t_string!(i18n, scrip_sources_coverage_hint).to_string()
                                                    >
                                                        {t!(
                                                            i18n, scrip_sources_coverage_badge, priced =
                                                            data.priced_ingredients, total = data.total_ingredients
                                                        )}
                                                    </span>
                                                }
                                            })}
                                    </div>}.into_any(),
    "scrip-type" => view! {<div  class="text-right w-full min-w-0">
                                        <span class={format!("text-xs {}", scrip_color_class(data.scrip_type))}>
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
                                    </div>}.into_any(), _ => ().into_any()}}
     />
                 </div>
            </div>
        }
}

#[component]
pub fn ScripSources() -> impl IntoView {
    crate::query_defaults::seed_analyzer_default_view("scrip-sources");
    provide_grid_saved_views("scrip-sources-grid");
    let i18n = use_i18n();
    let (selected_world, set_selected_world) = use_analyzer_world("/scrip-sources");
    // Listings only price the ingredients bought for a turn-in, so the scope
    // can reach into the connected regions.
    let scope = use_buy_market_scope(Signal::derive(move || {
        selected_world.get().map(|world| world.name)
    }));
    let region = scope.name;

    let global_cheapest_listings = columnar_resource(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });
    let connected_listings = scope.connected_listings();

    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title=t_string!(i18n, scrip_sources_meta_title).to_string() />
            <MetaDescription text=t_string!(i18n, scrip_sources_meta_desc).to_string() />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, scrip_sources_title).to_string()
                    summary=t_string!(i18n, scrip_sources_summary).to_string()
                    context=t_string!(i18n, scrip_sources_context).to_string()
                    help_href="/help/scrip-sources"
                    help_body=t_string!(i18n, scrip_sources_help_body).to_string()
                    calculation=ToolCalculation::new(
                        t_string!(i18n, scrip_sources_efficiency_model).to_string(),
                        t_string!(i18n, scrip_sources_efficiency_formula).to_string(),
                        t_string!(i18n, scrip_sources_efficiency_details).to_string(),
                    )
                    assumptions=vec![
                        t_string!(i18n, scrip_sources_assumption_high_reward).to_string(),
                        t_string!(i18n, scrip_sources_assumption_market_cost).to_string(),
                        t_string!(i18n, scrip_sources_assumption_lower_better).to_string(),
                    ]
                >
                    <label class="text-[color:var(--brand-fg)] font-semibold">
                        {t!(i18n, scrip_sources_select_world)}
                    </label>
                    <div data-testid="analyzer-world-picker">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world
                        />
                    </div>
                </ToolHeader>

                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings =
                            buy_listings(global_cheapest_listings.get(), connected_listings.get());
                        match listings {
                            Some(Ok(listings)) => {
                                view! {
                                    <ScripSourceTable
                                        scope
                                        global_cheapest_listings=listings
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            Some(Err(e)) => {
                                view! {
                                    <div class="text-negative">
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A preset is applied by rebuilding the URL from its query, so a stray
    /// separator or an empty pair would ship straight into the address bar.
    #[test]
    fn every_preset_query_is_a_clean_query_string() {
        for query in PRESET_QUERIES {
            assert!(query.starts_with('?'), "{query}");
            assert!(!query.ends_with('&'), "{query}");
            assert!(!query.contains("&&"), "{query}");
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                assert!(!key.is_empty(), "{query}");
                assert!(!value.is_empty(), "{query}");
            }
        }
    }

    /// Renaming a sort token or retiring a filter would otherwise leave a
    /// built-in view quietly pointing at nothing.
    #[test]
    fn preset_queries_only_use_keys_this_page_still_reads() {
        for query in PRESET_QUERIES {
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').expect("key=value");
                match key {
                    "sort" => assert!(
                        std::str::FromStr::from_str(value)
                            .map(|_: SortMode| ())
                            .is_ok(),
                        "{query}"
                    ),
                    other => assert!(LEGACY_PRESET_FILTER_KEYS.contains(&other), "{query}"),
                }
            }
        }
    }

    fn row(item_id: i32, scrip_amount: u32, cost: i32) -> ScripSourceData {
        ScripSourceData {
            item_id: ItemId(item_id),
            item_name: format!("Item {item_id}"),
            level: 90,
            craft_type: Some(0),
            scrip_type: ScripType::PurpleCrafters,
            scrip_amount,
            cost,
            cost_per_scrip: cost as f32 / scrip_amount as f32,
            priced_ingredients: 3,
            total_ingredients: 3,
            cheapest_world_id: 0,
            market_item_id: 0,
            market_item_name: String::new(),
            market_hq: false,
            listing_price: None,
            pricing_fallback: false,
            pricing_pending: false,
            recipe: None,
        }
    }

    /// A row where only `priced` of `total` ingredients had market listings.
    fn partial_row(
        item_id: i32,
        scrip_amount: u32,
        cost: i32,
        priced: u32,
        total: u32,
    ) -> ScripSourceData {
        ScripSourceData {
            priced_ingredients: priced,
            total_ingredients: total,
            ..row(item_id, scrip_amount, cost)
        }
    }

    #[test]
    fn complete_price_filter_can_be_cleared_and_waits_for_selected_history() {
        let mut partial = partial_row(1, 100, 10, 1, 3);
        assert!(!partial.passes_complete_prices(true));
        assert!(
            partial.passes_complete_prices(false),
            "unrestricted view can inspect partial prices"
        );
        partial.pricing_pending = true;
        assert!(
            partial.passes_complete_prices(true),
            "a loading price basis cannot exclude a candidate"
        );
        assert!(row(2, 100, 10).passes_complete_prices(true));
    }

    #[test]
    fn builtin_presets_only_offer_supported_crafting_scrips() {
        assert!(
            PRESET_QUERIES
                .iter()
                .all(|query| !query.contains("Gatherers"))
        );
    }
    #[test]
    fn canonical_sort_keeps_the_same_turn_in_representative_as_legacy_sort() {
        let candidates = vec![row(1, 100, 1000), row(1, 200, 3000)];
        for (legacy, canonical) in [
            ("efficiency", "grid:cost-per-scrip"),
            ("amount", "grid:scrip-amount"),
            ("cost", "grid:cost"),
        ] {
            for dir in [SortDir::Asc, SortDir::Desc] {
                assert_eq!(
                    rank_scrip_sources(candidates.clone(), legacy.parse().unwrap(), dir),
                    rank_scrip_sources(candidates.clone(), canonical.parse().unwrap(), dir),
                    "{legacy}/{canonical}/{dir:?}",
                );
            }
        }
    }

    #[test]
    fn implicit_canonical_direction_selects_the_descending_turn_in() {
        let candidates = vec![row(1, 100, 1000), row(1, 200, 3000)];
        for (legacy, canonical) in [("efficiency", "grid:cost-per-scrip"), ("cost", "grid:cost")] {
            let mode = canonical.parse().unwrap();
            let retained = |raw, explicit| {
                rank_scrip_sources(
                    candidates.clone(),
                    mode,
                    candidate_sort_direction(Some(raw), explicit, mode),
                )[0]
                .cost
            };
            assert_eq!(retained(legacy, None), 1000);
            assert_eq!(retained(canonical, None), 3000);
            assert_eq!(retained(canonical, "invalid".parse().ok()), 3000);
            assert_eq!(retained(canonical, Some(SortDir::Asc)), 1000);
            assert_eq!(retained(legacy, Some(SortDir::Desc)), 3000);
        }
    }

    fn ids(rows: &[ScripSourceData]) -> Vec<i32> {
        rows.iter().map(|r| r.item_id.0).collect()
    }

    /// `collectables_shop_items` is a `std::collections::HashMap`, so the order
    /// rows are collected in is randomized per process (`RandomState`). The SSR
    /// server and the hydrating wasm client each build their own copy of the
    /// game data, so the same rows arrive here in different orders. If the
    /// ranking is not a total order, the two sides render different rows in
    /// different positions and tachys' hydration walker trips — the #6831
    /// crash class fixed for the item page by #960.
    #[test]
    fn ranking_is_independent_of_input_order() {
        for mode in [
            SortMode::ScripAmount,
            SortMode::Cost,
            SortMode::CostPerScrip,
        ] {
            for dir in [SortDir::Asc, SortDir::Desc] {
                // Every row ties on every sort key, which is what game data
                // actually looks like: `high_reward` is a small integer
                // shared by hundreds of items. Mixed coverage tiers so the
                // tier key is exercised too.
                let forward = vec![
                    row(1, 20, 1000),
                    partial_row(2, 20, 1000, 1, 3),
                    row(3, 20, 1000),
                    partial_row(4, 20, 1000, 2, 3),
                ];
                let reversed: Vec<_> = forward.iter().rev().cloned().collect();

                assert_eq!(
                    ids(&rank_scrip_sources(forward, mode, dir)),
                    ids(&rank_scrip_sources(reversed, mode, dir)),
                    "{mode:?}/{dir:?} ranking changed with input order"
                );
            }
        }
    }

    /// All rows beyond the former 100-row cap remain available in stable order.
    #[test]
    fn all_results_survive_regardless_of_input_order() {
        let forward: Vec<_> = (1..=150).map(|i| row(i, 20, 1000)).collect();
        let reversed: Vec<_> = forward.iter().rev().cloned().collect();
        assert_eq!(
            rank_scrip_sources(forward.clone(), SortMode::Cost, SortDir::Asc).len(),
            150
        );

        assert_eq!(
            ids(&rank_scrip_sources(
                forward,
                SortMode::ScripAmount,
                SortDir::Desc
            )),
            ids(&rank_scrip_sources(
                reversed,
                SortMode::ScripAmount,
                SortDir::Desc
            )),
        );
    }

    /// An item sold by several collectables shops at different reward tiers
    /// produces several rows. Those rows are not adjacent after a metric sort,
    /// so consecutive-only dedup leaves the duplicates on screen.
    #[test]
    fn repeated_items_collapse_even_when_not_adjacent() {
        // Item 1 at two reward tiers, with item 2 ranking between them.
        let rows = vec![row(1, 40, 1000), row(2, 30, 1000), row(1, 20, 1000)];

        let ranked = rank_scrip_sources(rows, SortMode::ScripAmount, SortDir::Desc);

        assert_eq!(ids(&ranked), vec![1, 2], "item 1 rendered twice");
    }

    /// Dedup must keep the best-ranked row for an item, not an arbitrary one.
    #[test]
    fn dedup_keeps_the_best_ranked_row_for_an_item() {
        let rows = vec![row(1, 40, 1000), row(2, 30, 1000), row(1, 20, 1000)];

        let ranked = rank_scrip_sources(rows, SortMode::ScripAmount, SortDir::Desc);

        assert_eq!(ranked[0].scrip_amount, 40);
    }

    #[test]
    fn sort_modes_still_rank_by_their_metric() {
        let rows = vec![row(1, 10, 3000), row(2, 30, 1000), row(3, 20, 2000)];

        // Most scrips first.
        assert_eq!(
            ids(&rank_scrip_sources(
                rows.clone(),
                SortMode::ScripAmount,
                SortMode::ScripAmount.default_dir()
            )),
            vec![2, 3, 1]
        );
        // Cheapest total cost first.
        assert_eq!(
            ids(&rank_scrip_sources(
                rows.clone(),
                SortMode::Cost,
                SortMode::Cost.default_dir()
            )),
            vec![2, 3, 1]
        );
        // Best gil-per-scrip first: 1000/30 < 2000/20 < 3000/10.
        assert_eq!(
            ids(&rank_scrip_sources(
                rows,
                SortMode::CostPerScrip,
                SortMode::CostPerScrip.default_dir()
            )),
            vec![2, 3, 1]
        );
    }

    /// Flipping `?dir=` reverses the metric order…
    #[test]
    fn direction_flip_reverses_the_metric() {
        let rows = vec![row(1, 10, 3000), row(2, 30, 1000), row(3, 20, 2000)];

        for (mode, asc, desc) in [
            (SortMode::Cost, vec![2, 3, 1], vec![1, 3, 2]),
            (SortMode::ScripAmount, vec![1, 3, 2], vec![2, 3, 1]),
            (SortMode::CostPerScrip, vec![2, 3, 1], vec![1, 3, 2]),
        ] {
            assert_eq!(
                ids(&rank_scrip_sources(rows.clone(), mode, SortDir::Asc)),
                asc,
                "{mode:?} ascending"
            );
            assert_eq!(
                ids(&rank_scrip_sources(rows.clone(), mode, SortDir::Desc)),
                desc,
                "{mode:?} descending"
            );
        }
    }

    /// …but ties still resolve by ascending item id in *both* directions, so
    /// the order stays a total order (the SSR/CSR hydration requirement) and
    /// tied rows don't shuffle when the user flips a column.
    #[test]
    fn direction_flip_keeps_the_stable_tiebreak() {
        let rows = vec![row(3, 20, 1000), row(1, 20, 1000), row(2, 20, 1000)];

        for dir in [SortDir::Asc, SortDir::Desc] {
            assert_eq!(
                ids(&rank_scrip_sources(rows.clone(), SortMode::Cost, dir)),
                vec![1, 2, 3],
                "{dir:?} tie order"
            );
        }
    }

    /// A row with unpriced ingredients has an *understated* cost, so however
    /// good its metric looks it must rank below every fully-priced row — in
    /// both directions. This is the fix for `unwrap_or(0)` floating exactly
    /// the least trustworthy rows to the top of the best-efficiency sort.
    #[test]
    fn partially_priced_rows_rank_below_fully_priced_rows() {
        for mode in [
            SortMode::ScripAmount,
            SortMode::Cost,
            SortMode::CostPerScrip,
        ] {
            for dir in [SortDir::Asc, SortDir::Desc] {
                // The partial row "wins" every metric: cheapest, most
                // scrips, best ratio.
                let rows = vec![
                    row(1, 10, 3000),
                    partial_row(2, 100, 1, 1, 4),
                    row(3, 20, 2000),
                ];

                let ranked = rank_scrip_sources(rows, mode, dir);

                assert_eq!(
                    ranked.last().map(|r| r.item_id.0),
                    Some(2),
                    "{mode:?}/{dir:?}: partially-priced row escaped the bottom tier"
                );
            }
        }
    }

    /// Within the partial tier, rows still follow the active sort.
    #[test]
    fn the_partial_tier_is_sorted_by_the_active_metric_too() {
        let rows = vec![
            partial_row(1, 10, 3000, 2, 3),
            row(2, 30, 1000),
            partial_row(3, 20, 500, 1, 3),
        ];

        assert_eq!(
            ids(&rank_scrip_sources(rows, SortMode::Cost, SortDir::Asc)),
            vec![2, 3, 1]
        );
    }

    /// Partial coverage still sorts last while every candidate remains available.
    #[test]
    fn ranking_keeps_partial_rows_after_fully_priced_rows() {
        let rows = vec![
            partial_row(1, 100, 1, 1, 4),
            row(2, 10, 3000),
            row(3, 20, 2000),
        ];

        let ranked = rank_scrip_sources(rows, SortMode::CostPerScrip, SortDir::Asc);

        assert_eq!(ids(&ranked), vec![3, 2, 1]);
    }

    /// A currency we have never seen must stay *visible*. Silently dropping
    /// unrecognised values is what blanked this page, and one new expansion
    /// adding `Currency = 8` would blank it again.
    #[test]
    fn an_unknown_currency_is_still_listed() {
        let unknown = ScripType::from_currency(8);

        assert_eq!(unknown, ScripType::Other(8));
        assert!(
            passes_scrip_filter(unknown, None),
            "unrecognised currency dropped from the unfiltered list"
        );
    }

    #[test]
    fn scrip_filter_selects_only_the_requested_type() {
        assert!(passes_scrip_filter(
            ScripType::PurpleCrafters,
            Some("PurpleCrafters")
        ));
        assert!(!passes_scrip_filter(
            ScripType::OrangeCrafters,
            Some("PurpleCrafters")
        ));
    }

    /// A hand-edited `?scrip=` value shouldn't empty the table.
    #[test]
    fn an_unrecognised_filter_value_shows_everything() {
        for filter in [None, Some(""), Some("nonsense")] {
            assert!(passes_scrip_filter(ScripType::PurpleCrafters, filter));
        }
    }
}
