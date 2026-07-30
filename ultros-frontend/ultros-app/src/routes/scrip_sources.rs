use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::xiv_data::tracked_data;
use crate::ws::realtime::use_realtime;
use crate::{
    api::get_cheapest_listings,
    components::{
        gil::*,
        icon::Icon,
        item_icon::*,
        realtime_status::RealtimeStatus,
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
use icondata as i;
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_location, use_navigate, use_query_map},
    location::Location,
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

impl SortMode {
    /// The direction each column sorts in when first clicked (and when no
    /// `?dir=` is present). Costs read best-first ascending; the scrip payout
    /// reads best-first descending.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::CostPerScrip | SortMode::Cost => SortDir::Asc,
            SortMode::ScripAmount => SortDir::Desc,
        }
    }
}

/// `?dir=` — sort direction override. Absent means the active mode's
/// [`SortMode::default_dir`].
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortDir {
    Asc,
    Desc,
}

impl std::str::FromStr for SortDir {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asc" => Ok(SortDir::Asc),
            "desc" => Ok(SortDir::Desc),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        })
    }
}

/// Maximum rows rendered by the table.
const ROW_LIMIT: usize = 100;

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

/// One sortable column header, after the analyzer's pattern.
///
/// Clicking an inactive column sorts by it in that column's default
/// direction; clicking the active column flips the direction. The arrow
/// reflects the direction actually applied. `dir` is omitted from the href
/// when it matches the mode's default so the common case stays a clean
/// `?sort=…`.
#[component]
fn SortHeader(
    mode: SortMode,
    #[prop(into)] label: String,
    sort_mode: Memo<Option<SortMode>>,
    sort_dir: Memo<Option<SortDir>>,
) -> impl IntoView {
    let Location {
        pathname, query, ..
    } = use_location();
    let is_active = Signal::derive(move || sort_mode().unwrap_or(SortMode::CostPerScrip) == mode);
    let dir = Signal::derive(move || {
        sort_dir().unwrap_or_else(|| sort_mode().unwrap_or(SortMode::CostPerScrip).default_dir())
    });
    view! {
        <a
            class=move || {
                if is_active() {
                    "!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                } else {
                    "!text-brand-300 hover:text-brand-200"
                }
            }
            aria-current=move || if is_active() { "true" } else { "false" }
            href=move || {
                let mut q = query();
                q.remove("sort");
                q.remove("dir");
                q.insert("sort".to_string(), mode.to_string());
                let next = if is_active() {
                    match dir() {
                        SortDir::Desc => SortDir::Asc,
                        SortDir::Asc => SortDir::Desc,
                    }
                } else {
                    mode.default_dir()
                };
                if next != mode.default_dir() {
                    q.insert("dir".to_string(), next.to_string());
                }
                format!("{}{}", pathname(), q.to_query_string())
            }
        >
            <div class="flex items-center gap-2">
                {label}
                {move || {
                    is_active()
                        .then(|| match dir() {
                            SortDir::Asc => view! { <Icon icon=i::BiSortUpRegular /> },
                            SortDir::Desc => view! { <Icon icon=i::BiSortDownRegular /> },
                        })
                }}
            </div>
        </a>
    }
    .into_any()
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
    let (scrip_filter, set_scrip_filter) = query_signal::<String>("scrip");
    let (job_filter, set_job_filter) = query_signal::<String>("job");

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
                let scrip_type = ScripType::from_currency(reward.currency as u32);

                if !passes_scrip_filter(scrip_type, scrip_filter_val.as_deref()) {
                    continue;
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
        }

        let mode = sort_mode().unwrap_or(SortMode::CostPerScrip);
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

            // Results summary: count, truncation note, pricing scope, and
            // realtime health. The world picker resolves to a *region* and
            // the fetch is region-cheapest, so say so — a "Gilgamesh"
            // selection otherwise silently produces NA-wide prices.
            <div class="panel px-4 py-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
                <div>
                    <span class="text-brand-300 font-semibold">{move || total_count()}</span>
                    " "
                    {t!(i18n, scrip_sources_results)}
                </div>
                <Show when=move || { total_count() > ROW_LIMIT }>
                    <div class="text-[color:var(--color-text-muted)]">
                        {t!(i18n, scrip_sources_top_note, limit = ROW_LIMIT)}
                    </div>
                </Show>
                <div class="text-[color:var(--color-text-muted)]">
                    {move || t!(i18n, scrip_sources_region_pricing, region = world())}
                </div>
                <RealtimeStatus status=realtime_status last_update=last_update />
            </div>

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
}
