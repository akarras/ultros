use crate::analyzer_kit::cells::{CellNote, CellValue, Enrich};
use crate::analyzer_kit::columns::{
    CellCtx, ColumnKind, ColumnSpec, Layer, LazyFeed, PickerContext, PickerGroup, Sortability,
    ToolColumnMeta, default_dir_for, grouped_picker_options, picker_options, sort_from_token,
    sort_token, sortability_for,
};
use crate::analyzer_kit::enrichment::{
    DEBOUNCE_MS, EnrichmentConfig, PREFETCH_MARGIN, SparkKey, SparkStore, SparkValue, Verdict,
    use_visible_enrichment, use_wide_viewport, verdict,
};
use crate::analyzer_kit::formula::{
    FormulaMarks, PriceSignal, ProfitFormula, RoiMath, Scope, SellScope, per_unit_cost, profit_line,
};
use crate::analyzer_kit::grid::{
    AnalyzerGrid, AnalyzerRow, CustomCell, GridLayout, HeaderExtra, HeaderExtras, HeaderLine2,
    HeaderPill, MarkLabels,
};
use crate::analyzer_kit::hop::{HopGain, WorldsToVisit, hop_gain, worlds_to_visit};
use crate::analyzer_kit::needed::{
    BodyRole, NeededSignals, RecipeNeeds, SALE_STATS_WINDOW_DAYS, STATS_30_WINDOW_DAYS,
    SignalWants, needed_bodies, needed_signals,
};
use crate::analyzer_kit::signals::{
    LateStats, PriceLookup, SignalView, StatsIndex, stat_only_cheapest, stat_row_either,
    stats_index,
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
    analysis::{
        SalesStats, VS_MEDIAN_DISPLAY_CEILING_PCT, analyze_sales, first_to_last_pct,
        is_troll_listing, profit_per_day_from_rate,
    },
    api::{get_cheapest_listings, get_recent_sales_for_world, get_sale_stats, post_sparklines},
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
    sparklines::{SparklineSeries, SparklinesRequest},
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
    /// The sell world's 7-day median sale for the quality this row's
    /// *statistics* came from (`stat_hq`) — the same `(item, stat_hq)` row
    /// every other 7-day figure here uses, and the Price tell's basis.
    ///
    /// Not necessarily the quality the *price* is: `market_price` comes from
    /// `lowest_gil()`, which mins across both qualities and never consults
    /// `require_hq`, so an item with an NQ stat row but no NQ listing prices
    /// from HQ against an NQ median. That residual is strictly smaller than
    /// what it replaced (`min(nq, hq)` is further from the price's quality by
    /// construction) and matches the approximation `vwap_pct` already makes
    /// one line above. Closing it properly means recording which side of
    /// `PriceSummary` won `lowest_gil()` and reading `stat_only(index, item,
    /// price_hq, SaleStat::Median)` with this as the fallback.
    ///
    /// Deliberately NOT `rev_alt[SaleMedian]`: that one is
    /// `stat_only_cheapest`, the cheaper of NQ and HQ, which is the right
    /// meaning for the "Sale median (7d)" *alternative revenue* column and
    /// the wrong one for a comparison against this row's own price. Prod
    /// showed the cost of confusing them: an HQ price measured against an
    /// NQ median read "vs median +399900%", in green.
    sell_median: Option<i32>,
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
    /// Scope vs home's state for this row: `Off` unless the column was
    /// asked for at a wider sell scope, then the two places' figures under
    /// the selected revenue signal, or `Unavailable` when either place has
    /// none. The column renders `place − home`.
    scope_vs_home: ScopeVsHome,
    /// `market_price` was read on the sell world itself, i.e. the sell
    /// scope is `Scope::World` — which is every URL that does not carry
    /// `?sell-scope=`, and every URL at all with the lab off.
    ///
    /// The 7-day figures that must not be compared against a scoped price
    /// are suppressed in the pass (`sell_median`, `vwap_pct`), but the
    /// 30-day body is client-only and lands after the rows are priced, so
    /// its cell has to make the same judgement itself. Carried on the row
    /// rather than on `CellCtx` for the reason `scope_vs_home` is: that
    /// struct is shared with the flip finder and has twenty exhaustive
    /// literals, and this one has two.
    price_is_sell_world: bool,
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

/// Which market the sale price is READ from. The same three tokens the buy
/// side uses, with their own "this world" label: the buy side's reads "This
/// world only" in a buying sentence, and this one sits in a chip about
/// where a price comes from. Datacenter and Region reuse the shared nouns.
fn sell_scope_options(i18n: I18nContext<Locale, I18nKeys>) -> Vec<(&'static str, String)> {
    vec![
        ("world", t_string!(i18n, sell_scope_this_world).to_string()),
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
/// Phase F: which market the sale price is read from. Default `world`,
/// stripped from the URL at the default, read only under the
/// `analyzer-recipe` lab.
const FILTER_SELL_SCOPE: &str = "sell-scope";
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

/// The sell scope the page acts on: `None` — i.e. `Term::Fixed(World)`,
/// today's ledger exactly — whenever the `analyzer-recipe` lab is off, so a
/// bookmarked `?sell-scope=region` is inert on the flag-off page down to
/// the "no active filters" hint.
fn sell_scope_for(preview: bool, param: Option<SellScope>) -> Option<SellScope> {
    preview.then_some(param).flatten()
}

/// Seat the sell scope on a formula, through the lab gate.
///
/// **The only caller of [`ProfitFormula::with_sell_scope`] in the crate**,
/// and deliberately so. The page builds a `formula_page` for its fetch
/// keys and the table builds its own `formula` for the pricing pass; only
/// the second one reaches `price_rows`, so a scope seated on the first
/// alone yields a column of dashes that every unit test passes — which is
/// how Phase E2's median tell shipped broken. One function, three callers
/// (the page memo, the table memo, the pricing harness), and a source-read
/// test in Task 8 that counts them.
fn seat_sell_scope(f: ProfitFormula, preview: bool, param: Option<SellScope>) -> ProfitFormula {
    match sell_scope_for(preview, param) {
        Some(s) => f.with_sell_scope(s),
        None => f,
    }
}

/// The name revenue is priced at: the sell world under the default sell
/// scope, its datacenter or the region otherwise. `sell_place` stays the
/// sell **world**, and the difference is load-bearing — the market columns'
/// "7d · ‹place›" sub-labels, the sparkline feed, the 30-day body and Hop
/// gain's home run all read the sell world's own data at every sell scope
/// (spec §4), so naming the scope there would be a lie.
fn revenue_place_for(
    preview: bool,
    param: Option<SellScope>,
    sell_world: &str,
    datacenter: Option<&str>,
    region: &str,
) -> String {
    match sell_scope_for(preview, param)
        .map(SellScope::scope)
        .unwrap_or(Scope::World)
    {
        Scope::World => sell_world.to_string(),
        // No datacenter resolved yet: the region is the honest wider name,
        // and it is what the fetch key uses too — `sell_scope_key` is handed
        // this very string, so the body fetched and the place named on the
        // strip cannot be two different markets.
        Scope::Datacenter => datacenter.unwrap_or(region).to_string(),
        Scope::Region => region.to_string(),
    }
}

/// What `sell_place` reads before a sell world resolves — a first paint
/// with no `?world=` and no home-world cookie. Named because two other
/// places have to recognise it: it is not a market, so it is neither
/// fetchable nor comparable.
const UNRESOLVED_PLACE: &str = "…";

/// A place name that can be sent to the API and compared against another.
///
/// The comparison half is the load-bearing one. `sell_scope_is_buy_scope`
/// is a raw name equality, and an unresolved name equal to another
/// unresolved name would answer `true` — the dedupe would then reuse a
/// buy-scope body that `needed_bodies` never put in the set, which is the
/// shape that leaves a revenue cell permanently showing a dash.
fn place_resolved(name: &str) -> bool {
    !name.is_empty() && name != UNRESOLVED_PLACE
}

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
/// Phase F, appended for the same reason: an old serialized `?cols=` must
/// round-trip byte-identically.
const COL_SCOPE_VS_HOME: &str = "scope-vs-home";

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

/// The page-level handles E2's market columns read and write. Page-level,
/// not table-level, because the table remounts whenever one of its
/// resources changes — a cost-basis switch does — and the store, the hook's
/// claim set and the 30-day body all have to survive that. Only a sell-world
/// change resets them, which is exactly the hook's own rule.
///
/// Both signals are handed to every `CellCtx` this page builds, on the
/// server as well as the client. That is deliberate: `spark_with` reads a
/// `None` handle as `Loading` while `late_30` reads it as `Missing`, and
/// `CellValue::LateCount` renders those two with different *text*, so a
/// handle that existed only on the client would be an SSR text-node
/// mismatch. Present on both sides, both stores are empty on both sides,
/// and every lazy cell paints its loading shape either way.
#[derive(Copy, Clone)]
struct MarketHandles {
    /// Filled by `use_visible_enrichment`, called at page level.
    sparklines: RwSignal<SparkStore>,
    /// Filled by the page's 30-day `Effect`; `None` until it lands.
    stats_30: LateStats,
    /// Written by the scroller through the grid's `visible_range` prop.
    visible_range: RwSignal<(usize, usize)>,
    /// The table's sorted rows, mirrored for the hook. Empty unless Trend
    /// or Drift is visible, so the toggle-off page never fetches.
    rows: RwSignal<Vec<(usize, RecipeRow)>>,
}

/// The enrichment key: the item and the quality its statistics came from,
/// so one request serves Trend and Drift and both agree with the 7-day
/// numbers beside them.
fn recipe_spark_key((_, row): &(usize, RecipeRow)) -> SparkKey {
    (row.recipe.item_result, row.stat_hq)
}

/// One wire series to one stored value. The colour driver is computed here
/// (both ends are on the wire), so no cell ever scans the points.
fn spark_entry(s: SparklineSeries) -> (SparkKey, SparkValue) {
    (
        (s.item_id, s.hq),
        SparkValue {
            // Before `points`: the key and this field must be read while
            // `s` is whole, and `points` moves it.
            delta_pct: first_to_last_pct(s.first_price, s.last_price),
            points: s.points,
        },
    )
}

/// The visible window's sparkline fetch. A world that has not resolved yet
/// and a failed request both yield nothing; the hook settles every
/// requested key either way, so a cell goes loading → "—" rather than
/// shimmering forever. Only ever called from the hook's effect (`post_api`
/// is `unreachable!` under SSR).
async fn fetch_recipe_sparklines(
    world: Option<String>,
    keys: Vec<SparkKey>,
) -> Vec<(SparkKey, SparkValue)> {
    let Some(world) = world else {
        return Vec::new();
    };
    match post_sparklines(
        &world,
        SparklinesRequest {
            items: keys,
            hours: Some(RECIPE_TREND_FEED.hours()),
        },
    )
    .await
    {
        Ok(res) => res.series.into_iter().map(spark_entry).collect(),
        Err(_) => Vec::new(),
    }
}

const RECIPE_ENRICHMENT: EnrichmentConfig = EnrichmentConfig {
    prefetch_margin: PREFETCH_MARGIN,
    debounce_ms: DEBOUNCE_MS,
    // The sparklines endpoint rejects a request above 200 keys.
    max_keys_per_request: 200,
};

/// The grid's geometry, hoisted out of the `view!` so the window test
/// derives the batch size from the same numbers the scroller uses.
const RECIPE_GRID: GridLayout = GridLayout {
    viewport_height: 720.0,
    row_height: 60.0,
    header_height: 64.0,
    overscan: 8,
};

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
fn label_scope_vs_home(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_scope_vs_home).to_string()
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
    group: PickerGroup::Market,
};
static SPEC_LAST_SOLD: ColumnSpec = ColumnSpec {
    kind: ColumnKind::LastSold,
    label: label_last_sold,
    group: PickerGroup::Market,
};
static SPEC_VOLUME: ColumnSpec = ColumnSpec {
    kind: ColumnKind::VolumeUnits7,
    label: label_volume,
    group: PickerGroup::Market,
};
static SPEC_VWAP: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Vwap7,
    label: label_vwap,
    group: PickerGroup::Market,
};
static SPEC_TAX: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Tax,
    label: label_tax,
    group: PickerGroup::Market,
};
static SPEC_WORLD: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ListingWorld,
    label: label_world,
    group: PickerGroup::Location,
};
static SPEC_DC: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ListingDc,
    label: label_dc,
    group: PickerGroup::Location,
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
static SPEC_SCOPE_VS_HOME: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ScopeVsHome,
    label: label_scope_vs_home,
    // Travel, beside Hop gain: it answers the same question from the other
    // side of the ledger.
    group: PickerGroup::Travel,
};

// Cell extractors. `Custom` = the page renders it (needs context the row
// does not carry: item names, the world link, the on-hand list button).
fn cell_custom(_: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Custom
}
fn cell_roi(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::RoiBadge(r.return_on_investment)
}
/// The Price slot. Under the toggle it carries an always-present note
/// sub-line: the listing tell when the price fell back to a listing, and
/// the signed percent the price sits above or below the sell world's
/// 7-day sale median — the revenue-side answer to "is this listing-min
/// price real?" (#1202). The median is on the row already (`sell_median`,
/// filled from the body the page always fetches), so the tell costs no
/// request.
fn cell_price(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    if !ctx.preview {
        return CellValue::Gil(r.market_price);
    }
    CellValue::GilWithNote {
        amount: r.market_price,
        note: price_note(r.market_price, r.sell_median, r.revenue_fell_back),
    }
}

/// The Price sub-line, given the row's price and the 7-day median of the
/// *same quality* (`RecipeProfitData::sell_median`).
///
/// `alt` = the price, `input` = the median: this line sits under Price and
/// reads "this price is n% above/below the 7-day median" — the opposite
/// orientation from `rev_alt_cell`, where the alternative is what the cell
/// renders. So a price of 138 against a median of 100 is `+38%` (green) and
/// a price of 75 against the same median is `-25%` (red): the suspiciously
/// cheap listing is the one that reads as a warning, not as good news.
/// `delta_pct` still yields `None` when the median is unpriced *or* equal to
/// the price, so the median basis never shows itself "+0%".
///
/// Two guards keep "above the median" from reading as unbounded good news,
/// which is what shipped in #1264 and what prod showed:
///
/// * At [`is_troll_listing`] — 50x the median, the multiple the rest of this
///   codebase already refuses to price against — the percentage is dropped
///   for a warning. Painting a listing emerald that `flip_estimated_sale_price`
///   discards as unreal tells the user the opposite of what the tool believes.
/// * Below that, the figure is clamped to
///   [`VS_MEDIAN_DISPLAY_CEILING_PCT`], for the same reason ROI is clamped:
///   past it the exact number carries no decision value.
fn price_note(price: i32, median: Option<i32>, listing: bool) -> CellNote {
    if median.is_some_and(|m| is_troll_listing(price, m)) {
        return CellNote::Troll { listing };
    }
    match median.and_then(|m| delta_pct(Some(price), m)) {
        Some(pct) => CellNote::VsMedian {
            listing,
            pct: pct.min(VS_MEDIAN_DISPLAY_CEILING_PCT),
        },
        None if listing => CellNote::ListingFallback,
        None => CellNote::None,
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

/// The delta the cell renders and the comparator sorts by: one function, so
/// a header click can never order rows by a number the cell does not show.
fn scope_vs_home_delta(r: &RecipeProfitData) -> Option<i32> {
    match r.scope_vs_home {
        ScopeVsHome::Pair { place, home, .. } => Some(place - home),
        _ => None,
    }
}

/// The percentage under the delta, against the HOME value: "the wider
/// market is 10% below your world".
///
/// `None` — which `signed_delta_class` renders as no colour at all — in the
/// two cases where a coloured percentage would say the opposite of what it
/// means. Under a listing signal the delta cannot be positive (a region
/// contains the world), so the figure would be a permanent red stripe and
/// the sign already carries the whole message. And a `place` that clears
/// `is_troll_listing` against `home` is not a wide-market finding: it is a
/// home figure so thin that the analyzer refuses to price against it
/// elsewhere, and painting that emerald is exactly the defect #1266
/// removed from the Price tell. Otherwise the same display ceiling
/// applies, for the same reason ROI is clamped.
fn scope_vs_home_pct(state: ScopeVsHome) -> Option<f32> {
    match state {
        ScopeVsHome::Pair {
            place,
            home,
            two_sided: true,
        } if !is_troll_listing(place, home) => {
            delta_pct(Some(place), home).map(|p| p.min(VS_MEDIAN_DISPLAY_CEILING_PCT))
        }
        _ => None,
    }
}

fn cell_scope_vs_home(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::SignedGil {
        delta: scope_vs_home_delta(r),
        pct: scope_vs_home_pct(r.scope_vs_home),
        unavailable: r.scope_vs_home == ScopeVsHome::Unavailable,
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

/// The 30-day VWAP, and its percent against Price.
///
/// The percent is dropped at a wider sell scope, and the absolute figure is
/// not. Same split, and same reason, as the 7-day twin in `price_rows`:
/// `market_price` follows the sell scope while `s.vwap` comes from the
/// 30-day sell-**world** body, so at `datacenter` / `region` the numerator
/// is the cheapest across strictly more worlds and the percentage goes
/// structurally negative page-wide from the user's own setting rather than
/// from the market. The VWAP itself is a sell-world figure whose column
/// says so; only the comparison against a price from somewhere else is
/// meaningless.
fn cell_vwap_30(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    let price = r.price_is_sell_world.then_some(r.market_price);
    CellValue::LateGilWithPct(late_30(r, ctx, move |s| {
        (s.vwap, price.and_then(|p| vwap_pct(p, s.vwap)))
    }))
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

/// Daily sales and Confidence become two-line headers *only* while their
/// header extra is in effect ([`HeaderExtra::header_class`]): baking these
/// into the column table would move the toggle-off DOM, which has to stay
/// byte-identical. `md:flex`, not `md:block` — `SortableHeaderCell` appends
/// `flex flex-col justify-center` for a two-line header and a later
/// `md:block` would override it at md+. The widths are `HEAD_MD`'s and
/// `HEAD_28_MD`'s unchanged; only the padding tightens to make room for
/// line 2, exactly as `FORMULA_HEAD` does for the marked columns.
const HEAD_MD_2: &str = "w-32 shrink-0 px-4 py-2 leading-tight hidden md:flex";
const HEAD_28_MD_2: &str = "w-28 shrink-0 px-4 py-2 leading-tight hidden md:flex";

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
static RECIPE_COLUMNS: [ToolColumnMeta<RecipeRow, SortMode>; 31] = [
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
        spec: &SPEC_SCOPE_VS_HOME,
        id: COL_SCOPE_VS_HOME,
        sort_id: COL_SCOPE_VS_HOME,
        sort: sortability_for(Layer::Computed, Some(SortMode::ScopeVsHome)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_MD,
        default_on: false,
        cell: cell_scope_vs_home,
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
    let visible_rev = RECIPE_COLUMNS
        .iter()
        .filter(|c| !c.id.is_empty() && visible.contains(c.id))
        .filter_map(|c| match c.spec.kind {
            ColumnKind::RevSignal(s) => Some(s),
            _ => None,
        })
        .collect();
    let sort_rev = match sort {
        Some(SortMode::RevSignal(s)) => Some(s),
        _ => None,
    };
    SignalWants {
        visible_cost,
        sort_cost,
        hop: visible.contains(COL_HOP_GAIN) || sort == Some(SortMode::HopGain),
        worlds: visible.contains(COL_HOP_WORLDS) || sort == Some(SortMode::HopWorlds),
        // Flag-off these three are still the placeholders they replaced:
        // every `rev-*` token and `scope-vs-home` is outside
        // `BASE_COLUMN_ORDER`, and both sort modes are `lab_only`, so
        // `visible` cannot hold one and `sort` cannot be one.
        visible_rev,
        sort_rev,
        scope_vs_home: visible.contains(COL_SCOPE_VS_HOME) || sort == Some(SortMode::ScopeVsHome),
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

/// A 30-day column is visible or the sort target, *and* the viewport is
/// wide enough to draw one — the only reason to fetch that body. Not "the
/// toggle is on": with it off neither token survives `parse_visible_cols`
/// (the contract is `BASE_COLUMN_ORDER`) and neither mode survives
/// `SortMode::lab_only`, so this is false by construction.
///
/// `wide` is [`use_wide_viewport`]: both 30-day columns are `hidden md:*`
/// in header and cell alike, so below `md` this body buys 438 KB of
/// transfer and a 3.25 MB main-thread `serde_json` parse for zero pixels.
/// The sort target is gated with them on purpose — its only effect is the
/// order of a column nobody can see, and the recipe analyzer has no mobile
/// sort control, so a `?sort=vwap-30d` on a phone can only have arrived in
/// a link copied from a desktop.
fn stats_30_wanted(visible: &HashSet<&'static str>, sort: Option<SortMode>, wide: bool) -> bool {
    wide && (visible.contains(COL_VOLUME_30D)
        || visible.contains(COL_VWAP_30D)
        || matches!(sort, Some(SortMode::Volume30 | SortMode::Vwap30)))
}

/// The 30-day body's key: the sell world's name when that body is needed,
/// `None` (no fetch) otherwise. Goes through `needed_bodies` like every
/// other body, so the gate lives in one place.
fn stats_30_key(
    formula: &ProfitFormula,
    needs: &RecipeNeeds,
    world: Option<&str>,
) -> Option<String> {
    needed_bodies(formula, needs)
        .contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS))
        .then(|| world.map(str::to_string))
        .flatten()
}

/// Trend or Drift is visible at a width that draws it: the only reason to
/// mirror the table's sorted rows to the page, and so the only reason the
/// sparkline hook ever sees a window to fetch. Same construction as
/// [`stats_30_wanted`] — with the toggle off neither token survives
/// `parse_visible_cols`, so the mirror stays empty and no sparklines POST
/// is issued — and the same `wide` for the same reason: both columns are
/// `hidden md:*`, and the mirror costs ~2.2 KB per scroll settle.
fn spark_rows_wanted(visible: &HashSet<&'static str>, wide: bool) -> bool {
    wide && (visible.contains(COL_TREND) || visible.contains(COL_DRIFT))
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

/// A market column's second line: the window and where the number comes
/// from ("7d · Gilgamesh"), the kit's rule that a sub-label carries window
/// and source. The separator is the same one the signal columns use.
fn window_and_place(i18n: I18nContext<Locale, I18nKeys>, place: &str) -> String {
    format!("{} · {}", t_string!(i18n, recipe_analyzer_window_7d), place)
}

/// The header extra for a market-side column: its recipe-specific tooltip
/// (the flip finder's `analyzer_tooltip_*` describe 30-day resale-quality
/// numbers, which these are not), whether it carries the window line, and
/// the two-line classes the two pre-existing columns switch to while this
/// extra is in effect. `None` for every other kind, so Phase D's four arms
/// keep theirs.
///
/// Trend and Drift pass `None` for the class: their own `HEAD_LAZY_MD`
/// already carries `flex flex-col justify-center gap-0.5`, which the
/// grid's *unsortable* two-line arm does not append for them.
fn market_extra(
    i18n: I18nContext<Locale, I18nKeys>,
    kind: ColumnKind,
    sell_place: &str,
) -> Option<HeaderExtra> {
    // Each `t_string!` here is a plain key, so the tuple holds
    // `&'static str` and the one allocation happens at the end.
    let (title, windowed, header_class) = match kind {
        ColumnKind::SalesPerDay7 => (
            t_string!(i18n, recipe_analyzer_tooltip_daily_sales),
            true,
            Some(HEAD_MD_2),
        ),
        ColumnKind::Confidence => (
            t_string!(i18n, recipe_analyzer_tooltip_confidence),
            true,
            Some(HEAD_28_MD_2),
        ),
        ColumnKind::ProfitPerDay => (
            t_string!(i18n, recipe_analyzer_tooltip_profit_per_day),
            false,
            None,
        ),
        ColumnKind::Trend => (t_string!(i18n, recipe_analyzer_tooltip_trend), true, None),
        ColumnKind::DriftSpark => (t_string!(i18n, recipe_analyzer_tooltip_drift), true, None),
        // The 30-day pair says its window in its label, so line 2 would
        // only repeat it.
        ColumnKind::VolumeUnits30 => (
            t_string!(i18n, recipe_analyzer_tooltip_volume_30d),
            false,
            None,
        ),
        ColumnKind::Vwap30 => (
            t_string!(i18n, recipe_analyzer_tooltip_vwap_30d),
            false,
            None,
        ),
        _ => return None,
    };
    Some(HeaderExtra {
        title: title.to_string(),
        line2: windowed.then(|| HeaderLine2 {
            sub_label: window_and_place(i18n, sell_place),
            pill: None,
        }),
        header_class,
    })
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
    /// The sell-scope revenue signal minus the sell world's own.
    ScopeVsHome,
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
                | SortMode::ScopeVsHome
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
        SortMode::ScopeVsHome => cmp_none_last(
            scope_vs_home_delta(a),
            scope_vs_home_delta(b),
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
    /// Sell-**world** listings (absent before a world resolves). Hop gain's
    /// home run and Scope vs home's home side price against these, and only
    /// these.
    sell_listings: Option<&'a CheapestListingsMap>,
    /// Buy-scope sale stats, indexed. `None` when not fetched.
    buy_stats: Option<&'a StatsIndex>,
    /// Sell-**world** sale stats, indexed. Empty when not fetched. Velocity,
    /// avg price, confidence, last sold, volume, VWAP and the statistics
    /// quality every lazy column keys on all read this, at every sell scope
    /// (spec §4).
    sell_stats: &'a StatsIndex,
    /// Sell-**place** listings: the sell world's map under the default sell
    /// scope, the scope's own map otherwise. The `SignalView` `over` layer
    /// revenue is priced from.
    revenue_listings: Option<&'a CheapestListingsMap>,
    /// Sell-**place** sale stats. `Some(sell_stats)` under the default sell
    /// scope; `None` when a wider scope's body was not fetched, which makes
    /// every `rev-sale-*` cell "—" rather than a sell-world number under a
    /// scope heading. This is also what `ProfitFormula::effective`'s second
    /// argument was computed from at the call site, so a sale revenue
    /// signal with no body has already been downgraded before it gets here.
    revenue_stats: Option<&'a StatsIndex>,
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

/// Scope vs home's three states. Not an `Option`, because a bare `None`
/// would make the dash mean four things at once and the header tooltip can
/// only name one of them.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
enum ScopeVsHome {
    /// The column was not asked for, or the sell scope IS the sell world.
    /// The whole column is dashes and the header tooltip's last sentence is
    /// what explains it, so the cell adds no title of its own.
    #[default]
    Off,
    /// Asked for at a wider scope, but one of the two markets has no figure
    /// for the selected revenue signal — the dominant case under a sale
    /// signal, where the 7-day window covers a small minority of items. The
    /// cell titles its dash, the way `CellValue::LazyPct`'s empty state
    /// does.
    Unavailable,
    /// Both markets answered. `two_sided` is "the revenue signal is a sale
    /// statistic", i.e. the delta can genuinely go either way and a
    /// percentage against `home` answers a real question; under a listing
    /// signal a wider market can only undercut, the sign is the whole
    /// message, and Task 4 drops the percentage rather than painting a
    /// permanent red stripe. Page-wide rather than per-row, and carried on
    /// the row anyway because `CellCtx` is shared with the flip finder and
    /// has twenty exhaustive literals.
    Pair {
        place: i32,
        home: i32,
        two_sided: bool,
    },
}

/// The bare number one revenue signal reads at one place: the cheapest
/// listing with **no** statistics overlay and no cross-place fallback, or
/// the statistic with no listing fallback. `None` means "this place has no
/// such number", never 0.
///
/// One function for both places on purpose. `rev_alt` reads it at the sell
/// place; Scope vs home's home side reads it at the sell world with the
/// same signal, and a fixture that swaps the maps under it can therefore
/// tell the two apart.
fn rev_signal_at(
    listings: Option<&CheapestListingsMap>,
    stats: Option<&StatsIndex>,
    item: i32,
    signal: PriceSignal,
) -> Option<i32> {
    match signal.sale_stat() {
        None => listings
            .and_then(|l| l.find_matching_listings(item).lowest_gil())
            .filter(|p| *p > 0),
        Some(stat) => stats.and_then(|s| stat_only_cheapest(s, item, stat)),
    }
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
    let sell_scope_is_world = inp.formula.sell_scope() == Scope::World;
    let revenue_view = SignalView {
        over: inp.revenue_listings,
        base: inp.buy_listings,
        stats: inp
            .formula
            .revenue_signal()
            .sale_stat()
            .and_then(|stat| inp.revenue_stats.map(|idx| (idx, stat))),
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

        let item = recipe.item_result;
        // The bare sell-PLACE number per revenue signal, no fallback.
        let rev_alt = [
            rev_signal_at(
                inp.revenue_listings,
                inp.revenue_stats,
                item,
                PriceSignal::ListingMin,
            ),
            rev_signal_at(
                inp.revenue_listings,
                inp.revenue_stats,
                item,
                PriceSignal::SaleMin,
            ),
            rev_signal_at(
                inp.revenue_listings,
                inp.revenue_stats,
                item,
                PriceSignal::SaleMedian,
            ),
            rev_signal_at(
                inp.revenue_listings,
                inp.revenue_stats,
                item,
                PriceSignal::SaleAvg,
            ),
        ];
        let revenue_fell_back = rev_alt[inp.formula.revenue_signal().index()] != Some(market_price);

        // Scope vs home: the selected revenue signal at the sell place and
        // on the sell world's own map.
        let scope_vs_home = if !inp.needs.scope_vs_home || sell_scope_is_world {
            ScopeVsHome::Off
        } else {
            let signal = inp.formula.revenue_signal();
            let place = rev_alt[signal.index()];
            let home = rev_signal_at(inp.sell_listings, Some(inp.sell_stats), item, signal);
            match (place, home) {
                (Some(place), Some(home)) => ScopeVsHome::Pair {
                    place,
                    home,
                    two_sided: signal.sale_stat().is_some(),
                },
                _ => ScopeVsHome::Unavailable,
            }
        };

        // Sell-world stats row matching how revenue resolves: prefer
        // the HQ row when the analyzer requires HQ, otherwise NQ, and
        // fall back to whichever quality actually traded.
        let sell_stat = stat_row_either(inp.sell_stats, recipe.item_result, inp.require_hq);
        let stat_hq = sell_stat.map(|s| s.hq).unwrap_or(inp.require_hq);
        let vwap = sell_stat.map(|s| s.vwap).unwrap_or(0);
        // The Price median tell's operand, and only that. Left empty at a
        // wider sell scope: `market_price` then comes from a whole
        // datacenter or region while this median is one world's, so the
        // tell would compare two different markets and read red on nearly
        // every row — the user's own setting wearing the colour #1266 set
        // aside for a suspicious listing. `price_note` degrades to
        // `ListingFallback` / `None` and the sub-line keeps its shape.
        let sell_median = sell_scope_is_world
            .then(|| sell_stat.map(|s| s.median_price).filter(|p| *p > 0))
            .flatten();

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
            // Suppressed at a wider sell scope for the same reason as
            // `sell_median` above, and it is the same mismatch one line
            // apart: the numerator is the scope's cheapest across strictly
            // more worlds while `vwap` is one world's, so the percentage
            // would go structurally negative page-wide from the user's own
            // setting. The absolute `vwap` stays — it is a sell-world
            // figure and its column says so; only the comparison against a
            // price from somewhere else is meaningless. The decision table
            // lists `vwap_pct` under "stays on the sell world" without
            // noticing its numerator moved; this makes the code match that.
            vwap_pct: sell_scope_is_world
                .then(|| vwap_pct(market_price, vwap))
                .flatten(),
            tax: line.tax,
            confidence: sell_stat.map(|s| s.confidence).unwrap_or_default(),
            stat_hq,
            cost_alt,
            rev_alt,
            sell_median,
            revenue_fell_back,
            unpriced: breakdown.unpriced_market_lines,
            hop,
            worlds,
            scope_vs_home,
            price_is_sell_world: sell_scope_is_world,
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

/// The sell-scope bodies' resource key: `(place name, want listings, want
/// statistics)`, or `None` when nothing is needed. Both halves go through
/// [`needed_bodies`] so the gate lives in one place — the rule
/// [`buy_stats_scope_key`] and [`stats_30_key`] already follow — and they
/// are separate booleans because the dedupe against the buy scope can
/// cover one and not the other.
///
/// `place` is the page's `revenue_place`, the same string the strip chip,
/// the picker heading and the live sentence name. An unresolved one is
/// refused outright: `"…"` is not a market, and a request for it is a
/// guaranteed 404 under a label the player is reading as a place.
fn sell_scope_key(
    formula: &ProfitFormula,
    needs: &RecipeNeeds,
    place: &str,
) -> Option<(String, bool, bool)> {
    if !place_resolved(place) {
        return None;
    }
    let bodies = needed_bodies(formula, needs);
    let want_listings = bodies.contains(&BodyRole::CheapestSellScope);
    let want_stats = bodies.contains(&BodyRole::SellScopeStats(SALE_STATS_WINDOW_DAYS));
    (want_listings || want_stats).then(|| (place.to_string(), want_listings, want_stats))
}

/// Where the table reads one half of the revenue side from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RevenueSource {
    /// The sell world's own body: every pre-Phase-F page, and every page at
    /// the default sell scope.
    SellWorld,
    /// The buy-scope body stands in — the sell scope resolved to the same
    /// place name, which is why `needed_bodies` skipped the fetch.
    BuyScope,
    /// The sell scope's own body.
    Scope,
    /// A wider scope whose body did not arrive. Listings fall through
    /// `SignalView`'s base layer to the buy scope and `rev-sale-*` cells
    /// render "—"; the amber banner names the place.
    Missing,
}

fn revenue_source(scope: Scope, is_buy_scope: bool, have_body: bool) -> RevenueSource {
    match scope {
        Scope::World => RevenueSource::SellWorld,
        _ if have_body => RevenueSource::Scope,
        _ if is_buy_scope => RevenueSource::BuyScope,
        _ => RevenueSource::Missing,
    }
}

/// The cheapest map revenue's `over` layer reads.
fn revenue_listings_source(scope: Scope, is_buy_scope: bool, have_body: bool) -> RevenueSource {
    revenue_source(scope, is_buy_scope, have_body)
}

/// The statistics index a sale revenue signal reads. Same rule, named
/// separately because the dedupe can cover one half and not the other:
/// `CheapestBuyScope` is unconditional while `BuyScopeStats(7)` is not.
fn revenue_stats_source(scope: Scope, is_buy_scope: bool, have_body: bool) -> RevenueSource {
    revenue_source(scope, is_buy_scope, have_body)
}

/// One sell-scope payload. Two bodies behind one resource so the Suspense
/// join stays one tuple and the "which half did we get" logic lives in
/// one place, the way [`SellHistory`] already folds the rollup and its
/// failover.
// `ArcResource` values round-trip through `JsonSerdeCodec`, so serde is
// required (both field types already derive it).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct SellScopeBodies {
    listings: Option<CheapestListings>,
    stats: Option<BulkSaleStats>,
    /// The cheapest map was asked for and did not arrive: revenue falls
    /// through `SignalView`'s base layer to the buy scope, which is a
    /// different market from the one every label still names.
    listings_failed: bool,
    /// A statistics body was asked for and did not arrive: the revenue
    /// signal degrades to the listing, exactly as a failed buy-scope or
    /// sell-world body does.
    stats_failed: bool,
}

/// Where a failed sell-scope payload actually left the revenue numbers.
///
/// The arms are **different markets**, and one string cannot describe them.
/// Only `ToBuyScope` means the numbers left the place the strip, the picker
/// heading and the live sentence all still name; the other two mean they
/// stayed and it is the *source* that changed.
///
/// An earlier version of this claimed "a failed cheapest map subsumes the
/// other — with no `over` layer the statistics have nothing to overlay".
/// That is false, and `SignalView::quality` is where to see it: it computes
/// `over.or(base)` and then, when a non-zero stat row exists, returns the
/// **stat price regardless of which layer produced the listing**. The
/// statistics never needed the `over` layer. So cheapest-down /
/// history-up — the ordinary transient shape, since `fetch_sell_scope`
/// joins two independent endpoints — still prices every item with a stat
/// row at this market's own sale median, and telling the player it fell
/// back to where ingredients are bought would be exactly the defect this
/// enum exists to prevent, wearing the other arm's clothes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ScopeFallback {
    /// Nothing here can price revenue: the numbers are the buy scope's.
    BuyScope,
    /// The statistics missed: revenue is this market's own listing.
    ScopeListings,
    /// The cheapest map missed but a sale signal reads the statistics,
    /// which arrived: revenue is this market's own sale history.
    ScopeStats,
}

/// `None` when there is no payload at all — which is every flag-off page
/// and every URL at the default sell scope — or when both halves arrived.
///
/// `revenue_is_sale_stat` is what decides the listings-failed case, because
/// a listing revenue signal never reads the statistics and so cannot be
/// rescued by them.
fn scope_fallback(
    bodies: &Option<SellScopeBodies>,
    revenue_is_sale_stat: bool,
) -> Option<ScopeFallback> {
    let b = bodies.as_ref()?;
    match (b.listings_failed, b.stats_failed) {
        (false, false) => None,
        (false, true) => Some(ScopeFallback::ScopeListings),
        (true, false) if revenue_is_sale_stat => Some(ScopeFallback::ScopeStats),
        (true, _) => Some(ScopeFallback::BuyScope),
    }
}

async fn fetch_sell_scope(name: String, want_listings: bool, want_stats: bool) -> SellScopeBodies {
    // Joined, not sequential. Both are wanted together under a sale revenue
    // signal at a wider scope, both are heavy (the plan budgets ~578 KB for
    // a region), and both sit on the Suspense gate — so awaiting them in
    // turn is table latency the user watches. `routes/analyzer.rs` joins its
    // two independent feeds for the same reason.
    let (listings, stats) = futures::join!(
        async {
            match want_listings {
                true => get_cheapest_listings(&name).await.ok(),
                false => None,
            }
        },
        async {
            match want_stats {
                true => get_sale_stats(&name, SALE_STATS_WINDOW_DAYS).await.ok(),
                false => None,
            }
        }
    );
    SellScopeBodies {
        listings_failed: want_listings && listings.is_none(),
        stats_failed: want_stats && stats.is_none(),
        listings,
        stats,
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
    /// The sell world's name, for the market columns' "7d · ‹place›"
    /// sub-labels. Never the sell scope: those figures come from the sell
    /// world's own data whatever the scope is.
    #[prop(into)]
    sell_place: Signal<String>,
    /// The sell PLACE's name: the sell world under the default sell scope,
    /// its datacenter or region otherwise. Everything that names where
    /// revenue came from reads this; everything that names where the 7-day
    /// figures came from reads `sell_place`.
    #[prop(into)]
    revenue_place: Signal<String>,
    /// The sell scope the page resolved through `sell_scope_for` — `None`
    /// with the lab off and at the default scope. A plain value, not a
    /// signal: the page reads it inside the Suspense closure, so a scope
    /// change rebuilds the table, which is what makes the pricing path
    /// re-resolve (Task 8). Never `#[prop(optional)]` — that strips the
    /// `Option` from the builder setter (Global Constraint 3).
    sell_scope: Option<SellScope>,
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
    /// Phase F's payload: the sell scope's cheapest map and, under a sale
    /// revenue signal, its statistics. `None` at the default sell scope —
    /// which is every flag-off page. Read here for the failure banner; the
    /// pricing side is Task 8's.
    sell_scope_bodies: Option<SellScopeBodies>,
    /// The sell scope resolved to the buy scope's place, so the buy-side
    /// bodies stand in for it (Task 8's resolution reads this).
    sell_scope_is_buy_scope: bool,
    #[prop(into)] home_world_id: Signal<i32>,
    on_pill: Callback<ColumnKind>,
    /// The page-level handles E2's market columns use: the sparkline store
    /// the page's hook fills, the client-only 30-day body, the scroller's
    /// rendered range and the rows mirror the hook reads.
    market: MarketHandles,
    /// `use_wide_viewport()`, created once on the page so a table remount
    /// does not churn the media-query listener. **Fetch path only** — it is
    /// read by the rows-mirror gate below and by nothing that renders. See
    /// [`use_wide_viewport`] for why that boundary is load-bearing.
    wide_viewport: Signal<bool>,
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
    // Only the setter: `Clear all` writes it, and everything that READS the
    // scope inside this component reads the `sell_scope` prop, which the
    // page already put through the lab gate.
    let (_, set_sell_scope) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);
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

    // Where revenue is priced. Resolved once, from the scope the PAGE
    // gated and handed down — never from a `get_untracked()` read of the
    // query signal. The page passes this prop from inside the Suspense
    // closure, so a scope change rebuilds the table and re-runs this;
    // `the_sell_scope_is_counted_and_cleared_like_the_other_market_params`
    // pins both halves of that (the prop read inside the closure, and no
    // untracked read anywhere), because it is otherwise an accidental
    // invariant.
    let sell_scope_value = sell_scope.map(SellScope::scope).unwrap_or(Scope::World);
    let scope_prices = sell_scope_bodies
        .as_ref()
        .and_then(|b| b.listings.clone())
        .map(|l| Arc::new(CheapestListingsMap::from(l)));
    let scope_stats_index: Option<Arc<StatsIndex>> = sell_scope_bodies
        .as_ref()
        .and_then(|b| b.stats.as_ref())
        .map(|s| Arc::new(stats_index(s)));
    let revenue_prices: Option<Arc<CheapestListingsMap>> = match revenue_listings_source(
        sell_scope_value,
        sell_scope_is_buy_scope,
        scope_prices.is_some(),
    ) {
        RevenueSource::SellWorld => sell_world_prices.clone(),
        RevenueSource::BuyScope => Some(prices.clone()),
        RevenueSource::Scope => scope_prices,
        RevenueSource::Missing => None,
    };
    // `revenue_stats_loaded` is what `effective()` downgrades on, so it
    // must say "the body REVENUE reads arrived", never "the sell world's
    // did". `sell_stats_loaded` keeps its own meaning for `hop_signal`.
    let (revenue_stats_index, revenue_stats_loaded): (Option<Arc<StatsIndex>>, bool) =
        match revenue_stats_source(
            sell_scope_value,
            sell_scope_is_buy_scope,
            scope_stats_index.is_some(),
        ) {
            RevenueSource::SellWorld => (Some(sell_stats_index.clone()), sell_stats_loaded),
            RevenueSource::BuyScope => (buy_stats_index.clone(), buy_stats_loaded),
            RevenueSource::Scope => (scope_stats_index, true),
            RevenueSource::Missing => (None, false),
        };

    // The table is the only place that knows how each stats body actually
    // resolved; publish the loaded pair once so the page's strip and info
    // panel derive the fallback from the same two booleans the rows did.
    // The second half is the REVENUE side's body, not the sell world's —
    // `effective()`'s second argument — or a failed sell-scope fetch would
    // leave the strip's dot dark while the headers say the signal fell
    // back. It sits here, below the resolution, for that one reason.
    Effect::new(move |_| stats_loaded.set((buy_stats_loaded, revenue_stats_loaded)));

    let formula = Memo::new(move |_| {
        // Through the SAME function the page and the pricing harness use.
        // The page's `formula_page` answers fetch keys; THIS one prices
        // every row, and a scope seated only on the first is a column of
        // dashes that no unit test can see (Phase E2's median tell).
        let mut f = seat_sell_scope(
            ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope()),
            preview,
            sell_scope,
        )
        .effective(buy_stats_loaded, revenue_stats_loaded);
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
            let m = f.marks(revenue_place.get(), buy_place.get());
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
        // Read once: the market arm below runs for every column in the
        // table, and `sell_place.get()` clones a `String` each time.
        //
        // Two names, one character apart, and the split is the point:
        // `sell_now` is the sell WORLD — the market columns' 7-day figures
        // come from its own data at every sell scope — while `revenue_now`
        // is the sell PLACE the revenue signal was actually read across.
        // They are the same string unless a lab-on URL widened the scope.
        let sell_now = sell_place.get();
        let revenue_now = revenue_place.get();
        for col in RECIPE_COLUMNS.iter() {
            let extra = match col.spec.kind {
                ColumnKind::RevSignal(s) => HeaderExtra {
                    title: signal_help(i18n, s),
                    line2: Some(HeaderLine2 {
                        sub_label: if s == f.revenue_signal() {
                            t_string!(i18n, analyzer_equals_price_slot).to_string()
                        } else {
                            format!("{} · {}", short_signal(i18n, s), revenue_now)
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
                // The sign convention — "negative means the wider market
                // prices lower, and under the cheapest listing it never
                // goes above zero" — exists only in this string, and the
                // catch-all below would drop the column's tooltip
                // entirely. No second line: `HEAD_28_MD` is a one-line
                // class, and the place a `7d · ‹place›` sub-label would
                // name is exactly the thing this column compares two of.
                ColumnKind::ScopeVsHome => HeaderExtra {
                    title: t_string!(i18n, analyzer_scope_vs_home_help).to_string(),
                    line2: None,
                    header_class: None,
                },
                kind => match market_extra(i18n, kind, &sell_now) {
                    Some(extra) => extra,
                    None => continue,
                },
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
        // Resolved above, once per payload. The closure is `move`, so the
        // two revenue handles need their own clones here exactly as the
        // four above do.
        let revenue_prices = revenue_prices.clone();
        let revenue_stats_index = revenue_stats_index.clone();
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
                // The resolved revenue side. At the default sell scope both
                // resolve to `RevenueSource::SellWorld`, i.e. exactly
                // `sell_world_prices` and `Some(&sell_stats_index)` — the
                // values spelled out here before this task, so no
                // pre-Phase-F URL moves.
                revenue_listings: revenue_prices.as_deref(),
                revenue_stats: revenue_stats_index.as_deref(),
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
        // Reactive on the 30-day body: `None` until it lands, and forever
        // when no 30-day column asked for it. `RwSignal::set` notifies
        // whatever it is handed, so the world-change reset below only
        // writes when there is something to clear — otherwise every
        // sell-world change would re-sort the whole table to absorb a
        // `None` -> `None`, on the flag-off page too.
        let stats_30 = market.stats_30.get();
        filter_and_sort(
            &priced(),
            &t,
            &world_names_for_rows,
            mode,
            dir,
            stats_30.as_deref(),
        )
    });

    // Publish the sorted rows for the page's lazy fetch — the hook reads
    // this mirror, so an empty mirror is no request at all. The clone is
    // one `Arc` per row and only happens while a lazy column is on.
    let wants_lazy = Memo::new(move |_| {
        let wide = wide_viewport.get();
        visible_cols.with(|v| spark_rows_wanted(v, wide))
    });
    Effect::new(move |_| {
        if wants_lazy.get() {
            market.rows.set(computed_data.get());
        } else if !market.rows.with_untracked(Vec::is_empty) {
            market.rows.set(Vec::new());
        }
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
        // Lab-gated at the source, unlike the three above: those are
        // pre-lab params, and a bookmarked `?sell-scope=` must not change
        // the flag-off page's "no active filters" hint.
        if sell_scope.is_some() {
            active.push(FILTER_SELL_SCOPE);
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
                    // The Revenue group's heading names where the revenue
                    // signals are read, so it follows the sell scope.
                    sell_place: revenue_place.get(),
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
        // Deliberately not lab-gated: clearing an absent param is a no-op,
        // and a user who turns the lab off after setting a scope must
        // still be able to clear it.
        set_sell_scope(None);
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
        // Copy handles: reading them costs nothing until a lazy cell
        // actually looks inside, inside the row's own closure. Handed over
        // unconditionally — on the server too — so both sides agree what a
        // lazy cell's "no data yet" looks like.
        sparklines: Some(market.sparklines),
        stats_30: Some(market.stats_30),
    });

    // Hoisted out of the `view!` below so both arms of the one child that
    // renders it can move it; its condition and its text are exactly what
    // they were.
    let stats_line = (buy_stats_error || sell_stats_error).then(|| {
        view! {
            <div class="text-amber-400 text-sm">
                {t!(i18n, recipe_analyzer_sale_stats_unavailable)}
            </div>
        }
    });

    view! {
        <div class="flex flex-col gap-6">
            <ActiveListBanner />
            // ONE child, not two. An `Option` child that resolves to `None`
            // still writes a `<!>` hydration marker (tachys; the same rule
            // `sort_header.rs` and Global Constraint 2 turn on), so an
            // unconditional second `.then(..)` beside the line above would
            // add a marker to EVERY page — including every flag-off one,
            // where `sell_scope_bodies` is always `None`. Routing both lines
            // through one `match` keeps the no-payload render byte-identical
            // to today's, which is what
            // `a_failed_sell_scope_body_says_so_instead_of_silently_repricing`
            // renders both shapes to prove.
            // `get_untracked`, deliberately: this banner describes the
            // payload the table was BUILT with, and `sell_scope_bodies` is a
            // plain prop captured at that same moment. Reading the signal
            // reactively here would let the sentence describe one revenue
            // signal while the bodies beside it belong to another. A change
            // that matters re-keys `sell_scope_source` and rebuilds anyway.
            {match scope_fallback(
                &sell_scope_bodies,
                formula.get_untracked().revenue_signal().sale_stat().is_some(),
            ) {
                None => stats_line.into_any(),
                // A sell-scope body was asked for and did not arrive. Name
                // the market — this line is otherwise indistinguishable
                // from the one above it — and name the RIGHT one: the two
                // arms leave the numbers in two different places, and a
                // banner that describes the other one is worse than none.
                Some(fallback) => view! {
                    {stats_line}
                    <div class="text-amber-400 text-sm">
                        {match fallback {
                            // The cheapest map missed, so `SignalView`'s
                            // `over` layer is empty and revenue fell
                            // through to the buy scope while the strip, the
                            // picker heading and the live sentence all
                            // still name the scope.
                            ScopeFallback::BuyScope => view! {
                                {t!(
                                    i18n,
                                    recipe_analyzer_sell_scope_unavailable,
                                    place = move || revenue_place.get(),
                                )}
                            }
                            .into_any(),
                            // Only the statistics missed: `quality` returns
                            // the scope's OWN listing, so the numbers are
                            // still the market this banner names and it is
                            // the signal that degraded.
                            ScopeFallback::ScopeListings => view! {
                                {t!(
                                    i18n,
                                    recipe_analyzer_sell_scope_stats_unavailable,
                                    place = move || revenue_place.get(),
                                )}
                            }
                            .into_any(),
                            // The mirror image, and the one the first cut of
                            // this banner got wrong: the cheapest map missed
                            // but a sale signal reads the statistics, which
                            // arrived. `quality` applies them without needing
                            // the `over` layer, so the numbers are still this
                            // market's — via its sale history rather than its
                            // listings.
                            ScopeFallback::ScopeStats => view! {
                                {t!(
                                    i18n,
                                    recipe_analyzer_sell_scope_listings_unavailable,
                                    place = move || revenue_place.get(),
                                )}
                            }
                            .into_any(),
                        }}
                    </div>
                }
                .into_any(),
            }}
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
                    layout=RECIPE_GRID
                    header_class=RECIPE_HEADER_CLASS
                    row_min_width=RECIPE_ROW_MIN_WIDTH
                    row_class=stripe
                    marks=marks
                    extras=header_extras
                    on_pill=on_pill
                    lab_columns=preview
                    visible_range=market.visible_range
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
    // Phase F's fourth pricing param. Read only through `sell_scope_for`,
    // never raw; the setter strips the default (Task 6).
    let (sell_scope, set_sell_scope) = filter_query_signal::<SellScope>(FILTER_SELL_SCOPE);
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
        // The lab gate, never the raw param: with the toggle off this
        // leaves `Term::Fixed(Scope::World)`, which is what every
        // pre-Phase-F URL has always produced, so `needed_bodies` skips its
        // Phase F block and no new request is issued.
        seat_sell_scope(
            ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope()),
            preview.get(),
            sell_scope(),
        )
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
            .unwrap_or_else(|| UNRESOLVED_PLACE.to_string())
    });
    // The second name, and the whole of Task 5: everything that says where
    // *revenue* came from reads this, everything that says where the 7-day
    // figures came from reads `sell_place`. Equal at every scope with the
    // lab off, and equal at the default scope with it on.
    let revenue_place = Memo::new(move |_| {
        revenue_place_for(
            preview.get(),
            sell_scope(),
            &sell_place.get(),
            datacenter().as_deref(),
            &region.get(),
        )
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
                // The sell PLACE, not the sell world: the header mark
                // beside this chip, the picker's Revenue heading and the
                // live sentence all moved to it in Task 5, and a chip
                // reading `· Gilgamesh` under a mark reading `Aether`
                // describes two markets at once.
                place: Some(revenue_place.into()),
                select: Some(StripSelect {
                    value: Signal::derive(move || revenue_metric().unwrap_or_default().to_string()),
                    options: cost_basis_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<RevenueMetric>().ok();
                        set_revenue_metric(parsed.filter(|m| *m != RevenueMetric::default()));
                    }),
                    aria: t_string!(i18n, formula_change_revenue_aria).to_string(),
                }),
                // The spec's "fourth Market select". Reads through the lab
                // gate like every other consumer of the param, so a
                // flag-off page that somehow rendered this chip would show
                // `world` rather than a bookmarked `?sell-scope=region`.
                place_select: Some(StripSelect {
                    value: Signal::derive(move || {
                        sell_scope_for(preview.get(), sell_scope())
                            .unwrap_or_default()
                            .to_string()
                    }),
                    options: sell_scope_options(i18n),
                    on_change: Callback::new(move |v: String| {
                        let parsed = v.parse::<SellScope>().ok();
                        // `SellScope::default()` is the WORLD, not
                        // `Scope::default()`'s datacenter: stripping the
                        // wrong one here would rewrite every URL.
                        set_sell_scope(parsed.filter(|s| *s != SellScope::default()));
                    }),
                    aria: t_string!(i18n, formula_change_sell_scope_aria).to_string(),
                }),
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
            // Honest constants, not placeholders: this key answers the
            // BUY-scope body alone, and `needed_bodies`' sell-scope rules
            // are reached only from `sell_scope_key`, which builds its own
            // `RecipeNeeds` from the page's real gates.
            sell_scope_is_buy_scope: false,
            rev_signals: BTreeSet::new(),
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

    // E2's market columns. One set of handles for the page, so a table
    // remount keeps every settled sparkline key and the 30-day body.
    let market = MarketHandles {
        sparklines: RwSignal::new(SparkStore::default()),
        stats_30: RwSignal::new(None),
        visible_range: RwSignal::new((0, 0)),
        rows: RwSignal::new(Vec::new()),
    };
    // Is the viewport wide enough to *draw* a lazy market column? Every one
    // of the four is `hidden md:*` in header and cell alike, so below `md`
    // both bodies below are paid for and never seen: 438 KB transferred and
    // 3.25 MB parsed on the main thread for the 30-day pair, ~2.2 KB per
    // scroll settle for the sparkline pair.
    //
    // Created here, at page level, for the same reason the handles above
    // are: the table remounts whenever one of its resources changes, and
    // the media-query listener should not churn with it. Read on the fetch
    // path only — see `use_wide_viewport` — never in a `view!`, so SSR and
    // the first client render are byte-identical to what they are today.
    let wide_viewport = use_wide_viewport();

    // Trend and Drift: the flip finder's visible-window fetch, scoped to the
    // sell world (the sparklines endpoint takes a world, never a datacenter).
    // The hook's own effect resets the store when that world changes. Its
    // rows come from the table's mirror, which stays empty unless one of
    // those two columns is on, so the toggle-off page asks for nothing.
    use_visible_enrichment(
        market.sparklines,
        market.rows.into(),
        market.visible_range.into(),
        sell_world_name.into(),
        recipe_spark_key,
        fetch_recipe_sparklines,
        RECIPE_ENRICHMENT,
    );

    // The 30-day statistics body: client-only, one per sell world, fetched
    // the first time a 30-day column is visible or the sort target and kept
    // across column toggles. Never a `Resource`: it must not join the
    // Suspense gate, or the whole table would wait 700 ms for a column two
    // players use.
    // `(wanted, key)`, split deliberately: `stats_30_key` returns `None`
    // both when no column asked for the body and when there is no sell
    // world to ask about, and those two need different endings. Nothing
    // asked: do nothing. Asked but worldless: settle the cells, because a
    // body that can never arrive must not leave them shimmering — the
    // sparkline pair already degrades that way, and this is the same
    // "an empty index means settled" convention the failed fetch uses.
    let stats_30_source = Memo::new(move |_| {
        let needs = RecipeNeeds {
            stats_30: stats_30_wanted(&visible_cols.get(), sort_mode.get(), wide_viewport.get()),
            ..RecipeNeeds::default()
        };
        let formula = formula_page.get();
        let wanted = needed_bodies(&formula, &needs)
            .contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS));
        (
            wanted,
            stats_30_key(&formula, &needs, sell_world_name.get().as_deref()),
        )
    });
    let stats_30_fetching = StoredValue::new(false);
    let stats_30_world = StoredValue::new(None::<String>);
    // Bumped once per spawn. The world alone cannot tell two runs apart:
    // a flip A -> B -> A while A is still in flight leaves the first
    // response passing a world check that the second run also passes.
    let stats_30_gen = StoredValue::new(0u64);
    Effect::new(move |_| {
        let world = sell_world_name.get();
        // A world change drops the stored body even when nothing wants one
        // right now: it describes the old world.
        if stats_30_world.get_value() != world {
            stats_30_world.set_value(world);
            if market.stats_30.with_untracked(Option::is_some) {
                market.stats_30.set(None);
            }
            stats_30_fetching.set_value(false);
        }
        let (wanted, key) = stats_30_source.get();
        let Some(name) = key else {
            if wanted && market.stats_30.with_untracked(Option::is_none) {
                market.stats_30.set(Some(Arc::new(StatsIndex::default())));
            }
            return;
        };
        if stats_30_fetching.get_value() || market.stats_30.with_untracked(Option::is_some) {
            return;
        }
        stats_30_fetching.set_value(true);
        let my_gen = stats_30_gen.get_value() + 1;
        stats_30_gen.set_value(my_gen);
        let captured = Some(name.clone());
        leptos::task::spawn_local(async move {
            // A failed fetch stores the empty index on purpose: the cells
            // settle to "—" instead of shimmering forever, and the next
            // world change is what retries.
            let index = get_sale_stats(&name, STATS_30_WINDOW_DAYS)
                .await
                .map(|body| stats_index(&body))
                .unwrap_or_default();
            // Past the await the page may be gone and the world may have
            // moved: every touch is a `try_*`.
            if verdict(sell_world_name.try_get_untracked(), &captured) != Verdict::Proceed {
                return;
            }
            // A newer run owns the flag and the store from here on; leave
            // both to it, exactly as the stale-world path does.
            if stats_30_gen.try_get_value() != Some(my_gen) {
                return;
            }
            let _ = market.stats_30.try_set(Some(Arc::new(index)));
            let _ = stats_30_fetching.try_update_value(|f| *f = false);
        });
    });

    // The sell scope resolved to the same place the buy side already
    // fetches: its cheapest body holds these rows, and (when a sale cost
    // signal fetched it) its statistics body does too. A raw name equality,
    // guarded on both sides — `"…" == "…"` before a world resolves would
    // claim a body nobody fetched.
    let sell_scope_is_buy_scope = Memo::new(move |_| {
        place_resolved(&revenue_place.get())
            && place_resolved(&buy_scope_name.get())
            && revenue_place.get() == buy_scope_name.get()
    });

    // Phase F's bodies. A formula body, so it joins the Suspense gate: the
    // table cannot price a row without the map revenue comes from. `None` —
    // no fetch — at the default sell scope, which is every flag-off page and
    // every URL that has not asked for a wider one.
    let sell_scope_source = Memo::new(move |_| {
        let formula = formula_page.get();
        let signals = needs_page.get();
        let needs = RecipeNeeds {
            sell_scope_is_buy_scope: sell_scope_is_buy_scope.get(),
            // The page's REAL alias gate. `needed_bodies` computes
            // `BuyScopeStats` from this, and the sell side's dedupe only
            // fires when that body is actually in the set — a defaulted
            // `false` here would claim a body nobody fetched and leave
            // every `rev-sale-*` cell permanently "—".
            buy_scope_is_sell_world: buy_scope_is_sell_world.get(),
            cost_signals: signals.cost,
            // `NeededSignals::rev`'s first production reader. Until this
            // line it was written by `needed_signals` and read by nothing
            // that ships, and the dead-code lint could not say so: the
            // derived `Debug`/`PartialEq` count as reads.
            rev_signals: signals.rev,
            ..RecipeNeeds::default()
        };
        // The one name. `revenue_place` is what the strip chip, the picker
        // heading and the live sentence say, fallback arm included, so the
        // body fetched, the body deduped against and the body labelled are
        // the same market.
        let place = revenue_place.get();
        sell_scope_key(&formula, &needs, &place)
    });
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

    // Constructed LAST of the page's resources, deliberately. Every
    // `ArcResource` takes a hydration id at construction and serialises one
    // entry into the SSR payload whether or not it ever resolves, so a new
    // one is an unavoidable flag-off byte delta — but built here it APPENDS
    // an id instead of renumbering the three resources that would otherwise
    // follow it. One extra entry rather than one extra plus three shifted.
    let sell_scope_bodies = ArcResource::new(
        sell_scope_source,
        move |key: Option<(String, bool, bool)>| async move {
            match key {
                Some((name, listings, stats)) => {
                    Some(fetch_sell_scope(name, listings, stats).await)
                }
                None => None,
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
                                let scoped = sell_scope_for(preview.get(), sell_scope())
                                    .is_some_and(|s| s.scope() != Scope::World);
                                // The connectives are translated: this is a
                                // template, never a `format!` in Rust.
                                //
                                // Two keys, not one edited key: "on {{sell}}"
                                // is right for a world and wrong for a
                                // datacenter, and rewording the shared string
                                // would move the default page's sentence.
                                if scoped {
                                    t_string!(
                                        i18n,
                                        recipe_analyzer_calc_formula_live_scoped,
                                        revenue = label_of(f.revenue_signal()),
                                        sell = revenue_place.get(),
                                        tax = t_string!(i18n, formula_term_tax).to_string(),
                                        cost = label_of(f.cost_signal()),
                                        buy = buy_place.get()
                                    )
                                    .to_string()
                                } else {
                                    t_string!(
                                        i18n,
                                        recipe_analyzer_calc_formula_live,
                                        revenue = label_of(f.revenue_signal()),
                                        sell = revenue_place.get(),
                                        tax = t_string!(i18n, formula_term_tax).to_string(),
                                        cost = label_of(f.cost_signal()),
                                        buy = buy_place.get()
                                    )
                                    .to_string()
                                }
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
                        // A formula body: the table cannot price a row
                        // without the map revenue comes from, so it joins
                        // the gate rather than filling in late.
                        let scope_bodies = sell_scope_bodies.get();
                        match (listings, stats, sell_listings, history, raw, scope_bodies) {
                            (
                                Some(Ok(listings)),
                                Some(stats),
                                Some(sell_listings),
                                Some(history),
                                Some(raw),
                                Some(bodies),
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
                                        revenue_place=revenue_place
                                        sell_scope=sell_scope_for(preview.get(), sell_scope())
                                        buy_place=buy_place
                                        strip_terms=Callback::new(move |()| strip_terms())
                                        preview=preview.get()
                                        needs=needs_page
                                        buy_stats_aliased=buy_scope_is_sell_world.get()
                                        sell_scope_bodies=bodies
                                        sell_scope_is_buy_scope=sell_scope_is_buy_scope.get()
                                        home_world_id=home_world_id
                                        on_pill=on_pill
                                        market=market
                                        wide_viewport=wide_viewport
                                    />
                                }.into_any()
                            }
                            (Some(Err(e)), _, _, _, _, _) => {
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
    // Only the tests read these — the window ones, the median tell's sign,
    // which asserts the colour the note renders in rather than only its
    // sign, and `Term`, the ledger slot's discriminant. (`Scope` was here
    // too until Task 3 gave it a production reader on this page; it is
    // imported at module level now.) Imported here rather than at
    // module level: they have no
    // production caller on this page, and `--all-targets` also compiles the
    // lib without `cfg(test)`, where `-D warnings` turns an unused import
    // into a failure.
    use crate::analysis::{DELTA_DEAD_BAND_PCT, signed_delta_class};
    use crate::analyzer_kit::enrichment::{chunk_keys, visible_keys};
    use crate::analyzer_kit::formula::Term;
    use crate::components::virtual_scroller::{
        first_visible_row, rendered_range, rows_for_viewport,
    };
    use std::collections::BTreeSet;
    use ultros_api_types::cheapest_listings::CheapestListingItem;
    use xiv_gen::ClassJobId;

    /// This module's production half. `include_str!` pulls in the test
    /// module's own source, so a literal needle would satisfy itself;
    /// splitting on the test attribute keeps every search below to the code
    /// that actually ships. Split on two anchors rather than one needle
    /// holding a real newline: a CRLF checkout would make that needle miss.
    fn production_source() -> &'static str {
        const SRC: &str = include_str!("recipe_analyzer.rs");
        let (production, rest) = SRC
            .split_once(&format!("#[cfg({})]", "test"))
            .expect("the production half ends at the test module attribute");
        assert!(
            rest.trim_start().starts_with(&format!("mod {} {{", "test")),
            "the attribute ending the production half must be the test module's"
        );
        production
    }

    /// `production_source()` with all whitespace removed, so a needle
    /// cannot be broken by rustfmt's line wrapping (or by a CRLF
    /// checkout). Assert against this whenever the thing being pinned is a
    /// multi-argument call: rustfmt breaks any call it cannot fit in 100
    /// columns onto one line per argument, and a needle written as one
    /// line then pins text the formatter will never emit — a test that can
    /// only fail.
    fn production_squeezed() -> String {
        production_source()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

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
        // Phase F. Not addable from `+ Filter` (it is a Market control, like
        // the three above), but it IS a bookmark contract and IS counted in
        // the active-filter list, so its key is pinned here with them.
        assert_eq!(FILTER_SELL_SCOPE, "sell-scope");
        assert!(
            !ADDABLE_FILTERS.contains(&FILTER_SELL_SCOPE),
            "sell-scope is a Market control, not a row filter"
        );
        // Set by clicking a cheapest-listing world/DC cell, not the menu.
        assert_eq!(FILTER_LISTING_WORLD, "listing-world");
        assert_eq!(FILTER_LISTING_DC, "listing-dc");
    }

    /// Both Phase F gates, together, because they are two halves of one
    /// rule: with the lab off the param is dropped, and a formula that
    /// never reaches `with_sell_scope` is `Term::Fixed(World)` — the exact
    /// value `recipe_from_query` has produced since Phase A, so the
    /// flag-off ledger is `PartialEq`-identical to today's.
    #[test]
    fn the_sell_scope_gate_and_its_seating_are_inert_with_the_toggle_off() {
        let base = ProfitFormula::recipe_from_query(None, None, None);
        for param in [
            None,
            Some(SellScope(Scope::Region)),
            Some(SellScope(Scope::Datacenter)),
            Some(SellScope::default()),
        ] {
            assert_eq!(sell_scope_for(false, param), None, "{param:?}");
            let off = seat_sell_scope(base, false, param);
            assert_eq!(off.sell_scope, Term::Fixed(Scope::World), "{param:?}");
            assert_eq!(off, base, "the flag-off ledger must be the same value");
        }
        // Lab on: the param passes through, and `None` still seats nothing.
        assert_eq!(sell_scope_for(true, None), None);
        assert_eq!(seat_sell_scope(base, true, None), base);
        assert_eq!(
            sell_scope_for(true, Some(SellScope(Scope::Datacenter))),
            Some(SellScope(Scope::Datacenter))
        );
        assert_eq!(
            seat_sell_scope(base, true, Some(SellScope(Scope::Region))).sell_scope(),
            Scope::Region
        );
        // The one combination the loop above cannot reach: a hand-typed
        // `?sell-scope=world` with the lab ON. The gate passes it through,
        // so `with_sell_scope` runs and the slot becomes
        // `Term::Select(World)` — which is NOT `PartialEq`-equal to the
        // untouched `Term::Fixed(World)`, so the whole `ProfitFormula`
        // compares unequal and a `Memo<ProfitFormula>` would notify.
        //
        // It prices identically: `sell_scope()` collapses both terms to
        // `Scope::World`, so every lookup reads the same market and Global
        // Constraint 8 is untouched. Inert only while nothing renders on
        // the discriminant; asserted here, in the task that owns the
        // setter, so the day something does render on it this is where it
        // is noticed rather than in a screenshot.
        assert_eq!(
            sell_scope_for(true, Some(SellScope::default())),
            Some(SellScope::default())
        );
        let on_world = seat_sell_scope(base, true, Some(SellScope::default()));
        assert_eq!(on_world.sell_scope, Term::Select(Scope::World));
        assert_ne!(
            on_world, base,
            "lab-on `?sell-scope=world` moves the term's discriminant"
        );
        assert_eq!(
            on_world.sell_scope(),
            base.sell_scope(),
            "…and prices identically to the untouched ledger"
        );
        assert_eq!(on_world.sell_scope(), Scope::World);
    }

    /// One strip term can carry BOTH selects — the signal and the place —
    /// and still show the resolved place name between them. That is the
    /// mechanism behind the spec's "fourth Market select": the cost chip
    /// already has two, and Phase F gives the revenue chip its second.
    ///
    /// This renders a hand-built term, so it pins the COMPONENT, not the
    /// page's `strip_terms` (a closure over the page's signals, which no
    /// unit test can call). The production half is pinned by the
    /// source-read assertions below it.
    #[test]
    fn a_strip_term_carries_both_a_signal_select_and_a_place_select() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let terms = vec![
                StripTerm::fixed(TermRole::Result, Signal::derive(|| "Profit / unit".into())),
                StripTerm {
                    role: TermRole::Revenue,
                    label: Signal::derive(String::new),
                    place: Some(Signal::derive(|| "Aether".to_string())),
                    select: Some(StripSelect {
                        value: Signal::derive(|| "listing-min".to_string()),
                        options: cost_basis_options(i18n),
                        on_change: Callback::new(|_: String| {}),
                        aria: "signal".into(),
                    }),
                    place_select: Some(StripSelect {
                        value: Signal::derive(|| "datacenter".to_string()),
                        options: sell_scope_options(i18n),
                        on_change: Callback::new(|_: String| {}),
                        aria: t_string!(i18n, formula_change_sell_scope_aria).to_string(),
                    }),
                    degraded: Signal::derive(|| false),
                },
            ];
            let html = view! { <FormulaStrip terms=terms layout=StripLayout::Stacked /> }.to_html();
            assert_eq!(
                html.matches("<select").count(),
                2,
                "one revenue term, two selects: {html}"
            );
            assert!(
                html.contains("Aether"),
                "the resolved place stays visible: {html}"
            );
            assert!(html.contains("value=\"region\""), "{html}");
            // The aria-label is the sell scope's own, not the buy side's:
            // handing `formula_change_scope_aria` to this select would
            // render "Change where ingredients are bought" over a control
            // that moves the sale price, and every assertion above would
            // still pass.
            assert!(
                html.contains(&format!(
                    "aria-label=\"{}\"",
                    t_string!(i18n, formula_change_sell_scope_aria)
                )),
                "{html}"
            );
        });

        // The production strip: the revenue term really does grow the
        // second select, and the page really does end up with four.
        let production = production_source();
        assert_eq!(
            production
                .matches("place_select: Some(StripSelect {")
                .count(),
            2,
            "the cost chip's and the revenue chip's — four selects on the strip"
        );
        // Both of the next two are scoped to the text BEFORE the second
        // `place_select`, i.e. the revenue chip alone. Whole-file existence
        // checks would stay green if the two chips' aria keys were simply
        // swapped — which re-introduces exactly the defect the second
        // assertion exists to catch, one chip over. Squeezed, so rustfmt's
        // wrapping cannot decide whether a needle matches.
        let second_select = production
            .match_indices("place_select: Some(StripSelect {")
            .nth(1)
            .expect("two place_selects on the strip")
            .0;
        let revenue_chip: String = production[..second_select]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(
            revenue_chip.contains(&format!("options:{}(i18n),", "sell_scope_options")),
            "…and the revenue one offers the sell-scope tokens"
        );
        // …under its own aria-label. The rendered assertion above cannot
        // see this: the term it renders is built in this test. Writing
        // this select by copying the cost chip's — which is how it would
        // be written — leaves `formula_change_scope_aria` behind, and
        // "Change where ingredients are bought" then narrates a control
        // that moves the sale price. An unused i18n key raises no warning,
        // so nothing else in the build would notice.
        assert!(
            revenue_chip.contains(&format!(
                "aria:t_string!(i18n,{}).to_string(),",
                "formula_change_sell_scope_aria"
            )),
            "…and names itself with the sell side's aria-label"
        );
        // The chip's own place name follows the sell PLACE, not the sell
        // world: it was the last revenue-side label still naming the
        // world, and leaving it would put `· Gilgamesh` on the chip beside
        // a header mark reading `Aether` on the same screen. Asserted in
        // both directions — the positive alone would survive a chip that
        // grew a second `place`, and the negative alone would survive the
        // whole `place` field being deleted.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(&format!("place:Some({}.into()),", "revenue_place")),
            "the revenue chip names the sell PLACE"
        );
        assert!(
            !squeezed.contains(&format!("place:Some({}.into())", "sell_place")),
            "…and no strip chip names the sell WORLD any more"
        );
    }

    /// The three sell-scope tokens are the buy-scope tokens, and every one
    /// of them has a label in every locale — a select whose option renders
    /// blank is how a bookmarked value becomes unreachable. The `world`
    /// label is its own key, not the buy side's: "This world only" belongs
    /// to a buying sentence, and this one is where a price is READ.
    #[test]
    fn every_sell_scope_token_has_a_picker_label() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let options = sell_scope_options(i18n);
            let tokens: Vec<&str> = options.iter().map(|(t, _)| *t).collect();
            assert_eq!(tokens, ["world", "datacenter", "region"]);
            for (token, label) in &options {
                assert!(!label.is_empty(), "{token} has no label");
                assert_eq!(token.parse::<SellScope>().unwrap().to_string(), *token);
            }
            assert_ne!(
                options[0].1,
                t_string!(i18n, buy_scope_home_world),
                "the sell side's `world` label is its own string"
            );
        });
    }

    /// The sell scope is counted like the three pricing params it sits
    /// beside, and Clear all resets it — but the count is driven by the
    /// prop the page already gated, so a bookmarked `?sell-scope=` cannot
    /// change the flag-off page's "no active filters" hint.
    #[test]
    fn the_sell_scope_is_counted_and_cleared_like_the_other_market_params() {
        let production = production_source();
        assert!(
            production.contains(&format!("if {}.is_some() {{", "sell_scope")),
            "active_filters counts the lab-gated prop, not a raw query read"
        );
        assert!(
            production.contains(&format!("{}(FILTER_SELL_SCOPE)", "active.push")),
            "…and pushes the same key the URL uses"
        );
        assert!(
            production.contains(&format!("{}(None);", "set_sell_scope")),
            "Clear all must reset it"
        );
        assert!(
            !production.contains(&format!("{}.get_untracked()", "sell_scope")),
            "the table never reads the scope untracked: the page resolves it \
             inside the Suspense closure and hands it down"
        );
        // The positive half of that rule, and the thing the plan's own
        // self-review called out as unpinned: the scope has to be READ
        // inside the Suspense closure, because that read is what makes a
        // scope change rebuild the table and re-run the pricing memo. The
        // negative assertion above only bans the wrong way of doing it.
        // Squeezed (rustfmt does not touch `view!` bodies, but a needle
        // that survives reformatting either way costs nothing), and
        // anchored on the `sell_scope=` prop prefix: the identical call is
        // written twice more in this module — the strip select's `value`
        // and the live sentence's `scoped` — and only this one is the
        // hand-off that forces the rebuild. (The brief said three; the
        // third, `revenue_place`, goes through `revenue_place_for`, which
        // holds the gate itself.)
        assert!(
            production_squeezed().contains("sell_scope=sell_scope_for(preview.get(),sell_scope())"),
            "the page must resolve the scope INSIDE the Suspense closure and \
             pass it as a prop; nothing else rebuilds the table when it moves"
        );
        // The setter strips the SELL side's default. `SellScope::default()`
        // is the world; `Scope::default()` is the datacenter, and the
        // page's other three selects all spell the second form — so a
        // copy-pasted `!= Scope::default()` here would leave
        // `?sell-scope=world` in every URL, strip `?sell-scope=datacenter`
        // out of the ones that meant it, and re-price them on the world.
        // (It would not even compile against `Option<SellScope>`; this pins
        // the shape anyway, because the fix that does compile is
        // `SellScope(Scope::default())`.)
        assert!(
            production_squeezed()
                .contains("set_sell_scope(parsed.filter(|s|*s!=SellScope::default()));"),
            "the sell-scope setter strips the sell side's default, not the buy side's"
        );
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

    const ALL_SORT_MODES: [SortMode; 25] = [
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
        SortMode::ScopeVsHome,
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
        assert_eq!(SortMode::ScopeVsHome.to_string(), "scope-vs-home");
        assert_eq!(
            "scope-vs-home".parse::<SortMode>(),
            Ok(SortMode::ScopeVsHome)
        );
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
                // Phase F, appended for the same reason E2's five were.
                "scope-vs-home",
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
        /// Give the sell world BOTH qualities, the HQ row four times
        /// dearer on even item ids and four times cheaper on odd ones. The
        /// NQ-only and HQ-only fixtures above cannot tell `stat_row_either`
        /// (quality-matched) from `stat_only_cheapest` (cheaper of the two)
        /// apart, because with one quality present they return the same
        /// row — which is why #1264's Price tell shipped reading the wrong
        /// one. Prod's shape is the dearer half (one item in five carries
        /// an HQ median more than 3x its NQ one); the cheaper half is here
        /// because it makes the two lookups disagree without `require_hq`,
        /// which costs every ingredient HQ and leaves only a couple of rows
        /// past the drop rule.
        stats_both: bool,
        /// With `stats_both`, write HQ dearer for EVERY item rather than
        /// only the even ids. The alternating split gives the NQ run
        /// divergence in both directions; a run that keeps only a couple of
        /// rows needs the direction guaranteed, not drawn.
        hq_dearer_only: bool,
        require_hq: bool,
        /// The sell scope. `None` = `Scope::World`, i.e. today's behaviour
        /// and `Term::Fixed`.
        sell_scope: Option<Scope>,
        /// Hand the pass the scope maps from `scope_fixture`. Off with a
        /// non-`World` scope models "the body was asked for and failed",
        /// where revenue falls through to the buy-scope layer.
        scope_bodies: bool,
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
                stats_both: false,
                hq_dearer_only: false,
                require_hq: false,
                sell_scope: None,
                scope_bodies: false,
            }
        }
    }

    /// The HQ figure `RunOpts::stats_both` writes for one NQ figure.
    /// Dearer on the even ids, cheaper on the odd ones — unless
    /// `hq_dearer_only`, which makes every item dearer.
    fn hq_scaled(item_id: i32, nq: i32, dearer_only: bool) -> i32 {
        if dearer_only || item_id % 2 == 0 {
            nq * 4
        } else {
            nq / 4
        }
    }

    /// The sell-scope fixture: the HOME price view, scaled.
    ///
    /// Derived through a `SignalView` with the same layering the pass uses,
    /// so every quality the home run can resolve is present here too and
    /// scaled the same way. NQ-only would leave HQ falling through to the
    /// buy scope and pin `min(lq, hq)` at the unscaled number for most ids.
    ///   * even output ids  -> HALF the home price (a wider market
    ///     undercuts: the realistic direction),
    ///   * odd output ids   -> DOUBLE it (impossible in production, and
    ///     exactly why it is here: a lookup that read the home map, or took
    ///     `min(scope, home)`, would still pass on the even half alone),
    ///   * every third recipe -> absent from the scope map entirely, so the
    ///     `SignalView` `over` layer falls through to the buy-scope `base`.
    ///
    /// Statistics move the same three ways, and every figure of theirs that
    /// is NOT a price is stamped with a value the sell world's own row does
    /// not carry - see the comment on the `ItemSaleStats` literal below.
    ///
    /// Ingredients that are not themselves a fixture output are scaled in
    /// too, for the reason given at the second loop.
    fn scope_fixture(
        recipes: &[&'static Recipe],
        buy: &CheapestListingsMap,
        sell: &CheapestListingsMap,
        sell_stats: &StatsIndex,
    ) -> (CheapestListingsMap, StatsIndex) {
        let home = SignalView {
            over: Some(sell),
            base: buy,
            stats: None,
        };
        // Keyed on the ITEM's own parity, so an item scales the same way
        // whether it is reached as an output or as an ingredient.
        let scale_at = |id: i32, p: i32| if id % 2 == 0 { p / 2 } else { p * 2 };
        let scoped_rows = |item: i32| -> Vec<CheapestListingItem> {
            let pair = home.find_matching_listings(item);
            [(false, pair.lq), (true, pair.hq)]
                .into_iter()
                .filter_map(|(hq, found)| {
                    found.map(|l| CheapestListingItem {
                        item_id: item,
                        hq,
                        cheapest_price: scale_at(item, l.price),
                        world_id: 9,
                    })
                })
                .collect()
        };
        let outputs: BTreeSet<i32> = recipes.iter().map(|r| r.item_result).collect();
        let mut listings = Vec::new();
        let mut stats = StatsIndex::new();
        for (i, r) in recipes.iter().enumerate() {
            if i % 3 == 2 {
                continue; // absent from the scope entirely
            }
            let out = r.item_result;
            let scale = |p: i32| scale_at(out, p);
            listings.extend(scoped_rows(out));
            for hq in [false, true] {
                if let Some(row) = sell_stats.get(&(out, hq)) {
                    stats.insert(
                        (out, hq),
                        ItemSaleStats {
                            min_price: scale(row.min_price),
                            median_price: scale(row.median_price),
                            avg_price: scale(row.avg_price),
                            // Velocity, volume, VWAP, last sold and the
                            // confidence band are sell-WORLD figures at
                            // every sell scope. Scaling the three prices
                            // alone leaves them equal to the sell world's
                            // by construction (`..*row`), so a pass that
                            // read THIS map for them would agree with one
                            // that read the world's - verified by mutation:
                            // with these five left at `..*row`,
                            // `stat_row_either(revenue_stats, ..)` passes
                            // `the_sell_worlds_own_figures_ignore_the_sell_scope`.
                            // A fixture that does not vary the
                            // discriminator cannot tell two lookups apart.
                            num_sold: row.num_sold + 1,
                            units_sold: row.units_sold + 5,
                            vwap: row.vwap + 7,
                            last_sold_unix: row.last_sold_unix + 3_600,
                            confidence: ConfidenceBand::High,
                            ..*row
                        },
                    );
                }
            }
        }
        // A real sell-scope cheapest-listings body carries the INGREDIENTS
        // too, not only the outputs, and leaving them out is not neutral:
        // with an output-only map, Hop gain's home run (`home_view`, whose
        // `over` layer must stay `None`) reads nothing but ingredients, so
        // pointing it at the scope map changes no number and
        // `assert_eq!(r.hop, h.hop)` cannot fail. Verified by mutation.
        // Items that are some fixture recipe's OUTPUT are skipped, so the
        // "absent from the scope" class stays absent.
        let mut seen = BTreeSet::new();
        for r in recipes.iter() {
            for id in r.ingredient.iter() {
                if *id == 0 || outputs.contains(id) || !seen.insert(*id) {
                    continue;
                }
                listings.extend(scoped_rows(*id));
            }
        }
        (
            CheapestListingsMap::from(CheapestListings {
                cheapest_listings: listings,
            }),
            stats,
        )
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
        } else if o.stats_both {
            let mut both = index.clone();
            for s in &stats.stats {
                let scale = |p: i32| hq_scaled(s.item_id, p, o.hq_dearer_only);
                both.insert(
                    (s.item_id, true),
                    ItemSaleStats {
                        hq: true,
                        min_price: scale(s.min_price),
                        median_price: scale(s.median_price),
                        avg_price: scale(s.avg_price),
                        ..*s
                    },
                );
            }
            both
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
        let (scope_listings, scope_stats) = scope_fixture(&recipes, &buy, &sell, &sell_index);
        let wider = o.sell_scope.is_some_and(|s| s != Scope::World);
        let use_scope = wider && o.scope_bodies;
        // The SAME resolver the table runs, so the harness cannot pick a
        // map by a rule production does not use. `is_buy_scope` is `false`
        // here — the fixture's buy maps are a different place — and that
        // arm is covered directly by
        // `the_table_resolves_the_revenue_side_from_the_pages_scope`.
        let revenue_at = revenue_source(o.sell_scope.unwrap_or(Scope::World), false, use_scope);
        // Seated through the SAME function production uses. Two
        // constructions of one ledger is exactly how Phase E2's median tell
        // shipped past a green suite; `seat_sell_scope(f, true, None)`
        // returns `f`, so every existing run is byte-identical.
        let formula = seat_sell_scope(
            ProfitFormula::recipe_from_query(Some(cost), Some(revenue), o.scope),
            true,
            o.sell_scope.map(SellScope),
        );
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
            revenue_listings: match revenue_at {
                RevenueSource::SellWorld => o.sell_listings.then_some(&sell),
                RevenueSource::BuyScope => Some(&buy),
                RevenueSource::Scope => Some(&scope_listings),
                RevenueSource::Missing => None,
            },
            revenue_stats: match revenue_at {
                RevenueSource::SellWorld => o.sell_stats.then_some(&sell_index),
                RevenueSource::BuyScope => Some(&index),
                RevenueSource::Scope => Some(&scope_stats),
                RevenueSource::Missing => None,
            },
            formula,
            levels: &levels,
            job_filter: None,
            use_subcrafts: false,
            require_hq: o.require_hq,
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
            visible_rev: PriceSignal::ALL.to_vec(),
            sort_rev: None,
            scope_vs_home: true,
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

    /// One row of the revenue projection: everything the sell-stat lookup
    /// produces that `price_rows_matches_recorded_oracle_on_fixture` cannot
    /// see.
    type RevProjection = (i32, i32, [Option<i32>; 4], bool, Option<i32>, bool);

    fn revenue_projection(rows: &[RecipeProfitData]) -> Vec<RevProjection> {
        rows.iter()
            .take(12)
            .map(|r| {
                (
                    r.recipe.key_id.0,
                    r.market_price,
                    r.rev_alt,
                    r.revenue_fell_back,
                    r.sell_median,
                    r.stat_hq,
                )
            })
            .collect()
    }

    /// The revenue-side characterization oracle, in the two fixture shapes
    /// that matter: every output has a sell-world listing (`WITH`), and no
    /// output has one (`WITHOUT`) — the spec's "includes items with no
    /// sell-world listing" parity case, which the default fixture cannot
    /// produce because it lists every output.
    ///
    /// Recorded at `c662eec0` (base `e3db0888`) before Phase F split the sell place from the
    /// sell world; regenerate ONLY if a phase moves these numbers on
    /// purpose (run with `--nocapture` and copy the printed tuples).
    #[test]
    fn revenue_projection_is_unchanged_at_the_default_sell_scope() {
        let with = revenue_projection(&run(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            false,
        ));
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::ListingMin),
            Some(PriceSignal::SaleMedian),
            None,
        );
        let without = revenue_projection(&run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs: needed_signals(&f, &SignalWants::default(), false),
                sell_listings: false,
                ..RunOpts::default()
            },
        ));
        println!("REVENUE_ORACLE_WITH = {with:?}");
        println!("REVENUE_ORACLE_WITHOUT = {without:?}");
        const WITH: &[RevProjection] = &[
            (0, 120, [Some(120), None, None, None], true, None, false),
            (1, 220, [Some(220), None, None, None], true, None, false),
            (2, 318, [Some(321), None, None, None], true, None, false),
            (
                3,
                455,
                [Some(540), Some(440), Some(455), Some(459)],
                false,
                Some(455),
                false,
            ),
            (
                4,
                294,
                [Some(346), Some(279), Some(294), Some(298)],
                false,
                Some(294),
                false,
            ),
            (
                5,
                434,
                [Some(514), Some(419), Some(434), Some(438)],
                false,
                Some(434),
                false,
            ),
            (7, 514, [Some(556), None, None, None], true, None, false),
            (9, 738, [Some(825), None, None, None], true, None, false),
            (
                12,
                378,
                [Some(447), Some(363), Some(378), Some(382)],
                false,
                Some(378),
                false,
            ),
            (13, 724, [Some(808), None, None, None], true, None, false),
            (
                14,
                497,
                [Some(590), Some(482), Some(497), Some(501)],
                false,
                Some(497),
                false,
            ),
            (15, 507, [Some(548), None, None, None], true, None, false),
        ];
        const WITHOUT: &[RevProjection] = &[
            (1, 184, [None, None, None, None], true, None, false),
            (
                3,
                455,
                [None, Some(440), Some(455), Some(459)],
                false,
                Some(455),
                false,
            ),
            (
                4,
                294,
                [None, Some(279), Some(294), Some(298)],
                false,
                Some(294),
                false,
            ),
            (
                5,
                434,
                [None, Some(419), Some(434), Some(438)],
                false,
                Some(434),
                false,
            ),
            (7, 464, [None, None, None, None], true, None, false),
            (9, 688, [None, None, None, None], true, None, false),
            (
                12,
                378,
                [None, Some(363), Some(378), Some(382)],
                false,
                Some(378),
                false,
            ),
            (13, 674, [None, None, None, None], true, None, false),
            (
                14,
                497,
                [None, Some(482), Some(497), Some(501)],
                false,
                Some(497),
                false,
            ),
            (15, 457, [None, None, None, None], true, None, false),
            (16, 548, [None, None, None, None], true, None, false),
            (
                18,
                770,
                [None, Some(755), Some(770), Some(774)],
                false,
                Some(770),
                false,
            ),
        ];
        assert_eq!(with.as_slice(), WITH);
        assert_eq!(without.as_slice(), WITHOUT, "no sell-world listing");
        assert!(
            without.iter().any(|(_, _, alt, ..)| alt[0].is_none()),
            "the WITHOUT shape must contain rows whose sell-world listing is absent, or it is not the parity case the spec asks for"
        );
    }

    /// `run_with`'s `else if wider { None }` arm — "the body was asked for
    /// and did not arrive" — which every other scope test skips: each one
    /// pairs `sell_scope: Some(..)` with `scope_bodies: true`. It is the
    /// state the amber banner exists for, so what the pass produces in it
    /// is a contract, not an accident: the alternative revenue cells go
    /// blank, Scope vs home reads `Unavailable` rather than a delta against
    /// a market that never answered, and the headline price still resolves
    /// — through `SignalView`'s base layer, i.e. the BUY scope, which is a
    /// different market from the one the strip, the picker heading and the
    /// live sentence all still name.
    #[test]
    fn a_sell_scope_body_that_never_arrived_falls_through_to_the_buy_scope() {
        let opts = |scope_bodies| RunOpts {
            needs: everything_wanted(PriceSignal::ListingMin),
            sell_scope: Some(Scope::Region),
            scope_bodies,
            ..RunOpts::default()
        };
        let failed = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &opts(false),
        );
        assert!(
            !failed.is_empty(),
            "the failed-body run must keep rows, or every assertion below is vacuous"
        );
        for r in &failed {
            assert_eq!(
                r.scope_vs_home,
                ScopeVsHome::Unavailable,
                "a market that never answered has no delta to show"
            );
            assert_eq!(
                r.rev_alt, [None; 4],
                "every alternative revenue cell reads \"—\": the sell place \
                 has no figure, and the buy scope's is not its figure"
            );
            assert!(
                r.revenue_fell_back,
                "the selected signal did not come from the sell place"
            );
            // The fixture lists every item NQ at `100 + (id % 97) * 7` on
            // the buy scope, so the fall-through price is computable per
            // row rather than merely "some number".
            let out = r.recipe.item_result;
            assert_eq!(
                r.market_price,
                100 + (out % 97) * 7,
                "the price must come from the buy-scope base layer"
            );
        }
        // …and that is emphatically NOT the market the labels name. The
        // fixture's sell world lists the same outputs 20% higher, so had
        // the body arrived every one of these rows would carry a different
        // number under the same heading — which is the whole reason the
        // banner says which market missed.
        let arrived = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &opts(true),
        );
        let by_key: HashMap<i32, &RecipeProfitData> =
            arrived.iter().map(|r| (r.recipe.key_id.0, r)).collect();
        let mut compared = 0;
        for r in &failed {
            let Some(a) = by_key.get(&r.recipe.key_id.0) else {
                continue;
            };
            if a.rev_alt[PriceSignal::ListingMin.index()].is_none() {
                // `scope_fixture` leaves every third recipe out of the scope
                // map on purpose, so the arrived run fell through to the
                // same base layer this one did: the two agree by design and
                // comparing them would prove nothing either way.
                continue;
            }
            assert_ne!(
                (r.market_price, r.rev_alt),
                (a.market_price, a.rev_alt),
                "recipe {}: a missing body must not price like a present one",
                r.recipe.key_id.0
            );
            compared += 1;
        }
        assert!(
            compared > 0,
            "no shared row was actually present in the scope map, so nothing \
             above compared a missing body against a present one"
        );
    }

    /// Revenue follows the sell scope, and the fixture proves each surviving
    /// row actually discriminates. The classes are read off
    /// `rev_alt[ListingMin]` rather than off `market_price`: that entry is
    /// the bare scope-map lookup with no HQ clamp and no base fallback, so
    /// `None` means "absent from the scope map" and nothing else, while a
    /// price comparison cannot tell a fall-through from an undercut (the
    /// buy-scope NQ price is below the home price too).
    #[test]
    fn revenue_reads_the_sell_scope_and_every_class_of_row_says_so() {
        let li = PriceSignal::ListingMin.index();
        for signal in [PriceSignal::ListingMin, PriceSignal::SaleMedian] {
            let f =
                ProfitFormula::recipe_from_query(Some(PriceSignal::ListingMin), Some(signal), None);
            let needs = needed_signals(&f, &SignalWants::default(), false);
            let home = run_with(
                PriceSignal::ListingMin,
                signal,
                &RunOpts {
                    needs: needs.clone(),
                    ..RunOpts::default()
                },
            );
            let scoped = run_with(
                PriceSignal::ListingMin,
                signal,
                &RunOpts {
                    needs,
                    sell_scope: Some(Scope::Region),
                    scope_bodies: true,
                    ..RunOpts::default()
                },
            );
            let home_by_key: HashMap<i32, &RecipeProfitData> =
                home.iter().map(|r| (r.recipe.key_id.0, r)).collect();

            let (mut cheaper, mut dearer, mut fell_through) = (0, 0, 0);
            let (mut price_down, mut price_up) = (0, 0);
            for r in &scoped {
                let Some(h) = home_by_key.get(&r.recipe.key_id.0) else {
                    continue;
                };
                match (r.rev_alt[li], h.rev_alt[li]) {
                    (None, Some(_)) => {
                        fell_through += 1;
                        assert!(
                            r.market_price > 0,
                            "the base layer must keep a scope-missing row priceable"
                        );
                    }
                    (Some(s), Some(hh)) if s < hh => cheaper += 1,
                    (Some(s), Some(hh)) if s > hh => dearer += 1,
                    pair => panic!("{signal:?}: undiscriminating row {pair:?}"),
                }
                match r.market_price.cmp(&h.market_price) {
                    Ordering::Less => price_down += 1,
                    Ordering::Greater => price_up += 1,
                    Ordering::Equal => {}
                }
            }
            assert!(
                cheaper > 0 && dearer > 0,
                "{signal:?}: the fixture must move the scope lookup BOTH ways \
                 (cheaper {cheaper}, dearer {dearer}); a one-directional \
                 fixture cannot tell a scope lookup from a clamp"
            );
            assert!(
                fell_through > 0,
                "{signal:?}: no row was absent from the scope map"
            );
            assert!(
                price_down > 0 && price_up > 0,
                "{signal:?}: the headline price must move both ways too \
                 (down {price_down}, up {price_up})"
            );

            // `over` and `stats` are two separate fields of one
            // `SignalView`, and every count above moves with the listing
            // layer alone: verified by mutation, a build whose revenue
            // STATISTICS kept reading the sell world passes all of them.
            // Under a sale signal the sell place's own statistic is what
            // `quality()` returns for whichever quality carries it, so the
            // headline price can never sit above it - and reading the
            // world's unscaled statistic instead puts it there on the
            // halved (even-id) rows.
            if signal.sale_stat().is_some() {
                let mut priced_at_the_sell_places_statistic = 0;
                for r in &scoped {
                    let Some(stat) = r.rev_alt[signal.index()] else {
                        continue;
                    };
                    assert!(
                        r.market_price <= stat,
                        "row {} priced at {} above its own sell-place {signal:?} of {stat}",
                        r.recipe.key_id.0,
                        r.market_price
                    );
                    priced_at_the_sell_places_statistic += usize::from(r.market_price == stat);
                }
                assert!(
                    priced_at_the_sell_places_statistic > 0,
                    "{signal:?}: no row priced AT the sell place's statistic, so the \
                     assertion above cannot see the statistics layer"
                );
            }
        }
    }

    /// The sell world's own figures do NOT follow the sell scope: velocity,
    /// avg price, confidence, last sold, volume, VWAP, the statistics
    /// quality (the sparkline and 30-day key) and Hop gain's home run all
    /// stay where the spec puts them.
    #[test]
    fn the_sell_worlds_own_figures_ignore_the_sell_scope() {
        let needs = everything_wanted(PriceSignal::ListingMin);
        let home = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs: needs.clone(),
                scope: Some(BuyScope::Region),
                ..RunOpts::default()
            },
        );
        let scoped = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs,
                scope: Some(BuyScope::Region),
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        let by_key: HashMap<i32, &RecipeProfitData> =
            home.iter().map(|r| (r.recipe.key_id.0, r)).collect();
        let mut compared = 0;
        for r in &scoped {
            let Some(h) = by_key.get(&r.recipe.key_id.0) else {
                continue;
            };
            compared += 1;
            assert_eq!(r.daily_sales, h.daily_sales, "{}", r.recipe.key_id.0);
            assert_eq!(r.avg_price, h.avg_price);
            assert_eq!(r.units_sold, h.units_sold);
            assert_eq!(r.vwap, h.vwap);
            assert_eq!(r.last_sold_unix, h.last_sold_unix);
            assert_eq!(r.confidence, h.confidence);
            assert_eq!(r.stat_hq, h.stat_hq);
            assert_eq!(
                r.hop, h.hop,
                "Hop gain is buy-side and prices home at the world"
            );
            assert_eq!(r.worlds, h.worlds);
        }
        assert!(compared > 20, "only {compared} rows compared");
    }

    /// The Price median tell is SUPPRESSED at a wider sell scope, not
    /// re-based. `price_note` compares the row's price against
    /// `sell_median`; move the price to a region and the two operands stop
    /// describing the same market, so the tell would read negative and red
    /// on nearly every row - caused by the user's own setting rather than
    /// by a suspicious listing. #1266 was merged to make that tell
    /// trustworthy; a page-wide false alarm is how a colour stops being
    /// read. The sub-line keeps its shape: `price_note` falls to
    /// `ListingFallback` or `None`.
    #[test]
    fn the_price_median_tell_is_suppressed_at_a_wider_sell_scope() {
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::ListingMin),
            Some(PriceSignal::SaleMedian),
            None,
        );
        let needs = needed_signals(&f, &SignalWants::default(), false);
        let home = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs: needs.clone(),
                ..RunOpts::default()
            },
        );
        assert!(
            home.iter().any(|r| r.sell_median.is_some()),
            "the fixture must carry medians at the default scope, or this \
             test cannot tell suppression from an empty fixture"
        );
        let scoped = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs,
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(
            scoped.iter().all(|r| r.sell_median.is_none()),
            "a wider sell scope must leave the median tell's operand empty"
        );
        // ...and the note therefore never carries a percentage.
        for r in &scoped {
            assert!(
                !matches!(
                    price_note(r.market_price, r.sell_median, r.revenue_fell_back),
                    CellNote::VsMedian { .. } | CellNote::Troll { .. }
                ),
                "row {} still renders a median tell",
                r.recipe.key_id.0
            );
        }
    }

    /// Scope vs home: both places under one signal, both directions of
    /// sign, and every non-`Pair` state the design names.
    #[test]
    fn scope_vs_home_records_both_places_and_only_when_asked() {
        let wanted = NeededSignals {
            scope_vs_home: true,
            ..NeededSignals::default()
        };
        // Not asked for: never computed, whatever the scope.
        let quiet = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(quiet.iter().all(|r| r.scope_vs_home == ScopeVsHome::Off));

        // Asked for, but the sell scope IS the world: nothing to compare,
        // and the whole column is `Off` (the header tooltip says why).
        let flat = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: wanted.clone(),
                ..RunOpts::default()
            },
        );
        assert!(flat.iter().all(|r| r.scope_vs_home == ScopeVsHome::Off));

        // Asked for at a wider scope: both directions appear, and a row the
        // scope map does not hold is `Unavailable`, never `Off`.
        let scoped = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: wanted,
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(scoped.iter().all(|r| r.scope_vs_home != ScopeVsHome::Off));
        let deltas: Vec<i32> = scoped
            .iter()
            .filter_map(|r| match r.scope_vs_home {
                ScopeVsHome::Pair { place, home, .. } => Some(place - home),
                _ => None,
            })
            .collect();
        assert!(!deltas.is_empty());
        assert!(
            deltas.iter().any(|d| *d < 0),
            "no row where the scope undercuts"
        );
        assert!(
            deltas.iter().any(|d| *d > 0),
            "no row where the scope is dearer"
        );
        assert!(
            scoped
                .iter()
                .any(|r| r.scope_vs_home == ScopeVsHome::Unavailable),
            "the fixture's third class must reach the Unavailable state"
        );
        // Every recorded pair has a real value on BOTH sides, and a listing
        // signal is one-sided so the percentage will be dropped in Task 4.
        assert!(scoped.iter().all(|r| match r.scope_vs_home {
            ScopeVsHome::Pair {
                place,
                home,
                two_sided,
            } => place > 0 && home > 0 && !two_sided,
            _ => true,
        }));

        // The two-sided half. Every `Pair` above is `ListingMin`, so
        // `two_sided` is `false` on all of them and `scope_vs_home_pct`
        // returns `None` every time: before this block BOTH coloured arms
        // of the percentage — the red one and, far more importantly, the
        // emerald one #1266's troll guard exists to police — were reached
        // only by hand-set rows. A sale signal is what makes the delta go
        // either way, and the fixture's parity split is what makes both
        // directions appear in one pass.
        let sale = run_with(
            PriceSignal::ListingMin,
            PriceSignal::SaleMedian,
            &RunOpts {
                needs: NeededSignals {
                    scope_vs_home: true,
                    ..NeededSignals::default()
                },
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        let pairs: Vec<ScopeVsHome> = sale
            .iter()
            .map(|r| r.scope_vs_home)
            .filter(|s| {
                matches!(
                    s,
                    ScopeVsHome::Pair {
                        two_sided: true,
                        ..
                    }
                )
            })
            .collect();
        assert!(
            !pairs.is_empty(),
            "a sale revenue signal must produce two-sided pairs"
        );
        let pcts: Vec<f32> = pairs.iter().filter_map(|s| scope_vs_home_pct(*s)).collect();
        assert!(
            pcts.iter().any(|p| *p > 0.0),
            "no row reaches the EMERALD arm of the percentage — the one the \
             troll guard polices, and the one Phase E2 shipped inverted"
        );
        assert!(
            pcts.iter().any(|p| *p < 0.0),
            "no row reaches the red arm of the percentage"
        );
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
            sell_median: None,
            revenue_fell_back: false,
            unpriced: 0,
            hop: None,
            worlds: None,
            scope_vs_home: ScopeVsHome::Off,
            price_is_sell_world: true,
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
        assert_eq!(n_ids, 23);
        assert_eq!(
            n_sorts, 25,
            "the eleven sorts at HEAD, the ten signal and hop columns, E2's three \
             and F's Scope vs home; listing world/dc, trend and drift do not sort"
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
            16
        );
    }

    /// `scope_row` returns a `RecipeRow`, i.e. `Arc<RecipeProfitData>`, the
    /// way `hop_row` and `price_row` do: every cell fn takes `&RecipeRow`,
    /// and `compare_recipes` takes `&RecipeProfitData`, which `&Arc<T>`
    /// deref-coerces into.
    fn scope_row(key: i32, state: ScopeVsHome) -> RecipeRow {
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.scope_vs_home = state;
        Arc::new(r)
    }

    fn pair(place: i32, home: i32, two_sided: bool) -> ScopeVsHome {
        ScopeVsHome::Pair {
            place,
            home,
            two_sided,
        }
    }

    /// Scope vs home renders the delta, its percent against the home value,
    /// and nothing at all when there is no pair. The sort key is the same
    /// delta, and it sorts none-last in both directions like every other
    /// optional-value column on this page.
    #[test]
    fn scope_vs_home_cell_and_sort_read_the_same_delta() {
        let ctx = test_ctx();
        let cheaper = scope_row(1, pair(900, 1_000, true));
        let dearer = scope_row(2, pair(1_100, 1_000, true));
        let off = scope_row(3, ScopeVsHome::Off);
        let missing = scope_row(4, ScopeVsHome::Unavailable);
        assert_eq!(
            cell_scope_vs_home(&cheaper, &ctx),
            CellValue::SignedGil {
                delta: Some(-100),
                pct: Some(-10.0),
                unavailable: false,
            }
        );
        assert_eq!(
            cell_scope_vs_home(&dearer, &ctx),
            CellValue::SignedGil {
                delta: Some(100),
                pct: Some(10.0),
                unavailable: false,
            }
        );
        assert_eq!(
            cell_scope_vs_home(&off, &ctx),
            CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: false,
            }
        );
        assert_eq!(
            cell_scope_vs_home(&missing, &ctx),
            CellValue::SignedGil {
                delta: None,
                pct: None,
                unavailable: true,
            },
            "a dash that could have been a figure says so"
        );
        assert_eq!(scope_vs_home_delta(&cheaper), Some(-100));
        assert_eq!(scope_vs_home_delta(&off), None);
        assert_eq!(scope_vs_home_delta(&missing), None);

        for dir in [SortDir::Asc, SortDir::Desc] {
            assert_eq!(
                compare_recipes(SortMode::ScopeVsHome, dir, &cheaper, &missing, None),
                Ordering::Less,
                "a row with no pair sorts last whichever way the header points"
            );
            assert_eq!(
                compare_recipes(SortMode::ScopeVsHome, dir, &missing, &dearer, None),
                Ordering::Greater
            );
        }
        assert_eq!(
            compare_recipes(
                SortMode::ScopeVsHome,
                SortDir::Desc,
                &dearer,
                &cheaper,
                None
            ),
            Ordering::Less,
            "descending puts the biggest gain first"
        );
        assert_eq!(SortMode::ScopeVsHome.default_dir(), SortDir::Desc);
    }

    /// Phase E2 shipped a coloured percentage whose GREEN arm meant "do not
    /// trust this figure", and #1266 corrected it with a display ceiling
    /// and a troll guard. Scope vs home inherits both rather than
    /// re-earning them:
    ///
    /// * under a listing signal the delta is structurally <= 0, so the
    ///   percentage is dropped and the cell renders uncoloured — a
    ///   permanently red stripe in the codebase's warning colour teaches
    ///   players to ignore the colour;
    /// * a scope figure 50x the home one is not a finding, it is a thin or
    ///   laundered home median, and `is_troll_listing` is the same helper
    ///   `price_note` gates on;
    /// * anything below that is clamped to the same ceiling that exists
    ///   because prod rendered "+399900%".
    #[test]
    fn scope_vs_home_never_paints_a_thin_home_median_green() {
        let ctx = test_ctx();
        let pct_of = |r: &RecipeRow| match cell_scope_vs_home(r, &ctx) {
            CellValue::SignedGil { pct, .. } => pct,
            other => panic!("{other:?}"),
        };
        // One-sided: the listing signal. The gil delta survives, the
        // percentage does not.
        let listing = scope_row(1, pair(900, 1_000, false));
        assert_eq!(scope_vs_home_delta(&listing), Some(-100));
        assert_eq!(pct_of(&listing), None);
        // Troll-shaped: the only way this column renders green.
        let thin = scope_row(2, pair(100_000, 100, true));
        assert!(is_troll_listing(100_000, 100));
        assert_eq!(
            pct_of(&thin),
            None,
            "a home figure the analyzer would not price against must not be \
             the baseline for an emerald percentage"
        );
        assert_eq!(scope_vs_home_delta(&thin), Some(99_900));
        // Below the troll multiple, the ceiling still applies.
        let big = scope_row(3, pair(2_000, 100, true));
        assert!(!is_troll_listing(2_000, 100));
        assert_eq!(pct_of(&big), Some(VS_MEDIAN_DISPLAY_CEILING_PCT));
        // And an ordinary figure is untouched.
        assert_eq!(pct_of(&scope_row(4, pair(1_100, 1_000, true))), Some(10.0));
    }

    /// Task 3 suppressed the 7-day VWAP percentage at a wider sell scope
    /// because its numerator (`market_price`) follows the sell scope while
    /// its denominator is the sell world's own figure. The 30-day twin has
    /// the identical mismatch, one body later: `cell_vwap_30` divides the
    /// same `market_price` by a VWAP from the 30-day sell-WORLD payload. It
    /// cannot be suppressed in the pass — that body is client-only and
    /// lands after the rows are priced — so the row carries the one bit the
    /// cell needs.
    ///
    /// The absolute VWAP survives in both cases: it is a sell-world figure
    /// and its column says so. Only the comparison moves.
    #[test]
    fn the_30_day_vwap_percentage_is_suppressed_at_a_wider_sell_scope() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let key = fixture_recipes()[0].key_id.0;
            let mut home = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
            home.market_price = 150;
            let item = home.recipe.item_result;
            let mut scoped = home.clone();
            scoped.price_is_sell_world = false;
            let index: StatsIndex = [((item, false), stats_row(item, false, 9, 100))]
                .into_iter()
                .collect();
            let store: LateStats = RwSignal::new(Some(Arc::new(index)));
            let ctx = CellCtx {
                stats_30: Some(store),
                ..test_ctx()
            };
            assert!(
                home.price_is_sell_world,
                "the default fixture row must be the un-scoped case, or this \
                 test cannot tell suppression from an empty comparison"
            );
            assert_eq!(
                cell_vwap_30(&Arc::new(home), &ctx),
                CellValue::LateGilWithPct(Enrich::Ready((100, Some(50.0)))),
                "on the sell world the percentage is 150 against 100"
            );
            assert_eq!(
                cell_vwap_30(&Arc::new(scoped), &ctx),
                CellValue::LateGilWithPct(Enrich::Ready((100, None))),
                "at a wider sell scope the VWAP stays and the percentage goes"
            );
        });
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

    fn price_row(key: i32, price: i32, median: Option<i32>, fell_back: bool) -> RecipeRow {
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.market_price = price;
        r.sell_median = median;
        r.revenue_fell_back = fell_back;
        Arc::new(r)
    }

    /// The Price note gains the signed percent the price sits above or
    /// below the sell world's 7-day median, keeps the listing tell in front
    /// of it when both apply, and is exactly the pre-Phase-C cell with the
    /// toggle off.
    ///
    /// The orientation is the point of this test. `delta_pct(alt, input)` is
    /// `(alt - input) / input`, and the two arguments are trivially
    /// swappable, so both concrete cases are pinned here: a price ABOVE the
    /// median reads positive (and `signed_delta_class` paints it emerald), a
    /// price BELOW it reads negative (red). The inverted orientation —
    /// `delta_pct(Some(median), price)` — would flip both, painting a
    /// suspiciously cheap listing as good news.
    #[test]
    fn the_price_note_carries_the_median_tell_under_the_toggle() {
        let key = fixture_recipes()[0].key_id.0;
        let ctx = test_ctx();
        let off = CellCtx {
            preview: false,
            ..test_ctx()
        };
        // A price of 138 against a median of 100 is 38% ABOVE it: positive,
        // and green. (The other orientation — the median measured against
        // the price — would paint a fake-low listing green.)
        assert_eq!(
            cell_price(&price_row(key, 138, Some(100), false), &ctx),
            CellValue::GilWithNote {
                amount: 138,
                note: CellNote::VsMedian {
                    listing: false,
                    pct: 38.0
                }
            }
        );
        // Below the median, and the listing tell keeps its place in front.
        assert_eq!(
            cell_price(&price_row(key, 75, Some(100), true), &ctx),
            CellValue::GilWithNote {
                amount: 75,
                note: CellNote::VsMedian {
                    listing: true,
                    pct: -25.0
                }
            }
        );
        // The two four-percent cases, spelled out with the colour each
        // renders, because the sign is the whole decision here: a listing
        // cheaper than the median is the warning (red), a listing dearer
        // than it reads positive (emerald).
        let colour = |v: CellValue| match v {
            CellValue::GilWithNote {
                note: CellNote::VsMedian { pct, .. },
                ..
            } => signed_delta_class(Some(pct), DELTA_DEAD_BAND_PCT),
            other => panic!("expected a median tell, got {other:?}"),
        };
        assert_eq!(
            colour(cell_price(&price_row(key, 960, Some(1000), false), &ctx)),
            "text-red-300",
            "960 against a median of 1000 is -4%: the suspiciously cheap \
             listing is the warning, not the good news"
        );
        assert_eq!(
            colour(cell_price(&price_row(key, 1040, Some(1000), false), &ctx)),
            "text-emerald-300",
            "1040 against a median of 1000 is +4%"
        );
        // No sale history on the sell world: Phase D's note, unchanged.
        assert_eq!(
            cell_price(&price_row(key, 100, None, true), &ctx),
            CellValue::GilWithNote {
                amount: 100,
                note: CellNote::ListingFallback
            }
        );
        // Price IS the median (the median basis): no "+0%" tell.
        assert_eq!(
            cell_price(&price_row(key, 100, Some(100), false), &ctx),
            CellValue::GilWithNote {
                amount: 100,
                note: CellNote::None
            }
        );
        // Toggle off: no note line at all.
        assert_eq!(
            cell_price(&price_row(key, 138, Some(100), false), &off),
            CellValue::Gil(138)
        );
    }

    /// The tell reads `sell_median` — the quality-matched 7-day median — and
    /// nothing else. `rev_alt[SaleMedian]` is the cheaper-of-both-qualities
    /// figure behind the "Sale median (7d)" column, and feeding it to this
    /// comparison is exactly the #1264 defect: it is still populated here,
    /// and it must not produce a tell.
    #[test]
    fn the_median_tell_ignores_the_cheapest_quality_column() {
        let key = fixture_recipes()[0].key_id.0;
        let ctx = test_ctx();
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.market_price = 40_000_000;
        // The alternative-revenue column's basis: present, and wildly below
        // the price. Under #1264 this alone rendered "+399900%" in emerald.
        r.rev_alt[PriceSignal::SaleMedian.index()] = Some(10_000);
        r.sell_median = None;
        assert_eq!(
            cell_price(&Arc::new(r), &ctx),
            CellValue::GilWithNote {
                amount: 40_000_000,
                note: CellNote::None
            },
            "no quality-matched median means no tell, whatever rev_alt holds"
        );
    }

    /// Above the median is not unbounded good news. At 50x — the multiple
    /// `is_troll_listing` already refuses to price against — the percentage
    /// gives way to the warning; below that it is clamped, for the reason
    /// ROI is.
    #[test]
    fn a_price_far_above_the_median_warns_instead_of_reading_emerald() {
        let key = fixture_recipes()[0].key_id.0;
        let ctx = test_ctx();
        let note =
            |price, median| match cell_price(&price_row(key, price, Some(median), false), &ctx) {
                CellValue::GilWithNote { note, .. } => note,
                other => panic!("expected a note, got {other:?}"),
            };
        // The prod row that prompted this: Agate Ring of Slaying, priced
        // 40,000,000 against a ~10,000 median. 4000x, so: the warning.
        assert_eq!(note(40_000_000, 10_000), CellNote::Troll { listing: false });
        // `is_troll_listing` is a strict `>`, so exactly 50x is still a
        // percentage — clamped, because +4900% is not a figure anyone acts
        // on — and 50x plus one gil is the warning.
        assert_eq!(
            note(50_000, 1_000),
            CellNote::VsMedian {
                listing: false,
                pct: VS_MEDIAN_DISPLAY_CEILING_PCT
            }
        );
        assert_eq!(note(50_001, 1_000), CellNote::Troll { listing: false });
        assert_eq!(
            note(11_000, 1_000),
            CellNote::VsMedian {
                listing: false,
                pct: VS_MEDIAN_DISPLAY_CEILING_PCT
            },
            "+1000% clamps"
        );
        // Inside the ceiling nothing is touched. (Compared with a tolerance:
        // `delta_pct` divides in f32, so a figure this large is not exactly
        // representable and an equality here would pin a rounding artefact,
        // not the behaviour.)
        let pct_of = |price, median| match note(price, median) {
            CellNote::VsMedian { pct, .. } => pct,
            other => panic!("expected a percentage, got {other:?}"),
        };
        let under = pct_of(10_980, 1_000);
        assert!(
            (under - 998.0).abs() < 0.01,
            "just under the ceiling is untouched, got {under}"
        );
        // The clamp really is one-sided: the low side cannot reach it,
        // because the numerator is a positive price over a positive median.
        assert_eq!(pct_of(1, 100), -99.0);
        // The listing tell keeps its place in front of the warning.
        assert_eq!(
            cell_price(&price_row(key, 40_000_000, Some(10_000), true), &ctx),
            CellValue::GilWithNote {
                amount: 40_000_000,
                note: CellNote::Troll { listing: true }
            }
        );
        // And with the toggle off the troll row is the bare gil cell, the
        // same value a row with no median at all produces.
        let off = CellCtx {
            preview: false,
            ..test_ctx()
        };
        assert_eq!(
            cell_price(&price_row(key, 40_000_000, Some(10_000), true), &off),
            CellValue::Gil(40_000_000)
        );
        assert_eq!(
            cell_price(&price_row(key, 40_000_000, None, false), &off),
            CellValue::Gil(40_000_000)
        );
    }

    /// With the Labs toggle off the Price cell is the markup it has always
    /// been: `CellValue::Gil`, identical whatever the row carries, and
    /// identical to what every other gil column renders. Asserted on the
    /// HTML rather than on the enum, because an enum equality cannot see a
    /// change to the `Gil` render arm. There is no width in it — the class
    /// is a static prop and the arm has no responsive branch — so one
    /// comparison covers every viewport.
    #[test]
    fn the_flag_off_price_cell_is_the_plain_gil_markup() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let key = fixture_recipes()[0].key_id.0;
            let off = CellCtx {
                preview: false,
                ..test_ctx()
            };
            let html = |v| {
                crate::analyzer_kit::cells::render_cell("w-32", v, i18n, &off)
                    .unwrap()
                    .to_html()
            };
            let baseline = html(CellValue::Gil(40_000_000));
            for (median, fell_back) in [
                // The prod row: 40,000,000 against a ~44k median. Troll.
                (Some(43_995), false),
                (Some(43_995), true),
                // Just under the troll multiple, so a clamped percentage.
                (Some(1_000_000), false),
                // No sale history at all, and the degenerate equal case.
                (None, false),
                (None, true),
                (Some(40_000_000), false),
            ] {
                assert_eq!(
                    html(cell_price(
                        &price_row(key, 40_000_000, median, fell_back),
                        &off
                    )),
                    baseline,
                    "median {median:?}, fell_back {fell_back}"
                );
            }
            assert!(
                !baseline.contains("vs median")
                    && !baseline.contains("troll")
                    && !baseline.contains("listing"),
                "{baseline}"
            );
        });
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

    /// Every market column's header says what window it covers and where
    /// the number comes from; the two 30-day columns carry the window in
    /// their label instead, so they get a tooltip only.
    #[test]
    fn market_headers_carry_their_tooltip_and_the_window() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let daily = market_extra(i18n, ColumnKind::SalesPerDay7, "Gilgamesh").unwrap();
            let line2 = daily.line2.clone().expect("a second line");
            assert_eq!(line2.sub_label, "7d · Gilgamesh");
            assert!(line2.pill.is_none(), "no formula input to write");
            assert_eq!(daily.header_class, Some(HEAD_MD_2));
            assert!(!daily.title.is_empty());
            for kind in [
                ColumnKind::Confidence,
                ColumnKind::Trend,
                ColumnKind::DriftSpark,
            ] {
                let e = market_extra(i18n, kind, "Gilgamesh").unwrap();
                assert_eq!(e.line2.expect("a second line").sub_label, "7d · Gilgamesh");
            }
            assert_eq!(
                market_extra(i18n, ColumnKind::Confidence, "Gilgamesh")
                    .unwrap()
                    .header_class,
                Some(HEAD_28_MD_2)
            );
            // Trend and Drift take the grid's *unsortable* two-line arm,
            // which appends nothing: they keep their own `HEAD_LAZY_MD*`,
            // whose `flex flex-col` is what stacks the two spans. Handing
            // them a `header_class` here would silently drop it.
            for (kind, id) in [
                (ColumnKind::Trend, COL_TREND),
                (ColumnKind::DriftSpark, COL_DRIFT),
            ] {
                assert_eq!(
                    market_extra(i18n, kind, "Gilgamesh").unwrap().header_class,
                    None,
                    "{id}: the column's own class stacks the lines"
                );
                let class = RECIPE_COLUMNS
                    .iter()
                    .find(|c| c.id == id)
                    .expect("column in the table")
                    .header_class;
                assert!(
                    class.contains("flex-col"),
                    "{id}: a second line with no flex-col lays the two spans \
                     side by side ({class})"
                );
            }
            for kind in [
                ColumnKind::ProfitPerDay,
                ColumnKind::VolumeUnits30,
                ColumnKind::Vwap30,
            ] {
                let e = market_extra(i18n, kind, "Gilgamesh").unwrap();
                assert!(e.line2.is_none(), "{kind:?}: the label carries the window");
                assert_eq!(e.header_class, None, "{kind:?}: classes do not move");
                assert!(!e.title.is_empty());
            }
            // Phase D's kinds keep their own extras; a plain column has none.
            assert!(market_extra(i18n, ColumnKind::HopGain, "Gilgamesh").is_none());
            assert!(market_extra(i18n, ColumnKind::Item, "Gilgamesh").is_none());
        });
    }

    /// `market_extra` puts the place it is GIVEN on line 2 — it has no
    /// other source for one, so this pins the composition (`7d · ‹place›`)
    /// and nothing more. Which place actually reaches the call is a
    /// different question and a different test
    /// (`the_two_places_reach_the_labels_they_belong_to`), because the two
    /// variables are one character apart in `header_extras`.
    #[test]
    fn market_extras_put_the_place_they_are_given_on_the_second_line() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            for kind in [
                ColumnKind::SalesPerDay7,
                ColumnKind::Confidence,
                ColumnKind::Trend,
                ColumnKind::DriftSpark,
            ] {
                let one = market_extra(i18n, kind, "Gilgamesh").expect("a market extra");
                let two = market_extra(i18n, kind, "Aether").expect("a market extra");
                let (l1, l2) = (
                    one.line2.expect("a second line").sub_label,
                    two.line2.expect("a second line").sub_label,
                );
                assert!(l1.ends_with("Gilgamesh"), "{kind:?}: {l1}");
                assert!(l2.ends_with("Aether"), "{kind:?}: {l2}");
                assert_ne!(l1, l2, "{kind:?}: the place is interpolated, not baked in");
            }
        });
    }

    /// `market_extra` takes the sell WORLD; the marks, the alternative
    /// revenue headers, the picker heading and the live sentence take the
    /// sell PLACE. Reading the production half back out of the source is
    /// the only way to see which variable reached which call — the same
    /// technique `the_page_wires_both_gates_to_what_it_fetches` uses.
    ///
    /// Every needle aimed at a multi-argument call goes through
    /// `production_squeezed()`: rustfmt breaks any call it cannot fit in
    /// 100 columns onto one line per argument, and this phase has already
    /// shipped one pin that could never match because of it.
    #[test]
    fn the_two_places_reach_the_labels_they_belong_to() {
        let production = production_source();
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(&format!("{}(i18n,kind,&{})", "market_extra", "sell_now")),
            "market_extra takes the sell WORLD's name"
        );
        assert!(
            production.contains(&format!("let {} = {}.get();", "sell_now", "sell_place")),
            "and `sell_now` is the sell world"
        );
        assert!(
            squeezed.contains(&format!(
                "f.{}({}.get(),buy_place.get())",
                "marks", "revenue_place"
            )),
            "the header marks name the sell PLACE"
        );
        assert!(
            production.contains(&format!(
                "let {} = {}.get();",
                "revenue_now", "revenue_place"
            )),
            "and `revenue_now` is the sell place"
        );
        // The alternative-revenue headers read "‹signal› · ‹place›", and
        // that place is where the signal would be READ, not where the
        // 7-day figures live. Reverting this one to `sell_now` leaves
        // `revenue_now` merely unused — a warning, and only at Task 9's
        // `-D warnings` — so pin the argument itself.
        assert!(
            squeezed.contains(&format!("short_signal(i18n,s),{})", "revenue_now")),
            "the alternative revenue sub-labels name the sell PLACE"
        );
        assert!(
            production.contains(&format!("{}: {}.get(),", "sell_place", "revenue_place")),
            "the picker's Revenue heading names the sell PLACE"
        );
        assert!(
            production.contains(&format!("{} = {}.get(),", "sell", "revenue_place")),
            "the live formula sentence names the sell PLACE"
        );
        // …and the place memo itself goes through the pure resolver, whose
        // own body holds the lab gate — or a flag-off page with
        // `?sell-scope=region` would rename every revenue label it shows.
        //
        // Aimed at `revenue_place_for`, NOT at a bare
        // `sell_scope_for(preview.get(), sell_scope())`: that string is
        // also written by the live-sentence branch added in this same
        // task, and by Task 6's strip select and table prop, so it would
        // pass without `revenue_place` consulting anything. The gate's own
        // behaviour is what `the_two_places_agree_until_the_scope_moves`
        // proves; this pins that the memo actually calls the function that
        // has it.
        assert!(
            squeezed.contains(&format!(
                "{}(preview.get(),{}(),",
                "revenue_place_for", "sell_scope"
            )),
            "`revenue_place` must resolve through `revenue_place_for`, which \
             is where the lab gate lives"
        );
    }

    /// The two names are the same string until a lab-on URL asks for a
    /// wider scope. This is the flag-off byte-identity proof for every
    /// label this task moved: with the toggle off, or at the default scope,
    /// `revenue_place` and `sell_place` are indistinguishable, so the marks,
    /// the picker heading, the alternative revenue sub-labels and the live
    /// sentence render exactly what they render today.
    #[test]
    fn the_two_places_agree_until_the_scope_moves() {
        for preview in [false, true] {
            for param in [None, Some(SellScope::default())] {
                assert_eq!(
                    revenue_place_for(preview, param, "Gilgamesh", Some("Aether"), "North-America"),
                    "Gilgamesh",
                    "preview={preview} param={param:?}"
                );
            }
        }
        // Lab off, EVERY param the URL can carry: still the sell world, so
        // no label this task moved can differ from today's on a flag-off
        // page, whatever `?sell-scope=` a bookmark holds.
        for scope in [Scope::World, Scope::Datacenter, Scope::Region] {
            assert_eq!(
                revenue_place_for(
                    false,
                    Some(SellScope(scope)),
                    "Gilgamesh",
                    Some("Aether"),
                    "North-America"
                ),
                "Gilgamesh",
                "flag-off ?sell-scope={scope:?}"
            );
        }
        // Lab on, wider param: the wider name, and the region when no
        // datacenter has resolved yet.
        assert_eq!(
            revenue_place_for(
                true,
                Some(SellScope(Scope::Datacenter)),
                "Gilgamesh",
                Some("Aether"),
                "North-America"
            ),
            "Aether"
        );
        assert_eq!(
            revenue_place_for(
                true,
                Some(SellScope(Scope::Datacenter)),
                "Gilgamesh",
                None,
                "North-America"
            ),
            "North-America"
        );
        assert_eq!(
            revenue_place_for(
                true,
                Some(SellScope(Scope::Region)),
                "Gilgamesh",
                Some("Aether"),
                "North-America"
            ),
            "North-America"
        );
    }

    /// `header_extras` ends in a catch-all that delegates to
    /// `market_extra`, which returns `None` for a non-market kind and makes
    /// the whole column `continue`. A column with no arm of its own
    /// therefore ships a header with no tooltip and the key it was written
    /// for ships dead in seven locales. Two arms already exist for exactly
    /// this reason (`HopGain`, `HopWorlds`); Scope vs home needs the third,
    /// because the sign convention only exists in that string.
    #[test]
    fn the_scope_vs_home_header_has_its_own_extras_arm() {
        let production = production_source();
        assert!(
            production.contains("ColumnKind::ScopeVsHome => HeaderExtra {"),
            "no `header_extras` arm: the catch-all's `market_extra` returns \
             None for this kind and the tooltip never renders"
        );
        assert_eq!(
            production.matches("analyzer_scope_vs_home_help").count(),
            1,
            "the tooltip key is read exactly once, by that arm"
        );
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            assert!(
                market_extra(use_i18n(), ColumnKind::ScopeVsHome, "Aether").is_none(),
                "if this ever returns Some, delete the arm instead of keeping both"
            );
        });
    }

    /// `recipe_analyzer_calc_formula_live` reads "‹revenue› **on** {{sell}}"
    /// against "‹cost› **across** {{buy}}" deliberately: `on` is a world,
    /// `across` is a scope. Feeding a datacenter into the `on` slot would
    /// read "Sale median on Aether" two rows under "Sell on: Gilgamesh" and
    /// assert the one thing retainers cannot do. A scoped variant is
    /// selected when the sell scope is wider, and the default sentence is
    /// untouched — which is also what keeps the flag-off and default-scope
    /// rendering byte-identical.
    #[test]
    fn the_live_formula_sentence_scopes_the_sell_slot() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let plain = t_string!(
                i18n,
                recipe_analyzer_calc_formula_live,
                revenue = "Sale median".to_string(),
                sell = "Gilgamesh".to_string(),
                tax = "5% tax".to_string(),
                cost = "Cheapest listing".to_string(),
                buy = "Aether".to_string()
            )
            .to_string();
            let scoped = t_string!(
                i18n,
                recipe_analyzer_calc_formula_live_scoped,
                revenue = "Sale median".to_string(),
                sell = "Aether".to_string(),
                tax = "5% tax".to_string(),
                cost = "Cheapest listing".to_string(),
                buy = "Aether".to_string()
            )
            .to_string();
            assert!(plain.contains("on Gilgamesh"), "{plain}");
            // The world preposition must be gone from the sell slot — the
            // cost half's own "across {{buy}}" is what the scoped sentence
            // reuses, so a copy-paste that left `on` in place would still
            // contain "across Aether" and pass the assertion below.
            assert!(!scoped.contains("on Aether"), "{scoped}");
            assert!(scoped.contains("across Aether"), "{scoped}");
        });
        let production = production_source();
        assert!(
            production.contains("recipe_analyzer_calc_formula_live_scoped"),
            "the scoped variant must actually be selected somewhere"
        );
        // And selected on the LAB-GATED scope, widened past the sell
        // world. `sell_scope().is_some()` would compile, read the same at
        // a glance, and hand a flag-off `?sell-scope=world` page the
        // scoped sentence — the one rendered string on this page that a
        // bookmarked URL could move with the toggle off.
        assert!(
            production_squeezed().contains(
                "sell_scope_for(preview.get(),sell_scope()).is_some_and(|s|s.scope()!=Scope::World)"
            ),
            "the sentence must switch on the lab gate and on a scope wider \
             than the sell world"
        );
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

    /// The row's median is its own quality's; the "Sale median (7d)"
    /// alternative-revenue column's is still the cheaper of the two. Both
    /// are asserted off one run, because the defect was one silently
    /// standing in for the other — and no other test in this file can see
    /// the difference: every existing fixture gives an item ONE quality of
    /// statistics, and with one quality present `stat_row_either` and
    /// `stat_only_cheapest` return the same row.
    #[test]
    fn the_rows_median_is_quality_matched_and_the_alt_column_is_not() {
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::ListingMin), None, None);
        let opts = |require_hq| RunOpts {
            stats_both: true,
            // The require_hq run overrides this; the NQ run wants the
            // alternating split so both directions are covered.
            hq_dearer_only: require_hq,
            require_hq,
            needs: needed_signals(&f, &SignalWants::default(), false),
            ..RunOpts::default()
        };
        // The fixture's NQ median, and which items get a statistics row.
        let median_of = |out: i32| 100 + (out % 97) * 7 + 5;
        let has_stats = |out: i32| out % 3 == 0 && out != 0;

        // NQ recipes: the row reads the median of the quality it produces,
        // whichever quality happens to be cheaper. On the odd item ids —
        // where the fixture makes HQ a quarter of NQ — the two lookups
        // disagree, and #1264 rendered the second of them.
        let nq = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &opts(false),
        );
        let (mut checked, mut diverged) = (0, 0);
        for r in nq.iter().filter(|r| has_stats(r.recipe.item_result)) {
            let (out, m) = (r.recipe.item_result, median_of(r.recipe.item_result));
            let alt = r.rev_alt[PriceSignal::SaleMedian.index()];
            assert!(!r.stat_hq, "recipe {}", r.recipe.key_id.0);
            assert_eq!(r.sell_median, Some(m), "recipe {}", r.recipe.key_id.0);
            assert_eq!(
                alt,
                Some(m.min(hq_scaled(out, m, false))),
                "the Sale median (7d) column is still the cheaper quality"
            );
            checked += 1;
            diverged += usize::from(r.sell_median != alt);
        }
        assert!(checked > 5, "only {checked} rows carried statistics");
        assert!(
            diverged > 5,
            "only {diverged} rows told the two lookups apart"
        );

        // Require HQ and the row follows: its median is the HQ one — four
        // times dearer on the even ids, which is prod's shape and the
        // direction that read "+399900%" in green — while the
        // alternative-revenue column keeps meaning "the cheaper quality".
        let hq = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &opts(true),
        );
        let mut split = 0;
        // `hq_dearer_only` above is why this run is worth anything. With the
        // alternating split, every row that survived `require_hq` had an ODD
        // id, where `hq_scaled` is `m / 4` and both assertions below expect
        // the same number — the old basis satisfied them and the loop passed
        // while proving nothing about the HQ-dearer direction, the one that
        // read "+399900%" in green on prod. Forcing HQ dearer for every item
        // makes every surviving row discriminate, and the assert_ne! in the
        // loop proves that rather than counting on the draw.
        for r in hq.iter().filter(|r| has_stats(r.recipe.item_result)) {
            let (out, m) = (r.recipe.item_result, median_of(r.recipe.item_result));
            assert!(r.stat_hq, "recipe {}", r.recipe.key_id.0);
            assert_eq!(
                r.sell_median,
                Some(hq_scaled(out, m, true)),
                "recipe {}",
                r.recipe.key_id.0
            );
            assert_eq!(
                r.rev_alt[PriceSignal::SaleMedian.index()],
                Some(m.min(hq_scaled(out, m, true)))
            );
            split += 1;
            assert_ne!(
                hq_scaled(out, m, true),
                m.min(hq_scaled(out, m, true)),
                "recipe {} does not tell the two lookups apart",
                r.recipe.key_id.0
            );
        }
        // Only a couple of rows: `require_hq` costs every ingredient HQ and
        // the drop rule takes the rest. The 20-row pass above carries the
        // weight; this one covers the HQ *preference* reaching the row.
        assert!(
            split > 0,
            "the require_hq run dropped every row with statistics"
        );
    }

    #[test]
    fn lab_only_sort_modes_are_exactly_the_fourteen() {
        assert_eq!(ALL_SORT_MODES.iter().filter(|m| m.lab_only()).count(), 14);
        assert!(!SortMode::CostPerUnit.lab_only() && !SortMode::Price.lab_only());
        assert!(SortMode::ProfitPerDay.lab_only() && SortMode::Vwap30.lab_only());
        assert!(SortMode::ScopeVsHome.lab_only());
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
            assert_eq!(ids.len(), 23);
            assert!(ids.iter().all(|id| OPTIONAL_COLUMN_ORDER.contains(id)));
            let flat: Vec<&str> = picker_options(&RECIPE_COLUMNS, i18n)
                .iter()
                .map(|o| o.id)
                .collect();
            assert_eq!(flat, BASE_COLUMN_ORDER.as_slice());
        });
    }

    /// Every optional column is in a named group, and the two new ones hold
    /// what the kit says they hold.
    #[test]
    fn the_grouped_picker_lists_market_and_location() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let ctx = PickerContext {
                sell_place: "Gilgamesh".into(),
                buy_place: "Aether".into(),
                revenue: PriceSignal::ListingMin,
                cost: PriceSignal::ListingMin,
                capped: BTreeSet::new(),
            };
            let got = grouped_picker_options(&RECIPE_COLUMNS, i18n, &ctx);
            let mut headings: Vec<String> = got
                .iter()
                .map(|o| o.group.as_ref().expect("a heading").label.clone())
                .collect();
            headings.dedup();
            assert_eq!(
                headings,
                vec![
                    "Revenue · Gilgamesh",
                    "Cost · Aether",
                    "Travel",
                    "Market",
                    "Location"
                ]
            );
            let ids_in = |label: &str| -> Vec<&str> {
                got.iter()
                    .filter(|o| o.group.as_ref().unwrap().label == label)
                    .map(|o| o.id)
                    .collect()
            };
            assert_eq!(
                ids_in("Market"),
                [
                    "confidence",
                    "last-sold",
                    "volume",
                    "vwap",
                    "tax",
                    "profit-per-day",
                    "trend",
                    "drift",
                    "volume-30d",
                    "vwap-30d"
                ]
            );
            assert_eq!(
                ids_in("Travel"),
                ["hop-gain", "hop-worlds", "scope-vs-home"],
                "the picker groups by (group, table index), so the appended \
                 column still lists third in Travel"
            );
            assert_eq!(ids_in("Location"), ["listing-world", "listing-dc"]);
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
        // The revenue side is read the same way, from the same table: the
        // visible `rev-*` columns and a `rev-*` sort target. Both were
        // placeholders until Scope vs home needed them.
        assert_eq!(w.visible_rev, vec![PriceSignal::SaleMin]);
        assert_eq!(w.sort_rev, None);
        assert!(!w.scope_vs_home);
        let w = signal_wants(&visible, Some(SortMode::RevSignal(PriceSignal::SaleAvg)));
        assert_eq!(w.sort_rev, Some(PriceSignal::SaleAvg));
        assert_eq!(w.sort_cost, None);
        let w = signal_wants(&HashSet::new(), Some(SortMode::HopGain));
        assert!(w.hop && !w.worlds);
        let visible: HashSet<&'static str> = [COL_HOP_WORLDS].into_iter().collect();
        let w = signal_wants(&visible, None);
        assert!(w.worlds && !w.hop);
        // Scope vs home is wanted by its column OR its sort target, and by
        // nothing else — `hop` and `worlds` are the neighbouring flags a
        // copy-paste would reach for.
        assert!(!w.scope_vs_home);
        assert!(
            signal_wants(
                &[COL_SCOPE_VS_HOME].into_iter().collect(),
                Some(SortMode::Profit)
            )
            .scope_vs_home
        );
        assert!(signal_wants(&HashSet::new(), Some(SortMode::ScopeVsHome)).scope_vs_home);
        assert_eq!(
            signal_wants(&HashSet::new(), Some(SortMode::Profit)),
            SignalWants::default()
        );
        // Flag-off, all three new derivations are the placeholders they
        // replaced, and that is checked rather than argued: with the lab
        // off the `?cols=` contract is `BASE_COLUMN_ORDER`, which holds no
        // lab token, and the page filters a `lab_only` sort to `None`
        // before `signal_wants` is ever called (`:4054`).
        let off = parse_visible_cols(
            Some("scope-vs-home,rev-sale-min,hop-gain"),
            &BASE_COLUMN_ORDER,
            &DEFAULT_COLS,
        );
        assert_eq!(
            signal_wants(&off, None),
            SignalWants::default(),
            "no lab token survives parsing flag-off, so the pass is asked \
             for exactly what it was asked for before Phase F"
        );
        assert!(
            SortMode::ScopeVsHome.lab_only()
                && SortMode::RevSignal(PriceSignal::SaleMin).lab_only()
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

    /// The sell-scope resource key goes through `needed_bodies`, so the
    /// fetch gate lives in exactly one place — the rule `buy_stats_scope_key`
    /// and `stats_30_key` already follow.
    #[test]
    fn the_sell_scope_bodies_are_only_requested_when_a_wider_scope_is() {
        let world = ProfitFormula::recipe_from_query(None, None, None);
        let needs = RecipeNeeds::default();
        assert_eq!(sell_scope_key(&world, &needs, "Aether"), None);

        // Datacenter, listing revenue: the cheapest map only.
        // (`ProfitFormula` is `Copy`; a `.clone()` here is a
        // `clippy::clone_on_copy` failure under Task 9's `-D warnings`.)
        let dc = seat_sell_scope(world, true, Some(SellScope(Scope::Datacenter)));
        assert_eq!(
            sell_scope_key(&dc, &needs, "Aether"),
            Some(("Aether".to_string(), true, false))
        );

        // A place that has not resolved is not a market. `revenue_place`
        // reads `UNRESOLVED_PLACE` until a sell world exists, and a body
        // fetched under that name is a guaranteed miss wearing a label the
        // player reads as a place.
        assert_eq!(sell_scope_key(&dc, &needs, UNRESOLVED_PLACE), None);
        assert_eq!(sell_scope_key(&dc, &needs, ""), None);

        // Datacenter, sale revenue: both halves.
        let dc_stats = seat_sell_scope(
            ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMedian), None),
            true,
            Some(SellScope(Scope::Datacenter)),
        );
        assert_eq!(
            sell_scope_key(&dc_stats, &needs, "Aether"),
            Some(("Aether".to_string(), true, true))
        );

        // The scope matched the buy scope, whose cheapest body is
        // unconditional: only the statistics half is left to fetch.
        let deduped = RecipeNeeds {
            sell_scope_is_buy_scope: true,
            ..RecipeNeeds::default()
        };
        assert_eq!(
            sell_scope_key(&dc_stats, &deduped, "Aether"),
            Some(("Aether".to_string(), false, true))
        );
        // …and with a sale COST signal the buy side already fetched those
        // statistics, so there is nothing left at all.
        let both = seat_sell_scope(
            ProfitFormula::recipe_from_query(
                Some(PriceSignal::SaleMin),
                Some(PriceSignal::SaleMedian),
                Some(BuyScope::Datacenter),
            ),
            true,
            Some(SellScope(Scope::Datacenter)),
        );
        assert_eq!(sell_scope_key(&both, &deduped, "Aether"), None);

        // But if the buy scope ALIASES the sell world, `BuyScopeStats` is
        // never in the set and there is nothing to reuse — which is why the
        // page fills `buy_scope_is_sell_world` from its real gate rather
        // than letting `Default` answer `false`.
        //
        // `Some(BuyScope::World)`, spelled out: `BuyScope::default()` is the
        // DATACENTER, so the brief's `None` here left the alias rule unfired
        // and `BuyScopeStats` in the set — the same default trap that bit
        // Task 2's dedupe case 3, in the same test position.
        //
        // The page cannot produce this exact tuple (a datacenter's name is
        // never the sell world's, so `sell_scope_is_buy_scope` and a
        // World-aliased buy scope cannot both hold); it is kept for the same
        // reason Task 2 kept its case 3 — it is the only case here that
        // kills a `buy_covers` that ignores set membership.
        let aliased = RecipeNeeds {
            sell_scope_is_buy_scope: true,
            buy_scope_is_sell_world: true,
            ..RecipeNeeds::default()
        };
        let world_buy = seat_sell_scope(
            ProfitFormula::recipe_from_query(
                Some(PriceSignal::SaleMin),
                Some(PriceSignal::SaleMedian),
                Some(BuyScope::World),
            ),
            true,
            Some(SellScope(Scope::Datacenter)),
        );
        assert_eq!(
            sell_scope_key(&world_buy, &aliased, "Aether"),
            Some(("Aether".to_string(), false, true))
        );

        // Flag-off, `seat_sell_scope` hands the formula straight back, so a
        // bookmarked `?sell-scope=region` asks for nothing at all.
        let off = seat_sell_scope(world, false, Some(SellScope(Scope::Region)));
        assert_eq!(sell_scope_key(&off, &needs, "Aether"), None);
    }

    /// The page consults the gate rather than a constant, fills the needs
    /// from its real page state, and does not smuggle in a third viewport
    /// read. `-D warnings` proves only that *something* calls each one.
    #[test]
    fn the_page_wires_the_sell_scope_to_what_it_fetches() {
        // Squeezed throughout: every needle below is a multi-argument call
        // or a chained condition rustfmt is free to wrap, and a needle
        // written as one line then pins text the formatter never emits.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(&format!(
                "{}(&formula,&needs,&{})",
                "sell_scope_key", "place"
            )),
            "the resource key must come from `sell_scope_key`"
        );
        // A COUNT, not an existence check: `buy_sale_stats_scope` has
        // carried this exact line since Phase C, so `contains` alone is
        // satisfied before this task writes anything and can never fail.
        assert_eq!(
            squeezed
                .matches(&format!(
                    "{}:{}.get(),",
                    "buy_scope_is_sell_world", "buy_scope_is_sell_world"
                ))
                .count(),
            2,
            "the sell-scope needs must read the page's real alias gate too, \
             not `RecipeNeeds::default()`'s `false`"
        );
        // …and the formula that key is built from must come through the lab
        // gate. Reverting `formula_page` to a bare `recipe_from_query`
        // leaves `needed_bodies` looking at `Scope::World` on a lab-ON
        // `?sell-scope=` URL, so nothing is ever fetched — silently, because
        // the labels Tasks 5 and 6 wired read the param, not this formula.
        // (Task 8 pins the seating function's caller COUNT; this pins the
        // one caller this task's fetch depends on.)
        assert!(
            squeezed.contains(&format!(
                "{}(ProfitFormula::recipe_from_query(cost_basis(),revenue_metric(),buy_scope()),preview.get(),sell_scope(),)",
                "seat_sell_scope"
            )),
            "`formula_page` must seat the sell scope through the lab gate"
        );
        // `NeededSignals::rev`'s first production reader. It was written by
        // `needed_signals` and read by nothing that ships until this line,
        // and the dead-code lint cannot say so — the derived `Debug` and
        // `PartialEq` count as reads — so a forgotten wiring would leave
        // CI green and the feature doing nothing.
        assert!(
            squeezed.contains(&format!("{}:signals.{},", "rev_signals", "rev")),
            "the sell-scope needs must carry `NeededSignals::rev`, or a \
             visible `rev-sale-*` column never fetches its body"
        );
        // The two places must be ONE place. `revenue_place`'s datacenter
        // arm falls back to the region when no datacenter has resolved
        // yet; `sell_scope_key` sends a name to the API and
        // `sell_scope_is_buy_scope` compares a name against the buy
        // scope's. If either of those reads anything but `revenue_place`,
        // the page fetches one market, dedupes against a second and labels
        // a third — with no test able to see it, because each half is
        // internally consistent.
        assert!(
            squeezed.contains(&format!("let{}={}.get();", "place", "revenue_place")),
            "the name `sell_scope_key` sends is `revenue_place`, fallback arm \
             included — not `sell_place`, and not a second resolution"
        );
        // The whole condition, not just the equality: an unresolved name
        // compared against another unresolved name answers `true`, and the
        // dedupe then reuses a body `needed_bodies` never put in the set.
        assert!(
            squeezed.contains(&format!(
                "{p}(&{r}.get())&&{p}(&{b}.get())&&{r}.get()=={b}.get()",
                p = "place_resolved",
                r = "revenue_place",
                b = "buy_scope_name"
            )),
            "the dedupe gate compares `revenue_place` against the buy \
             scope's name, and only once both have resolved"
        );
        // Both new props must carry the page's real values. Nothing else
        // here would notice a literal at the call site: a
        // `sell_scope_bodies=None` simply never raises the banner, and a
        // `sell_scope_is_buy_scope=false` is Task 8's dedupe silently
        // reading the wrong body.
        assert!(
            squeezed.contains(&format!("{}=bodies", "sell_scope_bodies")),
            "the table must be handed the resource's payload"
        );
        assert!(
            squeezed.contains(&format!("{s}={s}.get()", s = "sell_scope_is_buy_scope")),
            "…and the page's real dedupe gate, not a constant"
        );
        // Global Constraint 6: Phase F adds no lazy fetch, so the viewport
        // signal is still read by exactly the two E2 gates.
        let reads = production_source().replace("use_wide_viewport", "");
        assert_eq!(
            reads.matches("wide_viewport.get()").count(),
            2,
            "Phase F must not add a third viewport-gated fetch"
        );
    }

    /// A sell-scope body that was asked for and did not arrive must be
    /// said, not silently re-priced: revenue falls through `SignalView`'s
    /// base layer to the buy scope while the strip, the picker heading and
    /// the live sentence all still name the scope. Both halves count —
    /// `listings_failed` as much as `stats_failed`, because the listing
    /// half is the one a listing-min URL depends on.
    #[test]
    fn a_failed_sell_scope_body_says_so_instead_of_silently_repricing() {
        let none = SellScopeBodies {
            listings: None,
            stats: None,
            listings_failed: false,
            stats_failed: false,
        };
        // The signal argument decides the listings-failed case, so every
        // assertion below names it. `true` = a sale signal, which reads the
        // statistics; `false` = listing-min, which cannot.
        for sale in [false, true] {
            assert_eq!(scope_fallback(&None, sale), None);
            assert_eq!(scope_fallback(&Some(none.clone()), sale), None);
            // Only the STATISTICS missed. `SignalView::quality` still holds
            // the scope's own cheapest listing, so revenue is priced at the
            // very place every label names and it is the *signal* that
            // degraded, not the market. Task 7 shipped one string for both
            // arms, saying prices "fall back to where ingredients are
            // priced" — false here, in seven locales.
            assert_eq!(
                scope_fallback(
                    &Some(SellScopeBodies {
                        stats_failed: true,
                        ..none.clone()
                    }),
                    sale
                ),
                Some(ScopeFallback::ScopeListings)
            );
            // Both gone: nothing here can price revenue, whatever the signal.
            assert_eq!(
                scope_fallback(
                    &Some(SellScopeBodies {
                        listings_failed: true,
                        stats_failed: true,
                        ..none.clone()
                    }),
                    sale
                ),
                Some(ScopeFallback::BuyScope)
            );
        }
        // The cheapest map missed and the statistics arrived. This is the
        // case Task 8 shipped wrong, and the one `SignalView::quality`
        // settles: it applies a non-zero stat row REGARDLESS of which layer
        // produced the listing, so a sale signal is still priced from this
        // market's own history and only a listing signal leaves it.
        let listings_only = Some(SellScopeBodies {
            listings_failed: true,
            ..none
        });
        assert_eq!(
            scope_fallback(&listings_only, true),
            Some(ScopeFallback::ScopeStats),
            "a sale signal reads the statistics, which arrived — the numbers \
             never left this market"
        );
        assert_eq!(
            scope_fallback(&listings_only, false),
            Some(ScopeFallback::BuyScope),
            "a listing signal cannot be rescued by statistics it never reads"
        );
        // Scoped to the ARM, not to the file: two existence checks would be
        // satisfied by a build that swapped the two keys, which is exactly
        // the defect being fixed wearing the other arm's clothes (Task 6's
        // review, minor 3). `view!` bodies are untouched by rustfmt, so the
        // squeezed text is stable.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(&format!(
                "ScopeFallback::BuyScope=>view!{{{{t!(i18n,{},",
                "recipe_analyzer_sell_scope_unavailable"
            )),
            "the failed-cheapest-map arm must say the numbers left the place \
             the labels name"
        );
        assert!(
            squeezed.contains(&format!(
                "ScopeFallback::ScopeListings=>view!{{{{t!(i18n,{},",
                "recipe_analyzer_sell_scope_stats_unavailable"
            )),
            "…and the stats-only arm must say they stayed there"
        );
        assert!(
            squeezed.contains(&format!(
                "ScopeFallback::ScopeStats=>view!{{{{t!(i18n,{},",
                "recipe_analyzer_sell_scope_listings_unavailable"
            )),
            "…and the listings-only arm must say they stayed there too, via \
             the sale history — the arm Task 8 first shipped as ToBuyScope"
        );
        // Squeezed: the call is multi-line, so a raw needle would search for
        // text rustfmt will never emit — the failure this phase already
        // shipped once.
        assert!(
            squeezed.contains(&format!(
                "{}(&sell_scope_bodies,formula.get_untracked().revenue_signal().sale_stat().is_some(),)",
                "scope_fallback"
            )),
            "…off the same helper this test pins, and told which revenue \
             signal is in play — without that argument the listings-only arm \
             cannot be distinguished from the both-failed one"
        );

        // The second line must cost the no-payload page NOTHING, and the
        // no-payload page is every flag-off one (`sell_scope_bodies` is
        // `None` there by construction — `sell_scope_key` returns `None`).
        // An `Option` child that resolves to `None` still writes a `<!>`
        // hydration marker, so a bare second `.then(..)` beside the
        // existing amber line — which is what this task's brief called
        // for — would add a marker to every page and break flag-off
        // byte-identity. Both shapes are rendered here because the property
        // is the CONSTRUCTION's, not this page's; the source read below is
        // what ties production to the construction, and this half is worth
        // exactly that much.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let line = || view! { <div class="text-amber-400 text-sm">"amber"</div> };
            for stats_error in [false, true] {
                let today = view! {
                    <div class="flex flex-col gap-6">{stats_error.then(line)}</div>
                }
                .to_html();
                let with_second_line = view! {
                    <div class="flex flex-col gap-6">
                        {match false {
                            false => stats_error.then(line).into_any(),
                            true => view! {
                                {stats_error.then(line)}
                                <div class="text-amber-400 text-sm">"scope"</div>
                            }
                            .into_any(),
                        }}
                    </div>
                }
                .to_html();
                assert_eq!(
                    today, with_second_line,
                    "one `match` child renders the no-payload page exactly as \
                     it renders today (stats_error={stats_error})"
                );
                // The control the assertion above would be worthless
                // without: two `Option` children really do differ.
                let two_children = view! {
                    <div class="flex flex-col gap-6">
                        {stats_error.then(line)}
                        {false.then(line)}
                    </div>
                }
                .to_html();
                assert_ne!(
                    today, two_children,
                    "a second `Option` child writes a second `<!>` marker"
                );
            }
        });
        // Two halves rather than one literal: the call grew a second
        // argument, and pinning the whole expression here would duplicate
        // the call-shape needle above and break on every future argument.
        // What this assertion is FOR is the `None` arm — one child, not two.
        let squeezed_page = production_squeezed();
        assert!(
            squeezed_page.contains(&format!("match{}(&sell_scope_bodies,", "scope_fallback")),
            "the amber block must be one match on the fallback helper"
        );
        assert!(
            squeezed_page.contains("){None=>stats_line.into_any(),"),
            "production must route both amber lines through ONE child, or \
             the flag-off DOM grows a hydration marker"
        );
    }

    /// Every `.rs` under `src/`, paired with its production half, read off
    /// disk so a file added later is covered without anyone remembering to
    /// list it.
    ///
    /// The single-seam invariant this feeds is a **crate** property, not a
    /// file property. `with_sell_scope` is `pub` and `analyzer_kit` is
    /// reachable from everywhere, so a second production caller added in
    /// (say) `analyzer_kit/signals.rs` would satisfy `dead_code` — the
    /// method is live by now — and be completely invisible to a needle
    /// that reads only this file. `pub(in ...)` cannot close it either:
    /// `formula.rs` is not an ancestor of `routes::recipe_analyzer`.
    ///
    /// A file is split at its first `#[cfg(test)]` **only** when what
    /// follows really is a trailing test module; otherwise the whole file
    /// counts as production. Two files in this crate put a `#[cfg(test)]`
    /// helper ahead of real code (`components/data_table.rs`,
    /// `components/crafting_cost.rs`), and a blind split would hide
    /// everything after it. Over-counting is the safe direction for a test
    /// whose job is to find a caller nobody declared.
    fn crate_production_halves() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
                .expect("the crate's src tree is readable")
                .map(|e| e.expect("a readable directory entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("a readable source file");
                // Normalised so the assertion below reads the same on
                // Windows as it does in CI.
                let full = path.to_string_lossy().replace('\\', "/");
                let name = match full.rsplit_once("/src/") {
                    Some((_, rel)) => rel.to_string(),
                    None => full,
                };
                let production = match src.split_once(&format!("#[cfg({})]", "test")) {
                    Some((head, rest))
                        if rest.trim_start().starts_with(&format!("mod {}", "test")) =>
                    {
                        head.to_string()
                    }
                    _ => src,
                };
                out.push((name, production));
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        assert!(
            out.len() > 100,
            "the walk must reach the whole crate, not one directory"
        );
        out
    }

    /// **The Phase F pin.** The page's ledger and the table's ledger are two
    /// different constructions and only the table's prices rows, so a scope
    /// seated on the page alone yields a column of dashes behind a green
    /// suite — which is exactly how Phase E2's median tell shipped. Four
    /// assertions, in order of how hard they are to fool:
    ///
    /// 1. `with_sell_scope` has ONE caller in the crate's production half,
    ///    and it is in this file. A second one means somebody re-inlined
    ///    the seating and the two paths can drift again.
    /// 2. `seat_sell_scope` has exactly three call sites here: its own
    ///    definition, the page's `formula_page`, and the TABLE's
    ///    `formula`. Unwire the table and this drops to two.
    /// 3. The two seatings are TOLD APART, so the count cannot be
    ///    satisfied by a wrapper. `fn table_formula(..) { seat_sell_scope(..) }`
    ///    keeps the count at three while the table stops calling it, so
    ///    the counts only bite when something also pins the two call
    ///    *shapes*.
    /// 4. A pricing pass whose formula came out of that function — the same
    ///    call `run_with` makes — actually fills the column. This is the
    ///    behavioural half: the counts could all hold while the seating
    ///    did nothing.
    ///
    /// Assertion 3's needles are matched against `production_squeezed()`,
    /// not `production_source()`. Both seatings are calls rustfmt is
    /// obliged to break one-argument-per-line: the first argument alone,
    /// `ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(),
    /// buy_scope()),`, is 76 characters at indent 12, so the call cannot
    /// fit in 100 columns and a single-line needle would pin text the
    /// formatter will never emit.
    #[test]
    fn the_tables_own_formula_is_what_fills_the_scope_column() {
        // The needle carries the leading dot so it counts CALLS: a bare
        // `with_sell_scope(` also matches the method's own definition in
        // `formula.rs`, which would make the "nowhere else" half of this
        // assertion unwritable.
        let call = format!(".{}(", "with_sell_scope");
        let callers: Vec<(String, usize)> = crate_production_halves()
            .into_iter()
            .map(|(name, production)| (name, production.matches(&call).count()))
            .filter(|(_, n)| *n > 0)
            .collect();
        assert_eq!(
            callers,
            vec![("routes/recipe_analyzer.rs".to_string(), 1)],
            "`with_sell_scope` is called from exactly one place in the whole \
             crate — `seat_sell_scope` — and from nowhere else, or the page \
             and the table can seat the scope differently again"
        );

        let production = production_source();
        assert_eq!(
            production
                .matches(&format!("{}(", "with_sell_scope"))
                .count(),
            1,
            "`with_sell_scope` is called in exactly one place: `seat_sell_scope`"
        );
        assert_eq!(
            production
                .matches(&format!("{}(", "seat_sell_scope"))
                .count(),
            3,
            "its definition, the page's `formula_page`, and the TABLE's \
             `formula` memo — if this reads 2, the table is unwired and the \
             column ships as dashes"
        );
        // The two call SHAPES, which is what makes the count above bite.
        // They are distinguishable on purpose: the page seats from signals
        // (`preview.get()`, `sell_scope()`), the table from its two props
        // (`preview`, `sell_scope`), so neither needle can stand in for the
        // other and a wrapper that keeps the count at three fails here.
        //
        // Both needles are anchored on `seat_sell_scope(` itself, which the
        // brief's text was not: without the name, a three-argument
        // `table_formula(ProfitFormula::recipe_from_query(..), preview,
        // sell_scope)` wrapper matches the needle character for character,
        // so the "a wrapper fails here" claim in the doc above would have
        // been false. Verified by mutation both ways.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(
                "seat_sell_scope(ProfitFormula::recipe_from_query(cost_basis(),\
                 revenue_metric(),buy_scope()),preview,sell_scope,)"
            ),
            "the TABLE's formula memo must seat the scope from its own props"
        );
        assert!(
            squeezed.contains(
                "seat_sell_scope(ProfitFormula::recipe_from_query(cost_basis(),\
                 revenue_metric(),buy_scope()),preview.get(),sell_scope(),)"
            ),
            "…and the page's `formula_page` from its own signals"
        );

        // The behavioural half. `run_with` builds its formula with the same
        // function, so this exercises the production seating rather than a
        // hand-written `with_sell_scope`.
        let wanted = NeededSignals {
            scope_vs_home: true,
            ..NeededSignals::default()
        };
        let rows = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                needs: wanted.clone(),
                sell_scope: Some(Scope::Region),
                scope_bodies: true,
                ..RunOpts::default()
            },
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.scope_vs_home, ScopeVsHome::Pair { .. })),
            "a pass seated through `seat_sell_scope` must fill the column"
        );
        // …and the flag-off arm of the same function leaves it empty.
        let off = seat_sell_scope(
            ProfitFormula::recipe_from_query(None, None, None),
            false,
            Some(SellScope(Scope::Region)),
        );
        assert_eq!(off.sell_scope(), Scope::World);
    }

    /// Which body the table prices revenue from. The middle case — the
    /// scope resolved to the buy scope's place, so the buy-side body stands
    /// in — is a silent re-price if it is wrong, and it is unreachable from
    /// a unit test while it lives inside the component, so it does not.
    #[test]
    fn the_table_resolves_the_revenue_side_from_the_pages_scope() {
        // NO `use RevenueSource::*;` here. Its `Scope` and `BuyScope`
        // variants land in the TYPE namespace and shadow the `Scope` alias
        // and the `BuyScope` enum this module imports from
        // `analyzer_kit::formula`, and `Scope::World` then fails to resolve
        // with `E0433: Scope is a variant, not a module`. Spell the
        // variants out.
        use RevenueSource::{BuyScope as FromBuyScope, Missing, Scope as FromScope, SellWorld};
        // Default scope: the sell world's own bodies, whatever else is true.
        for is_buy in [false, true] {
            for have in [false, true] {
                assert_eq!(
                    revenue_listings_source(Scope::World, is_buy, have),
                    SellWorld
                );
                assert_eq!(revenue_stats_source(Scope::World, is_buy, have), SellWorld);
            }
        }
        // Wider, body present: the scope's own, even if it also happens to
        // be the buy scope's place.
        assert_eq!(
            revenue_listings_source(Scope::Region, false, true),
            FromScope
        );
        assert_eq!(
            revenue_listings_source(Scope::Region, true, true),
            FromScope
        );
        // Wider, no body, but the place IS the buy scope: reuse it. That is
        // the dedupe `needed_bodies` counted on when it skipped the fetch.
        assert_eq!(
            revenue_listings_source(Scope::Datacenter, true, false),
            FromBuyScope
        );
        assert_eq!(
            revenue_stats_source(Scope::Datacenter, true, false),
            FromBuyScope
        );
        // Wider, no body, not the buy scope: nothing. `SignalView` falls to
        // its base layer for listings and `rev-sale-*` cells go "—" — and
        // Task 7's banner is what tells the player.
        assert_eq!(
            revenue_listings_source(Scope::Region, false, false),
            Missing
        );
        assert_eq!(revenue_stats_source(Scope::Region, false, false), Missing);

        // Squeezed, per `production_squeezed()`'s doc: both are three-argument
        // calls rustfmt breaks onto one line per argument, so `…_source(`
        // and `sell_scope_value` never share a source line.
        let squeezed = production_squeezed();
        assert!(
            squeezed.contains(&format!("{}(sell_scope_value,", "revenue_listings_source"))
                && squeezed.contains(&format!("{}(sell_scope_value,", "revenue_stats_source")),
            "the table must resolve through both helpers, not an inline match"
        );
        // …and the four arms each reach the value they name. The rule above
        // is a pure function with a truth table; the arm -> value mapping
        // lives inside the component and no unit test can render it, so a
        // swapped arm — `Scope => sell_world_prices`, the silent re-price
        // this whole task exists to prevent — is invisible to everything
        // else in this suite. A source read is worth exactly this much, and
        // it is more than nothing.
        assert!(
            squeezed.contains(
                "RevenueSource::SellWorld=>sell_world_prices.clone(),\
                 RevenueSource::BuyScope=>Some(prices.clone()),\
                 RevenueSource::Scope=>scope_prices,\
                 RevenueSource::Missing=>None,"
            ),
            "the cheapest-map arms must each read the body they name"
        );
        assert!(
            squeezed.contains(
                "RevenueSource::SellWorld=>(Some(sell_stats_index.clone()),sell_stats_loaded),\
                 RevenueSource::BuyScope=>(buy_stats_index.clone(),buy_stats_loaded),\
                 RevenueSource::Scope=>(scope_stats_index,true),\
                 RevenueSource::Missing=>(None,false),"
            ),
            "…and the statistics arms must publish the loaded flag that goes \
             with the body they read, or a failed scope fetch leaves the \
             strip's dot lit over fallen-back numbers"
        );
        // The two consumers of that flag. Substituting `sell_stats_loaded`
        // back into either is a one-token edit that nothing else in this
        // suite can see — both were live survivors of the mutation campaign
        // until these two needles existed — and each is a real defect: the
        // first lets the header marks name a sale signal the rows fell back
        // from, the second lets the strip's amber dot stay dark while they
        // did.
        assert!(
            squeezed.contains(".effective(buy_stats_loaded,revenue_stats_loaded)"),
            "the table's formula must downgrade on the body REVENUE reads"
        );
        assert!(
            squeezed.contains("stats_loaded.set((buy_stats_loaded,revenue_stats_loaded))"),
            "…and the pair it publishes to the strip must say the same thing"
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

    /// The enrichment key is the item the recipe produces plus the quality
    /// the row's *statistics* resolved to, so Trend, Drift and the 7-day
    /// numbers beside them all describe the same market.
    #[test]
    fn recipe_spark_key_is_item_and_stat_quality() {
        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(1)
            .map(|r| r.key_id.0)
            .collect();
        let mut r = Arc::try_unwrap(row(keys[0], 0, 0, 1.0, 1)).ok().unwrap();
        assert_eq!(
            recipe_spark_key(&(0, Arc::new(r.clone()))),
            (r.recipe.item_result, false)
        );
        r.stat_hq = true;
        assert_eq!(
            recipe_spark_key(&(3, Arc::new(r.clone()))),
            (r.recipe.item_result, true),
            "the key follows the quality the row's statistics came from"
        );
    }

    /// One series in, one keyed value out: the colour driver is computed
    /// here, so the cell never scans the points.
    #[test]
    fn a_series_becomes_a_keyed_spark_value() {
        let up = SparklineSeries {
            item_id: 42,
            hq: true,
            world_id: 1,
            points: vec![100, 0, 150],
            first_price: 100,
            last_price: 150,
        };
        let (key, value) = spark_entry(up);
        assert_eq!(key, (42, true));
        assert_eq!(value.points, vec![100, 0, 150]);
        assert_eq!(value.delta_pct, Some(50.0));
        // Nothing traded anywhere in the window (`first_price` is the first
        // non-zero point): no percentage, so the sparkline reads neutral and
        // Drift shows the dash.
        let quiet = SparklineSeries {
            item_id: 7,
            hq: false,
            world_id: 1,
            points: vec![0, 0],
            first_price: 0,
            last_price: 0,
        };
        assert_eq!(spark_entry(quiet).1.delta_pct, None);
    }

    /// The visible window is one request, derived from the grid's own
    /// geometry rather than a literal, and under the endpoint's 200-key cap.
    #[test]
    fn the_recipe_window_is_one_request_per_scroll_settle() {
        let rendered = rows_for_viewport(
            RECIPE_GRID.viewport_height - RECIPE_GRID.header_height,
            RECIPE_GRID.row_height,
            RECIPE_GRID.overscan,
        ) as usize;
        assert_eq!(rendered, 19, "11 rows in 656 px plus 8 overscan");
        let keys: Vec<SparkKey> = (0..rendered + 2 * PREFETCH_MARGIN)
            .map(|i| (i as i32, false))
            .collect();
        assert_eq!(keys.len(), 79);
        assert_eq!(
            chunk_keys(&keys, RECIPE_ENRICHMENT.max_keys_per_request).len(),
            1
        );
        assert_eq!(RECIPE_TREND_FEED.hours(), 168);
    }

    /// Rows for the fetch-window tests: one per fixture recipe, so every
    /// key is a distinct row rather than a repeat of the same item.
    fn window_rows() -> Vec<(usize, RecipeRow)> {
        let recipes = fixture_recipes();
        let base = Arc::try_unwrap(row(recipes[0].key_id.0, 0, 0, 1.0, 1))
            .ok()
            .unwrap();
        recipes
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut d = base.clone();
                d.recipe = r;
                (i, Arc::new(d))
            })
            .collect()
    }

    /// The window the lazy fetch actually asks for. Task 5 added the
    /// `visible_range` prop and proved only that it is additive on the
    /// server render; nothing exercised a real range. Both failure
    /// directions matter here: a range that never moves means the columns
    /// below the fold never load, and a range that spans the table means
    /// one scroll settle fetches every row on the page.
    #[test]
    fn the_visible_range_follows_the_scroll_and_bounds_the_fetch() {
        let shown = rows_for_viewport(
            RECIPE_GRID.viewport_height - RECIPE_GRID.header_height,
            RECIPE_GRID.row_height,
            RECIPE_GRID.overscan,
        ) as usize;

        // Where the scroller starts rendering, for this grid's row height.
        // Uniform rows, so no measured per-row deltas.
        let first_at = |scroll: f64, len: usize| {
            first_visible_row(
                len,
                RECIPE_GRID.row_height,
                scroll,
                |_| 0.0,
                RECIPE_GRID.overscan,
            ) as usize
        };
        // Unscrolled, the window starts at the top.
        assert_eq!(first_at(0.0, 500), 0);
        assert_eq!(first_at(1.0, 500), 0, "part of a row still shows row 0");
        // Half the overscan renders above the fold, so a scroll of exactly
        // n rows starts at n - 4: the range moves with the scroll.
        assert_eq!(first_at(100.0 * RECIPE_GRID.row_height, 500), 96);
        assert_eq!(first_at(200.0 * RECIPE_GRID.row_height, 500), 196);

        // ... and the range that first row publishes.
        assert_eq!(rendered_range(0, shown, 500), (0, 19));
        assert_eq!(rendered_range(96, shown, 500), (96, 115));
        // Near the end it clamps to the data instead of running past it.
        assert_eq!(rendered_range(495, shown, 500), (495, 500));
        // Fewer rows than the viewport holds: the whole table, once.
        assert_eq!(rendered_range(0, shown, 4), (0, 4));
        // Nothing rendered, nothing to fetch.
        assert_eq!(rendered_range(0, shown, 0), (0, 0));

        // What the hook does with that range: the rendered window plus the
        // prefetch margin either side, in row order, never the whole table.
        let rows = window_rows();
        assert!(rows.len() > shown + 2 * PREFETCH_MARGIN);
        let scrolled = first_at(100.0 * RECIPE_GRID.row_height, rows.len());
        let range = rendered_range(scrolled, shown, rows.len());
        assert_eq!(range, (96, 115));
        let keys = visible_keys(
            &rows,
            range,
            PREFETCH_MARGIN,
            &HashSet::new(),
            recipe_spark_key,
        );
        let expected: Vec<SparkKey> = rows[66..145]
            .iter()
            .map(|(_, r)| (r.recipe.item_result, r.stat_hq))
            .collect();
        assert_eq!(keys, expected);
        assert_eq!(keys.len(), 79);
        assert!(
            keys.len() < rows.len(),
            "a scroll settle must not fetch the whole table"
        );
        // Settling again in the same place: every key is claimed already, so
        // the hook has nothing left to ask for.
        let seen: HashSet<SparkKey> = keys.into_iter().collect();
        assert!(visible_keys(&rows, range, PREFETCH_MARGIN, &seen, recipe_spark_key).is_empty());
    }

    /// The 30-day body is fetched only when a 30-day column asks for it,
    /// and cannot be asked for at all with the toggle off. Nothing in
    /// `needed.rs` can catch a gate that is never computed: with `stats_30`
    /// left false those two columns shimmer forever and every test there
    /// still passes.
    #[test]
    fn the_thirty_day_body_is_only_requested_when_a_30d_column_is() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let idle = RecipeNeeds::default();
        assert_eq!(stats_30_key(&f, &idle, Some("Gilgamesh")), None);
        let wants = RecipeNeeds {
            stats_30: true,
            ..RecipeNeeds::default()
        };
        assert_eq!(
            stats_30_key(&f, &wants, Some("Gilgamesh")),
            Some("Gilgamesh".into())
        );
        // No sell world resolved yet: nothing to fetch from.
        assert_eq!(stats_30_key(&f, &wants, None), None);

        // Visibility and the sort target are separate paths into the gate,
        // and each 30-day column reaches it on its own.
        for token in [COL_VOLUME_30D, COL_VWAP_30D] {
            let on = parse_visible_cols(Some(token), &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS);
            assert!(stats_30_wanted(&on, None, true), "{token} visible");
            // Toggle off: the token is not in the contract, so it never
            // survives parsing and the body is unreachable.
            let off = parse_visible_cols(Some(token), &BASE_COLUMN_ORDER, &DEFAULT_COLS);
            assert!(
                !stats_30_wanted(&off, None, true),
                "{token} with the toggle off"
            );
        }
        for mode in [SortMode::Volume30, SortMode::Vwap30] {
            assert!(stats_30_wanted(&HashSet::new(), Some(mode), true), "{mode}");
            // ... and off, where a lab-only sort token reads as unset.
            assert!(mode.lab_only(), "{mode}");
        }
        // The off direction: neither a plain page nor another sort target
        // reaches the 438 KB body.
        assert!(!stats_30_wanted(&HashSet::new(), None, true));
        assert!(!stats_30_wanted(
            &HashSet::new(),
            Some(SortMode::Profit),
            true
        ));
        let default_page = parse_visible_cols(None, &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS);
        assert!(!stats_30_wanted(&default_page, None, true));

        // End to end, the way the page composes them: the visible columns
        // and the sort target are what the fetch key is built from.
        let from = |visible: &HashSet<&'static str>, sort| {
            stats_30_key(
                &f,
                &RecipeNeeds {
                    stats_30: stats_30_wanted(visible, sort, true),
                    ..RecipeNeeds::default()
                },
                Some("Gilgamesh"),
            )
        };
        let vwap_on = parse_visible_cols(Some(COL_VWAP_30D), &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS);
        assert_eq!(from(&vwap_on, None), Some("Gilgamesh".into()));
        assert_eq!(
            from(&HashSet::new(), Some(SortMode::Volume30)),
            Some("Gilgamesh".into())
        );
        assert_eq!(from(&default_page, None), None);
    }

    /// The sparkline half of the same guarantee: the page mirrors its sorted
    /// rows for the hook only while a lazy column is on, and an empty mirror
    /// is no request at all. With the toggle off neither token survives
    /// `parse_visible_cols`, so the flag-off page issues no sparklines POST.
    #[test]
    fn the_sparkline_fetch_is_unreachable_with_the_toggle_off() {
        for token in [COL_TREND, COL_DRIFT] {
            let on = parse_visible_cols(Some(token), &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS);
            assert!(spark_rows_wanted(&on, true), "{token} visible");
            let off = parse_visible_cols(Some(token), &BASE_COLUMN_ORDER, &DEFAULT_COLS);
            assert!(
                !spark_rows_wanted(&off, true),
                "{token} with the toggle off"
            );
        }
        // The default page wants neither, toggle or no toggle.
        assert!(!spark_rows_wanted(
            &parse_visible_cols(None, &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS),
            true
        ));
        // An empty mirror is what the hook sees then, and it selects no
        // keys at all: its effect returns before it schedules a fetch.
        let empty: Vec<(usize, RecipeRow)> = Vec::new();
        assert!(
            visible_keys(
                &empty,
                rendered_range(0, 19, 0),
                PREFETCH_MARGIN,
                &HashSet::new(),
                recipe_spark_key,
            )
            .is_empty()
        );
    }

    /// Below `md` every one of the four lazy market columns is `hidden`, so
    /// neither body can put a pixel on screen and neither gate may open —
    /// however loudly `?cols=` or `?sort=` asks. Every input that opens a
    /// gate at `md` and up is re-run narrow here, including the two sort
    /// targets: the recipe analyzer has no mobile sort control, so a
    /// `?sort=volume-30d` on a phone can only have come from a link copied
    /// off a desktop, and `effective_sort_mode` already reads an unloaded
    /// 30-day body as Profit — the order the page paints anyway.
    #[test]
    fn a_narrow_viewport_closes_both_gates() {
        for token in [COL_VOLUME_30D, COL_VWAP_30D] {
            let on = parse_visible_cols(Some(token), &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS);
            assert!(stats_30_wanted(&on, None, true), "{token} wide");
            assert!(!stats_30_wanted(&on, None, false), "{token} narrow");
        }
        for mode in [SortMode::Volume30, SortMode::Vwap30] {
            assert!(stats_30_wanted(&HashSet::new(), Some(mode), true), "{mode}");
            assert!(
                !stats_30_wanted(&HashSet::new(), Some(mode), false),
                "{mode} narrow"
            );
            // A 30-day sort with no body behind it is Profit, which is the
            // order the first paint uses at every width. So the narrow page
            // keeps its painted order instead of shuffling once 438 KB has
            // landed for a column it cannot draw.
            assert_eq!(effective_sort_mode(mode, false), SortMode::Profit);
        }
        for token in [COL_TREND, COL_DRIFT] {
            let on = parse_visible_cols(Some(token), &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS);
            assert!(spark_rows_wanted(&on, true), "{token} wide");
            assert!(!spark_rows_wanted(&on, false), "{token} narrow");
        }
        // Both columns at once, and every optional column the page offers:
        // still nothing, because nothing is visible.
        let everything: HashSet<&'static str> = OPTIONAL_COLUMN_ORDER.iter().copied().collect();
        assert!(!stats_30_wanted(&everything, Some(SortMode::Vwap30), false));
        assert!(!spark_rows_wanted(&everything, false));

        // A closed 30-day gate must reach the page's "nothing asked for
        // it" ending, not its "asked but unanswerable" one. The two differ:
        // the second stores an empty index to settle the cells, and that
        // index would then satisfy the effect's `is_some` guard, so a
        // phone rotated into landscape would show a permanent "—" instead
        // of fetching. Narrow is temporary and reverses without a world
        // change, so it must leave the store untouched.
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let narrow = RecipeNeeds {
            stats_30: stats_30_wanted(&everything, Some(SortMode::Vwap30), false),
            ..RecipeNeeds::default()
        };
        assert!(
            !needed_bodies(&f, &narrow).contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS))
        );
        assert_eq!(stats_30_key(&f, &narrow, Some("Gilgamesh")), None);

        // And the sparkline half all the way to its spawn site: a closed
        // gate empties the rows mirror, an empty mirror selects no keys,
        // and `use_visible_enrichment` returns at `keys.is_empty()` before
        // it bumps `fetch_id` — so no `spawn_local`, not merely no request.
        let mirror: Vec<(usize, RecipeRow)> = Vec::new();
        assert!(
            visible_keys(
                &mirror,
                rendered_range(0, 19, 0),
                PREFETCH_MARGIN,
                &HashSet::new(),
                recipe_spark_key,
            )
            .is_empty()
        );
    }

    /// Both gates above are pure functions, and nothing in a unit test can
    /// render this page to see whether it consults them. `-D warnings`
    /// proves only that something calls each one; it cannot see that the
    /// answer is what the page acts on. So read the module's production half
    /// back out of the source, the way
    /// `the_grid_call_opts_into_a_sized_row_spacer` reads the grid call.
    ///
    /// The asymmetry is why this is worth a test: a gate wired to a constant
    /// `false` leaves every test in `needed.rs` and every test here green
    /// while the two 30-day columns shimmer forever, and a gate wired to a
    /// constant `true` ships a 438 KB body to the default page.
    #[test]
    fn the_page_wires_both_gates_to_what_it_fetches() {
        const SRC: &str = include_str!("recipe_analyzer.rs");
        // Assembled at run time: `include_str!` pulls in this test module
        // too, so a literal needle would satisfy itself. Splitting on the
        // module header keeps the search to the production half.
        // Anchored on the attribute too: a bare `mod test {` could appear
        // in a doc comment or a string above and silently truncate the
        // region being searched, failing this test with nothing wrong.
        // Split on the two anchors rather than one needle holding a
        // real newline: a CRLF checkout would make that needle miss and
        // panic here with nothing actually wrong.
        let (production, rest) = SRC
            .split_once(&format!("#[cfg({})]", "test"))
            .expect("the production half ends at the test module attribute");
        assert!(
            rest.trim_start().starts_with(&format!("mod {} {{", "test")),
            "the attribute ending the production half must be the test module's"
        );
        assert!(
            production.contains(&format!("{}: {}(", "stats_30", "stats_30_wanted")),
            "the page's RecipeNeeds must take stats_30 from the visible columns and the sort target"
        );
        assert!(
            production.contains(&format!("{}(v, {})", "spark_rows_wanted", "wide")),
            "the rows mirror must be gated on a visible Trend or Drift column"
        );
        // And the viewport reaches both of them. `-D warnings` proves only
        // that *something* is passed for `wide`; a page that passed a
        // literal `true` would compile, ship the 438 KB body to a phone,
        // and leave every assertion above green.
        assert!(
            production.contains(&format!(
                "let {} = {}();",
                "wide_viewport", "use_wide_viewport"
            )),
            "the page must own the viewport signal, not a constant"
        );
        // The helper's own name contains the signal's, so strip it first —
        // otherwise a doc comment mentioning `use_wide_viewport()` reads as
        // a call on the signal.
        let reads = production.replace("use_wide_viewport", "");
        assert_eq!(
            reads.matches("wide_viewport.get()").count(),
            2,
            "the viewport signal is read by exactly the two fetch gates"
        );
        // A `.get()` count alone is bypassable by the exact mistake it
        // guards: `Signal<bool>` is callable, so `move || !wide_viewport()`
        // inside a `view!` would leave the count at 2 and this test green
        // while putting a `matchMedia` read into markup. Ban the other read
        // forms outright — a fetch-path reader has no reason to need them.
        assert!(
            !reads.contains("wide_viewport("),
            "call syntax on the viewport signal bypasses the `.get()` count              above; if a fetch path needs it, spell it `.get()`"
        );
        assert!(
            !reads.contains("wide_viewport.with"),
            "`.with` on the viewport signal bypasses the `.get()` count above"
        );
        // The rule that keeps SSR and the first client render identical:
        // the signal is a fetch-path input and never a rendered value. A
        // `view!` binding is a prop, not markup, so the one occurrence
        // there is the hand-off to the table.
        assert_eq!(
            production
                .matches(&format!("{}={}", "wide_viewport", "wide_viewport"))
                .count(),
            1,
            "the viewport signal is handed to the table once, as a prop"
        );
    }
}
