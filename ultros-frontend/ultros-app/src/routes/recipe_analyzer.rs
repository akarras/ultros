use crate::analyzer_kit::cells::{CellNote, CellValue, Enrich};
use crate::analyzer_kit::columns::{
    CellCtx, ColumnKind, ColumnSpec, Layer, LazyFeed, PickerContext, PickerGroup, Sortability,
    ToolColumnMeta, default_dir_for, grouped_picker_options, picker_options, sort_from_token,
    sort_token, sortability_for,
};
use crate::analyzer_kit::enrichment::SparkValue;
use crate::analyzer_kit::formula::{
    FormulaMarks, PriceSignal, ProfitFormula, RoiMath, SaleStat, per_unit_cost, profit_line,
};
use crate::analyzer_kit::grid::{
    AnalyzerGrid, AnalyzerRow, CustomCell, GridLayout, HeaderExtra, HeaderExtras, HeaderLine2,
    HeaderPill, MarkLabels,
};
use crate::analyzer_kit::hop::{HopGain, WorldsToVisit, hop_gain, worlds_to_visit};
use crate::analyzer_kit::needed::{
    BodyRole, NeededSignals, RecipeNeeds, SALE_STATS_WINDOW_DAYS, SignalWants, needed_bodies,
    needed_signals,
};
use crate::analyzer_kit::signals::{
    PriceLookup, SignalView, StatsIndex, stat_only_cheapest, stat_row_either, stats_index,
};
use crate::analyzer_kit::strip::{FormulaStrip, StripLayout, StripSelect, StripTerm};
use crate::components::crafting_cost::{
    CostBreakdown, CraftingCostOptions, EmptyOnHand, OnHand, ShardsMode, compute_cost,
    vendor_price_map,
};
use crate::components::dismissable::use_dismissable;
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::on_hand_input::{ActiveListBanner, LocalOnHand, OnHandMap};
use crate::components::related_items::is_shard_item;
use crate::components::term_badge::TermRole;
use crate::global_state::craft_options::{self, CraftOptions};
use crate::global_state::labs::{LAB_ANALYZER_RECIPE, use_lab};
use crate::global_state::region_for_world::use_datacenter_for_world;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::price_basis::{BuyScope, CostBasis, RevenueMetric};
use crate::query_defaults::{DEFAULT_MIN_DAILY_SALES, filter_query_signal, seed_query_default};
use crate::ws::realtime::use_realtime;
use crate::{
    analysis::{SalesStats, analyze_sales, profit_per_day_from_rate},
    api::{get_cheapest_listings, get_recent_sales_for_world, get_sale_stats},
    components::{
        add_recipe_to_list::AddRecipeToList,
        control_bar::{ControlBar, FilterOption, parse_visible_cols, serialize_visible_cols},
        crafter_settings::CrafterSettings,
        filter_chip::FilterChip,
        gil::*,
        icon::Icon,
        item_icon::*,
        query_button::QueryButton,
        realtime_status::RealtimeStatus,
        skeleton::{BoxSkeleton, InlineStatusSkeleton},
        sort_header::{SortColumn, SortDir, cmp_none_last},
        tool_help::*,
        tooltip::Tooltip,
        world_picker::WorldOnlyPicker,
    },
    global_state::{
        LocalWorldData, cookies::Cookies, crafter_levels::CrafterLevels,
        home_world::use_home_world, region_for_world::use_region_for_world,
    },
};
use icondata as i;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_i18n::I18nContext;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_query_map},
};
use percent_encoding::utf8_percent_encode;
use std::collections::{BTreeSet, HashSet};
use std::sync::LazyLock;
use std::{cmp::Ordering, collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
use thousands::Separable;
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
    sale_stats::{BulkSaleStats, ItemSaleStats},
    trends::ConfidenceBand,
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
    // Sell-world market context, from the widened sale stats. All zero /
    // `Unknown` when the stats aren't fetched (no stats-backed column
    // visible and a listing revenue basis) or the item had no sales.
    last_sold_unix: i64,
    units_sold: u64,
    vwap: i32,
    /// Current sell price vs the window VWAP, as a percent. `None` when
    /// there is no VWAP to compare against.
    vwap_pct: Option<f32>,
    /// The market board's cut of one unit's sale at `market_price`.
    tax: i32,
    confidence: ConfidenceBand,
    /// Which quality the sell-world statistics above came from: the
    /// required one, or the other when only that one traded. The lazy
    /// sparkline feed and the 30-day columns key on it, so every figure in
    /// a row describes the same quality.
    stat_hq: bool,
    /// Per-unit cost under each cost signal that was run, by
    /// `PriceSignal::index`; `None` = not run (not needed, capped, or a
    /// sale signal with no buy-scope body).
    cost_alt: [Option<i32>; 4],
    /// The bare sell-world statistic (or listing) per revenue signal, no
    /// fallback; `None` = no row.
    rev_alt: [Option<i32>; 4],
    /// `market_price` is not the selected signal on the sell world: the
    /// stat was missing, or the listing fell back to the buy scope.
    revenue_fell_back: bool,
    /// Marketable ingredient lines no listing priced, under the selected
    /// signal. They cost 0 here (row membership unchanged) and are said so.
    unpriced: u16,
    /// `None` when Hop gain was not wanted, or there is no sell-world
    /// listing body to price the home side against.
    hop: Option<HopGain>,
    /// `None` when Worlds to visit was not wanted, or Buy from = This world.
    worlds: Option<WorldsToVisit>,
}

/// Current sell price vs the window VWAP, as a percent. `None` when there
/// is no VWAP to compare against (no sales in the window, or an old
/// server that doesn't serve the column).
fn vwap_pct(market_price: i32, vwap: i32) -> Option<f32> {
    (vwap > 0).then(|| (market_price - vwap) as f32 / vwap as f32 * 100.0)
}

/// Collapse the NQ/HQ rows from the 7-day rollup into the sale summary used
/// by the always-visible velocity and average-price columns. The rollup is the
/// default source so the analyzer does not need a second recent-sales payload;
/// raw samples remain available when the user explicitly enables outlier
/// filtering.
fn sales_stats_from_rollup(
    stats: &HashMap<(i32, bool), ItemSaleStats>,
    item_id: i32,
) -> Option<SalesStats> {
    let rows = [false, true]
        .into_iter()
        .filter_map(|hq| stats.get(&(item_id, hq)))
        .filter(|row| row.num_sold > 0)
        .collect::<Vec<_>>();
    let total_sales = rows.iter().map(|row| row.num_sold).sum::<i64>();
    if total_sales <= 0 {
        return None;
    }
    let price_total = rows
        .iter()
        .map(|row| i128::from(row.avg_price) * i128::from(row.num_sold))
        .sum::<i128>();

    Some(SalesStats {
        daily_sales: total_sales as f32 / f32::from(SALE_STATS_WINDOW_DAYS),
        avg_price: (price_total / i128::from(total_sales)) as i32,
        total_sales: usize::try_from(total_sales).unwrap_or(usize::MAX),
    })
}

/// Whether a row's cheapest-listing location passes the listing-world /
/// listing-dc filters. Rows whose cheapest world is unknown (`world_id`
/// resolved to no name — e.g. the stat-overlay's placeholder 0) fail any
/// active location filter rather than slipping through it.
fn listing_location_passes(
    names: Option<&(String, String)>,
    world_filter: Option<&str>,
    dc_filter: Option<&str>,
) -> bool {
    if world_filter.is_none() && dc_filter.is_none() {
        return true;
    }
    match names {
        None => false,
        Some((world, dc)) => {
            world_filter.is_none_or(|f| f == world) && dc_filter.is_none_or(|f| f == dc)
        }
    }
}

/// Sort ordinal for the confidence band: better bands sort higher, and
/// rows without deep-scan data (`Unknown`) sort below everything so a
/// descending confidence sort surfaces trustworthy rows first.
fn confidence_rank(band: ConfidenceBand) -> u8 {
    match band {
        ConfidenceBand::Unknown => 0,
        ConfidenceBand::Unusable => 1,
        ConfidenceBand::Low => 2,
        ConfidenceBand::Medium => 3,
        ConfidenceBand::High => 4,
    }
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

// --- Pricing methodology options -------------------------------------------
// (token, localized label) pairs for the three pricing selects. Free
// functions rather than closures inside the page component because both the
// chip row and [`MarketMenu`] render them.

fn cost_basis_options(i18n: I18nContext<Locale, I18nKeys>) -> Vec<(&'static str, String)> {
    vec![
        (
            "listing-min",
            t_string!(i18n, price_basis_listing_min).to_string(),
        ),
        (
            "sale-median",
            t_string!(i18n, price_basis_sale_median).to_string(),
        ),
        (
            "sale-min",
            t_string!(i18n, price_basis_sale_min).to_string(),
        ),
        (
            "sale-avg",
            t_string!(i18n, price_basis_sale_avg).to_string(),
        ),
    ]
}

fn buy_scope_options(i18n: I18nContext<Locale, I18nKeys>) -> Vec<(&'static str, String)> {
    vec![
        ("world", t_string!(i18n, buy_scope_home_world).to_string()),
        ("datacenter", t_string!(i18n, datacenter).to_string()),
        ("region", t_string!(i18n, region).to_string()),
    ]
}

/// The header sub-labels for one set of marks: each priced side names the
/// signal *and* the place it was read from, and the result column carries
/// the tool's own sub-line. Built key by key and never iterated, so no
/// `HashMap` ordering can reach the DOM.
fn mark_labels(
    m: &FormulaMarks,
    cost_short: &str,
    revenue_short: &str,
    profit_sub: &str,
) -> MarkLabels {
    MarkLabels {
        labels: [
            (TermRole::Result, profit_sub.to_string()),
            (
                TermRole::Revenue,
                format!("{revenue_short} · {}", m.sell_place),
            ),
            (TermRole::Cost, format!("{cost_short} · {}", m.buy_place)),
        ]
        .into_iter()
        .collect(),
    }
}

/// The short name a header sub-label or a strip chip uses for a signal
/// ("listing", "7d median"), as opposed to the long picker labels in
/// [`cost_basis_options`].
fn short_signal(i18n: I18nContext<Locale, I18nKeys>, s: PriceSignal) -> String {
    match s {
        PriceSignal::ListingMin => t_string!(i18n, signal_short_listing_min).to_string(),
        PriceSignal::SaleMin => t_string!(i18n, signal_short_sale_min).to_string(),
        PriceSignal::SaleMedian => t_string!(i18n, signal_short_sale_median).to_string(),
        PriceSignal::SaleAvg => t_string!(i18n, signal_short_sale_avg).to_string(),
    }
}

/// One labelled select inside the [`MarketMenu`] popover. Commits on
/// `change` — unlike [`FilterChip`]'s select, this one stays mounted after a
/// commit (the popover only closes on dismiss), so committing per keystroke
/// of keyboard browsing does not tear the control down mid-navigation.
#[component]
fn PricingSelect(
    #[prop(into)] label: String,
    #[prop(into)] value: Signal<String>,
    options: Vec<(&'static str, String)>,
    #[prop(into)] on_change: Callback<String>,
) -> impl IntoView {
    view! {
        <label class="flex flex-col gap-1 text-[color:var(--color-text)]">
            <span class="text-xs text-[color:var(--color-text-muted)]">{label}</span>
            <select
                class="input input-sm"
                prop:value=move || value.get()
                on:change=move |ev| on_change.run(event_target_value(&ev))
            >
                {options
                    .into_iter()
                    .map(|(val, lab)| {
                        view! {
                            <option value=val selected=move || value.get() == val>
                                {lab}
                            </option>
                        }
                    })
                    .collect_view()}
            </select>
        </label>
    }
}

/// Always-visible `Market` button in the control bar's first row, opening a
/// popover with the buy-scope / cost-basis / revenue-metric selects — or,
/// while the analyzer-recipe lab is on, the stacked formula strip and the
/// four price-basis explanations.
///
/// These existed as permanent toolbar fields (#1206), then the
/// Toolbar→ControlBar migration (#1214) filed them under `+ Filter` — where
/// #1233 reported the whole feature as gone. They are not row filters: they
/// change how every row is priced, so they get a standing entry point in row
/// 1 (same shape as the flip finder's `SavedViewsMenu`). Reads and writes the
/// same query params as the page's signals, so the non-default chips in the
/// filter row stay in sync automatically.
#[component]
fn MarketMenu(
    /// The same ledger chips the inline strip renders, built once on the
    /// page (this component lives inside the table's `ControlBar`).
    terms: Callback<(), Vec<StripTerm>>,
    /// The `analyzer-recipe` Labs toggle. Off = exactly the three selects
    /// below.
    preview: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let (cost_basis, set_cost_basis) = filter_query_signal::<CostBasis>(FILTER_COST_BASIS);
    let (revenue_metric, set_revenue_metric) = filter_query_signal::<RevenueMetric>(FILTER_REVENUE);
    let (buy_scope, set_buy_scope) = filter_query_signal::<BuyScope>(FILTER_BUY_SCOPE);

    let open = RwSignal::new(false);
    let container = NodeRef::<leptos::html::Div>::new();
    use_dismissable(container, move || open.set(false));

    view! {
        <div class="relative flex items-center" node_ref=container>
            // Icon-only below `md`, same yield rules as every row-1 button —
            // the bar is height-locked and this row cannot wrap.
            <button
                class="sticky-bar-button sticky-bar-button-shrink"
                aria-label=t_string!(i18n, recipe_analyzer_market_button)
                aria-expanded=move || open.get().to_string()
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <Icon icon=icondata::MdiCashMultiple />
                <span class="hidden md:inline sticky-bar-button-label">
                    {t!(i18n, recipe_analyzer_market_button)}
                </span>
            </button>
            <Show when=move || open.get()>
                <div class=move || {
                    if preview {
                        "sticky-bar-popover p-3 w-[min(92vw,20rem)] flex flex-col gap-2 text-sm"
                    } else {
                        "sticky-bar-popover p-3 w-[min(92vw,16rem)] flex flex-col gap-2 text-sm"
                    }
                }>
                    <Show
                        when=move || preview
                        fallback=move || {
                            view! {
                                <PricingSelect
                                    label=t_string!(i18n, recipe_analyzer_buy_from_label).to_string()
                                    value=Signal::derive(move || {
                                        buy_scope().unwrap_or_default().to_string()
                                    })
                                    options=buy_scope_options(i18n)
                                    on_change=Callback::new(move |v: String| {
                                        let parsed = v.parse::<BuyScope>().ok();
                                        set_buy_scope(parsed.filter(|s| *s != BuyScope::default()));
                                    })
                                />
                                <PricingSelect
                                    label=t_string!(i18n, recipe_analyzer_cost_basis_label).to_string()
                                    value=Signal::derive(move || {
                                        cost_basis().unwrap_or_default().to_string()
                                    })
                                    options=cost_basis_options(i18n)
                                    on_change=Callback::new(move |v: String| {
                                        let parsed = v.parse::<CostBasis>().ok();
                                        set_cost_basis(parsed.filter(|b| *b != CostBasis::default()));
                                    })
                                />
                                <PricingSelect
                                    label=t_string!(i18n, recipe_analyzer_revenue_label).to_string()
                                    value=Signal::derive(move || {
                                        revenue_metric().unwrap_or_default().to_string()
                                    })
                                    options=cost_basis_options(i18n)
                                    on_change=Callback::new(move |v: String| {
                                        let parsed = v.parse::<RevenueMetric>().ok();
                                        set_revenue_metric(
                                            parsed.filter(|m| *m != RevenueMetric::default()),
                                        );
                                    })
                                />
                            }
                        }
                    >
                        <FormulaStrip terms=terms.run(()) layout=StripLayout::Stacked />
                        // What each price basis actually means, so the
                        // strip's selects are choosable without leaving
                        // the page. Each line opens with the picker label
                        // it explains, so a sentence can be matched to the
                        // option it belongs to.
                        <div class="flex flex-col gap-1 text-xs text-[color:var(--color-text-muted)]">
                            <span>
                                <span class="font-medium text-[color:var(--color-text)]">
                                    {t!(i18n, price_basis_listing_min)}
                                </span>
                                " "
                                {t!(i18n, price_basis_listing_min_help)}
                            </span>
                            <span>
                                <span class="font-medium text-[color:var(--color-text)]">
                                    {t!(i18n, price_basis_sale_median)}
                                </span>
                                " "
                                {t!(i18n, price_basis_sale_median_help)}
                            </span>
                            <span>
                                <span class="font-medium text-[color:var(--color-text)]">
                                    {t!(i18n, price_basis_sale_min)}
                                </span>
                                " "
                                {t!(i18n, price_basis_sale_min_help)}
                            </span>
                            <span>
                                <span class="font-medium text-[color:var(--color-text)]">
                                    {t!(i18n, price_basis_sale_avg)}
                                </span>
                                " "
                                {t!(i18n, price_basis_sale_avg_help)}
                            </span>
                        </div>
                    </Show>
                </div>
            </Show>
        </div>
    }
    .into_any()
}

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_PROFIT: &str = "profit";
const FILTER_ROI: &str = "roi";
const FILTER_MIN_SALES: &str = "min-sales";
const FILTER_JOB: &str = "job";
const FILTER_COST_BASIS: &str = "cost-basis";
const FILTER_REVENUE: &str = "revenue";
const FILTER_BUY_SCOPE: &str = "buy-scope";
// Set by clicking a world/DC name in the cheapest-listing columns (same
// `QueryButton` flow as the flip finder), not from the `+ Filter` menu —
// hence not in `ADDABLE_FILTERS`. `world`/`datacenter` are taken by the
// sell-world picker and legacy params on this route, so these get their
// own keys.
const FILTER_LISTING_WORLD: &str = "listing-world";
const FILTER_LISTING_DC: &str = "listing-dc";
const FILTER_SUBCRAFTS: &str = "subcrafts";
const FILTER_REQUIRE_HQ: &str = "require-hq";
const FILTER_OUTLIERS: &str = "filter-outliers";
const FILTER_EXCLUDE_SHARDS: &str = "shards-exclude";
const FILTER_USE_ON_HAND: &str = "on-hand";

/// Filters the `+ Filter` menu can add, in the old toolbar's left-to-right
/// order.
// The pricing methodology controls (cost basis, revenue metric, scope) are
// deliberately *not* in this list: they change how every row is priced rather
// than which rows show, so they live behind the always-visible `Market`
// button in row 1 (see [`MarketMenu`]) instead of the `+ Filter` menu, where
// #1233 reported them as impossible to find.
const ADDABLE_FILTERS: &[&str] = &[
    FILTER_PROFIT,
    FILTER_ROI,
    FILTER_MIN_SALES,
    FILTER_JOB,
    FILTER_SUBCRAFTS,
    FILTER_REQUIRE_HQ,
    FILTER_OUTLIERS,
    FILTER_EXCLUDE_SHARDS,
    FILTER_USE_ON_HAND,
];

// --- Optional columns ------------------------------------------------------
// `?cols=` namespace, distinct from the filter registry above. Order here is
// the columns-picker + serialization order.
const COL_CONFIDENCE: &str = "confidence";
const COL_LAST_SOLD: &str = "last-sold";
const COL_VOLUME: &str = "volume";
const COL_VWAP: &str = "vwap";
const COL_TAX: &str = "tax";
const COL_LISTING_WORLD: &str = "listing-world";
const COL_LISTING_DC: &str = "listing-dc";
// Phase D, behind `analyzer-recipe`: appended after the seven
// above so every serialized old URL stays byte-identical.
const COL_REV_LISTING_MIN: &str = "rev-listing-min";
const COL_REV_SALE_MIN: &str = "rev-sale-min";
const COL_REV_SALE_MEDIAN: &str = "rev-sale-median";
const COL_REV_SALE_AVG: &str = "rev-sale-avg";
const COL_COST_LISTING_MIN: &str = "cost-listing-min";
const COL_COST_SALE_MIN: &str = "cost-sale-min";
const COL_COST_SALE_MEDIAN: &str = "cost-sale-median";
const COL_COST_SALE_AVG: &str = "cost-sale-avg";
const COL_HOP_GAIN: &str = "hop-gain";
const COL_HOP_WORLDS: &str = "hop-worlds";
// Phase E2's market columns, appended after the ten above for the same
// reason: an old serialized `?cols=` must round-trip byte-identically.
const COL_PROFIT_PER_DAY: &str = "profit-per-day";
const COL_TREND: &str = "trend";
const COL_DRIFT: &str = "drift";
const COL_VOLUME_30D: &str = "volume-30d";
const COL_VWAP_30D: &str = "vwap-30d";

/// The lazy feed the Trend and Drift columns share: 168 hourly points, one
/// request per visible window. `RECIPE_TREND_FEED.hours()` is what the
/// fetch sends, so the column table and the request can never disagree.
const RECIPE_TREND_FEED: LazyFeed = LazyFeed::Sparklines { hours: 168 };

/// `?cols=` order, derived from the table so the URL contract has one
/// source: the picker, the grid and the serializer cannot disagree.
static OPTIONAL_COLUMN_ORDER: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty())
        .map(|c| c.id)
        .collect()
});
/// The `?cols=` contract while the signal-columns lab is off: every token
/// not gated by a lab. `parse_visible_cols` over this slice drops the
/// Phase D tokens, so a shared `?cols=hop-gain` renders as before the
/// phase for a player without the lab.
static BASE_COLUMN_ORDER: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty() && c.lab.is_none())
        .map(|c| c.id)
        .collect()
});
/// Default-visible optional columns, derived from `default_on`. Sales/day
/// is already an always-on column; the confidence chip joins it by default
/// so stale or manipulated sell-world markets don't silently top the
/// ranking.
static DEFAULT_COLS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty() && c.default_on)
        .map(|c| c.id)
        .collect()
});

// --- The column table ------------------------------------------------------
// One `static` describes every column: its `?cols=` token, its `?sort=`
// token and default direction, the classes it renders with, and how to pull
// its value off a row. `SortMode`'s context-free `FromStr`/`Display` and the
// `&'static` slices `parse_visible_cols` needs both read it.

type RecipeRow = Arc<RecipeProfitData>;

impl AnalyzerRow for RecipeRow {
    type Key = xiv_gen::RecipeId;
    fn key(&self) -> Self::Key {
        self.recipe.key_id
    }
}

// Labels: one fn per column so the table can be a `static`.
fn label_item(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, item).to_string()
}
fn label_profit(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, profit).to_string()
}
fn label_roi(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, roi).to_string()
}
fn label_cost(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_cost_per_unit).to_string()
}
fn label_price(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price).to_string()
}
fn label_daily(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, daily_sales).to_string()
}
fn label_avg(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, avg_price).to_string()
}
fn label_confidence(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_confidence).to_string()
}
fn label_last_sold(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_last_sold).to_string()
}
fn label_volume(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_volume).to_string()
}
fn label_vwap(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_vwap).to_string()
}
fn label_tax(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_tax).to_string()
}
fn label_world(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_world).to_string()
}
fn label_dc(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_datacenter).to_string()
}
fn label_actions(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, actions).to_string()
}
fn label_listing_min(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_listing_min).to_string()
}
fn label_sale_min(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_sale_min).to_string()
}
fn label_sale_median(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_sale_median).to_string()
}
fn label_sale_avg(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, price_basis_sale_avg).to_string()
}
fn label_hop_gain(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_hop_gain).to_string()
}
fn label_hop_worlds(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_hop_worlds).to_string()
}
fn label_profit_per_day(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_profit_per_day).to_string()
}
fn label_trend(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_spark).to_string()
}
/// The recipe analyzer's own Drift label, *not* the flip finder's
/// `analyzer_col_drift`: fr and de translate that key and
/// `analyzer_col_spark` to the same word, which would put two
/// identically-labelled columns side by side here and two identical
/// checkboxes in the Market picker group.
fn label_drift(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_drift).to_string()
}
fn label_volume_30d(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_volume_30d).to_string()
}
fn label_vwap_30d(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_vwap_30d).to_string()
}

static SPEC_ITEM: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Item,
    label: label_item,
    group: PickerGroup::Other,
};
static SPEC_PROFIT: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Profit,
    label: label_profit,
    group: PickerGroup::Other,
};
static SPEC_ROI: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Roi,
    label: label_roi,
    group: PickerGroup::Other,
};
static SPEC_COST: ColumnSpec = ColumnSpec {
    kind: ColumnKind::CostSlot,
    label: label_cost,
    group: PickerGroup::Other,
};
static SPEC_PRICE: ColumnSpec = ColumnSpec {
    kind: ColumnKind::RevenueSlot,
    label: label_price,
    group: PickerGroup::Other,
};
static SPEC_DAILY: ColumnSpec = ColumnSpec {
    kind: ColumnKind::SalesPerDay7,
    label: label_daily,
    group: PickerGroup::Other,
};
static SPEC_AVG: ColumnSpec = ColumnSpec {
    kind: ColumnKind::AvgPrice,
    label: label_avg,
    group: PickerGroup::Other,
};
static SPEC_CONFIDENCE: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Confidence,
    label: label_confidence,
    group: PickerGroup::Other,
};
static SPEC_LAST_SOLD: ColumnSpec = ColumnSpec {
    kind: ColumnKind::LastSold,
    label: label_last_sold,
    group: PickerGroup::Other,
};
static SPEC_VOLUME: ColumnSpec = ColumnSpec {
    kind: ColumnKind::VolumeUnits7,
    label: label_volume,
    group: PickerGroup::Other,
};
static SPEC_VWAP: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Vwap7,
    label: label_vwap,
    group: PickerGroup::Other,
};
static SPEC_TAX: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Tax,
    label: label_tax,
    group: PickerGroup::Other,
};
static SPEC_WORLD: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ListingWorld,
    label: label_world,
    group: PickerGroup::Other,
};
static SPEC_DC: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ListingDc,
    label: label_dc,
    group: PickerGroup::Other,
};
static SPEC_ACTIONS: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Actions,
    label: label_actions,
    group: PickerGroup::Other,
};

static SPEC_REV_LISTING_MIN: ColumnSpec = ColumnSpec {
    kind: ColumnKind::RevSignal(PriceSignal::ListingMin),
    label: label_listing_min,
    group: PickerGroup::Revenue,
};
static SPEC_REV_SALE_MIN: ColumnSpec = ColumnSpec {
    kind: ColumnKind::RevSignal(PriceSignal::SaleMin),
    label: label_sale_min,
    group: PickerGroup::Revenue,
};
static SPEC_REV_SALE_MEDIAN: ColumnSpec = ColumnSpec {
    kind: ColumnKind::RevSignal(PriceSignal::SaleMedian),
    label: label_sale_median,
    group: PickerGroup::Revenue,
};
static SPEC_REV_SALE_AVG: ColumnSpec = ColumnSpec {
    kind: ColumnKind::RevSignal(PriceSignal::SaleAvg),
    label: label_sale_avg,
    group: PickerGroup::Revenue,
};
static SPEC_COST_LISTING_MIN: ColumnSpec = ColumnSpec {
    kind: ColumnKind::CostSignal(PriceSignal::ListingMin),
    label: label_listing_min,
    group: PickerGroup::Cost,
};
static SPEC_COST_SALE_MIN: ColumnSpec = ColumnSpec {
    kind: ColumnKind::CostSignal(PriceSignal::SaleMin),
    label: label_sale_min,
    group: PickerGroup::Cost,
};
static SPEC_COST_SALE_MEDIAN: ColumnSpec = ColumnSpec {
    kind: ColumnKind::CostSignal(PriceSignal::SaleMedian),
    label: label_sale_median,
    group: PickerGroup::Cost,
};
static SPEC_COST_SALE_AVG: ColumnSpec = ColumnSpec {
    kind: ColumnKind::CostSignal(PriceSignal::SaleAvg),
    label: label_sale_avg,
    group: PickerGroup::Cost,
};
static SPEC_HOP_GAIN: ColumnSpec = ColumnSpec {
    kind: ColumnKind::HopGain,
    label: label_hop_gain,
    group: PickerGroup::Travel,
};
static SPEC_HOP_WORLDS: ColumnSpec = ColumnSpec {
    kind: ColumnKind::HopWorlds,
    label: label_hop_worlds,
    group: PickerGroup::Travel,
};
static SPEC_PROFIT_PER_DAY: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ProfitPerDay,
    label: label_profit_per_day,
    group: PickerGroup::Market,
};
static SPEC_TREND: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Trend,
    label: label_trend,
    group: PickerGroup::Market,
};
static SPEC_DRIFT: ColumnSpec = ColumnSpec {
    kind: ColumnKind::DriftSpark,
    label: label_drift,
    group: PickerGroup::Market,
};
static SPEC_VOLUME_30D: ColumnSpec = ColumnSpec {
    kind: ColumnKind::VolumeUnits30,
    label: label_volume_30d,
    group: PickerGroup::Market,
};
static SPEC_VWAP_30D: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Vwap30,
    label: label_vwap_30d,
    group: PickerGroup::Market,
};

// Cell extractors. `Custom` = the page renders it (needs context the row
// does not carry: item names, the world link, the on-hand list button).
fn cell_custom(_: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Custom
}
fn cell_roi(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::RoiBadge(r.return_on_investment)
}
/// The Price slot: under the lab it carries the always-present note
/// sub-line so a price that fell back to a listing says so.
fn cell_price(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    if ctx.preview {
        CellValue::GilWithNote {
            amount: r.market_price,
            note: if r.revenue_fell_back {
                CellNote::ListingFallback
            } else {
                CellNote::None
            },
        }
    } else {
        CellValue::Gil(r.market_price)
    }
}
fn cell_avg(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Gil(r.avg_price)
}
fn cell_confidence(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Confidence(r.confidence)
}
fn cell_last_sold(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::LastSoldUnix(r.last_sold_unix)
}
fn cell_volume(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Count(r.units_sold)
}
fn cell_vwap(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::GilWithPct {
        amount: r.vwap,
        pct: r.vwap_pct,
    }
}
fn cell_tax(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Gil(r.tax)
}

/// Percent of an alternative against the same-side formula input; `None`
/// when either is unpriced, or when they are equal (the selected signal's
/// own duplicate column shows no "+0%").
fn delta_pct(alt: Option<i32>, input: i32) -> Option<f32> {
    let alt = alt.filter(|a| *a > 0)?;
    (input > 0 && alt != input).then(|| (alt - input) as f32 / input as f32 * 100.0)
}

fn cost_alt_cell(r: &RecipeRow, ctx: &CellCtx, s: PriceSignal) -> CellValue {
    let alt = r.cost_alt[s.index()];
    CellValue::MutedGil {
        amount: alt,
        pct: delta_pct(alt, r.cost),
        side: TermRole::Cost,
        capped: ctx.capped_cost[s.index()],
    }
}
fn rev_alt_cell(r: &RecipeRow, s: PriceSignal) -> CellValue {
    let alt = r.rev_alt[s.index()];
    CellValue::MutedGil {
        amount: alt,
        pct: delta_pct(alt, r.market_price),
        side: TermRole::Revenue,
        capped: false,
    }
}
// One `fn` per column: the table needs fn pointers, not closures.
fn cell_rev_listing_min(r: &RecipeRow, _: &CellCtx) -> CellValue {
    rev_alt_cell(r, PriceSignal::ListingMin)
}
fn cell_rev_sale_min(r: &RecipeRow, _: &CellCtx) -> CellValue {
    rev_alt_cell(r, PriceSignal::SaleMin)
}
fn cell_rev_sale_median(r: &RecipeRow, _: &CellCtx) -> CellValue {
    rev_alt_cell(r, PriceSignal::SaleMedian)
}
fn cell_rev_sale_avg(r: &RecipeRow, _: &CellCtx) -> CellValue {
    rev_alt_cell(r, PriceSignal::SaleAvg)
}
fn cell_cost_listing_min(r: &RecipeRow, c: &CellCtx) -> CellValue {
    cost_alt_cell(r, c, PriceSignal::ListingMin)
}
fn cell_cost_sale_min(r: &RecipeRow, c: &CellCtx) -> CellValue {
    cost_alt_cell(r, c, PriceSignal::SaleMin)
}
fn cell_cost_sale_median(r: &RecipeRow, c: &CellCtx) -> CellValue {
    cost_alt_cell(r, c, PriceSignal::SaleMedian)
}
fn cell_cost_sale_avg(r: &RecipeRow, c: &CellCtx) -> CellValue {
    cost_alt_cell(r, c, PriceSignal::SaleAvg)
}
fn cell_hop_gain(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Hop {
        gain: r.hop.unwrap_or(HopGain::Unavailable),
        daily_sales: r.daily_sales,
    }
}

fn cell_profit_per_day(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Gil(profit_per_day_from_rate(r.profit, r.daily_sales))
}

/// One read of the page's sparkline store, projected. The read happens
/// inside the row's reactive closure, which is what makes the cell re-render
/// when a batch merges; with no store (every other page, and every test)
/// the cell stays on its loading shape, which is also the server's.
fn spark_with<V>(r: &RecipeRow, ctx: &CellCtx, f: impl Fn(&SparkValue) -> V) -> Enrich<V> {
    let key = (r.recipe.item_result, r.stat_hq);
    match ctx.sparklines {
        Some(store) => store.with(|s| s.state(&key).map(f)),
        None => Enrich::Loading,
    }
}

fn cell_trend(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    CellValue::Sparkline(spark_with(r, ctx, SparkValue::clone))
}

fn cell_drift(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    CellValue::LazyPct(spark_with(r, ctx, |v| v.delta_pct))
}

/// The same, for the client-only 30-day body: `Loading` while it is in
/// flight, `Missing` once it has landed with no row for this item (and on a
/// page that has no such body).
fn late_30<V>(r: &RecipeRow, ctx: &CellCtx, f: impl Fn(&ItemSaleStats) -> V) -> Enrich<V> {
    let Some(stats) = ctx.stats_30 else {
        return Enrich::Missing;
    };
    stats.with(|index| match index {
        None => Enrich::Loading,
        Some(index) => match stat_row_either(index, r.recipe.item_result, r.stat_hq) {
            Some(row) => Enrich::Ready(f(row)),
            None => Enrich::Missing,
        },
    })
}

fn cell_volume_30(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    CellValue::LateCount(late_30(r, ctx, |s| s.units_sold))
}

fn cell_vwap_30(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    let price = r.market_price;
    CellValue::LateGilWithPct(late_30(r, ctx, move |s| (s.vwap, vwap_pct(price, s.vwap))))
}

const CELL_R: &str = "px-4 py-2 w-32 shrink-0 text-right";
const CELL_R_MD: &str = "px-4 py-2 w-32 shrink-0 text-right hidden md:block";
const CELL_28_MD: &str = "px-4 py-2 w-28 shrink-0 text-right hidden md:block";
const HEAD: &str = "w-32 shrink-0 p-4";
const HEAD_MD: &str = "w-32 shrink-0 p-4 hidden md:block";
const HEAD_28_MD: &str = "w-28 shrink-0 p-4 hidden md:block";

/// The kit's `VirtualScroller` runs in **container** mode, where the row
/// spacer carries no width of its own and so resolves to the port width,
/// clipping every row there while the header — a sibling outside that box —
/// keeps painting the full grid. Sizing the spacer is what reaches the
/// scroller's scrollable overflow region; widening the rows alone cannot,
/// because the row box carries `contain: layout`.
///
/// `max-content` rather than an arithmetic sum: every column here is a fixed
/// `w-*` with `shrink-0`, the rows are fixed-height (no `content-visibility`
/// to make an intrinsic measurement unstable), and it follows the
/// `hidden md:block` columns across the breakpoint on its own, so no constant
/// has to track `RECIPE_COLUMNS`.
const RECIPE_ROW_MIN_WIDTH: &str = "max-content";

/// `min-w-max` so the header's tint band spans the whole scrolled width
/// instead of stopping at the viewport edge, matching the spacer above.
const RECIPE_HEADER_CLASS: &str = "min-w-max flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]";

/// The two-line, wider variants a formula column switches to while the
/// ledger marks are on.
const FORMULA_HEAD: &str = "w-40 shrink-0 px-3 py-2 leading-tight";
const FORMULA_CELL: &str = "px-3 py-2 w-40 shrink-0 text-right";

/// The alternative-signal columns: two-line headers (sub-label + pill)
/// at the formula width, desktop only. `md:flex`, not `md:block`:
/// `SortableHeaderCell` appends `flex flex-col justify-center` for a
/// two-line header, and a later `md:block` would override it at md+.
const HEAD_40_MD: &str = "w-40 shrink-0 px-3 py-2 leading-tight hidden md:flex";
const CELL_40_MD: &str = "px-3 py-2 w-40 shrink-0 text-right hidden md:block";

/// The two lazy columns' headers: the grid draws these itself (they are
/// unsortable), so the class carries `flex flex-col` for the label and its
/// "7d · ‹sell world›" line. `md:flex`, never `md:block`, for the same
/// reason `HEAD_40_MD` is.
const HEAD_LAZY_MD: &str =
    "w-28 shrink-0 px-4 py-2 leading-tight hidden md:flex flex-col justify-center gap-0.5";
const HEAD_LAZY_MD_END: &str = "w-28 shrink-0 px-4 py-2 leading-tight hidden md:flex flex-col \
     justify-center gap-0.5 items-end";
/// A cell that centres a fixed-width graphic (the 80 px sparkline in a
/// `w-28` column, the same 16 px of padding either side as the numbers).
const CELL_28_MID_MD: &str = "px-4 py-2 w-28 shrink-0 hidden md:flex items-center justify-center";
/// A right-aligned numeric cell, as the 7-day Volume column already spells
/// it inline.
const CELL_28_NUM_MD: &str =
    "px-4 py-2 w-28 shrink-0 text-right hidden md:block font-mono tabular-nums";

/// Every field at its table-wide default, so each column below spells
/// out only what it actually differs in.
const RECIPE_BASE: ToolColumnMeta<RecipeRow, SortMode> = ToolColumnMeta {
    spec: &SPEC_ITEM,
    id: "",
    sort_id: "",
    sort: Sortability::No,
    default_dir: SortDir::Desc,
    header_class: "",
    cell_class: "",
    default_on: true,
    cell: cell_custom,
    side: None,
    formula_header_class: "",
    formula_cell_class: "",
    lab: None,
};

/// The recipe table, column by column, classes copied verbatim from the
/// markup this replaced. `id` = the `?cols=` token (always-on columns
/// have none); `sort_id` = the `?sort=` token.
static RECIPE_COLUMNS: [ToolColumnMeta<RecipeRow, SortMode>; 30] = [
    ToolColumnMeta {
        spec: &SPEC_ITEM,
        header_class: "w-64 md:w-80 shrink-0 p-4",
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_PROFIT,
        sort_id: "profit",
        sort: sortability_for(Layer::Computed, Some(SortMode::Profit)),
        header_class: HEAD,
        cell_class: CELL_R,
        // Custom, not `CellValue::Gil`: the marked cell carries the row's
        // arithmetic as a `title`, which no generic cell renders.
        cell: cell_custom,
        side: Some(TermRole::Result),
        formula_header_class: FORMULA_HEAD,
        formula_cell_class: FORMULA_CELL,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_ROI,
        sort_id: "roi",
        sort: sortability_for(Layer::Computed, Some(SortMode::Roi)),
        header_class: HEAD,
        cell_class: CELL_R,
        cell: cell_roi,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST,
        sort_id: "cost",
        sort: sortability_for(Layer::Computed, Some(SortMode::CostPerUnit)),
        default_dir: SortDir::Asc,
        header_class: HEAD,
        // A custom cell, but the class still comes from the table: the
        // grid hands it to the `custom` closure so a marked Cost cell
        // widens with its header.
        cell_class: CELL_R,
        side: Some(TermRole::Cost),
        formula_header_class: FORMULA_HEAD,
        formula_cell_class: FORMULA_CELL,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_PRICE,
        sort_id: "price",
        sort: sortability_for(Layer::RowLocal, Some(SortMode::Price)),
        header_class: HEAD,
        cell_class: CELL_R,
        cell: cell_price,
        side: Some(TermRole::Revenue),
        formula_header_class: FORMULA_HEAD,
        formula_cell_class: FORMULA_CELL,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_DAILY,
        sort_id: "velocity",
        sort: sortability_for(Layer::Bulk, Some(SortMode::Velocity)),
        header_class: HEAD_MD,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_AVG,
        sort_id: "avg-price",
        sort: sortability_for(Layer::Bulk, Some(SortMode::AvgPrice)),
        header_class: HEAD_MD,
        cell_class: CELL_R_MD,
        cell: cell_avg,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_CONFIDENCE,
        id: COL_CONFIDENCE,
        sort_id: "confidence",
        sort: sortability_for(Layer::Bulk, Some(SortMode::Confidence)),
        header_class: HEAD_28_MD,
        cell_class: "px-4 py-2 w-28 shrink-0 flex items-center justify-end hidden md:flex",
        cell: cell_confidence,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_LAST_SOLD,
        id: COL_LAST_SOLD,
        sort_id: "last-sold",
        sort: sortability_for(Layer::Bulk, Some(SortMode::LastSold)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_MD,
        default_on: false,
        cell: cell_last_sold,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_VOLUME,
        id: COL_VOLUME,
        sort_id: "volume",
        sort: sortability_for(Layer::Bulk, Some(SortMode::Volume)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_NUM_MD,
        default_on: false,
        cell: cell_volume,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_VWAP,
        id: COL_VWAP,
        sort_id: "vwap",
        sort: sortability_for(Layer::Bulk, Some(SortMode::Vwap)),
        header_class: HEAD_MD,
        cell_class: CELL_R_MD,
        default_on: false,
        cell: cell_vwap,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_TAX,
        id: COL_TAX,
        sort_id: "tax",
        sort: sortability_for(Layer::Computed, Some(SortMode::Tax)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_MD,
        default_on: false,
        cell: cell_tax,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_WORLD,
        id: COL_LISTING_WORLD,
        header_class: HEAD_28_MD,
        default_on: false,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_DC,
        id: COL_LISTING_DC,
        header_class: HEAD_28_MD,
        default_on: false,
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_REV_LISTING_MIN,
        id: COL_REV_LISTING_MIN,
        sort_id: COL_REV_LISTING_MIN,
        sort: sortability_for(
            Layer::RowLocal,
            Some(SortMode::RevSignal(PriceSignal::ListingMin)),
        ),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_listing_min,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_REV_SALE_MIN,
        id: COL_REV_SALE_MIN,
        sort_id: COL_REV_SALE_MIN,
        sort: sortability_for(Layer::Bulk, Some(SortMode::RevSignal(PriceSignal::SaleMin))),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_sale_min,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_REV_SALE_MEDIAN,
        id: COL_REV_SALE_MEDIAN,
        sort_id: COL_REV_SALE_MEDIAN,
        sort: sortability_for(
            Layer::Bulk,
            Some(SortMode::RevSignal(PriceSignal::SaleMedian)),
        ),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_sale_median,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_REV_SALE_AVG,
        id: COL_REV_SALE_AVG,
        sort_id: COL_REV_SALE_AVG,
        sort: sortability_for(Layer::Bulk, Some(SortMode::RevSignal(PriceSignal::SaleAvg))),
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_rev_sale_avg,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_LISTING_MIN,
        id: COL_COST_LISTING_MIN,
        sort_id: COL_COST_LISTING_MIN,
        sort: sortability_for(
            Layer::Computed,
            Some(SortMode::CostSignal(PriceSignal::ListingMin)),
        ),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_listing_min,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_SALE_MIN,
        id: COL_COST_SALE_MIN,
        sort_id: COL_COST_SALE_MIN,
        sort: sortability_for(
            Layer::Computed,
            Some(SortMode::CostSignal(PriceSignal::SaleMin)),
        ),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_sale_min,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_SALE_MEDIAN,
        id: COL_COST_SALE_MEDIAN,
        sort_id: COL_COST_SALE_MEDIAN,
        sort: sortability_for(
            Layer::Computed,
            Some(SortMode::CostSignal(PriceSignal::SaleMedian)),
        ),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_sale_median,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_COST_SALE_AVG,
        id: COL_COST_SALE_AVG,
        sort_id: COL_COST_SALE_AVG,
        sort: sortability_for(
            Layer::Computed,
            Some(SortMode::CostSignal(PriceSignal::SaleAvg)),
        ),
        default_dir: SortDir::Asc,
        header_class: HEAD_40_MD,
        cell_class: CELL_40_MD,
        default_on: false,
        cell: cell_cost_sale_avg,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_HOP_GAIN,
        id: COL_HOP_GAIN,
        sort_id: COL_HOP_GAIN,
        sort: sortability_for(Layer::Computed, Some(SortMode::HopGain)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_MD,
        default_on: false,
        cell: cell_hop_gain,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_HOP_WORLDS,
        id: COL_HOP_WORLDS,
        sort_id: COL_HOP_WORLDS,
        sort: sortability_for(Layer::Computed, Some(SortMode::HopWorlds)),
        default_dir: SortDir::Asc,
        header_class: HEAD_28_MD,
        // Custom: the tooltip needs the page's world names.
        cell_class: CELL_28_MD,
        default_on: false,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_PROFIT_PER_DAY,
        id: COL_PROFIT_PER_DAY,
        sort_id: COL_PROFIT_PER_DAY,
        sort: sortability_for(Layer::Computed, Some(SortMode::ProfitPerDay)),
        header_class: HEAD_MD,
        cell_class: CELL_R_MD,
        default_on: false,
        cell: cell_profit_per_day,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_TREND,
        id: COL_TREND,
        // Lazy: fetched per visible window, so it never sorts and carries
        // no `?sort=` token.
        sort: sortability_for(Layer::Lazy(RECIPE_TREND_FEED), None),
        header_class: HEAD_LAZY_MD,
        cell_class: CELL_28_MID_MD,
        default_on: false,
        cell: cell_trend,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_DRIFT,
        id: COL_DRIFT,
        // The same feed, read as a first-to-last percent: one request
        // serves both columns.
        sort: sortability_for(Layer::Lazy(RECIPE_TREND_FEED), None),
        header_class: HEAD_LAZY_MD_END,
        cell_class: CELL_28_NUM_MD,
        default_on: false,
        cell: cell_drift,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_VOLUME_30D,
        id: COL_VOLUME_30D,
        sort_id: COL_VOLUME_30D,
        // Bulk: a whole-scope body, even though this one is fetched
        // client-side after the table (`needed.rs`'s SellWorldStats(30)).
        sort: sortability_for(Layer::Bulk, Some(SortMode::Volume30)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_NUM_MD,
        default_on: false,
        cell: cell_volume_30,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_VWAP_30D,
        id: COL_VWAP_30D,
        sort_id: COL_VWAP_30D,
        sort: sortability_for(Layer::Bulk, Some(SortMode::Vwap30)),
        header_class: HEAD_MD,
        cell_class: CELL_R_MD,
        default_on: false,
        cell: cell_vwap_30,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_ACTIONS,
        header_class: "w-20 shrink-0 p-4",
        ..RECIPE_BASE
    },
];
/// Rewrite pre-market-model query params (#1206 era) to their successors.
/// Returns `None` when nothing needs rewriting (the common case — avoids a
/// navigate loop). `scope` carried region|datacenter and became
/// `buy-scope`; `revenue=world-min` described what is now the default and
/// simply drops.
fn migrate_legacy_params(pairs: &[(String, String)]) -> Option<Vec<(String, String)>> {
    let legacy = pairs
        .iter()
        .any(|(k, v)| k == "scope" || (k == "revenue" && v == "world-min"));
    if !legacy {
        return None;
    }
    Some(
        pairs
            .iter()
            .filter(|(k, v)| !(k == "revenue" && v == "world-min"))
            .map(|(k, v)| {
                if k == "scope" {
                    (FILTER_BUY_SCOPE.to_string(), v.clone())
                } else {
                    (k.clone(), v.clone())
                }
            })
            .collect(),
    )
}

/// What the visible columns and the sort target ask of the pricing pass.
/// Visible cost columns come out in table order (the cap claims them in
/// that order).
fn signal_wants(visible: &HashSet<&'static str>, sort: Option<SortMode>) -> SignalWants {
    let visible_cost = RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty() && visible.contains(c.id))
        .filter_map(|c| match c.spec.kind {
            ColumnKind::CostSignal(s) => Some(s),
            _ => None,
        })
        .collect();
    let sort_cost = match sort {
        Some(SortMode::CostSignal(s)) => Some(s),
        _ => None,
    };
    SignalWants {
        visible_cost,
        sort_cost,
        hop: visible.contains(COL_HOP_GAIN) || sort == Some(SortMode::HopGain),
        worlds: visible.contains(COL_HOP_WORLDS) || sort == Some(SortMode::HopWorlds),
    }
}

/// The buy-scope sale-stats resource key: the scope name when the body is
/// needed, `None` (no fetch) otherwise.
fn buy_stats_scope_key(
    formula: &ProfitFormula,
    needs: &RecipeNeeds,
    scope_name: String,
) -> Option<String> {
    needed_bodies(formula, needs)
        .contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS))
        .then_some(scope_name)
}

/// Which formula side a header pill writes, and the signal it writes.
fn pill_param(kind: ColumnKind) -> Option<(TermRole, PriceSignal)> {
    match kind {
        ColumnKind::RevSignal(s) => Some((TermRole::Revenue, s)),
        ColumnKind::CostSignal(s) => Some((TermRole::Cost, s)),
        _ => None,
    }
}

/// `NeededSignals::capped` as the `[bool; 4]` the cell context carries.
fn capped_flags(capped: &BTreeSet<PriceSignal>) -> [bool; 4] {
    let mut flags = [false; 4];
    for s in capped {
        flags[s.index()] = true;
    }
    flags
}

/// The full picker label of a signal ("Sale median (7d)").
fn signal_label(i18n: I18nContext<Locale, I18nKeys>, s: PriceSignal) -> String {
    match s {
        PriceSignal::ListingMin => t_string!(i18n, price_basis_listing_min).to_string(),
        PriceSignal::SaleMin => t_string!(i18n, price_basis_sale_min).to_string(),
        PriceSignal::SaleMedian => t_string!(i18n, price_basis_sale_median).to_string(),
        PriceSignal::SaleAvg => t_string!(i18n, price_basis_sale_avg).to_string(),
    }
}

/// The one-sentence definition of a signal, for header titles.
fn signal_help(i18n: I18nContext<Locale, I18nKeys>, s: PriceSignal) -> String {
    match s {
        PriceSignal::ListingMin => t_string!(i18n, price_basis_listing_min_help).to_string(),
        PriceSignal::SaleMin => t_string!(i18n, price_basis_sale_min_help).to_string(),
        PriceSignal::SaleMedian => t_string!(i18n, price_basis_sale_median_help).to_string(),
        PriceSignal::SaleAvg => t_string!(i18n, price_basis_sale_avg_help).to_string(),
    }
}

/// One Worlds-to-visit line: (world id, (world name, datacenter) when
/// known, ingredient lines priced there). An alias, or the tuple trips
/// `clippy::type_complexity`.
type WorldLine = (i32, Option<(String, String)>, u16);

/// The Worlds-to-visit tooltip: "• world · ingredients: n" lines grouped
/// by datacenter in first-appearance order (a `Vec`, never a map), then the
/// datacenter count and the buy-side note. An unknown world shows its id.
/// The bullet lives in the locale string, as the sub-craft tooltip's does.
fn worlds_tooltip(i18n: I18nContext<Locale, I18nKeys>, entries: &[WorldLine], dcs: u8) -> String {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for (id, names, n) in entries {
        let (world, dc) = match names {
            Some((w, d)) => (w.clone(), d.clone()),
            None => (id.to_string(), String::new()),
        };
        let line = t_string!(i18n, analyzer_hop_worlds_row, world = world, n = *n).to_string();
        match groups.iter_mut().find(|(g, _)| *g == dc) {
            Some((_, lines)) => lines.push(line),
            None => groups.push((dc, vec![line])),
        }
    }
    let mut out = String::new();
    for (dc, lines) in groups {
        if !dc.is_empty() {
            out.push_str(&dc);
            out.push('\n');
        }
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str(&t_string!(i18n, analyzer_hop_worlds_dcs, n = dcs).to_string());
    out.push('\n');
    // A plain-key `t_string!` is already a `&'static str`.
    out.push_str(t_string!(i18n, analyzer_hop_worlds_note));
    out
}

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
    LastSold,
    Volume,
    Vwap,
    Tax,
    Confidence,
    /// An alternative revenue column (`rev-‹token›`).
    RevSignal(PriceSignal),
    /// An alternative cost column (`cost-‹token›`).
    CostSignal(PriceSignal),
    HopGain,
    HopWorlds,
    /// Profit times the 7-day rollup rate. Computed per comparison; there
    /// is no row field to keep in sync.
    ProfitPerDay,
    /// Units sold over 30 days, from the client-only body.
    Volume30,
    /// The 30-day volume-weighted average price, from the same body.
    Vwap30,
}

impl SortMode {
    /// Sorts that exist only under the signal-columns lab. With the lab
    /// off the page treats them as unset, as it did before they existed.
    fn lab_only(self) -> bool {
        matches!(
            self,
            SortMode::RevSignal(_)
                | SortMode::CostSignal(_)
                | SortMode::HopGain
                | SortMode::HopWorlds
                | SortMode::ProfitPerDay
                | SortMode::Volume30
                | SortMode::Vwap30
        )
    }
}

impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        sort_from_token(&RECIPE_COLUMNS, s).ok_or(())
    }
}

impl Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Every variant is catalogued exactly once (pinned by test); the
        // fallback token only guards against a future variant added to the
        // enum before the table.
        f.write_str(sort_token(&RECIPE_COLUMNS, *self).unwrap_or("profit"))
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }

    /// Cost per unit reads best-first ascending — the cheapest craft is the
    /// interesting one. Everything else is a biggest-first metric; both come
    /// from the column table's `default_dir`.
    fn default_dir(self) -> SortDir {
        default_dir_for(&RECIPE_COLUMNS, self)
    }
}

fn hop_sort_key(hop: Option<HopGain>) -> Option<i32> {
    match hop {
        Some(HopGain::Gain(g)) => Some(g),
        _ => None,
    }
}

/// A row's 30-day statistics, when that body has landed. Keyed on the same
/// quality the row's 7-day figures came from.
fn stat_30<'a>(index: Option<&'a StatsIndex>, r: &RecipeProfitData) -> Option<&'a ItemSaleStats> {
    stat_row_either(index?, r.recipe.item_result, r.stat_hq)
}

/// The mode the rows are actually sorted by. The 30-day body is client-only
/// and lands after the first paint; sorting the whole table by "nothing
/// yet" would leave it in key order and then shuffle it, so until the body
/// arrives *with rows* a 30-day sort reads as Profit. "With rows", not
/// merely "present": a failed fetch and a world with no 30-day history both
/// store an empty index, which sorts no better than nothing. The header
/// still shows what was asked for, and the table re-sorts itself the moment
/// real rows land.
fn effective_sort_mode(mode: SortMode, stats_30_loaded: bool) -> SortMode {
    match mode {
        SortMode::Volume30 | SortMode::Vwap30 if !stats_30_loaded => SortMode::Profit,
        other => other,
    }
}

/// The ordering for `mode` with `dir` already applied. The plain modes
/// flip whole; the alternative-signal and hop modes flip only between two
/// present values (`cmp_none_last`), so "—" / "needed" rows stay last
/// whichever way the header points.
fn compare_recipes(
    mode: SortMode,
    dir: SortDir,
    a: &RecipeProfitData,
    b: &RecipeProfitData,
    stats_30: Option<&StatsIndex>,
) -> Ordering {
    let oriented = |o: Ordering| match dir {
        SortDir::Asc => o,
        SortDir::Desc => o.reverse(),
    };
    match mode {
        SortMode::Roi => oriented(a.return_on_investment.cmp(&b.return_on_investment)),
        SortMode::Profit => oriented(a.profit.cmp(&b.profit)),
        SortMode::Velocity => oriented(
            a.daily_sales
                .partial_cmp(&b.daily_sales)
                .unwrap_or(Ordering::Equal),
        ),
        SortMode::CostPerUnit => oriented(a.cost.cmp(&b.cost)),
        SortMode::Price => oriented(a.market_price.cmp(&b.market_price)),
        SortMode::AvgPrice => oriented(a.avg_price.cmp(&b.avg_price)),
        // Desc (the default) = most recent first: larger unix is newer.
        SortMode::LastSold => oriented(a.last_sold_unix.cmp(&b.last_sold_unix)),
        SortMode::Volume => oriented(a.units_sold.cmp(&b.units_sold)),
        SortMode::Vwap => oriented(a.vwap.cmp(&b.vwap)),
        SortMode::Tax => oriented(a.tax.cmp(&b.tax)),
        SortMode::Confidence => {
            oriented(confidence_rank(a.confidence).cmp(&confidence_rank(b.confidence)))
        }
        SortMode::RevSignal(s) => {
            cmp_none_last(a.rev_alt[s.index()], b.rev_alt[s.index()], dir, i32::cmp)
        }
        SortMode::CostSignal(s) => {
            cmp_none_last(a.cost_alt[s.index()], b.cost_alt[s.index()], dir, i32::cmp)
        }
        SortMode::HopGain => cmp_none_last(hop_sort_key(a.hop), hop_sort_key(b.hop), dir, i32::cmp),
        SortMode::HopWorlds => cmp_none_last(
            a.worlds.as_ref().map(|w| w.worlds.len()),
            b.worlds.as_ref().map(|w| w.worlds.len()),
            dir,
            usize::cmp,
        ),
        SortMode::ProfitPerDay => oriented(
            profit_per_day_from_rate(a.profit, a.daily_sales)
                .cmp(&profit_per_day_from_rate(b.profit, b.daily_sales)),
        ),
        SortMode::Volume30 => cmp_none_last(
            stat_30(stats_30, a).map(|s| s.units_sold),
            stat_30(stats_30, b).map(|s| s.units_sold),
            dir,
            u64::cmp,
        ),
        SortMode::Vwap30 => cmp_none_last(
            stat_30(stats_30, a).map(|s| s.vwap).filter(|v| *v > 0),
            stat_30(stats_30, b).map(|s| s.vwap).filter(|v| *v > 0),
            dir,
            i32::cmp,
        ),
    }
}

/// Everything the pricing pass reads, snapshotted out of the reactive
/// graph so the pass is a plain function (and unit-testable).
struct PriceInputs<'a> {
    recipes: &'a [&'static Recipe],
    recipe_level_tables: &'static HashMap<RecipeLevelTableId, xiv_gen::RecipeLevelTable>,
    recipes_by_output: &'a HashMap<ItemId, Vec<&'static Recipe>>,
    /// Buy-scope listings.
    buy_listings: &'a CheapestListingsMap,
    /// Sell-world listings (absent before a world resolves).
    sell_listings: Option<&'a CheapestListingsMap>,
    /// Buy-scope sale stats, indexed. `None` when not fetched.
    buy_stats: Option<&'a StatsIndex>,
    /// Sell-world sale stats, indexed. Empty when not fetched.
    sell_stats: &'a StatsIndex,
    /// Raw recent sales by item (both qualities merged), for the outlier
    /// filter and the rollup failover.
    raw_sales: &'a HashMap<i32, Vec<&'a SaleData>>,
    formula: ProfitFormula,
    levels: &'a CrafterLevels,
    job_filter: Option<&'a str>,
    use_subcrafts: bool,
    require_hq: bool,
    filter_outliers: bool,
    shards: ShardsMode,
    /// The on-hand stockpile when the on-hand toggle is on.
    // TODO(follow-up): when `CraftOptions::active_craft_list` is set, fetch
    // the list resource and build a `ListOnHand` from its items instead of
    // this local stockpile. The type is in place; the async resource fetch
    // is the missing piece.
    on_hand: Option<&'a HashMap<i32, i32>>,
    /// Which cost signals to run per recipe, and whether hop / worlds are
    /// wanted. The selected signal is always in the set.
    needs: &'a NeededSignals,
    /// Whether the sell-world stats body was fetched: hop's home side
    /// prices from it under a sale cost signal, else from the listing.
    sell_stats_loaded: bool,
    /// The sell world's id (0 while unresolved) — the "home" that Worlds
    /// to visit excludes.
    home_world_id: i32,
    /// World id → datacenter name, for Worlds to visit.
    dc_of: &'a dyn Fn(i32) -> Option<&'a str>,
}

/// One priced row per craftable recipe with a sell price, under the
/// selected formula. Unprofitable rows are dropped here (the formula's
/// drop rule); thresholds and sorting happen in [`filter_and_sort`].
fn price_rows(inp: &PriceInputs<'_>) -> (Vec<RecipeProfitData>, u32) {
    let mut results = Vec::new();

    // If no levels set, return empty (but we'll show a message)
    if !has_any_level(inp.levels) {
        return (results, 0);
    }

    let runs_done = std::cell::Cell::new(0u32);
    let selected = inp.formula.cost_signal();
    let scope_is_home = inp.formula.buy_scope() == BuyScope::World;
    // A buy-scope view under `signal`: the listing, or the stat over it.
    // Same two layers the cloned `override_listings` / `overlay_sale_stats`
    // maps used to build, now evaluated per lookup.
    let scope_view = |signal: PriceSignal| SignalView {
        over: None,
        base: inp.buy_listings,
        stats: signal
            .sale_stat()
            .and_then(|stat| inp.buy_stats.map(|idx| (idx, stat))),
    };
    let ingredient_view = scope_view(selected);
    let revenue_view = SignalView {
        over: inp.sell_listings,
        base: inp.buy_listings,
        stats: inp
            .formula
            .revenue_signal()
            .sale_stat()
            .map(|stat| (inp.sell_stats, stat)),
    };
    // Hop's home side: the sell world alone (deliberately not layered over
    // the buy scope, or an ingredient with no home listing would be priced
    // at the scope price and zero the gain for exactly the ingredients
    // that force the trip), under the selected cost signal when its
    // sell-world body is here, else the listing pass on both sides.
    let hop_signal = if inp.sell_stats_loaded {
        selected
    } else {
        PriceSignal::ListingMin
    };
    let home_view = inp.sell_listings.map(|sell| SignalView {
        over: None,
        base: sell,
        stats: hop_signal.sale_stat().map(|stat| (inp.sell_stats, stat)),
    });

    for recipe in inp.recipes.iter().copied() {
        // Filter by job and level
        let required_level = inp
            .recipe_level_tables
            .get(&RecipeLevelTableId(recipe.recipe_level_table))
            .map(|t| t.class_job_level as i32)
            .unwrap_or(0);

        let job_code = craft_type_acronym(recipe.craft_type);
        let user_level = level_for_job_code(inp.levels, job_code).unwrap_or(0);

        if let Some(filter) = inp.job_filter
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

        let sales_stats = if inp.filter_outliers {
            inp.raw_sales
                .get(&recipe.item_result)
                .map(|sales| analyze_sales(sales, true))
        } else {
            sales_stats_from_rollup(inp.sell_stats, recipe.item_result).or_else(|| {
                inp.raw_sales
                    .get(&recipe.item_result)
                    .map(|sales| analyze_sales(sales, false))
            })
        }
        .unwrap_or(SalesStats {
            daily_sales: 0.0,
            avg_price: 0,
            total_sales: 0,
        });

        let market_price = revenue_view
            .find_matching_listings(recipe.item_result)
            .lowest_gil()
            .unwrap_or(0);

        if market_price == 0 {
            continue;
        }

        // Deliberately the un-overlaid buy-scope listings, not the priced
        // view: `cheapest_world_id` must keep meaning "where the
        // scope-cheapest listing sits" regardless of which pricing bases
        // are selected.
        let scope_summary = inp.buy_listings.find_matching_listings(recipe.item_result);
        let cheapest_world_id = scope_summary
            .lq
            .map(|d| d.world_id)
            .or(scope_summary.hq.map(|d| d.world_id))
            .unwrap_or(0);

        // One `compute_cost` under `view`, over a fresh on-hand snapshot:
        // compute_cost consumes from the snapshot, and reusing one across
        // recipes (or across runs of one recipe) would wrongly deplete the
        // user's stockpile. `runs_done` feeds the debug timing log.
        let cost_run = |view: &SignalView<'_>| -> CostBreakdown {
            runs_done.set(runs_done.get() + 1);
            let active: Box<dyn OnHand> = match inp.on_hand {
                Some(map) => Box::new(LocalOnHand::from_map(map.clone())),
                None => Box::new(EmptyOnHand),
            };
            let opts = CraftingCostOptions {
                require_hq: inp.require_hq,
                max_subcraft_depth: if inp.use_subcrafts { 2 } else { 0 },
                shards: inp.shards,
                on_hand: active.as_ref(),
                vendor_prices: Some(vendor_price_map()),
            };
            compute_cost(recipe, view, inp.recipes_by_output, &opts, &is_shard_item)
        };
        let breakdown = cost_run(&ingredient_view);

        // `breakdown.cost` is the cost of one execution of the recipe, which
        // yields `amount_result` units; the market price is per unit, so
        // compare per unit.
        let cost_per_unit = per_unit_cost(breakdown.cost, recipe.amount_result);

        let (line, dropped) = profit_line(market_price, cost_per_unit, &inp.formula);
        if dropped {
            continue;
        }

        // Alternative cost runs, for kept rows only: the drop rule, ROI and
        // the row set are the selected pair's alone. A sale signal whose
        // buy-scope body is absent is not run — its cell shows "—" rather
        // than a listing number under a sale heading.
        let mut runs: [Option<CostBreakdown>; 4] = [None, None, None, None];
        for s in &inp.needs.cost {
            if *s == selected || (s.sale_stat().is_some() && inp.buy_stats.is_none()) {
                continue;
            }
            runs[s.index()] = Some(cost_run(&scope_view(*s)));
        }
        let run_for = |s: PriceSignal| -> Option<&CostBreakdown> {
            if s == selected {
                Some(&breakdown)
            } else {
                runs[s.index()].as_ref()
            }
        };
        let mut cost_alt = [None; 4];
        for s in PriceSignal::ALL {
            cost_alt[s.index()] = run_for(s).map(|b| per_unit_cost(b.cost, recipe.amount_result));
        }

        let hop = match (&home_view, inp.needs.hop) {
            // Buy from = This world only: no trip to price.
            (Some(_), true) if scope_is_home => Some(HopGain::Unavailable),
            (Some(home), true) => {
                let home_run = cost_run(home);
                let owned;
                // Reachable when a sale cost signal is selected but the
                // sell-world body failed: hop degrades to the listing
                // pass, which is not otherwise in the run set.
                let scope_run: &CostBreakdown = match run_for(hop_signal) {
                    Some(b) => b,
                    None => {
                        owned = cost_run(&scope_view(hop_signal));
                        &owned
                    }
                };
                Some(hop_gain(
                    &home_run,
                    scope_run,
                    recipe.amount_result,
                    scope_is_home,
                ))
            }
            _ => None,
        };
        // Worlds to visit reads the listing-min scope run whatever the
        // selected signal (`needed_signals` puts ListingMin in the set).
        let worlds = (inp.needs.worlds && !scope_is_home).then(|| {
            let owned;
            // Unreachable via needed_signals (it claims ListingMin first
            // whenever Worlds is wanted); kept for a hand-built NeededSignals.
            let listing_run: &CostBreakdown = match run_for(PriceSignal::ListingMin) {
                Some(b) => b,
                None => {
                    owned = cost_run(&scope_view(PriceSignal::ListingMin));
                    &owned
                }
            };
            worlds_to_visit(listing_run, inp.home_world_id, inp.dc_of)
        });

        // The bare sell-world number per revenue signal: the listing with
        // no buy-scope fallback, or the stat with no listing fallback.
        let item = recipe.item_result;
        let rev_alt = [
            inp.sell_listings
                .and_then(|s| s.find_matching_listings(item).lowest_gil())
                .filter(|p| *p > 0),
            stat_only_cheapest(inp.sell_stats, item, SaleStat::Min),
            stat_only_cheapest(inp.sell_stats, item, SaleStat::Median),
            stat_only_cheapest(inp.sell_stats, item, SaleStat::Avg),
        ];
        let revenue_fell_back = rev_alt[inp.formula.revenue_signal().index()] != Some(market_price);

        // Sell-world stats row matching how revenue resolves: prefer
        // the HQ row when the analyzer requires HQ, otherwise NQ, and
        // fall back to whichever quality actually traded.
        let sell_stat = stat_row_either(inp.sell_stats, recipe.item_result, inp.require_hq);
        let stat_hq = sell_stat.map(|s| s.hq).unwrap_or(inp.require_hq);
        let vwap = sell_stat.map(|s| s.vwap).unwrap_or(0);

        results.push(RecipeProfitData {
            recipe,
            profit: line.profit,
            return_on_investment: line.roi,
            cost: line.cost,
            market_price: line.revenue,
            cheapest_world_id,
            sub_crafts: breakdown.sub_crafts,
            daily_sales: sales_stats.daily_sales,
            avg_price: sales_stats.avg_price,
            total_sales: sales_stats.total_sales,
            required_level,
            last_sold_unix: sell_stat.map(|s| s.last_sold_unix).unwrap_or(0),
            units_sold: sell_stat.map(|s| s.units_sold).unwrap_or(0),
            vwap,
            vwap_pct: vwap_pct(market_price, vwap),
            tax: line.tax,
            confidence: sell_stat.map(|s| s.confidence).unwrap_or_default(),
            stat_hq,
            cost_alt,
            rev_alt,
            revenue_fell_back,
            unpriced: breakdown.unpriced_market_lines,
            hop,
            worlds,
        });
    }

    (results, runs_done.get())
}

/// The user's row filters. `None` = not set.
#[derive(Clone, Debug, PartialEq, Default)]
struct Thresholds {
    min_profit: Option<i32>,
    min_roi: Option<i32>,
    min_daily_sales: Option<f32>,
    listing_world: Option<String>,
    listing_dc: Option<String>,
}

/// Apply the thresholds and sort. Pure, so a header click never re-prices
/// by itself (a lab sort whose signal the pass has not run changes
/// `needs`, which does).
fn filter_and_sort(
    rows: &[Arc<RecipeProfitData>],
    t: &Thresholds,
    world_names: &HashMap<i32, (String, String)>,
    mode: SortMode,
    dir: SortDir,
    stats_30: Option<&StatsIndex>,
) -> Vec<(usize, Arc<RecipeProfitData>)> {
    // A failed fetch stores an *empty* index (Task 8) so the cells settle to
    // "—" rather than shimmering; for sorting that is still "not landed",
    // because every key would compare `None` against `None`, leave the
    // key-id tiebreak in charge, and put the table in recipe-id order.
    let stats_30 = stats_30.filter(|i| !i.is_empty());
    let mode = effective_sort_mode(mode, stats_30.is_some());
    let mut kept: Vec<Arc<RecipeProfitData>> = rows
        .iter()
        .filter(|d| t.min_profit.is_none_or(|min| d.profit >= min))
        .filter(|d| t.min_roi.is_none_or(|min| d.return_on_investment >= min))
        .filter(|d| t.min_daily_sales.is_none_or(|min| d.daily_sales >= min))
        .filter(|d| {
            if t.listing_world.is_none() && t.listing_dc.is_none() {
                return true;
            }
            listing_location_passes(
                world_names.get(&d.cheapest_world_id),
                t.listing_world.as_deref(),
                t.listing_dc.as_deref(),
            )
        })
        .cloned()
        .collect();
    // The table is virtualized, so retaining the full result set adds
    // browser-side rows without increasing DOM size or server work.
    kept.sort_by(|a, b| {
        // Deterministic tiebreak: the input comes from a std HashMap, so
        // without it ties could order differently on the server and the
        // client and mismatch the SSR-rendered rows.
        compare_recipes(mode, dir, a, b, stats_30)
            .then_with(|| a.recipe.key_id.0.cmp(&b.recipe.key_id.0))
    });
    kept.into_iter().enumerate().collect()
}

/// One sell-world history payload: the 7-day rollup plus, only when that
/// rollup failed, the raw recent sales as a failover. Keyed on the world
/// alone, so the opt-in outlier filter never re-requests the rollup — the
/// on-demand raw body is [`raw_sales_key`]'s separate resource.
// `ArcResource` values round-trip through `JsonSerdeCodec`, so serde is
// required (both field types already derive it).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct SellHistory {
    stats: Option<BulkSaleStats>,
    raw: Option<RecentSales>,
    stats_failed: bool,
    raw_failed: bool,
}

/// The raw-sales resource key. Deliberately built from URL state only — the
/// old key read the rollup resource inside a memo, which Leptos flags at
/// hydration (#1248 follow-up). The rollup's own failover is decided inside
/// [`fetch_sell_history`], off the reactive graph entirely.
fn raw_sales_key(world: Option<&str>, outliers: bool) -> Option<(String, bool)> {
    world.map(|w| (w.to_string(), outliers))
}

async fn fetch_sell_history(world: String) -> SellHistory {
    // The raw sales are only a failover here: if the rollup request fails,
    // fetch them so the analyzer stays useful while ClickHouse recovers.
    let stats = get_sale_stats(&world, SALE_STATS_WINDOW_DAYS).await;
    let raw = if stats.is_err() {
        Some(get_recent_sales_for_world(&world).await)
    } else {
        None
    };
    SellHistory {
        stats_failed: stats.is_err(),
        stats: stats.ok(),
        raw_failed: matches!(raw, Some(Err(_))),
        raw: raw.and_then(|r| r.ok()),
    }
}

#[component]
fn RecipeAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    recent_sales: Option<RecentSales>,
    /// Bulk sale statistics for the buy scope (cost bases); `None` while
    /// not requested (listing basis) or when the fetch failed.
    sale_stats: Option<BulkSaleStats>,
    /// Bulk sale statistics for the sell world (sale-stat revenue
    /// metrics); `None` while not requested or failed.
    sell_world_sale_stats: Option<BulkSaleStats>,
    /// True when a sale-stat cost basis is selected but the buy-scope
    /// stats fetch failed — the table silently degrades to the listing
    /// basis, so say so.
    buy_stats_error: bool,
    /// The same, for the sell world's sale history.
    sell_stats_error: bool,
    /// Cheapest listings on the analyzer's sell world. Revenue is always
    /// that world's price; absent only before a world resolves.
    sell_world_listings: Option<CheapestListings>,

    world: Signal<String>,
    /// Visible optional columns (`?cols=`), owned by the parent because the
    /// table remounts whenever its resources change.
    visible_cols: Memo<HashSet<&'static str>>,
    set_cols_param: SignalSetter<Option<String>>,
    /// Current `?sort=`, owned by the parent for the same remount reason.
    sort_mode: Memo<Option<SortMode>>,
    /// Current `?dir=`, owned by the parent for the same remount reason.
    sort_dir: Memo<Option<SortDir>>,
    /// `(buy, sell)` stats-*loaded* flags: the very pair this table's
    /// `formula` memo hands `ProfitFormula::effective`. Written here —
    /// this is where the resource outcomes are known — and read by the
    /// page's strip chips and info-panel sentence, so all three describe
    /// the same fallback the rows were priced with.
    stats_loaded: RwSignal<(bool, bool)>,
    /// The sell world's name, for the Price/Profit marks.
    #[prop(into)]
    sell_place: Signal<String>,
    /// The buy scope's name, for the Cost mark.
    #[prop(into)]
    buy_place: Signal<String>,
    /// The ledger chips, built on the page (the popover that renders them
    /// lives inside this table's `ControlBar`).
    strip_terms: Callback<(), Vec<StripTerm>>,
    /// The `analyzer-recipe` Labs toggle: the formula strip and marks, the
    /// clamped ROI, the profit readout, the alternative columns and pills,
    /// the grouped picker, the Price tell, the "n unpriced" note and the
    /// market columns all hang off this one flag. A plain bool: the page
    /// reads the lab inside its Suspense join, so a flip remounts this
    /// table (the grid's header is built once per mount).
    preview: bool,
    /// The cost signals to run per recipe and the hop flags (page-level,
    /// because the fetch gate reads the same value).
    needs: Memo<NeededSignals>,
    /// The buy scope IS the sell world: reuse its stats index as the
    /// buy-scope index instead of a second identical body.
    buy_stats_aliased: bool,
    #[prop(into)] home_world_id: Signal<i32>,
    on_pill: Callback<ColumnKind>,
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
    let prices = Arc::new(CheapestListingsMap::from(global_cheapest_listings));
    // An absent payload behaves as "no sales anywhere": every sale-stat
    // basis degrades to the listing basis (`ProfitFormula::effective`).
    let sell_stats_loaded = sell_world_sale_stats.is_some();
    // Aliased = the sell body IS the buy body, so its outcome is the buy
    // outcome: a failed sell fetch degrades the cost signal too, and
    // `effective()` must see that (labels never name a signal the numbers
    // fell back from).
    let buy_stats_loaded = sale_stats.is_some() || (buy_stats_aliased && sell_stats_loaded);
    let sale_stats = sale_stats.unwrap_or_default();
    let sell_world_sale_stats = sell_world_sale_stats.unwrap_or_default();
    let sell_world_prices = sell_world_listings.map(|l| Arc::new(CheapestListingsMap::from(l)));
    let data = tracked_data();
    let items = &data.items;
    let recipes = &data.recipes;
    let recipe_level_tables = &data.recipe_level_tables;
    let i18n = use_i18n();

    // The table is the only place that knows how each stats body actually
    // resolved; publish the loaded pair once so the page's strip and info
    // panel derive the fallback from the same two booleans the rows did.
    Effect::new(move |_| stats_loaded.set((buy_stats_loaded, sell_stats_loaded)));

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

    // Filter params use `filter_query_signal` (replace: true, scroll: false):
    // editing a chip writes the URL on every keystroke, and plain
    // `query_signal`'s defaults would push a history entry and yank the
    // window to the top each time.
    let (minimum_profit, set_minimum_profit) = filter_query_signal::<i32>(FILTER_PROFIT);
    let (minimum_roi, set_minimum_roi) = filter_query_signal::<i32>(FILTER_ROI);
    let (job_filter, set_job_filter) = filter_query_signal::<String>(FILTER_JOB);
    let (use_subcrafts, set_use_subcrafts) = filter_query_signal::<bool>(FILTER_SUBCRAFTS);
    // Seeded by RecipeAnalyzer so a first-time visitor isn't shown recipes
    // whose output sells once a month. Same velocity floor as the analyzer's
    // 1d default.
    let (min_daily_sales, set_min_daily_sales) = filter_query_signal::<f32>(FILTER_MIN_SALES);
    let (require_hq, set_require_hq) = filter_query_signal::<bool>(FILTER_REQUIRE_HQ);
    let (filter_outliers, set_filter_outliers) = filter_query_signal::<bool>(FILTER_OUTLIERS);
    let (exclude_shards_url, set_exclude_shards) =
        filter_query_signal::<bool>(FILTER_EXCLUDE_SHARDS);
    let (use_on_hand_url, set_use_on_hand) = filter_query_signal::<bool>(FILTER_USE_ON_HAND);
    let (cost_basis, set_cost_basis) = filter_query_signal::<CostBasis>(FILTER_COST_BASIS);
    let (revenue_metric, set_revenue_metric) = filter_query_signal::<RevenueMetric>(FILTER_REVENUE);
    let (buy_scope, set_buy_scope) = filter_query_signal::<BuyScope>(FILTER_BUY_SCOPE);
    let (listing_world_filter, set_listing_world_filter) =
        filter_query_signal::<String>(FILTER_LISTING_WORLD);
    let (listing_dc_filter, set_listing_dc_filter) =
        filter_query_signal::<String>(FILTER_LISTING_DC);

    // `cheapest_world_id` -> (world name, datacenter name), for the
    // cheapest-listing columns and their filters. World data is static for
    // the session, so this is built once.
    let world_names: Arc<HashMap<i32, (String, String)>> = {
        let helper = use_context::<LocalWorldData>()
            .expect("Should always have local world data")
            .0
            .unwrap();
        Arc::new(
            helper
                .get_inner_data()
                .regions
                .iter()
                .flat_map(|r| r.datacenters.iter())
                .flat_map(|dc| {
                    dc.worlds
                        .iter()
                        .map(move |w| (w.id, (w.name.clone(), dc.name.clone())))
                })
                .collect(),
        )
    };

    // A filter picked from the `+ Filter` menu but not yet committed — its
    // chip mounts in edit state with an empty input (see currency_exchange.rs
    // for the same pattern). Only the three free-typed numeric filters use
    // this; selects and toggles commit a sensible value immediately.
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

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

    // Indexes are built once per payload, not once per recompute.
    let sell_stats_index: Arc<StatsIndex> = Arc::new(stats_index(&sell_world_sale_stats));
    let buy_stats_index: Option<Arc<StatsIndex>> = buy_stats_loaded.then(|| {
        if buy_stats_aliased {
            sell_stats_index.clone()
        } else {
            Arc::new(stats_index(&sale_stats))
        }
    });
    let all_recipes: Arc<Vec<&'static Recipe>> = Arc::new(recipes.values().collect());

    let formula = Memo::new(move |_| {
        let mut f = ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope())
            .effective(buy_stats_loaded, sell_stats_loaded);
        // The phase's one number change, and it only happens under the
        // lab: a 363,884% ROI off a single fake listing reads as noise, so
        // the clamped policy caps it at the display ceiling.
        if preview {
            f.roi = RoiMath::ClampedF64;
        }
        f
    });

    // Header marks come from the *effective* formula above, never from the
    // raw selection: a header must not name a signal the numbers fell back
    // from. `None` leaves every column exactly as it renders today.
    // A `Memo`, not a derived signal: the grid reads this once per formula
    // cell per row, and a derived signal would rebuild the label map (and
    // re-render every row on any unrelated query change) each time.
    let marks = Memo::new(move |_| {
        preview.then(|| {
            let f = formula.get();
            let m = f.marks(sell_place.get(), buy_place.get());
            mark_labels(
                &m,
                &short_signal(i18n, m.cost),
                &short_signal(i18n, m.revenue),
                t_string!(i18n, recipe_analyzer_profit_sub),
            )
        })
    });

    // Line 2 and titles for the alternative-signal and hop headers. The
    // "(= …)" mark follows the *effective* formula (what the numbers use);
    // the pill's pressed state follows the *selected* one (what pressing
    // it writes). Empty with the lab off: every header renders as before.
    let header_extras = Memo::new(move |_| {
        let mut by_kind = HashMap::new();
        if !preview {
            return HeaderExtras { by_kind };
        }
        let f = formula.get();
        let selected_cost = cost_basis().unwrap_or_default();
        let selected_revenue = revenue_metric().unwrap_or_default();
        for col in RECIPE_COLUMNS.iter() {
            let extra = match col.spec.kind {
                ColumnKind::RevSignal(s) => HeaderExtra {
                    title: signal_help(i18n, s),
                    line2: Some(HeaderLine2 {
                        sub_label: if s == f.revenue_signal() {
                            t_string!(i18n, analyzer_equals_price_slot).to_string()
                        } else {
                            format!("{} · {}", short_signal(i18n, s), sell_place.get())
                        },
                        pill: Some(HeaderPill {
                            aria: t_string!(
                                i18n,
                                analyzer_use_as_revenue_aria,
                                signal = signal_label(i18n, s)
                            )
                            .to_string(),
                            pressed: s == selected_revenue,
                        }),
                    }),
                    header_class: None,
                },
                ColumnKind::CostSignal(s) => HeaderExtra {
                    title: signal_help(i18n, s),
                    line2: Some(HeaderLine2 {
                        sub_label: if s == f.cost_signal() {
                            t_string!(i18n, analyzer_equals_cost_slot).to_string()
                        } else {
                            format!("{} · {}", short_signal(i18n, s), buy_place.get())
                        },
                        pill: Some(HeaderPill {
                            aria: t_string!(
                                i18n,
                                analyzer_use_as_cost_aria,
                                signal = signal_label(i18n, s)
                            )
                            .to_string(),
                            pressed: s == selected_cost,
                        }),
                    }),
                    header_class: None,
                },
                ColumnKind::HopGain => HeaderExtra {
                    title: t_string!(i18n, analyzer_hop_gain_help).to_string(),
                    line2: None,
                    header_class: None,
                },
                ColumnKind::HopWorlds => HeaderExtra {
                    title: t_string!(i18n, analyzer_hop_worlds_help).to_string(),
                    line2: None,
                    header_class: None,
                },
                _ => continue,
            };
            by_kind.insert(col.spec.kind, extra);
        }
        HeaderExtras { by_kind }
    });

    // The pricing pass. Rebuilt only when a pricing input changes — a
    // header click or a threshold edit re-runs `filter_and_sort` alone,
    // unless the new sort target adds a signal to `needs`.
    let on_hand_map = use_context::<OnHandMap>();
    let world_names_for_pricing = world_names.clone();
    let priced: Memo<Arc<Vec<Arc<RecipeProfitData>>>> = {
        let prices = prices.clone();
        let sell_world_prices = sell_world_prices.clone();
        let sell_stats_index = sell_stats_index.clone();
        let buy_stats_index = buy_stats_index.clone();
        let all_recipes = all_recipes.clone();
        Memo::new(move |_| {
            let raw_sales: HashMap<i32, Vec<&SaleData>> = recent_sales
                .as_ref()
                .map(|sales| {
                    let mut map: HashMap<i32, Vec<&SaleData>> = HashMap::new();
                    for sale in &sales.sales {
                        map.entry(sale.item_id).or_default().push(sale);
                    }
                    map
                })
                .unwrap_or_default();
            let levels = crafter_levels.get().unwrap_or_default();
            let job = job_filter();
            let on_hand = use_on_hand_enabled()
                .then(|| on_hand_map.map(|m| m.0.get_untracked()).unwrap_or_default());
            let recipes_by_output = recipes_by_output();
            let needs = needs.get();
            let dc_of = |id: i32| world_names_for_pricing.get(&id).map(|(_, dc)| dc.as_str());
            let inp = PriceInputs {
                recipes: &all_recipes,
                recipe_level_tables,
                recipes_by_output: &recipes_by_output,
                buy_listings: &prices,
                sell_listings: sell_world_prices.as_deref(),
                buy_stats: buy_stats_index.as_deref(),
                sell_stats: &sell_stats_index,
                raw_sales: &raw_sales,
                formula: formula(),
                levels: &levels,
                job_filter: job.as_deref(),
                use_subcrafts: use_subcrafts().unwrap_or(false),
                require_hq: require_hq().unwrap_or(false),
                filter_outliers: filter_outliers().unwrap_or(false),
                shards: if exclude_shards_enabled() {
                    ShardsMode::ExcludeShards
                } else {
                    ShardsMode::IncludeMarket
                },
                on_hand: on_hand.as_ref(),
                needs: &needs,
                sell_stats_loaded,
                home_world_id: home_world_id.get(),
                dc_of: &dc_of,
            };
            #[cfg(all(debug_assertions, feature = "hydrate"))]
            let t0 = js_sys::Date::now();
            let (rows, cost_runs) = price_rows(&inp);
            #[cfg(all(debug_assertions, feature = "hydrate"))]
            leptos::logging::log!(
                "price_rows: {} recipes priced in {:.1} ms ({} compute_cost calls, hop {})",
                rows.len(),
                js_sys::Date::now() - t0,
                cost_runs,
                inp.needs.hop
            );
            #[cfg(not(all(debug_assertions, feature = "hydrate")))]
            let _ = cost_runs;
            Arc::new(rows.into_iter().map(Arc::new).collect())
        })
    };

    let world_names_for_rows = world_names.clone();
    let computed_data = Memo::new(move |_| {
        let t = Thresholds {
            min_profit: minimum_profit(),
            min_roi: minimum_roi(),
            min_daily_sales: min_daily_sales(),
            listing_world: listing_world_filter(),
            listing_dc: listing_dc_filter(),
        };
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        // `None`: the 30-day body's signal is wired in Task 8. Until then
        // the two 30-day sorts fall back to Profit for everyone.
        filter_and_sort(&priced(), &t, &world_names_for_rows, mode, dir, None)
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

    // Filters currently drawn as a chip. Drives the "no active filters" hint
    // and keeps `+ Filter` from offering a second copy of something the user
    // can already see.
    let active_filters = Memo::new(move |_| {
        let mut active: Vec<&'static str> = Vec::new();
        if minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT) {
            active.push(FILTER_PROFIT);
        }
        if minimum_roi().is_some() || pending_filter.get() == Some(FILTER_ROI) {
            active.push(FILTER_ROI);
        }
        if min_daily_sales().is_some() || pending_filter.get() == Some(FILTER_MIN_SALES) {
            active.push(FILTER_MIN_SALES);
        }
        if job_filter().is_some() || pending_filter.get() == Some(FILTER_JOB) {
            active.push(FILTER_JOB);
        }
        if cost_basis().is_some() {
            active.push(FILTER_COST_BASIS);
        }
        if revenue_metric().is_some() {
            active.push(FILTER_REVENUE);
        }
        if buy_scope().is_some() {
            active.push(FILTER_BUY_SCOPE);
        }
        if listing_world_filter().is_some() {
            active.push(FILTER_LISTING_WORLD);
        }
        if listing_dc_filter().is_some() {
            active.push(FILTER_LISTING_DC);
        }
        if use_subcrafts().unwrap_or(false) {
            active.push(FILTER_SUBCRAFTS);
        }
        if require_hq().unwrap_or(false) {
            active.push(FILTER_REQUIRE_HQ);
        }
        if filter_outliers().unwrap_or(false) {
            active.push(FILTER_OUTLIERS);
        }
        // These two only show a chip once the URL explicitly overrides the
        // cookie default — otherwise the page is silently using the user's
        // saved crafting-cost preference, not filtering anything.
        if exclude_shards_url().is_some() {
            active.push(FILTER_EXCLUDE_SHARDS);
        }
        if use_on_hand_url().is_some() {
            active.push(FILTER_USE_ON_HAND);
        }
        active
    });

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, recipe_analyzer_filter_profit_min_label).to_string(),
            FILTER_ROI => t_string!(i18n, recipe_analyzer_filter_roi_min_label).to_string(),
            FILTER_MIN_SALES => {
                t_string!(i18n, recipe_analyzer_filter_daily_sales_min_label).to_string()
            }
            FILTER_JOB => t_string!(i18n, recipe_analyzer_filter_job_label).to_string(),
            FILTER_SUBCRAFTS => t_string!(i18n, recipe_analyzer_filter_subcrafts_label).to_string(),
            FILTER_REQUIRE_HQ => {
                t_string!(i18n, recipe_analyzer_filter_require_hq_label).to_string()
            }
            FILTER_OUTLIERS => t_string!(i18n, filter_outliers).to_string(),
            FILTER_EXCLUDE_SHARDS => {
                t_string!(i18n, recipe_analyzer_filter_exclude_shards_label).to_string()
            }
            FILTER_USE_ON_HAND => {
                t_string!(i18n, recipe_analyzer_filter_use_on_hand_label).to_string()
            }
            _ => String::new(),
        }
    };

    let job_chip_options = move || {
        JOB_CODES
            .iter()
            .map(|code| (*code, job_name(code)))
            .collect::<Vec<_>>()
    };
    let on_off_options = move || {
        vec![
            ("true", t_string!(i18n, toolbar_pill_on).to_string()),
            ("false", t_string!(i18n, toolbar_pill_off).to_string()),
        ]
    };

    // Optional-column picker, flip-finder style. Long labels for the picker
    // (recognition, not recall — same rationale as the filter menu), read
    // straight off the column table.
    let column_options = Signal::derive(move || {
        if preview {
            let f = formula.get();
            grouped_picker_options(
                &RECIPE_COLUMNS,
                i18n,
                &PickerContext {
                    sell_place: sell_place.get(),
                    buy_place: buy_place.get(),
                    revenue: f.revenue_signal(),
                    cost: f.cost_signal(),
                    capped: needs.get().capped,
                },
            )
        } else {
            picker_options(&RECIPE_COLUMNS, i18n)
        }
    });
    let toggle_column = Callback::new(move |col: &'static str| {
        let mut set = visible_cols.get_untracked();
        if !set.remove(col) {
            set.insert(col);
        }
        set_cols_param.set(Some(serialize_visible_cols(&set, &OPTIONAL_COLUMN_ORDER)));
    });
    let reset_columns = Callback::new(move |_| set_cols_param.set(None));

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

    // Adding a filter seeds it with a value the user can see and edit
    // straight away, rather than mounting a select with nothing chosen —
    // except `FILTER_JOB`, where "seeding" would mean silently narrowing the
    // whole table to one crafter before the user has picked anything (a
    // regression vs. the old "All Jobs" default). That one mounts blank via
    // `pending_filter`, same as the three free-typed numeric filters and
    // leve_analyzer's identical job filter. Every other select commits a
    // sensible non-default value immediately, same as the flip finder's
    // select-type filters.
    let add_filter = Callback::new(move |id: &'static str| match id {
        FILTER_PROFIT => pending_filter.set(Some(FILTER_PROFIT)),
        FILTER_ROI => pending_filter.set(Some(FILTER_ROI)),
        FILTER_MIN_SALES => pending_filter.set(Some(FILTER_MIN_SALES)),
        FILTER_JOB => pending_filter.set(Some(FILTER_JOB)),
        FILTER_SUBCRAFTS => set_use_subcrafts(Some(true)),
        FILTER_REQUIRE_HQ => set_require_hq(Some(true)),
        FILTER_OUTLIERS => set_filter_outliers(Some(true)),
        FILTER_EXCLUDE_SHARDS => set_exclude_shards(Some(true)),
        FILTER_USE_ON_HAND => set_use_on_hand(Some(true)),
        _ => {}
    });

    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        set_minimum_profit(None);
        set_minimum_roi(None);
        set_min_daily_sales(None);
        set_job_filter(None);
        set_cost_basis(None);
        set_revenue_metric(None);
        set_buy_scope(None);
        set_listing_world_filter(None);
        set_listing_dc_filter(None);
        set_use_subcrafts(None);
        set_require_hq(None);
        set_filter_outliers(None);
        set_exclude_shards(None);
        set_use_on_hand(None);
    });

    // The cells the grid hands back to the page: they need context the row
    // does not carry (item names and icons, the world link, the on-hand
    // list button). Every branch is the old cell's markup verbatim, keyed
    // by the column's kind.
    let world_names_for_cells = world_names.clone();
    let custom: CustomCell<RecipeRow> = Arc::new(move |data, kind, class| {
        let data = data.clone();
        let item_id = ItemId(data.recipe.item_result);
        match kind {
            ColumnKind::Item => {
                let item = items
                    .get(&item_id)
                    .map(|i| i.name.as_str())
                    .unwrap_or("Unknown");
                let item_level = items.get(&item_id).map(|i| i.level_item).unwrap_or(0);
                let job_abbrev = craft_type_acronym(data.recipe.craft_type);
                view! {
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
                                    {t_string!(i18n, recipe_analyzer_item_level_label, level = data.required_level, ilvl = item_level).to_string()}
                                    " " {job_abbrev}
                                </span>
                            </div>
                        </a>
                    </div>
                }
                .into_any()
            }
            // The ledger's `=` result term. The class is the grid's, so a
            // marked cell tracks its header's width; the readout spells
            // the row's own arithmetic out in a `title`.
            ColumnKind::Profit => {
                let readout = {
                    let data = data.clone();
                    move || {
                        preview.then(|| {
                            t_string!(
                                i18n,
                                recipe_analyzer_profit_readout,
                                price = data.market_price.separate_with_commas(),
                                tax = data.tax.separate_with_commas(),
                                cost = data.cost.separate_with_commas(),
                                profit = data.profit.separate_with_commas()
                            )
                            .to_string()
                        })
                    }
                };
                view! {
                    // `title` is an `Option`: with the lab off the cell
                    // carries no attribute at all.
                    <div role="cell" class=class title=readout>
                        <Gil amount=data.profit />
                    </div>
                }
                .into_any()
            }
            ColumnKind::CostSlot => {
                let yield_note = {
                    let data_for_yield = data.clone();
                    (data.recipe.amount_result > 1).then(|| view! {
                        <div class="text-xs text-[color:var(--color-text-muted)]">
                            {t!(i18n, recipe_analyzer_yield_note, n = move || data_for_yield.recipe.amount_result)}
                        </div>
                    })
                };
                let sub_badge = {
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
                                                let mut tooltip = t_string!(i18n, recipe_analyzer_subcraft_header).to_string();
                                                for (name, amount, cost) in &sub_crafts_details {
                                                    tooltip.push_str(
                                                        &t_string!(i18n, recipe_analyzer_subcraft_row, count = *amount, name = name.clone(), gil = *cost).to_string(),
                                                    );
                                                }
                                                tooltip
                                            })
                                        }
                                    >
                                        <div class="text-xs text-brand-300 flex items-center justify-end gap-1 cursor-help">
                                            <Icon icon=i::FaHammerSolid width="0.8em" height="0.8em" />
                                            <span>{count} " " {t!(i18n, recipe_analyzer_sub_suffix)}</span>
                                        </div>
                                    </Tooltip>
                                }
                            }
                        </Show>
                    }
                };
                if preview && data.unpriced > 0 {
                    let n = data.unpriced;
                    view! {
                        <div role="cell" class=class>
                            <Gil amount=data.cost />
                            {yield_note}
                            {sub_badge}
                            <div
                                class="text-[10px] leading-3 text-amber-300 cursor-help"
                                title=t_string!(i18n, analyzer_cost_unpriced_title, n = n).to_string()
                            >
                                {t_string!(i18n, analyzer_cost_unpriced, n = n).to_string()}
                            </div>
                        </div>
                    }
                    .into_any()
                } else {
                    view! {
                        <div role="cell" class=class>
                            <Gil amount=data.cost />
                            {yield_note}
                            {sub_badge}
                        </div>
                    }
                    .into_any()
                }
            }
            ColumnKind::SalesPerDay7 => {
                // Window length, approximated back out of the sample count
                // and the per-day rate.
                let sales_tooltip = t_string!(
                    i18n,
                    recipe_analyzer_sales_tooltip,
                    count = data.total_sales,
                    days = format!(
                        "{:.1}",
                        data.total_sales as f32 / data.daily_sales.max(0.001)
                    )
                )
                .to_string();
                let per_day = t_string!(
                    i18n,
                    recipe_analyzer_sales_per_day,
                    sales = format!("{:.1}", data.daily_sales)
                )
                .to_string();
                view! {
                    <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right hidden md:block">
                        <span class="text-xs text-[color:var(--color-text-muted)]" title=sales_tooltip>
                            {per_day}
                        </span>
                    </div>
                }
                .into_any()
            }
            // (world, datacenter) names of the cheapest listing; `None` for
            // the stat-overlay placeholder world 0.
            ColumnKind::ListingWorld => {
                let listing_location = world_names_for_cells.get(&data.cheapest_world_id).cloned();
                match listing_location {
                    Some((world, _)) => {
                        let tooltip = t_string!(i18n, analyzer_only_show_world)
                            .to_string()
                            .replace("%world%", &world);
                        let value = Signal::derive({
                            let world = world.clone();
                            move || world.clone()
                        });
                        view! {
                            <div role="cell" class="px-4 py-2 w-28 shrink-0 hidden md:flex items-center">
                                <Tooltip tooltip_text=Signal::derive(move || tooltip.clone())>
                                    <QueryButton
                                        key=FILTER_LISTING_WORLD
                                        value=value
                                        class="!text-brand-300 hover:text-brand-200 truncate"
                                        active_classes="!text-neutral-300 hover:text-neutral-200 truncate"
                                        remove_queries=&[FILTER_LISTING_DC]
                                    >
                                        {move || value.get()}
                                    </QueryButton>
                                </Tooltip>
                            </div>
                        }.into_any()
                    }
                    None => view! {
                        <div role="cell" class="px-4 py-2 w-28 shrink-0 hidden md:flex items-center text-[color:var(--color-text-muted)]">"—"</div>
                    }.into_any(),
                }
            }
            ColumnKind::ListingDc => {
                let listing_location = world_names_for_cells.get(&data.cheapest_world_id).cloned();
                match listing_location {
                    Some((_, dc)) => {
                        let tooltip = t_string!(i18n, analyzer_only_show_world)
                            .to_string()
                            .replace("%world%", &dc);
                        let value = Signal::derive({
                            let dc = dc.clone();
                            move || dc.clone()
                        });
                        view! {
                            <div role="cell" class="px-4 py-2 w-28 shrink-0 hidden md:flex items-center">
                                <Tooltip tooltip_text=Signal::derive(move || tooltip.clone())>
                                    <QueryButton
                                        key=FILTER_LISTING_DC
                                        value=value
                                        class="!text-brand-300 hover:text-brand-200 truncate"
                                        active_classes="!text-neutral-300 hover:text-neutral-200 truncate"
                                        remove_queries=&[FILTER_LISTING_WORLD]
                                    >
                                        {move || value.get()}
                                    </QueryButton>
                                </Tooltip>
                            </div>
                        }.into_any()
                    }
                    None => view! {
                        <div role="cell" class="px-4 py-2 w-28 shrink-0 hidden md:flex items-center text-[color:var(--color-text-muted)]">"—"</div>
                    }.into_any(),
                }
            }
            ColumnKind::HopWorlds => {
                let (count, tooltip) = match &data.worlds {
                    Some(w) => {
                        let entries: Vec<WorldLine> = w
                            .worlds
                            .iter()
                            .map(|(id, n)| (*id, world_names_for_cells.get(id).cloned(), *n))
                            .collect();
                        (Some(w.worlds.len()), worlds_tooltip(i18n, &entries, w.dcs))
                    }
                    None => (None, t_string!(i18n, analyzer_hop_worlds_note).to_string()),
                };
                let text = count
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "—".to_string());
                let muted = if count.is_some() {
                    ""
                } else {
                    "text-[color:var(--color-text-muted)]"
                };
                // `Tooltip`'s children are an `Fn` closure: clone, never move.
                view! {
                    <div role="cell" class=class>
                        <Tooltip tooltip_text=Signal::derive(move || tooltip.clone())>
                            <span class=muted>{text.clone()}</span>
                        </Tooltip>
                    </div>
                }
                .into_any()
            }
            ColumnKind::Actions => view! {
                <div role="cell" class="px-4 py-2 w-20 shrink-0">
                    <AddRecipeToList recipe=data.recipe />
                </div>
            }
            .into_any(),
            other => unreachable!("no custom cell for column {other:?}"),
        }
    });

    // Zebra striping, verbatim from the markup the grid replaced.
    fn stripe(index: usize) -> &'static str {
        if index.is_multiple_of(2) {
            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
        } else {
            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
        }
    }
    let cell_ctx = Signal::derive(move || CellCtx {
        now_unix: chrono::Utc::now().timestamp(),
        preview,
        // `with`, not `get`: this is read once per rendered row and `get`
        // would clone both sets each time.
        capped_cost: needs.with(|n| capped_flags(&n.capped)),
        sparklines: None,
        stats_30: None,
    });

    view! {
        <div class="flex flex-col gap-6">
            <ActiveListBanner />
            {(buy_stats_error || sell_stats_error)
                .then(|| view! {
                    <div class="text-amber-400 text-sm">
                        {t!(i18n, recipe_analyzer_sale_stats_unavailable)}
                    </div>
                })}
            // Primary filter bar
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, recipe_analyzer_result_count, n = move || computed_data().len())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! {
                        <RealtimeStatus status=realtime_status last_update=last_update />
                        <MarketMenu terms=strip_terms preview=preview />
                    }
                        .into_any()
                }
                columns=column_options
                visible_columns=Signal::derive(move || visible_cols.get())
                on_toggle_column=toggle_column
                on_reset_columns=reset_columns
                available_filters=Signal::derive(filter_options)
                on_add_filter=add_filter
                on_clear_all=clear_all
                empty_label=Signal::derive(move || {
                    t_string!(i18n, recipe_analyzer_no_filters_hint).to_string()
                })
                is_empty=Signal::derive(move || active_filters().is_empty())
            >
                {move || {
                    (minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_PROFIT);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_chip_profit_min).to_string()
                                    value=Signal::derive(move || minimum_profit().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    step="1000"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_minimum_profit(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_PROFIT) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    (minimum_roi().is_some() || pending_filter.get() == Some(FILTER_ROI))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_ROI);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_chip_roi_min).to_string()
                                    value=Signal::derive(move || minimum_roi().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    step="10"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_minimum_roi(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_ROI) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    (min_daily_sales().is_some() || pending_filter.get() == Some(FILTER_MIN_SALES))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked()
                                == Some(FILTER_MIN_SALES);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_chip_daily_sales_min).to_string()
                                    value=Signal::derive(move || min_daily_sales().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    step="0.1"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_min_daily_sales(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_MIN_SALES) {
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
                                    label=t_string!(i18n, recipe_analyzer_filter_job_label).to_string()
                                    value=Signal::derive(job_filter)
                                    options=job_chip_options()
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
                {move || {
                    cost_basis()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_cost_basis_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=cost_basis_options(i18n)
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        let parsed = v.and_then(|v| v.parse::<CostBasis>().ok());
                                        set_cost_basis(parsed.filter(|b| *b != CostBasis::default()));
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    revenue_metric()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_revenue_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=cost_basis_options(i18n)
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        let parsed = v.and_then(|v| v.parse::<RevenueMetric>().ok());
                                        set_revenue_metric(
                                            parsed.filter(|m| *m != RevenueMetric::default()),
                                        );
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    buy_scope()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_buy_from_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=buy_scope_options(i18n)
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        let parsed = v.and_then(|v| v.parse::<BuyScope>().ok());
                                        set_buy_scope(parsed.filter(|s| *s != BuyScope::default()));
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    listing_world_filter()
                        .map(|_| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, analyzer_world_label).to_string()
                                    readonly=true
                                    value=Signal::derive(listing_world_filter)
                                    on_commit=Callback::new(move |_| set_listing_world_filter(None))
                                />
                            }
                        })
                }}
                {move || {
                    listing_dc_filter()
                        .map(|_| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, analyzer_datacenter_label).to_string()
                                    readonly=true
                                    value=Signal::derive(listing_dc_filter)
                                    on_commit=Callback::new(move |_| set_listing_dc_filter(None))
                                />
                            }
                        })
                }}
                {move || {
                    use_subcrafts()
                        .unwrap_or(false)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_filter_subcrafts_label).to_string()
                                    readonly=true
                                    value=Signal::derive(|| None::<String>)
                                    on_commit=Callback::new(move |_| set_use_subcrafts(None))
                                />
                            }
                        })
                }}
                {move || {
                    require_hq()
                        .unwrap_or(false)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_filter_require_hq_label).to_string()
                                    readonly=true
                                    value=Signal::derive(|| None::<String>)
                                    on_commit=Callback::new(move |_| set_require_hq(None))
                                />
                            }
                        })
                }}
                {move || {
                    filter_outliers()
                        .unwrap_or(false)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, filter_outliers).to_string()
                                    readonly=true
                                    value=Signal::derive(|| None::<String>)
                                    on_commit=Callback::new(move |_| set_filter_outliers(None))
                                />
                            }
                        })
                }}
                {move || {
                    exclude_shards_url()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_filter_exclude_shards_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=on_off_options()
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_exclude_shards(v.and_then(|v| v.parse().ok()));
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    use_on_hand_url()
                        .map(|current| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, recipe_analyzer_filter_use_on_hand_label).to_string()
                                    value=Signal::derive(move || Some(current.to_string()))
                                    options=on_off_options()
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_use_on_hand(v.and_then(|v| v.parse().ok()));
                                    })
                                />
                            }
                        })
                }}
            </ControlBar>

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
                <AnalyzerGrid
                    columns=&RECIPE_COLUMNS
                    rows=computed_data
                    visible_cols=visible_cols
                    sort_mode=sort_mode
                    sort_dir=sort_dir
                    ctx=cell_ctx
                    custom=custom
                    layout=GridLayout {
                        viewport_height: 720.0,
                        row_height: 60.0,
                        header_height: 64.0,
                        overscan: 8,
                    }
                    header_class=RECIPE_HEADER_CLASS
                    row_min_width=RECIPE_ROW_MIN_WIDTH
                    row_class=stripe
                    marks=marks
                    extras=header_extras
                    on_pill=on_pill
                    lab_columns=preview
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
    let datacenter = use_datacenter_for_world(move || query.with(|p| p.get("world").clone()));

    // The three pricing params are read here for the resources; under the
    // lab their setters also drive the formula strip's selects.
    let (buy_scope, set_buy_scope) = filter_query_signal::<BuyScope>(FILTER_BUY_SCOPE);
    let (cost_basis, set_cost_basis) = filter_query_signal::<CostBasis>(FILTER_COST_BASIS);
    let (revenue_metric, set_revenue_metric) = filter_query_signal::<RevenueMetric>(FILTER_REVENUE);
    let (filter_outliers, _) = filter_query_signal::<bool>(FILTER_OUTLIERS);

    let preview = use_lab(LAB_ANALYZER_RECIPE);
    // Sub-crafts drive the cost-column cap; read here so the fetch gate
    // (page level) and the pass (table) agree.
    let (use_subcrafts_page, _) = filter_query_signal::<bool>(FILTER_SUBCRAFTS);
    // `(buy, sell)` stats-loaded flags — written by an Effect inside the
    // table, where the resource outcomes are known. `(true, true)` until
    // then, so SSR and the pre-Effect client render name the selected
    // signals rather than flashing a fallback that never happened.
    let stats_loaded = RwSignal::new((true, true));
    // The *selected* formula. `effective()` is applied at each reader:
    // the table marks its headers from its own copy, and the info panel's
    // sentence and the strip's dots apply it below over `stats_loaded`.
    let formula_page = Memo::new(move |_| {
        ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope())
    });

    // `?cols=` lives here rather than in the table because the table
    // remounts whenever its resources change.
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    // `?sort=` / `?dir=` are hoisted for the same reason. A lab-only sort
    // reads as unset while the lab is off, exactly as its token did before
    // the variant existed.
    let (sort_param, _) = query_signal::<SortMode>("sort");
    let sort_mode = Memo::new(move |_| sort_param.get().filter(|m| preview.get() || !m.lab_only()));
    let (sort_dir, _) = query_signal::<SortDir>("dir");
    // The lab widens the `?cols=` contract; off, the Phase D tokens drop
    // like any unknown token.
    let visible_cols = Memo::new(move |_| {
        let all: &'static [&'static str] = if preview.get() {
            &OPTIONAL_COLUMN_ORDER
        } else {
            &BASE_COLUMN_ORDER
        };
        parse_visible_cols(cols_param().as_deref(), all, &DEFAULT_COLS)
    });

    // Which cost signals the pass runs per recipe. Computed here because
    // the buy-scope fetch key must see the sort target and the visible
    // columns. Off the lab this is exactly {selected}: today's fetches.
    let needs_page: Memo<NeededSignals> = Memo::new(move |_| {
        let f = formula_page.get();
        if preview.get() {
            needed_signals(
                &f,
                &signal_wants(&visible_cols.get(), sort_mode.get()),
                use_subcrafts_page().unwrap_or(false),
            )
        } else {
            needed_signals(&f, &SignalWants::default(), false)
        }
    });

    // Rewrite pre-market-model query params once on mount, before the
    // signals above are read reactively (see `migrate_legacy_params`).
    {
        let nav = use_navigate();
        Effect::new(move |_| {
            let pairs: Vec<(String, String)> = query.with_untracked(|q| {
                q.clone()
                    .into_iter()
                    .map(|(k, v)| (k.into_owned(), v))
                    .collect()
            });
            if let Some(migrated) = migrate_legacy_params(&pairs) {
                // `query` hands back decoded values, so they have to be
                // re-encoded on the way out - `world` is a bare world name
                // today, but a raw `format!` here would silently corrupt any
                // value that ever grows a space, `&`, or `=`.
                let qs = migrated
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{k}={}",
                            utf8_percent_encode(v, percent_encoding::NON_ALPHANUMERIC)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("&");
                nav(
                    &format!("?{qs}"),
                    NavigateOptions {
                        replace: true,
                        scroll: false,
                        ..Default::default()
                    },
                );
            }
        });
    }

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

    // The name fed to ingredient-pricing fetches: the sell world itself,
    // its datacenter (the default), or the whole region. World scope needs
    // a resolved world; before one exists (first paint without a home-world
    // cookie) fall back to the datacenter, then the region, so the resource
    // always has a fetchable name.
    let buy_scope_name = Memo::new(move |_| match buy_scope().unwrap_or_default() {
        BuyScope::World => selected_world
            .get()
            .map(|w| w.name)
            .or_else(|| datacenter.get())
            .unwrap_or_else(|| region.get()),
        BuyScope::Datacenter => datacenter().unwrap_or_else(|| region.get()),
        BuyScope::Region => region(),
    });

    // Where each side of the ledger is priced, by name. The sell world can
    // legitimately be unresolved on a first paint with no home-world cookie.
    let sell_place = Memo::new(move |_| {
        selected_world
            .get()
            .map(|w| w.name)
            .unwrap_or_else(|| "…".to_string())
    });
    let buy_place = Memo::new(move |_| buy_scope_name.get());

    // The ledger as chips: `[=] Profit / unit  [+] revenue · sell world
    // [−] 5% tax  [−] cost · buy scope`. Every select writes the same URL
    // param its Market-popover twin does, so the two stay in lockstep. Built
    // once and handed to both the inline row and the popover.
    let strip_terms = move || {
        vec![
            StripTerm::fixed(
                TermRole::Result,
                Signal::derive(move || t_string!(i18n, formula_term_profit_per_unit).to_string()),
            ),
            StripTerm {
                role: TermRole::Revenue,
                label: Signal::derive(String::new),
                place: Some(sell_place.into()),
                select: Some(StripSelect {
                    value: Signal::derive(move || revenue_metric().unwrap_or_default().to_string()),
                    options: cost_basis_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<RevenueMetric>().ok();
                        set_revenue_metric(parsed.filter(|m| *m != RevenueMetric::default()));
                    }),
                    aria: t_string!(i18n, formula_change_revenue_aria).to_string(),
                }),
                place_select: None,
                // Lit only when *this* term fell back: the effective
                // revenue signal differs from the selected one.
                degraded: Signal::derive(move || {
                    let loaded = stats_loaded.get();
                    let f = formula_page.get();
                    f.revenue_signal() != f.effective(loaded.0, loaded.1).revenue_signal()
                }),
            },
            StripTerm::fixed(
                TermRole::Tax,
                Signal::derive(move || t_string!(i18n, formula_term_tax).to_string()),
            ),
            StripTerm {
                role: TermRole::Cost,
                label: Signal::derive(String::new),
                place: None,
                select: Some(StripSelect {
                    value: Signal::derive(move || cost_basis().unwrap_or_default().to_string()),
                    options: cost_basis_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<CostBasis>().ok();
                        set_cost_basis(parsed.filter(|b| *b != CostBasis::default()));
                    }),
                    aria: t_string!(i18n, formula_change_cost_aria).to_string(),
                }),
                place_select: Some(StripSelect {
                    value: Signal::derive(move || buy_scope().unwrap_or_default().to_string()),
                    options: buy_scope_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<BuyScope>().ok();
                        set_buy_scope(parsed.filter(|s| *s != BuyScope::default()));
                    }),
                    aria: t_string!(i18n, formula_change_scope_aria).to_string(),
                }),
                degraded: Signal::derive(move || {
                    let loaded = stats_loaded.get();
                    let f = formula_page.get();
                    f.cost_signal() != f.effective(loaded.0, loaded.1).cost_signal()
                }),
            },
        ]
    };

    let global_cheapest_listings =
        ArcResource::new(buy_scope_name, move |scope_name: String| async move {
            get_cheapest_listings(&scope_name).await
        });

    // Buy from = This world only means the sell world itself: the
    // sell-world stats body doubles as the buy-scope body (one body, not
    // two identical ones). Lab-gated so the flag-off page fetches as before.
    let buy_scope_is_sell_world = Memo::new(move |_| {
        preview.get()
            && buy_scope().unwrap_or_default() == BuyScope::World
            && selected_world.get().is_some()
    });
    // Sale statistics back the sale-median/min/avg cost bases, over the buy
    // scope. Fetched lazily — `None` (no fetch) while the cost basis sits
    // on the listing basis and no sale-cost column is visible or sorted
    // (the lab), so the default page load is unchanged. Basis toggles
    // between sale stats recompute client-side; only a scope change
    // refetches. (Sale-stat *revenue* metrics read the sell world's stats,
    // fetched separately below.) This key answers the BUY-SCOPE body only,
    // so it pins `stats_30: false`: the 30-day sell-world body the opt-in
    // Volume/VWAP columns want is a different role with its own key.
    let buy_sale_stats_scope = Memo::new(move |_| {
        let formula = ProfitFormula::recipe_from_query(cost_basis(), None, buy_scope());
        let needs = RecipeNeeds {
            outliers: false,
            buy_scope_is_sell_world: buy_scope_is_sell_world.get(),
            cost_signals: needs_page.get().cost,
            stats_30: false,
        };
        buy_stats_scope_key(&formula, &needs, buy_scope_name.get())
    });
    let sale_stats = ArcResource::new(
        buy_sale_stats_scope,
        move |scope_name: Option<String>| async move {
            match scope_name {
                Some(name) => get_sale_stats(&name, SALE_STATS_WINDOW_DAYS)
                    .await
                    .map(Some),
                None => Ok(None),
            }
        },
    );

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

    // Revenue is always the sell world's price now, so its listings are
    // always needed (the old fetch was gated on the world-min metric).
    let sell_world_name = Memo::new(move |_| selected_world.get().map(|w| w.name));
    let sell_world_listings =
        ArcResource::new(sell_world_name, move |world: Option<String>| async move {
            match world {
                Some(world) => get_cheapest_listings(&world).await.map(Some),
                None => Ok(None),
            }
        });

    // The same 7-day rollup supplies velocity, average price, sale-stat
    // revenue metrics, and optional stats columns. It is the analyzer's one
    // default sale-history payload regardless of which columns are visible.
    // Keyed on the world alone: toggling the outlier chip must not re-request
    // this (heavy) body, so its failover raw sales live in here while the
    // on-demand raw sales get their own resource below.
    let sell_history = ArcResource::new(sell_world_name, move |world: Option<String>| async move {
        match world {
            Some(world) => Some(fetch_sell_history(world).await),
            None => None,
        }
    });

    // Raw sale samples are needed only for the opt-in outlier filter. Its own
    // resource, so flipping the chip on fetches exactly this body and
    // flipping it off fetches nothing at all.
    let raw_sales_source = Memo::new(move |_| {
        raw_sales_key(
            sell_world_name.get().as_deref(),
            filter_outliers().unwrap_or(false),
        )
    });
    let raw_sales = ArcResource::new(
        raw_sales_source,
        move |key: Option<(String, bool)>| async move {
            match key {
                Some((world, true)) => Some(get_recent_sales_for_world(&world).await),
                _ => None,
            }
        },
    );

    // A header pill writes exactly one param through the filter signal
    // (no scroll-to-top, no history spam); the default is stripped like
    // the Market popover's setters do.
    let on_pill = Callback::new(move |kind: ColumnKind| match pill_param(kind) {
        Some((TermRole::Cost, s)) => {
            set_cost_basis(Some(s).filter(|s| *s != CostBasis::default()));
        }
        Some((TermRole::Revenue, s)) => {
            set_revenue_metric(Some(s).filter(|s| *s != RevenueMetric::default()));
        }
        _ => {}
    });
    let home_world_id = Memo::new(move |_| selected_world.get().map(|w| w.id).unwrap_or(0));

    let sell_history_for_header = sell_history.clone();
    let raw_sales_for_header = raw_sales.clone();
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
                    calculation=ToolCalculation::new(
                        t_string!(i18n, recipe_analyzer_calc_title).to_string(),
                        Signal::derive(move || {
                            if preview.get() {
                                // The EFFECTIVE formula: a failed stats body
                                // downgrades the signal, and the sentence must
                                // never name a signal the numbers ignore.
                                let loaded = stats_loaded.get();
                                let f = formula_page.get().effective(loaded.0, loaded.1);
                                let label_of = |s: PriceSignal| {
                                    cost_basis_options(i18n)
                                        .into_iter()
                                        .find(|(t, _)| *t == s.to_string())
                                        .map(|(_, l)| l)
                                        .unwrap_or_default()
                                };
                                // The connectives are translated: this is a
                                // template, never a `format!` in Rust.
                                t_string!(
                                    i18n,
                                    recipe_analyzer_calc_formula_live,
                                    revenue = label_of(f.revenue_signal()),
                                    sell = sell_place.get(),
                                    tax = t_string!(i18n, formula_term_tax).to_string(),
                                    cost = label_of(f.cost_signal()),
                                    buy = buy_place.get()
                                )
                                .to_string()
                            } else {
                                t_string!(i18n, recipe_analyzer_calc_formula).to_string()
                            }
                        }),
                        Signal::derive(move || {
                            let mut details = t_string!(i18n, recipe_analyzer_calc_details).to_string();
                            if preview.get() {
                                details.push(' ');
                                details.push_str(t_string!(i18n, recipe_analyzer_calc_signal_semantics));
                            }
                            details
                        }),
                    )
                    assumptions=vec![
                        t_string!(i18n, recipe_analyzer_assumption_crafter_levels).to_string(),
                        t_string!(i18n, recipe_analyzer_assumption_subcraft_recursion).to_string(),
                        t_string!(i18n, recipe_analyzer_assumption_sales_velocity).to_string(),
                    ]
                >
                    <Suspense fallback=InlineStatusSkeleton>
                        {move || {
                            // Either raw-sales fetch failing shows this, exactly
                            // as the one pre-fold `recent_sales` resource did.
                            let on_demand_failed = matches!(
                                raw_sales_for_header.get().flatten(),
                                Some(Err(_))
                            );
                            let failover_failed = sell_history_for_header
                                .get()
                                .flatten()
                                .is_some_and(|h| h.raw_failed);
                            (on_demand_failed || failover_failed)
                                .then(|| view! { <div class="text-red-400 text-sm">{t!(i18n, error_loading_sales_data)}</div> })
                        }}
                    </Suspense>
                </ToolHeader>
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
                // Rendered unconditionally: gating on `selected_world.is_some()`
                // hid the only control that can set a world from a visitor who
                // has neither a home-world cookie nor `?world=` in the URL.
                <div class="flex flex-col md:flex-row items-center gap-2">
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, recipe_analyzer_sell_world_label)}</label>
                    <div class="w-full md:w-auto">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world.into()
                        />
                    </div>
                </div>
                // The ledger, directly under the world it sells on. md+ only:
                // below that the chips would wrap into four full-width rows
                // and push the table off the first screen — the Market
                // popover carries the same controls stacked.
                <Show when=move || preview.get()>
                    <div class="hidden md:flex flex-wrap items-center gap-2">
                        <FormulaStrip terms=strip_terms() layout=StripLayout::Inline />
                    </div>
                </Show>

                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        let stats = sale_stats.get();
                        let sell_listings = sell_world_listings.get();
                        let history = sell_history.get();
                        let raw = raw_sales.get();
                        match (listings, stats, sell_listings, history, raw) {
                            (
                                Some(Ok(listings)),
                                Some(stats),
                                Some(sell_listings),
                                Some(history),
                                Some(raw),
                            ) => {
                                // A failed stats fetch is non-fatal: the table
                                // degrades to the listing basis and says so.
                                let (sale_stats, buy_stats_error) = match stats {
                                    Ok(stats) => (stats, false),
                                    Err(_) => (None, true),
                                };
                                // No sell world resolved yet, so nothing was
                                // requested: reads as "no sales anywhere".
                                let history = history.unwrap_or(SellHistory {
                                    stats: None,
                                    raw: None,
                                    stats_failed: false,
                                    raw_failed: false,
                                });
                                // The on-demand body wins; the rollup's
                                // failover body fills in when it was fetched.
                                let recent_sales = raw
                                    .and_then(|r| r.ok())
                                    .or(history.raw);
                                view! {
                                    <RecipeAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=recent_sales
                                        sale_stats=sale_stats
                                        sell_world_sale_stats=history.stats
                                        buy_stats_error=buy_stats_error
                                        sell_stats_error=history.stats_failed
                                        sell_world_listings=sell_listings.ok().flatten()
                                        world=Signal::derive(buy_scope_name)
                                        visible_cols=visible_cols
                                        set_cols_param=set_cols_param
                                        sort_mode=sort_mode
                                        sort_dir=sort_dir
                                        stats_loaded=stats_loaded
                                        sell_place=sell_place
                                        buy_place=buy_place
                                        strip_terms=Callback::new(move |()| strip_terms())
                                        preview=preview.get()
                                        needs=needs_page
                                        buy_stats_aliased=buy_scope_is_sell_world.get()
                                        home_world_id=home_world_id
                                        on_pill=on_pill
                                    />
                                }.into_any()
                            }
                            (Some(Err(e)), _, _, _, _) => {
                                // The table — and the Effect that publishes
                                // the pair — is gone; leaving the last
                                // outcome behind would keep stale dots lit.
                                stats_loaded.set((true, true));
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
    use std::collections::BTreeSet;
    use ultros_api_types::cheapest_listings::CheapestListingItem;
    use xiv_gen::ClassJobId;

    /// `analyzer_kit::grid`'s own test pins the *plumbing* — that a
    /// `row_min_width` reaches the scroller's row spacer. Nothing pinned the
    /// *wiring*: deleting the two props from this page's `<AnalyzerGrid>`
    /// call re-blanks every cell past the viewport width and leaves the whole
    /// suite green. `AnalyzerGrid` cannot be rendered here without the full
    /// route context, so the call site is read back from source instead.
    #[test]
    fn the_grid_call_opts_into_a_sized_row_spacer() {
        const SRC: &str = include_str!("recipe_analyzer.rs");
        // Assembled at run time: `include_str!` pulls in this test's own
        // source too, so a literal needle would satisfy itself.
        let passes = |prop: &str, konst: &str| SRC.contains(&format!("{prop}={konst}"));

        assert_eq!(RECIPE_ROW_MIN_WIDTH, "max-content");
        assert!(
            passes("row_min_width", "RECIPE_ROW_MIN_WIDTH"),
            "the <AnalyzerGrid> call must pass row_min_width, or the spacer resolves to the port width and clips every row"
        );

        assert!(
            RECIPE_HEADER_CLASS.contains("min-w-max"),
            "the header band must span the scrolled width: {RECIPE_HEADER_CLASS}"
        );
        assert!(
            passes("header_class", "RECIPE_HEADER_CLASS"),
            "the <AnalyzerGrid> call must pass header_class"
        );
    }

    /// `ADDABLE_FILTERS`' ids are the `filter_query_signal` keys the old
    /// Toolbar wrote verbatim — a drifted id here silently breaks every
    /// bookmarked filter deep link (same contract currency_exchange.rs pins
    /// for its `RANGE_FILTERS`).
    #[test]
    fn filter_registry_keys_are_a_stable_url_contract() {
        assert_eq!(
            ADDABLE_FILTERS,
            &[
                FILTER_PROFIT,
                FILTER_ROI,
                FILTER_MIN_SALES,
                FILTER_JOB,
                FILTER_SUBCRAFTS,
                FILTER_REQUIRE_HQ,
                FILTER_OUTLIERS,
                FILTER_EXCLUDE_SHARDS,
                FILTER_USE_ON_HAND,
            ]
        );
        assert_eq!(
            [
                FILTER_PROFIT,
                FILTER_ROI,
                FILTER_MIN_SALES,
                FILTER_JOB,
                FILTER_SUBCRAFTS,
                FILTER_REQUIRE_HQ,
                FILTER_OUTLIERS,
                FILTER_EXCLUDE_SHARDS,
                FILTER_USE_ON_HAND,
            ],
            [
                "profit",
                "roi",
                "min-sales",
                "job",
                "subcrafts",
                "require-hq",
                "filter-outliers",
                "shards-exclude",
                "on-hand",
            ]
        );
        // Pricing params left the filter menu (#1233) but their URL keys
        // are still a bookmark contract.
        assert_eq!(FILTER_COST_BASIS, "cost-basis");
        assert_eq!(FILTER_REVENUE, "revenue");
        assert_eq!(FILTER_BUY_SCOPE, "buy-scope");
        // Set by clicking a cheapest-listing world/DC cell, not the menu.
        assert_eq!(FILTER_LISTING_WORLD, "listing-world");
        assert_eq!(FILTER_LISTING_DC, "listing-dc");
    }

    #[test]
    fn listing_location_filter_predicate() {
        let names = ("Gilgamesh".to_string(), "Aether".to_string());
        // No filter: everything passes, even unknown locations.
        assert!(listing_location_passes(None, None, None));
        assert!(listing_location_passes(Some(&names), None, None));
        // World filter.
        assert!(listing_location_passes(
            Some(&names),
            Some("Gilgamesh"),
            None
        ));
        assert!(!listing_location_passes(
            Some(&names),
            Some("Balmung"),
            None
        ));
        // DC filter.
        assert!(listing_location_passes(Some(&names), None, Some("Aether")));
        assert!(!listing_location_passes(
            Some(&names),
            None,
            Some("Crystal")
        ));
        // An unknown cheapest world must not slip through an active filter.
        assert!(!listing_location_passes(None, Some("Gilgamesh"), None));
        assert!(!listing_location_passes(None, None, Some("Aether")));
    }

    #[test]
    fn legacy_scope_param_becomes_buy_scope() {
        let out = migrate_legacy_params(&[
            ("world".into(), "Gilgamesh".into()),
            ("scope".into(), "datacenter".into()),
        ])
        .unwrap();
        assert_eq!(
            out,
            vec![
                ("world".to_string(), "Gilgamesh".to_string()),
                ("buy-scope".to_string(), "datacenter".to_string()),
            ]
        );
    }

    #[test]
    fn legacy_world_min_revenue_drops() {
        let out = migrate_legacy_params(&[("revenue".into(), "world-min".into())]).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn modern_urls_are_left_alone() {
        assert_eq!(
            migrate_legacy_params(&[
                ("buy-scope".into(), "region".into()),
                ("revenue".into(), "sale-median".into()),
            ]),
            None
        );
    }

    const ALL_SORT_MODES: [SortMode; 24] = [
        SortMode::Roi,
        SortMode::Profit,
        SortMode::Velocity,
        SortMode::CostPerUnit,
        SortMode::Price,
        SortMode::AvgPrice,
        SortMode::LastSold,
        SortMode::Volume,
        SortMode::Vwap,
        SortMode::Tax,
        SortMode::Confidence,
        SortMode::RevSignal(PriceSignal::ListingMin),
        SortMode::RevSignal(PriceSignal::SaleMin),
        SortMode::RevSignal(PriceSignal::SaleMedian),
        SortMode::RevSignal(PriceSignal::SaleAvg),
        SortMode::CostSignal(PriceSignal::ListingMin),
        SortMode::CostSignal(PriceSignal::SaleMin),
        SortMode::CostSignal(PriceSignal::SaleMedian),
        SortMode::CostSignal(PriceSignal::SaleAvg),
        SortMode::HopGain,
        SortMode::HopWorlds,
        SortMode::ProfitPerDay,
        SortMode::Volume30,
        SortMode::Vwap30,
    ];

    /// Display must produce exactly the token FromStr parses back — the
    /// shared SortHeader's hrefs depend on that round trip.
    #[test]
    fn sort_mode_round_trips_through_the_url() {
        for mode in ALL_SORT_MODES {
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert!("bogus".parse::<SortMode>().is_err());
        // malformed signal tokens are rejected
        assert!("rev-".parse::<SortMode>().is_err());
        assert!("cost-mars".parse::<SortMode>().is_err());
        assert!("rev-listing-min".parse::<SortMode>().is_ok());
        assert_eq!(
            SortMode::CostSignal(PriceSignal::SaleAvg).to_string(),
            "cost-sale-avg"
        );
        assert_eq!(SortMode::HopWorlds.to_string(), "hop-worlds");
        assert_eq!(SortMode::ProfitPerDay.to_string(), "profit-per-day");
        assert_eq!(SortMode::Volume30.to_string(), "volume-30d");
        assert_eq!("vwap-30d".parse::<SortMode>(), Ok(SortMode::Vwap30));
    }

    /// `?cols=` tokens and the default set are a bookmark contract; both
    /// are derived from `RECIPE_COLUMNS`, so a reordered or retokenised
    /// table would silently rewrite every shared link.
    #[test]
    fn recipe_optional_column_order_is_a_stable_url_contract() {
        assert_eq!(
            OPTIONAL_COLUMN_ORDER.as_slice(),
            &[
                "confidence",
                "last-sold",
                "volume",
                "vwap",
                "tax",
                "listing-world",
                "listing-dc",
                "rev-listing-min",
                "rev-sale-min",
                "rev-sale-median",
                "rev-sale-avg",
                "cost-listing-min",
                "cost-sale-min",
                "cost-sale-median",
                "cost-sale-avg",
                "hop-gain",
                "hop-worlds",
                // Phase E2, appended so every serialized old URL stays
                // byte-identical.
                "profit-per-day",
                "trend",
                "drift",
                "volume-30d",
                "vwap-30d",
            ]
        );
        // The contract the page uses while the lab is off: the seven of Phase B.
        assert_eq!(
            BASE_COLUMN_ORDER.as_slice(),
            &[
                "confidence",
                "last-sold",
                "volume",
                "vwap",
                "tax",
                "listing-world",
                "listing-dc"
            ]
        );
        assert_eq!(DEFAULT_COLS.as_slice(), &["confidence"]);
    }

    /// Every sort mode must be catalogued by exactly one column: two
    /// columns claiming one mode makes `Display` pick an arbitrary token,
    /// and none makes the mode unreachable from a URL.
    #[test]
    fn every_recipe_sort_mode_is_catalogued_exactly_once() {
        for mode in ALL_SORT_MODES {
            let hits = RECIPE_COLUMNS
                .iter()
                .filter(|c| matches!(c.sort, Sortability::By(m) if m == mode))
                .count();
            assert_eq!(hits, 1, "{mode:?} catalogued {hits} times");
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert_eq!(SortMode::CostPerUnit.default_dir(), SortDir::Asc);
        assert_eq!(SortMode::Profit.default_dir(), SortDir::Desc);
        // new default directions
        assert_eq!(SortMode::HopWorlds.default_dir(), SortDir::Asc);
        assert_eq!(SortMode::HopGain.default_dir(), SortDir::Desc);
        assert_eq!(
            SortMode::CostSignal(PriceSignal::SaleMin).default_dir(),
            SortDir::Asc
        );
        assert_eq!(
            SortMode::RevSignal(PriceSignal::SaleMin).default_dir(),
            SortDir::Desc
        );
    }

    /// Better bands sort above worse ones, and rows without deep-scan data
    /// (`Unknown`) sort below everything under the default descending sort.
    #[test]
    fn confidence_ranks_better_bands_higher() {
        let ordered = [
            ConfidenceBand::Unknown,
            ConfidenceBand::Unusable,
            ConfidenceBand::Low,
            ConfidenceBand::Medium,
            ConfidenceBand::High,
        ];
        for pair in ordered.windows(2) {
            assert!(confidence_rank(pair[0]) < confidence_rank(pair[1]));
        }
    }

    /// % vs VWAP guards the divide: no VWAP (no sales, old server) must be
    /// "no figure", never a NaN or an infinite percent.
    #[test]
    fn vwap_pct_math() {
        assert_eq!(vwap_pct(150, 100), Some(50.0));
        assert_eq!(vwap_pct(50, 100), Some(-50.0));
        assert_eq!(vwap_pct(100, 0), None);
        assert_eq!(vwap_pct(0, 0), None);
    }

    #[test]
    fn rollup_sales_summary_combines_quality_rows() {
        let stats = HashMap::from([
            (
                (42, false),
                ItemSaleStats {
                    item_id: 42,
                    hq: false,
                    avg_price: 100,
                    num_sold: 14,
                    ..Default::default()
                },
            ),
            (
                (42, true),
                ItemSaleStats {
                    item_id: 42,
                    hq: true,
                    avg_price: 400,
                    num_sold: 7,
                    ..Default::default()
                },
            ),
        ]);

        let summary = sales_stats_from_rollup(&stats, 42).unwrap();
        assert_eq!(summary.total_sales, 21);
        assert_eq!(summary.daily_sales, 3.0);
        assert_eq!(summary.avg_price, 200);
        assert!(sales_stats_from_rollup(&stats, 99).is_none());
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

    // --- `price_rows` / `filter_and_sort` -----------------------------------

    /// Deterministic synthetic market: every item `i` lists NQ at
    /// `100 + (i % 97) * 7` on world 1 and HQ at that plus 50 on world 2;
    /// the sell world lists the OUTPUT items of the fixture recipes 20%
    /// higher on world 3; 7d stats exist for every third item.
    fn fixture(
        recipes: &[&'static Recipe],
    ) -> (CheapestListingsMap, CheapestListingsMap, BulkSaleStats) {
        let mut buy = Vec::new();
        let mut sell = Vec::new();
        let mut stats = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for r in recipes {
            for id in r.ingredient.iter().chain(std::iter::once(&r.item_result)) {
                if *id == 0 || !seen.insert(*id) {
                    continue;
                }
                let nq = 100 + (*id % 97) * 7;
                buy.push(CheapestListingItem {
                    item_id: *id,
                    hq: false,
                    cheapest_price: nq,
                    world_id: 1,
                });
                buy.push(CheapestListingItem {
                    item_id: *id,
                    hq: true,
                    cheapest_price: nq + 50,
                    world_id: 2,
                });
                if *id % 3 == 0 {
                    stats.push(ItemSaleStats {
                        item_id: *id,
                        hq: false,
                        min_price: nq - 10,
                        median_price: nq + 5,
                        avg_price: nq + 9,
                        num_sold: 14,
                        ..Default::default()
                    });
                }
            }
            let out = r.item_result;
            let nq = 100 + (out % 97) * 7;
            sell.push(CheapestListingItem {
                item_id: out,
                hq: false,
                cheapest_price: nq * 12 / 10,
                world_id: 3,
            });
        }
        (
            CheapestListingsMap::from(CheapestListings {
                cheapest_listings: buy,
            }),
            CheapestListingsMap::from(CheapestListings {
                cheapest_listings: sell,
            }),
            BulkSaleStats { stats },
        )
    }

    fn fixture_recipes() -> Vec<&'static Recipe> {
        let data = xiv_gen_db::data();
        let mut all: Vec<&'static Recipe> = data.recipes.values().collect();
        all.sort_by_key(|r| r.key_id.0);
        all.into_iter().take(300).collect()
    }

    struct RunOpts {
        outliers: bool,
        needs: NeededSignals,
        sell_listings: bool,
        sell_stats: bool,
        scope: Option<BuyScope>,
        /// Give the sell world HQ-only statistics, so the pass has to fall
        /// back to the other quality and the row has to record that it did.
        stats_hq: bool,
    }

    impl Default for RunOpts {
        fn default() -> Self {
            Self {
                outliers: false,
                needs: NeededSignals::default(),
                sell_listings: true,
                sell_stats: true,
                scope: None,
                stats_hq: false,
            }
        }
    }

    fn run_with(cost: PriceSignal, revenue: PriceSignal, o: &RunOpts) -> Vec<RecipeProfitData> {
        let data = xiv_gen_db::data();
        let recipes = fixture_recipes();
        let (buy, sell, stats) = fixture(&recipes);
        let index = stats_index(&stats);
        let sell_index = if o.stats_hq {
            // The same rows, only HQ, and carrying the two figures the row
            // copies off its stat row: exercises the pass's fallback to the
            // other quality when the required one never traded, and lets a
            // test tell which row the numbers came from.
            stats
                .stats
                .iter()
                .map(|s| {
                    (
                        (s.item_id, true),
                        ItemSaleStats {
                            hq: true,
                            vwap: s.avg_price,
                            units_sold: 3,
                            ..*s
                        },
                    )
                })
                .collect()
        } else {
            index.clone()
        };
        let empty_index = StatsIndex::new();
        let by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let raw_sales = HashMap::new();
        let levels = CrafterLevels::default(); // 100 in every job
        // Fixture geography: buy NQ on world 1 (Aether), buy HQ on world 2
        // (Primal), the sell world is 3 (Aether). A closure, not a fn item:
        // a fn item's `Output` is fixed to `Option<&'static str>` and cannot
        // unsize into `dyn Fn(i32) -> Option<&'a str>` while `'a` borrows
        // the locals above.
        let fixture_dc = |w: i32| match w {
            1 | 3 => Some("Aether"),
            2 => Some("Primal"),
            _ => None,
        };
        let inp = PriceInputs {
            recipes: &recipes,
            recipe_level_tables: &data.recipe_level_tables,
            recipes_by_output: &by_output,
            buy_listings: &buy,
            sell_listings: o.sell_listings.then_some(&sell),
            buy_stats: Some(&index),
            sell_stats: if o.sell_stats {
                &sell_index
            } else {
                &empty_index
            },
            raw_sales: &raw_sales,
            formula: ProfitFormula::recipe_from_query(Some(cost), Some(revenue), o.scope),
            levels: &levels,
            job_filter: None,
            use_subcrafts: false,
            require_hq: false,
            filter_outliers: o.outliers,
            shards: ShardsMode::ExcludeShards,
            on_hand: None,
            needs: &o.needs,
            sell_stats_loaded: o.sell_stats,
            home_world_id: 3,
            dc_of: &fixture_dc,
        };
        price_rows(&inp).0
    }

    fn run(cost: PriceSignal, revenue: PriceSignal, outliers: bool) -> Vec<RecipeProfitData> {
        let f = ProfitFormula::recipe_from_query(Some(cost), Some(revenue), None);
        run_with(
            cost,
            revenue,
            &RunOpts {
                outliers,
                needs: needed_signals(&f, &SignalWants::default(), false),
                ..RunOpts::default()
            },
        )
    }

    fn everything_wanted(cost: PriceSignal) -> NeededSignals {
        let f = ProfitFormula::recipe_from_query(Some(cost), None, None);
        let wants = SignalWants {
            visible_cost: PriceSignal::ALL.to_vec(),
            sort_cost: None,
            hop: true,
            worlds: true,
        };
        needed_signals(&f, &wants, false)
    }

    /// The drop rule, ROI and the row set are the selected pair's alone;
    /// alternative columns are informational.
    #[test]
    fn alt_columns_never_change_row_membership() {
        let base = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        let full = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: everything_wanted(PriceSignal::ListingMin),
                ..RunOpts::default()
            },
        );
        assert_eq!(base.len(), full.len());
        for (a, b) in base.iter().zip(&full) {
            assert_eq!(a.recipe.key_id, b.recipe.key_id);
            assert_eq!(
                (a.profit, a.cost, a.market_price, a.return_on_investment),
                (b.profit, b.cost, b.market_price, b.return_on_investment)
            );
            assert_eq!(
                b.cost_alt[PriceSignal::ListingMin.index()],
                Some(b.cost),
                "the selected run is its own alt"
            );
        }
        assert!(
            full.iter()
                .any(|r| r.cost_alt[PriceSignal::SaleMedian.index()].is_some())
        );
        assert!(
            base.iter()
                .all(|r| r.cost_alt[PriceSignal::SaleMedian.index()].is_none()
                    && r.hop.is_none()
                    && r.worlds.is_none())
        );
    }

    /// An alternative cost column equals what selecting that signal would
    /// have priced the same row at.
    #[test]
    fn cost_alt_matches_a_dedicated_run() {
        let full = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: everything_wanted(PriceSignal::ListingMin),
                ..RunOpts::default()
            },
        );
        let median = run(PriceSignal::SaleMedian, PriceSignal::ListingMin, false);
        let by_key: HashMap<i32, i32> =
            median.iter().map(|r| (r.recipe.key_id.0, r.cost)).collect();
        let mut compared = 0;
        for r in &full {
            if let Some(cost) = by_key.get(&r.recipe.key_id.0) {
                assert_eq!(
                    r.cost_alt[PriceSignal::SaleMedian.index()],
                    Some(*cost),
                    "recipe {}",
                    r.recipe.key_id.0
                );
                compared += 1;
            }
        }
        assert!(compared > 20, "only {compared} rows compared");
    }

    /// Alternative revenue columns are the bare sell-world statistic (or
    /// listing): nothing falls back, so no sell world means "-" everywhere.
    #[test]
    fn revenue_alt_columns_are_none_without_sell_world_data() {
        let none = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                sell_listings: false,
                sell_stats: false,
                ..RunOpts::default()
            },
        );
        assert!(none.len() > 20);
        assert!(
            none.iter()
                .all(|r| r.rev_alt == [None; 4] && r.revenue_fell_back)
        );
        let some = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        for r in &some {
            let out = r.recipe.item_result;
            let nq = 100 + (out % 97) * 7;
            assert_eq!(
                r.rev_alt[PriceSignal::ListingMin.index()],
                Some(nq * 12 / 10),
                "sell listing, no fallback"
            );
            // The fixture writes a stats row for every third item, but
            // skips item 0 (recipe 0's degenerate output) entirely.
            let expect_stat = out % 3 == 0 && out != 0;
            assert_eq!(
                r.rev_alt[PriceSignal::SaleMedian.index()],
                expect_stat.then_some(nq + 5),
                "recipe {} (item {out})",
                r.recipe.key_id.0
            );
        }
    }

    /// The Price slot's "listing" tell: set exactly when the number shown
    /// is not the selected signal on the sell world.
    #[test]
    fn price_fallback_tell_marks_buy_scope_prices() {
        let rows = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        let mut fell = 0;
        for r in &rows {
            let nq = 100 + (r.recipe.item_result % 97) * 7;
            let sell_price = nq * 12 / 10;
            // The buy scope's HQ listing (nq + 50) undercuts the sell world
            // once nq > 250: that price came from the buy scope.
            assert_eq!(
                r.revenue_fell_back,
                r.market_price != sell_price,
                "recipe {}",
                r.recipe.key_id.0
            );
            fell += usize::from(r.revenue_fell_back);
        }
        assert!(fell > 0 && fell < rows.len(), "{fell} of {}", rows.len());
    }

    #[test]
    fn hop_and_worlds_are_computed_only_when_needed() {
        let full = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: everything_wanted(PriceSignal::ListingMin),
                ..RunOpts::default()
            },
        );
        assert!(full.iter().all(|r| r.hop.is_some() && r.worlds.is_some()));
        // The sell world lists only outputs: every market ingredient is
        // missing at home, so those rows read "needed". Depends on game
        // data: some kept row needs a non-vendor, non-shard ingredient that
        // is not one of the 300 fixture outputs (true for every pack so
        // far; re-check on a game-data bump).
        assert!(full.iter().any(|r| r.hop == Some(HopGain::Needed)));
        // Cheapest ingredient listings sit on buy world 1 (NQ beats HQ + 50).
        let with_trip: Vec<&RecipeProfitData> = full
            .iter()
            .filter(|r| !r.worlds.as_ref().unwrap().worlds.is_empty())
            .collect();
        assert!(!with_trip.is_empty());
        for r in with_trip {
            let w = r.worlds.as_ref().unwrap();
            assert!(w.worlds.iter().all(|(id, n)| *id == 1 && *n > 0), "{w:?}");
            assert_eq!(w.dcs, 1);
        }
        // Buy from = This world only: no trip to compute.
        let home_only = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: everything_wanted(PriceSignal::ListingMin),
                scope: Some(BuyScope::World),
                ..RunOpts::default()
            },
        );
        assert!(
            home_only
                .iter()
                .all(|r| r.hop == Some(HopGain::Unavailable) && r.worlds.is_none())
        );
        // Unpriced under the selected signal is carried on the row.
        assert!(
            full.iter().all(|r| r.unpriced == 0),
            "the fixture lists every ingredient"
        );
    }

    /// Every row obeys the formula's arithmetic and the drop rule; this
    /// runs over 300 real recipes with synthetic prices.
    #[test]
    fn price_rows_rows_obey_the_formula() {
        let rows = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        assert!(rows.len() > 50, "fixture priced only {} rows", rows.len());
        for r in &rows {
            let net = r.market_price as i64 * 95 / 100;
            assert!(
                (r.cost as i64) < net,
                "row kept with cost >= net: {:?}",
                r.recipe.key_id
            );
            assert_eq!(r.profit as i64, net - r.cost as i64);
            assert_eq!(r.tax as i64, r.market_price as i64 - net);
            let roi = if r.cost > 0 {
                (r.profit as f64 / r.cost as f64 * 100.0) as i32
            } else {
                0
            };
            assert_eq!(r.return_on_investment, roi);
            // Revenue is `lowest_gil()` over the sell world's NQ listing (20% up)
            // and the buy scope's HQ listing (`nq + 50`), whichever is lower:
            // exactly today's `override_listings` + `lowest_gil` behaviour.
            let nq = 100 + (r.recipe.item_result % 97) * 7;
            assert_eq!(r.market_price, (nq * 12 / 10).min(nq + 50));
        }
    }

    /// The characterization oracle. Regenerate ONLY if a phase changes the
    /// numbers on purpose: run with `--nocapture`, copy the printed tuples.
    #[test]
    fn price_rows_matches_recorded_oracle_on_fixture() {
        let rows = run(PriceSignal::SaleMedian, PriceSignal::ListingMin, false);
        let got: Vec<(i32, i32, i32, i32, i32, i32)> = rows
            .iter()
            .take(12)
            .map(|r| {
                (
                    r.recipe.key_id.0,
                    r.profit,
                    r.return_on_investment,
                    r.cost,
                    r.market_price,
                    r.tax,
                )
            })
            .collect();
        println!("ORACLE = {got:?}");
        // Recorded from the pre-refactor pipeline (Move A, commit above).
        const ORACLE: &[(i32, i32, i32, i32, i32, i32)] = &[
            (0, 114, 0, 0, 120, 6),
            (1, 89, 74, 120, 220, 11),
            (2, 35, 13, 267, 318, 16),
            (3, 203, 74, 272, 500, 25),
            (4, 47, 17, 275, 339, 17),
            (5, 332, 269, 123, 479, 24),
            (7, 209, 74, 279, 514, 26),
            (9, 421, 150, 280, 738, 37),
            (12, 134, 50, 267, 423, 22),
            (13, 413, 150, 274, 724, 37),
            (14, 238, 86, 276, 542, 28),
            (15, 210, 77, 271, 507, 26),
        ];
        assert_eq!(got, ORACLE);
    }

    fn row(key: i32, profit: i32, roi: i32, daily: f32, world: i32) -> Arc<RecipeProfitData> {
        let recipe = fixture_recipes()
            .into_iter()
            .find(|r| r.key_id.0 == key)
            .expect("fixture recipe");
        Arc::new(RecipeProfitData {
            recipe,
            profit,
            return_on_investment: roi,
            cost: 1,
            market_price: 2,
            cheapest_world_id: world,
            sub_crafts: vec![],
            daily_sales: daily,
            avg_price: 0,
            total_sales: 0,
            required_level: 1,
            last_sold_unix: 0,
            units_sold: 0,
            vwap: 0,
            vwap_pct: None,
            tax: 0,
            confidence: ConfidenceBand::Unknown,
            stat_hq: false,
            cost_alt: [None; 4],
            rev_alt: [None; 4],
            revenue_fell_back: false,
            unpriced: 0,
            hop: None,
            worlds: None,
        })
    }

    #[test]
    fn filter_and_sort_is_pure_and_inclusive() {
        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(4)
            .map(|r| r.key_id.0)
            .collect();
        // The two profit-200 rows are fed in DESCENDING key order, so a
        // stable sort without the key-id tiebreak would emit them the other
        // way round and this test would fail.
        let rows = vec![
            row(keys[0], 100, 10, 1.0, 7),
            row(keys[1], 300, 30, 0.5, 8),
            row(keys[3], 200, 5, 3.0, 9),
            row(keys[2], 200, 20, 2.0, 7),
        ];
        let names: HashMap<i32, (String, String)> = [
            (7, ("Gilgamesh".to_string(), "Aether".to_string())),
            (8, ("Balmung".to_string(), "Crystal".to_string())),
        ]
        .into_iter()
        .collect();
        let t = Thresholds {
            min_profit: Some(200),
            ..Default::default()
        };
        let out = filter_and_sort(&rows, &t, &names, SortMode::Profit, SortDir::Desc, None);
        // Inclusive `>=`; ties broken by key id ascending; indexes renumbered.
        let got: Vec<(usize, i32, i32)> = out
            .iter()
            .map(|(i, r)| (*i, r.profit, r.recipe.key_id.0))
            .collect();
        assert_eq!(
            got,
            vec![(0, 300, keys[1]), (1, 200, keys[2]), (2, 200, keys[3])]
        );
        // Ascending flips the order but keeps the same tiebreak direction.
        let out = filter_and_sort(&rows, &t, &names, SortMode::Profit, SortDir::Asc, None);
        assert_eq!(out[0].1.profit, 200);
        assert_eq!(out[0].1.recipe.key_id.0, keys[2]);
        // A listing-world filter drops unknown worlds (9 has no name).
        let t = Thresholds {
            listing_world: Some("Gilgamesh".into()),
            ..Default::default()
        };
        let out = filter_and_sort(&rows, &t, &names, SortMode::Profit, SortDir::Desc, None);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn signal_columns_have_unique_ids_and_sort_tokens() {
        let mut ids: Vec<&str> = RECIPE_COLUMNS
            .iter()
            .map(|c| c.id)
            .filter(|i| !i.is_empty())
            .collect();
        let mut sorts: Vec<&str> = RECIPE_COLUMNS
            .iter()
            .map(|c| c.sort_id)
            .filter(|i| !i.is_empty())
            .collect();
        let (n_ids, n_sorts) = (ids.len(), sorts.len());
        ids.sort_unstable();
        ids.dedup();
        sorts.sort_unstable();
        sorts.dedup();
        assert_eq!((ids.len(), sorts.len()), (n_ids, n_sorts));
        assert_eq!(n_ids, 22);
        assert_eq!(
            n_sorts, 24,
            "the eleven sorts at HEAD, the ten signal and hop columns, and E2's three; \
             listing world/dc, trend and drift do not sort"
        );
        for c in RECIPE_COLUMNS.iter().filter(|c| c.lab.is_some()) {
            assert!(!c.default_on, "{} must start hidden", c.id);
            assert_eq!(c.lab, Some(LAB_ANALYZER_RECIPE));
            assert!(
                c.header_class.contains("hidden md:"),
                "{}: desktop-only (kit decision 7)",
                c.id
            );
        }
        assert_eq!(
            RECIPE_COLUMNS.iter().filter(|c| c.lab.is_some()).count(),
            15
        );
    }

    fn hop_row(key: i32, hop: Option<HopGain>, alt: Option<i32>) -> Arc<RecipeProfitData> {
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.hop = hop;
        r.cost_alt[PriceSignal::SaleMedian.index()] = alt;
        Arc::new(r)
    }

    /// `Needed` / `Unavailable` (and an unrun alt signal) sort last in both
    /// directions; `HopWorlds` defaults ascending.
    #[test]
    fn hop_needed_sorts_last_both_directions() {
        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(4)
            .map(|r| r.key_id.0)
            .collect();
        let rows = vec![
            hop_row(keys[0], Some(HopGain::Gain(5)), Some(300)),
            hop_row(keys[1], Some(HopGain::Needed), None),
            hop_row(keys[2], Some(HopGain::Gain(-3)), Some(100)),
            hop_row(keys[3], Some(HopGain::Unavailable), Some(200)),
        ];
        let names = HashMap::new();
        let order = |mode: SortMode, dir: SortDir| -> Vec<i32> {
            filter_and_sort(&rows, &Thresholds::default(), &names, mode, dir, None)
                .into_iter()
                .map(|(_, r)| r.recipe.key_id.0)
                .collect()
        };
        assert_eq!(
            order(SortMode::HopGain, SortDir::Desc),
            vec![keys[0], keys[2], keys[1], keys[3]]
        );
        assert_eq!(
            order(SortMode::HopGain, SortDir::Asc),
            vec![keys[2], keys[0], keys[1], keys[3]]
        );
        let median = SortMode::CostSignal(PriceSignal::SaleMedian);
        assert_eq!(
            order(median, SortDir::Asc),
            vec![keys[2], keys[3], keys[0], keys[1]]
        );
        assert_eq!(
            order(median, SortDir::Desc),
            vec![keys[0], keys[3], keys[2], keys[1]]
        );
        // The pre-existing modes still flip whole.
        assert_eq!(order(SortMode::Profit, SortDir::Desc).len(), 4);
    }

    #[test]
    fn delta_pct_math() {
        assert_eq!(delta_pct(Some(138), 100), Some(38.0));
        assert_eq!(delta_pct(Some(50), 100), Some(-50.0));
        assert_eq!(delta_pct(None, 100), None);
        assert_eq!(
            delta_pct(Some(0), 100),
            None,
            "an unpriced alt has no delta"
        );
        assert_eq!(delta_pct(Some(100), 0), None);
        assert_eq!(
            delta_pct(Some(100), 100),
            None,
            "the duplicate column shows no +0%"
        );
    }

    fn test_ctx() -> CellCtx {
        CellCtx {
            now_unix: 1_700_000_000,
            preview: true,
            capped_cost: [false; 4],
            sparklines: None,
            stats_30: None,
        }
    }

    fn stats_row(item_id: i32, hq: bool, units_sold: u64, vwap: i32) -> ItemSaleStats {
        ItemSaleStats {
            item_id,
            hq,
            units_sold,
            vwap,
            ..Default::default()
        }
    }

    /// Profit/day is the row's profit times the 7-day rollup rate, computed
    /// in the cell and in the comparator from the same helper — no field,
    /// no fetch.
    #[test]
    fn profit_per_day_is_profit_times_the_rollup_rate() {
        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(2)
            .map(|r| r.key_id.0)
            .collect();
        let fast = row(keys[0], 1_000, 0, 3.0, 1);
        let slow = row(keys[1], 1_000, 0, 0.25, 1);
        assert_eq!(
            cell_profit_per_day(&fast, &test_ctx()),
            CellValue::Gil(3_000)
        );
        assert_eq!(cell_profit_per_day(&slow, &test_ctx()), CellValue::Gil(250));
        let out = filter_and_sort(
            &[slow, fast],
            &Thresholds::default(),
            &HashMap::new(),
            SortMode::ProfitPerDay,
            SortDir::Desc,
            None,
        );
        assert_eq!(
            out.iter()
                .map(|(_, r)| r.recipe.key_id.0)
                .collect::<Vec<_>>(),
            vec![keys[0], keys[1]],
            "the faster seller ranks first even at equal profit"
        );
    }

    /// A 30-day sort reads as Profit until the client-only body lands, then
    /// orders by the 30-day figure with the rows the body knows nothing
    /// about last in both directions.
    #[test]
    fn thirty_day_sorts_fall_back_to_profit_until_the_body_lands() {
        assert_eq!(
            effective_sort_mode(SortMode::Volume30, false),
            SortMode::Profit
        );
        assert_eq!(
            effective_sort_mode(SortMode::Vwap30, false),
            SortMode::Profit
        );
        assert_eq!(
            effective_sort_mode(SortMode::Volume30, true),
            SortMode::Volume30
        );
        assert_eq!(
            effective_sort_mode(SortMode::Profit, false),
            SortMode::Profit
        );

        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(3)
            .map(|r| r.key_id.0)
            .collect();
        let rows = vec![
            row(keys[0], 10, 0, 1.0, 1),
            row(keys[1], 20, 0, 1.0, 1),
            row(keys[2], 30, 0, 1.0, 1),
        ];
        // Three recipes, three distinct output items: without that the two
        // index rows below would collide and the ordering would be an
        // accident.
        let outputs: HashSet<i32> = rows.iter().map(|r| r.recipe.item_result).collect();
        assert_eq!(outputs.len(), 3, "the fixture rows price distinct items");
        let mut index: StatsIndex = StatsIndex::new();
        index.insert(
            (rows[0].recipe.item_result, false),
            stats_row(rows[0].recipe.item_result, false, 500, 100),
        );
        index.insert(
            (rows[1].recipe.item_result, false),
            stats_row(rows[1].recipe.item_result, false, 900, 200),
        );
        // rows[2] is not in the 30-day body at all.
        let order = |dir, index: Option<&StatsIndex>| {
            filter_and_sort(
                &rows,
                &Thresholds::default(),
                &HashMap::new(),
                SortMode::Volume30,
                dir,
                index,
            )
            .into_iter()
            .map(|(_, r)| r.recipe.key_id.0)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            order(SortDir::Desc, Some(&index)),
            vec![keys[1], keys[0], keys[2]]
        );
        assert_eq!(
            order(SortDir::Asc, Some(&index)),
            vec![keys[0], keys[1], keys[2]]
        );
        // No body yet: profit order, not "every row equal".
        assert_eq!(order(SortDir::Desc, None), vec![keys[2], keys[1], keys[0]]);
        // A failed fetch stores an empty index: still profit order, never
        // the recipe-id order an all-`None` comparison would leave behind.
        assert_eq!(
            order(SortDir::Desc, Some(&StatsIndex::new())),
            vec![keys[2], keys[1], keys[0]]
        );
    }

    /// The lazy pair is unreachable from a URL and unreachable from a
    /// header click: no `?sort=` token, `Sortability::LazyNever`.
    #[test]
    fn the_lazy_columns_never_sort() {
        for id in [COL_TREND, COL_DRIFT] {
            let col = RECIPE_COLUMNS
                .iter()
                .find(|c| c.id == id)
                .expect("column in the table");
            assert_eq!(col.sort, Sortability::LazyNever, "{id}");
            assert!(col.sort_id.is_empty(), "{id}");
        }
        assert!("trend".parse::<SortMode>().is_err());
        assert!("drift".parse::<SortMode>().is_err());
    }

    /// A `LazyNever` column with a second header line takes the grid's
    /// *unsortable* two-line arm, which emits two bare `<span>`s.
    /// `SortableHeaderCell` appends `flex flex-col justify-center gap-0.5`
    /// for its own two-line headers; that arm does not, so the column has to
    /// stack them itself or Task 9's "7d · ‹sell world›" line lands *beside*
    /// the label instead of under it — and `HEADER_SUB_LINE`'s `truncate` /
    /// `max-w-full` stay inert until the span is a flex item. `md:flex`,
    /// never `md:block`, which would override the direction at md+.
    #[test]
    fn the_lazy_headers_stack_their_own_two_lines() {
        for id in [COL_TREND, COL_DRIFT] {
            let class = RECIPE_COLUMNS
                .iter()
                .find(|c| c.id == id)
                .expect("column in the table")
                .header_class;
            assert!(
                class.contains("md:flex") && !class.contains("md:block"),
                "{id}: `hidden md:flex`, never `md:block` ({class})"
            );
            assert!(
                class.contains("flex-col")
                    && class.contains("justify-center")
                    && class.contains("gap-0.5"),
                "{id}: the grid's unsortable arm appends nothing, so the \
                 column carries everything SortableHeaderCell would have \
                 added — flex-col to stack, justify-center to sit level \
                 with its neighbours, gap-0.5 for the line spacing ({class})"
            );
        }
    }

    /// Trend and Drift sit side by side in the Market group, so they cannot
    /// share a label. The flip finder's `analyzer_col_drift` is "Tendance"
    /// in fr and "Trend" in de — the same word `analyzer_col_spark` uses —
    /// which is why the recipe analyzer has a key of its own.
    #[test]
    fn trend_and_drift_read_differently_in_every_locale() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let i18n = leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>();
            provide_context(i18n);
            for locale in [
                Locale::en,
                Locale::fr,
                Locale::de,
                Locale::ja,
                Locale::cn,
                Locale::ko,
                Locale::tc,
            ] {
                i18n.set_locale(locale);
                let (trend, drift) = (label_trend(i18n), label_drift(i18n));
                assert!(!trend.is_empty() && !drift.is_empty(), "{locale:?}");
                assert_ne!(trend, drift, "{locale:?}: two columns, one word");
            }
        });
    }

    /// The row records which quality its 7-day statistics came from, so the
    /// sparkline key and the 30-day lookups read the same quality the
    /// visible 7-day numbers did.
    #[test]
    fn stat_hq_records_the_quality_the_row_priced_from() {
        let nq = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        assert!(nq.iter().all(|r| !r.stat_hq), "the fixture trades NQ");
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::ListingMin), None, None);
        let hq = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                stats_hq: true,
                needs: needed_signals(&f, &SignalWants::default(), false),
                ..RunOpts::default()
            },
        );
        // `require_hq` is false, so the pass falls back to the HQ row — and
        // says so on the row.
        assert!(
            hq.iter().any(|r| r.stat_hq),
            "some rows have only an HQ row"
        );
        // And the row's figures came from that same lookup: the remapped
        // fixture rows are the only ones carrying a vwap or a unit count.
        assert!(
            hq.iter()
                .filter(|r| r.stat_hq)
                .all(|r| r.vwap > 0 && r.units_sold == 3),
            "the HQ row's figures are what the row carries"
        );
        assert!(hq.iter().filter(|r| !r.stat_hq).all(|r| r.vwap == 0));
    }

    #[test]
    fn lab_only_sort_modes_are_exactly_the_thirteen() {
        assert_eq!(ALL_SORT_MODES.iter().filter(|m| m.lab_only()).count(), 13);
        assert!(!SortMode::CostPerUnit.lab_only() && !SortMode::Price.lab_only());
        assert!(SortMode::ProfitPerDay.lab_only() && SortMode::Vwap30.lab_only());
    }

    /// Every picker entry is a `?cols=` token (both derive from the table).
    #[test]
    fn picker_columns_are_a_subset_of_optional_column_order() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let ctx = PickerContext {
                sell_place: String::new(),
                buy_place: String::new(),
                revenue: PriceSignal::ListingMin,
                cost: PriceSignal::ListingMin,
                capped: BTreeSet::new(),
            };
            let ids: Vec<&str> = grouped_picker_options(&RECIPE_COLUMNS, i18n, &ctx)
                .iter()
                .map(|o| o.id)
                .collect();
            assert_eq!(ids.len(), 22);
            assert!(ids.iter().all(|id| OPTIONAL_COLUMN_ORDER.contains(id)));
            let flat: Vec<&str> = picker_options(&RECIPE_COLUMNS, i18n)
                .iter()
                .map(|o| o.id)
                .collect();
            assert_eq!(flat, BASE_COLUMN_ORDER.as_slice());
        });
    }

    #[test]
    fn raw_sales_key_reads_outliers_not_resource_state() {
        assert_eq!(
            raw_sales_key(Some("Gilgamesh"), false),
            Some(("Gilgamesh".to_string(), false))
        );
        assert_eq!(
            raw_sales_key(Some("Gilgamesh"), true),
            Some(("Gilgamesh".to_string(), true))
        );
        assert_eq!(raw_sales_key(None, true), None);
    }

    /// The header sub-labels are built from the *effective* formula's
    /// marks, so a mark never names a signal the numbers fell back from.
    /// Each role's label pairs the short signal name with the place the
    /// price came from; the result carries the tool's own sub-line.
    #[test]
    fn formula_marks_labels_name_signal_and_place() {
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        let m = f.marks("Gilgamesh".into(), "Aether".into());
        let labels = mark_labels(&m, "7d median", "listing", "per unit · after 5% tax");
        assert_eq!(labels.labels[&TermRole::Cost], "7d median · Aether");
        assert_eq!(labels.labels[&TermRole::Revenue], "listing · Gilgamesh");
        assert_eq!(labels.labels[&TermRole::Result], "per unit · after 5% tax");
    }

    /// The info panel's `label_of` finds a signal's picker label by
    /// matching `PriceSignal`'s URL token against `cost_basis_options`'
    /// first field. A token that drifts out of that list would silently
    /// blank the sentence's revenue or cost name, so pin the pairing.
    #[test]
    fn every_price_signal_token_has_a_picker_label() {
        // `t_string!` needs an I18nContext, which spawns an Effect: stand
        // up the executor and the context, as the kit's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            for signal in [
                PriceSignal::ListingMin,
                PriceSignal::SaleMin,
                PriceSignal::SaleMedian,
                PriceSignal::SaleAvg,
            ] {
                let token = signal.to_string();
                let label = cost_basis_options(i18n)
                    .into_iter()
                    .find(|(t, _)| *t == token)
                    .map(|(_, l)| l);
                assert!(
                    label.is_some_and(|l| !l.is_empty()),
                    "no picker label for {token}"
                );
            }
        });
    }

    /// A pill writes exactly one param: `cost-basis` for a cost column,
    /// `revenue` for a revenue column, nothing for anything else.
    #[test]
    fn use_as_pill_writes_exactly_one_param() {
        assert_eq!(
            pill_param(ColumnKind::CostSignal(PriceSignal::SaleMedian)),
            Some((TermRole::Cost, PriceSignal::SaleMedian))
        );
        assert_eq!(
            pill_param(ColumnKind::RevSignal(PriceSignal::SaleAvg)),
            Some((TermRole::Revenue, PriceSignal::SaleAvg))
        );
        assert_eq!(pill_param(ColumnKind::HopGain), None);
        assert_eq!(pill_param(ColumnKind::CostSlot), None);
    }

    #[test]
    fn signal_wants_reads_visible_columns_and_the_sort_target() {
        let visible: HashSet<&'static str> = [
            COL_CONFIDENCE,
            COL_COST_SALE_AVG,
            COL_COST_LISTING_MIN,
            COL_REV_SALE_MIN,
        ]
        .into_iter()
        .collect();
        let w = signal_wants(&visible, Some(SortMode::CostSignal(PriceSignal::SaleMin)));
        assert_eq!(
            w.visible_cost,
            vec![PriceSignal::ListingMin, PriceSignal::SaleAvg],
            "table order"
        );
        assert_eq!(w.sort_cost, Some(PriceSignal::SaleMin));
        assert!(!w.hop && !w.worlds);
        let w = signal_wants(&HashSet::new(), Some(SortMode::HopGain));
        assert!(w.hop && !w.worlds);
        let visible: HashSet<&'static str> = [COL_HOP_WORLDS].into_iter().collect();
        let w = signal_wants(&visible, None);
        assert!(w.worlds && !w.hop);
        assert_eq!(
            signal_wants(&HashSet::new(), Some(SortMode::Profit)),
            SignalWants::default()
        );
    }

    #[test]
    fn buy_stats_fetch_only_when_a_sale_cost_signal_is_needed() {
        let listing = ProfitFormula::recipe_from_query(None, None, None);
        let plain = RecipeNeeds::default();
        assert_eq!(buy_stats_scope_key(&listing, &plain, "Aether".into()), None);
        let median = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        assert_eq!(
            buy_stats_scope_key(&median, &plain, "Aether".into()),
            Some("Aether".into())
        );
        // A visible / sorted sale-cost column forces the body under a listing basis.
        let mut wants_col = RecipeNeeds::default();
        wants_col.cost_signals.insert(PriceSignal::SaleMin);
        assert_eq!(
            buy_stats_scope_key(&listing, &wants_col, "Aether".into()),
            Some("Aether".into())
        );
        // A revenue signal never does: it reads the sell-world body.
        let rev = ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMedian), None);
        assert_eq!(buy_stats_scope_key(&rev, &plain, "Aether".into()), None);
    }

    #[test]
    fn buy_stats_key_is_none_when_buy_scope_is_the_sell_world() {
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::SaleMedian),
            None,
            Some(BuyScope::World),
        );
        let same = RecipeNeeds {
            buy_scope_is_sell_world: true,
            ..RecipeNeeds::default()
        };
        assert_eq!(buy_stats_scope_key(&f, &same, "Gilgamesh".into()), None);
        let other = RecipeNeeds::default();
        assert_eq!(
            buy_stats_scope_key(&f, &other, "Gilgamesh".into()),
            Some("Gilgamesh".into())
        );
        // Only a World scope can alias; a datacenter never does.
        let dc = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        assert_eq!(
            buy_stats_scope_key(&dc, &same, "Aether".into()),
            Some("Aether".into())
        );
    }

    #[test]
    fn capped_flags_index_by_signal() {
        let capped = [PriceSignal::SaleAvg, PriceSignal::SaleMin]
            .into_iter()
            .collect();
        assert_eq!(capped_flags(&capped), [false, true, false, true]);
        assert_eq!(capped_flags(&BTreeSet::new()), [false; 4]);
    }

    #[test]
    fn worlds_tooltip_groups_by_datacenter_in_first_appearance_order() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let entries = vec![
                (5, Some(("Cactuar".to_string(), "Aether".to_string())), 2),
                (9, Some(("Behemoth".to_string(), "Primal".to_string())), 1),
                (
                    7,
                    Some(("Adamantoise".to_string(), "Aether".to_string())),
                    1,
                ),
                (999, None, 1),
            ];
            let text = worlds_tooltip(i18n, &entries, 2);
            let aether = text.find("Aether").unwrap();
            let primal = text.find("Primal").unwrap();
            let cactuar = text.find("• Cactuar · ingredients: 2").unwrap();
            let adamantoise = text.find("• Adamantoise · ingredients: 1").unwrap();
            assert!(
                aether < cactuar && cactuar < adamantoise && adamantoise < primal,
                "{text}"
            );
            assert!(text.contains("• 999 · ingredients: 1"), "{text}");
            assert!(text.contains("Datacenters: 2"), "{text}");
            assert!(
                text.ends_with("buy side only · sub-craft materials not counted"),
                "{text}"
            );
        });
    }
}
