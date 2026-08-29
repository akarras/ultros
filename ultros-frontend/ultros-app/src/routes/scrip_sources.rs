use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::xiv_data::tracked_data;
use crate::query_defaults::filter_query_signal;
use crate::ws::realtime::use_realtime;
use crate::{
    api::get_cheapest_listings,
    components::{
        control_bar::{ControlBar, FilterOption},
        filter_chip::FilterChip,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortHeader},
        tool_help::*,
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
use std::{collections::HashSet, sync::Arc};
use thousands::Separable;
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
    /// Ingredients that had at least one market listing to price from.
    priced_ingredients: u32,
    /// Ingredients the recipe actually uses.
    total_ingredients: u32,
    cheapest_world_id: i32,
    recipe: Option<&'static Recipe>,
}

impl ScripSourceData {
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
    /// Map a `CollectablesShopRewardScrip.Currency` value to the scrip it pays.
    ///
    /// `Currency` is a small **enum index**, not an item id — every row in the
    /// 7.55 data carries `0`, `2`, `4`, `6` or `7`. Matching it against scrip
    /// item ids is what left this page blank, so the mapping below is derived
    /// from the game data instead. Joining `CollectablesShopItem` to the
    /// `RewardType = 1` (scrip-paying) shops, ignoring the material-exchange
    /// shops that reuse this column, gives:
    ///
    /// | Currency | rows | turn-ins |
    /// |---|---|---|
    /// | 2 | 1089 | crafted, lv 50-99 |
    /// | 4 |  163 | gathered/fished, lv 50-98 |
    /// | 6 |   93 | crafted, lv 78-80 and lv 100 |
    /// | 7 |   18 | gathered/fished, lv 100 |
    ///
    /// So `2`/`4` are the purple (levelling) crafter/gatherer pair and `6`/`7`
    /// the orange (level 100) pair. Currency `6`'s level-100 rows are exactly
    /// one item per crafting job — the eight "Rarefied" max-level crafts — which
    /// is what pins it to Orange Crafters' rather than the retired white scrip;
    /// its lv 78-80 rows are the Shadowbringers tier that collapsed into the
    /// same high-tier crafter slot when white scrips were removed in 7.0.
    fn from_currency(currency: u32) -> Self {
        match currency {
            2 => ScripType::PurpleCrafters,
            4 => ScripType::PurpleGatherers,
            6 => ScripType::OrangeCrafters,
            7 => ScripType::OrangeGatherers,
            other => ScripType::Other(other),
        }
    }

    /// The `?scrip=` query value that selects this type, as emitted by the
    /// toolbar `<select>`.
    fn from_filter_key(key: &str) -> Option<Self> {
        match key {
            "OrangeCrafters" => Some(ScripType::OrangeCrafters),
            "OrangeGatherers" => Some(ScripType::OrangeGatherers),
            "WhiteCrafters" => Some(ScripType::WhiteCrafters),
            "PurpleCrafters" => Some(ScripType::PurpleCrafters),
            "WhiteGatherers" => Some(ScripType::WhiteGatherers),
            "PurpleGatherers" => Some(ScripType::PurpleGatherers),
            _ => None,
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

    /// Gatherer scrips are paid for collectables that are *gathered*, not
    /// crafted, so the craft-cost model below can never price them. The page
    /// keeps the options selectable but explains the empty table instead of
    /// silently rendering nothing.
    fn is_gatherer(&self) -> bool {
        matches!(
            self,
            ScripType::OrangeGatherers | ScripType::WhiteGatherers | ScripType::PurpleGatherers
        )
    }
}

/// Does a row awarding `scrip_type` survive the `?scrip=` filter?
///
/// A row whose currency we don't recognise stays *visible*. Dropping unknown
/// values is what turned a stale `Currency` mapping into a blank page rather
/// than a few oddly-labelled rows, and one new expansion adding `Currency = 8`
/// would do it again. An unrecognised `?scrip=` value is likewise treated as
/// "no filter" instead of emptying the table.
fn passes_scrip_filter(scrip_type: ScripType, filter: Option<&str>) -> bool {
    match filter.and_then(ScripType::from_filter_key) {
        Some(wanted) => scrip_type == wanted,
        None => true,
    }
}

/// A single collectables turn-in: the item handed in, the scrip it pays and how
/// much it pays at maximum collectability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScripTurnIn {
    item_id: i32,
    scrip_type: ScripType,
    scrip_amount: u32,
}

/// `CollectablesShop.RewardType` for the turn-in counters that pay scrip.
const SCRIP_REWARD_TYPE: i32 = 1;
/// `CollectablesShop.RewardType` for the material exchanges, which hand back
/// items and pay no scrip.
const MATERIAL_EXCHANGE_REWARD_TYPE: i32 = 2;

/// `CollectablesShopItem` groups that belong *only* to material-exchange shops.
///
/// `CollectablesShop.ShopItems[..]` lists a shop's item groups, and a group is
/// the integer half of `CollectablesShopItem`'s `<group>.<index>` key — which is
/// how `collectables_shop_items` is keyed, so the two join directly.
///
/// This deliberately collects the groups to *exclude* rather than the ones to
/// keep. A group nobody claims, an unknown future `RewardType`, or a renamed
/// sheet that leaves `collectables_shops` empty then all degrade to today's
/// behaviour — a few oddly-labelled rows — instead of blanking the page, which
/// is the failure mode this route has already shipped once. A group claimed by
/// a scrip shop *and* an exchange shop stays visible for the same reason.
fn material_exchange_groups(data: &xiv_gen::Data) -> HashSet<i32> {
    let mut scrip_paying = HashSet::new();
    let mut exchange_only = HashSet::new();

    for shop in data.collectables_shops.values() {
        let bucket = match shop.reward_type {
            SCRIP_REWARD_TYPE => &mut scrip_paying,
            MATERIAL_EXCHANGE_REWARD_TYPE => &mut exchange_only,
            _ => continue,
        };
        for group in shop.shop_items {
            if group != 0 {
                bucket.insert(group);
            }
        }
    }

    exchange_only.retain(|group| !scrip_paying.contains(group));
    exchange_only
}

/// Every turn-in the collectables shops offer, before any UI filtering or
/// pricing.
///
/// Material-exchange trades are dropped here: they populate the same
/// `CollectablesShopRewardScrip.Currency` column the real turn-ins do, so
/// reading that column alone lists every one of them as a scrip source paying a
/// scrip it never awards.
fn scrip_turn_ins(data: &xiv_gen::Data) -> Vec<ScripTurnIn> {
    let exchange_only = material_exchange_groups(data);
    let mut turn_ins = Vec::new();

    for (group, item_vec) in &data.collectables_shop_items {
        if exchange_only.contains(&group.0) {
            continue;
        }
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

            let scrip_amount = reward.high_reward as u32;
            if scrip_amount == 0 {
                continue;
            }

            turn_ins.push(ScripTurnIn {
                item_id: item_entry.item,
                scrip_type: ScripType::from_currency(reward.currency as u32),
                scrip_amount,
            });
        }
    }

    turn_ins
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

/// Maximum rows rendered by the table.
const ROW_LIMIT: usize = 100;

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_SCRIP: &str = "scrip";
const FILTER_JOB: &str = "job";

/// Filters the `+ Filter` menu can add, in the old toolbar's left-to-right
/// order.
const ADDABLE_FILTERS: &[&str] = &[FILTER_SCRIP, FILTER_JOB];

/// Rank the collected rows, collapse repeated items, and cap the list.
///
/// The ranking has to be a *total* order. Rows are collected by iterating
/// `collectables_shop_items`, a `std::collections::HashMap`, so they arrive
/// here in an order that `RandomState` randomizes per process. The SSR server
/// and the hydrating wasm client each build their own copy of the game data,
/// so ranking that leaves ties unresolved puts different rows in different
/// places — and, at the `limit` boundary, drops a different *set* of rows
/// entirely — on the two sides. That is the hydration-mismatch class fixed for
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
    limit: usize,
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

    results.truncate(limit);
    results
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
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    // Filter params use `filter_query_signal` (replace: true, scroll: false):
    // typing into a chip writes the URL on every keystroke, and plain
    // `query_signal`'s defaults would push a history entry and yank the
    // window to the top each time.
    let (scrip_filter, set_scrip_filter) = filter_query_signal::<String>(FILTER_SCRIP);
    let (job_filter, set_job_filter) = filter_query_signal::<String>(FILTER_JOB);

    // A filter picked from the `+ Filter` menu but not yet committed — its
    // chip mounts in edit state with an empty input (see currency_exchange.rs
    // for the same pattern). Neither select has an "obviously correct"
    // default value, so both mount blank rather than seeding one.
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

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
        let mut results = Vec::new();
        let recipes_lookup = recipes_by_output();

        let scrip_filter_val = scrip_filter();
        let job_filter_val = job_filter();

        for turn_in in scrip_turn_ins(data) {
            let ScripTurnIn {
                item_id,
                scrip_type,
                scrip_amount,
            } = turn_in;

            if !passes_scrip_filter(scrip_type, scrip_filter_val.as_deref()) {
                continue;
            }

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

            if let Some(r) = recipe {
                // Sum ingredients
                for i in 0..8 {
                    let ing_id = r.ingredient[i];
                    let amount = r.amount_ingredient[i];
                    if ing_id == 0 || amount == 0 {
                        continue;
                    }
                    total_ingredients += 1;
                    let price_summary = prices.find_matching_listings(ing_id);
                    if let Some(price) = price_summary.lowest_gil() {
                        priced_ingredients += 1;
                        cost += price * amount;
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
                cheapest_world_id: 0, // Not tracked per ingredient
                recipe,
            });
        }

        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        // Rank the *full* set so the result count below is exact; the render
        // memo applies `ROW_LIMIT`.
        rank_scrip_sources(results, mode, dir, usize::MAX)
    });

    let total_count = Memo::new(move |_| ranked_rows.with(|r| r.len()));

    let computed_data = Memo::new(move |_| {
        ranked_rows.with(|rows| {
            rows.iter()
                .take(ROW_LIMIT)
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

    // Filters currently drawn as a chip. Drives the "no active filters" hint
    // and keeps `+ Filter` from offering a second copy of something the user
    // can already see.
    let active_filters = Memo::new(move |_| {
        let mut active: Vec<&'static str> = Vec::new();
        if scrip_filter().is_some() || pending_filter.get() == Some(FILTER_SCRIP) {
            active.push(FILTER_SCRIP);
        }
        if job_filter().is_some() || pending_filter.get() == Some(FILTER_JOB) {
            active.push(FILTER_JOB);
        }
        active
    });

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_SCRIP => t_string!(i18n, scrip_sources_scrip_type).to_string(),
            FILTER_JOB => t_string!(i18n, scrip_sources_job_filter).to_string(),
            _ => String::new(),
        }
    };

    // What the `+ Filter` menu offers: everything addable that is not already
    // on screen as a chip.
    let filter_options = Memo::new(move |_| {
        ADDABLE_FILTERS
            .iter()
            .copied()
            .filter(|id| !active_filters().contains(id))
            .map(|id| FilterOption {
                id,
                label: filter_label(id),
            })
            .collect::<Vec<_>>()
    });

    let add_filter = Callback::new(move |id: &'static str| match id {
        FILTER_SCRIP => pending_filter.set(Some(FILTER_SCRIP)),
        FILTER_JOB => pending_filter.set(Some(FILTER_JOB)),
        _ => {}
    });

    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        set_scrip_filter(None);
        set_job_filter(None);
    });

    view! {
        <div class="flex flex-col gap-6">
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, scrip_sources_results_count, n = move || total_count())}
                        </span>
                        <Show when=move || { total_count() > ROW_LIMIT }>
                            <span class="text-xs text-[color:var(--color-text-muted)] whitespace-nowrap truncate">
                                {t!(i18n, scrip_sources_top_note, limit = ROW_LIMIT)}
                            </span>
                        </Show>
                        <span class="text-xs text-[color:var(--color-text-muted)] whitespace-nowrap truncate">
                            {move || t!(i18n, scrip_sources_region_pricing, region = world())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! { <RealtimeStatus status=realtime_status last_update=last_update /> }
                        .into_any()
                }
                available_filters=Signal::derive(filter_options)
                on_add_filter=add_filter
                on_clear_all=clear_all
                empty_label=Signal::derive(move || {
                    t_string!(i18n, scrip_sources_no_filters_hint).to_string()
                })
                is_empty=Signal::derive(move || active_filters().is_empty())
            >
                {move || {
                    (scrip_filter().is_some() || pending_filter.get() == Some(FILTER_SCRIP))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_SCRIP);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, scrip_sources_scrip_type).to_string()
                                    value=Signal::derive(scrip_filter)
                                    options=scrip_options()
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_scrip_filter(v);
                                        if pending_filter.get_untracked() == Some(FILTER_SCRIP) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    (job_filter().is_some() || pending_filter.get() == Some(FILTER_JOB))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_JOB);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, scrip_sources_job_filter).to_string()
                                    value=Signal::derive(job_filter)
                                    options=job_options()
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_job_filter(v);
                                        if pending_filter.get_untracked() == Some(FILTER_JOB) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
            </ControlBar>

            // Empty states render as *siblings* of the scroller container,
            // never by unmounting it in a <Show>: the VirtualScroller wires
            // scroll-sync effects to node refs and remounting breaks them.
            <Show when=move || gatherer_filter_selected() && total_count() == 0>
                <ActionableEmptyState
                    title=t_string!(i18n, scrip_sources_gatherers_unsupported_title).to_string()
                    body=t_string!(i18n, scrip_sources_gatherers_unsupported_body).to_string()
                />
            </Show>
            <Show when=move || !gatherer_filter_selected() && total_count() == 0>
                <ActionableEmptyState
                    title=t_string!(i18n, scrip_sources_no_results_title).to_string()
                    body=t_string!(i18n, scrip_sources_no_results_body).to_string()
                />
            </Show>

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
                                <SortHeader
                                    mode=SortMode::CostPerScrip
                                    label=t_string!(i18n, scrip_sources_cost_per_scrip).to_string()
                                    sort_mode
                                    sort_dir
                                />
                             </div>
                             <div role="columnheader" class="w-30 p-4">
                                <SortHeader
                                    mode=SortMode::ScripAmount
                                    label=t_string!(i18n, scrip_sources_scrips).to_string()
                                    sort_mode
                                    sort_dir
                                />
                             </div>
                             <div role="columnheader" class="w-30 p-4">
                                <SortHeader
                                    mode=SortMode::Cost
                                    label=t_string!(i18n, scrip_sources_cost).to_string()
                                    sort_mode
                                    sort_dir
                                />
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
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    {data.scrip_amount}
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.cost />
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
                    title=t_string!(i18n, scrip_sources_title).to_string()
                    summary=t_string!(i18n, scrip_sources_summary).to_string()
                    context=t_string!(i18n, scrip_sources_context).to_string()
                    help_href="/help/scrip-sources"
                    help_body=t_string!(i18n, scrip_sources_help_body).to_string()
                />

                <div class="flex flex-col md:flex-row items-center gap-2">
                    <label class="text-[color:var(--brand-fg)] font-semibold">
                        {t!(i18n, scrip_sources_select_world)}
                    </label>
                    <div class="w-full md:w-auto">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world.into()
                        />
                    </div>
                </div>

                <div class="text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, scrip_sources_description)}
                </div>
                <CalculationSummary
                    title=t_string!(i18n, scrip_sources_efficiency_model).to_string()
                    formula=t_string!(i18n, scrip_sources_efficiency_formula).to_string()
                    details=t_string!(i18n, scrip_sources_efficiency_details).to_string()
                />
                <div class="flex flex-wrap gap-2">
                    <AssumptionBadge text=t_string!(i18n, scrip_sources_assumption_high_reward).to_string() />
                    <AssumptionBadge text=t_string!(i18n, scrip_sources_assumption_market_cost).to_string() />
                    <AssumptionBadge text=t_string!(i18n, scrip_sources_assumption_lower_better).to_string() />
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

#[cfg(test)]
mod tests {
    use super::*;

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
                    ids(&rank_scrip_sources(forward, mode, dir, ROW_LIMIT)),
                    ids(&rank_scrip_sources(reversed, mode, dir, ROW_LIMIT)),
                    "{mode:?}/{dir:?} ranking changed with input order"
                );
            }
        }
    }

    /// The truncation boundary is the sharp edge of the same bug: with ties
    /// spanning the cap, an unstable ranking changes *which* rows survive, so
    /// the two sides render genuinely different items.
    #[test]
    fn truncation_keeps_the_same_rows_regardless_of_input_order() {
        let forward: Vec<_> = (1..=10).map(|i| row(i, 20, 1000)).collect();
        let reversed: Vec<_> = forward.iter().rev().cloned().collect();

        assert_eq!(
            ids(&rank_scrip_sources(
                forward,
                SortMode::ScripAmount,
                SortDir::Desc,
                5
            )),
            ids(&rank_scrip_sources(
                reversed,
                SortMode::ScripAmount,
                SortDir::Desc,
                5
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

        let ranked = rank_scrip_sources(rows, SortMode::ScripAmount, SortDir::Desc, ROW_LIMIT);

        assert_eq!(ids(&ranked), vec![1, 2], "item 1 rendered twice");
    }

    /// Dedup must keep the best-ranked row for an item, not an arbitrary one.
    #[test]
    fn dedup_keeps_the_best_ranked_row_for_an_item() {
        let rows = vec![row(1, 40, 1000), row(2, 30, 1000), row(1, 20, 1000)];

        let ranked = rank_scrip_sources(rows, SortMode::ScripAmount, SortDir::Desc, ROW_LIMIT);

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
                SortMode::ScripAmount.default_dir(),
                ROW_LIMIT
            )),
            vec![2, 3, 1]
        );
        // Cheapest total cost first.
        assert_eq!(
            ids(&rank_scrip_sources(
                rows.clone(),
                SortMode::Cost,
                SortMode::Cost.default_dir(),
                ROW_LIMIT
            )),
            vec![2, 3, 1]
        );
        // Best gil-per-scrip first: 1000/30 < 2000/20 < 3000/10.
        assert_eq!(
            ids(&rank_scrip_sources(
                rows,
                SortMode::CostPerScrip,
                SortMode::CostPerScrip.default_dir(),
                ROW_LIMIT
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
                ids(&rank_scrip_sources(
                    rows.clone(),
                    mode,
                    SortDir::Asc,
                    ROW_LIMIT
                )),
                asc,
                "{mode:?} ascending"
            );
            assert_eq!(
                ids(&rank_scrip_sources(
                    rows.clone(),
                    mode,
                    SortDir::Desc,
                    ROW_LIMIT
                )),
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
                ids(&rank_scrip_sources(
                    rows.clone(),
                    SortMode::Cost,
                    dir,
                    ROW_LIMIT
                )),
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

                let ranked = rank_scrip_sources(rows, mode, dir, ROW_LIMIT);

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
            ids(&rank_scrip_sources(
                rows,
                SortMode::Cost,
                SortDir::Asc,
                ROW_LIMIT
            )),
            vec![2, 3, 1]
        );
    }

    /// The tier boundary is also a truncation boundary: with the cap inside
    /// the fully-priced tier, no partial row may sneak into the rendered set.
    #[test]
    fn truncation_prefers_fully_priced_rows() {
        let rows = vec![
            partial_row(1, 100, 1, 1, 4),
            row(2, 10, 3000),
            row(3, 20, 2000),
        ];

        let ranked = rank_scrip_sources(rows, SortMode::CostPerScrip, SortDir::Asc, 2);

        assert_eq!(ids(&ranked), vec![3, 2]);
    }

    /// Every `Currency` value that actually occurs in `CollectablesShopRewardScrip`
    /// (7.55: `0`, `2`, `4`, `6`, `7` — `0` being the null row, which is already
    /// dropped for having a zero reward). If any of these falls through to
    /// `Other`, every row awarding it disappears from the page.
    #[test]
    fn every_live_currency_value_is_recognised() {
        for currency in [2, 4, 6, 7] {
            assert!(
                !matches!(ScripType::from_currency(currency), ScripType::Other(_)),
                "currency {currency} is unmapped, so its rows never render"
            );
        }
    }

    /// `CollectablesShopRewardScrip.Currency` is a small **enum index**, not an
    /// item id: `2`/`4` are the purple crafter/gatherer pair paid by lv 50-99
    /// turn-ins, `6`/`7` the orange pair paid at level 100.
    #[test]
    fn currency_indices_map_to_the_right_scrip() {
        assert_eq!(ScripType::from_currency(2), ScripType::PurpleCrafters);
        assert_eq!(ScripType::from_currency(4), ScripType::PurpleGatherers);
        assert_eq!(ScripType::from_currency(6), ScripType::OrangeCrafters);
        assert_eq!(ScripType::from_currency(7), ScripType::OrangeGatherers);
    }

    /// The bug this replaced: `from_currency` was fed `reward.currency` but
    /// matched on scrip **item** ids, so no real currency value ever matched and
    /// the whole page rendered zero rows. Item ids must not be accepted here.
    #[test]
    fn scrip_item_ids_are_not_currency_values() {
        for item_id in [41784, 41785, 25199, 33913, 25200, 33914] {
            assert_eq!(
                ScripType::from_currency(item_id),
                ScripType::Other(item_id),
                "item id {item_id} was treated as a currency index"
            );
        }
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

    /// The material exchanges (`CollectablesShop.RewardType == 2`) hand back
    /// *items*, not scrip — but they populate the same
    /// `CollectablesShopRewardScrip.Currency` column the turn-in counters do, so
    /// reading that column without joining `RewardType` lists every one of their
    /// trades as a scrip source paying a scrip it never awards.
    ///
    /// These four are the craftable head of each `RewardType == 2` shop on the
    /// pinned 7.55 data; ids are used rather than names because `Item.name` is
    /// per-locale.
    #[test]
    fn material_exchange_trades_are_not_scrip_turn_ins() {
        let data = xiv_gen_db::data();
        let turn_ins = scrip_turn_ins(data);

        for (item_id, shop) in [
            (31101, "Oddly Specific Materials Exchange (Crafting)"),
            (31750, "Oddly Delicate Materials Exchange"),
            (36311, "Resplendent Materials Exchange"),
            (38756, "Trade Goods Exchange"),
        ] {
            assert!(
                !turn_ins.iter().any(|t| t.item_id == item_id),
                "item {item_id} is traded at the {shop}, which pays no scrip, \
                 but it is listed as a scrip turn-in"
            );
        }
    }

    /// Excluding the material exchanges must not empty the page — this route has
    /// already shipped once rendering zero rows, and a join that silently
    /// matches nothing would put it straight back there.
    #[test]
    fn the_real_turn_in_counters_survive_the_exclusion() {
        let data = xiv_gen_db::data();
        let turn_ins = scrip_turn_ins(data);

        assert!(
            turn_ins.len() > 1000,
            "only {} turn-ins survived; the RewardType join has stopped matching",
            turn_ins.len()
        );
        // A Dwarven collectable handed in for Orange Crafters' Scrip.
        assert!(
            turn_ins.iter().any(|t| t.item_id == 26271),
            "a known scrip turn-in was excluded along with the material exchanges"
        );
    }

    /// The exclusion set has to be non-empty, and must never swallow a group
    /// that a scrip-paying shop offers.
    #[test]
    fn only_material_exchange_groups_are_excluded() {
        let data = xiv_gen_db::data();
        let excluded = material_exchange_groups(data);

        assert!(
            !excluded.is_empty(),
            "no material-exchange groups found; CollectablesShop did not load"
        );
        for shop in data.collectables_shops.values() {
            if shop.reward_type != SCRIP_REWARD_TYPE {
                continue;
            }
            for group in shop.shop_items {
                assert!(
                    group == 0 || !excluded.contains(&group),
                    "group {group} pays scrip but was excluded"
                );
            }
        }
    }

    /// Gatherer scrips are paid for collectables that are *gathered*, so no
    /// turn-in awarding one can have a recipe. The page relies on this: it
    /// prices craft costs, skips anything without a recipe, and tells the user
    /// the gatherer filters are empty by design instead of rendering a blank
    /// table.
    ///
    /// Before the `RewardType` join this was false — 59 craftable material
    /// exchange trades carried `Currency = 4`, so `?scrip=PurpleGatherers`
    /// rendered 59 rows, every one of them wrong.
    #[test]
    fn no_craftable_turn_in_pays_a_gatherer_scrip() {
        let data = xiv_gen_db::data();
        let mut craftable = std::collections::HashSet::new();
        for recipe in data.recipes.values() {
            craftable.insert(recipe.item_result);
        }

        let offenders: Vec<i32> = scrip_turn_ins(data)
            .into_iter()
            .filter(|t| t.scrip_type.is_gatherer() && craftable.contains(&t.item_id))
            .map(|t| t.item_id)
            .collect();

        assert!(
            offenders.is_empty(),
            "{} craftable turn-ins are labelled a gatherer scrip, so the \
             gatherer filters render rows the page says can never exist: {:?}",
            offenders.len(),
            &offenders[..offenders.len().min(8)]
        );
    }
}
