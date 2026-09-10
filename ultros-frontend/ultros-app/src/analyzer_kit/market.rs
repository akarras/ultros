//! Shared market inputs and optional grid columns for every analyzer.
//!
//! Bulk statistics describe the selected scope and exact quality. Expensive
//! hourly history is fetched for the displayed window, accumulated for the
//! life of that scope, and advertises partial filter coverage to QueryGrid.

use std::{collections::HashMap, hash::Hash, sync::Arc};

use leptos::prelude::*;
use thousands::Separable;
use ultros_api_types::{
    cheapest_listings::{CheapestListingMapKey, CheapestListingsMap},
    listing_stats::ItemListingStats,
    sale_stats::ItemSaleStats,
    sparklines::{SparklinesRequest, SparklinesResponse},
    trends::ConfidenceBand,
};

use crate::{
    analysis::format_duration_short,
    api::{get_listing_stats, get_sale_stats, post_sparklines},
    components::{
        app_link::use_location_or_default,
        sparkline::Sparkline,
        virtual_grid::{
            GridColumn,
            metrics::{GridMetric, GridValue, active_metric_columns},
            query_grid::{MetricSortHeader, QueryGrid},
        },
    },
    global_state::LocalWorldData,
    i18n::*,
};

use super::{
    enrichment::{
        Absorb, DEBOUNCE_MS, Enrichment, EnrichmentConfig, PREFETCH_MARGIN, SparkValue,
        use_visible_enrichment,
    },
    formula::PriceSignal,
    signals::{StatsIndex, stat_only, stats_index},
    stat_columns::{
        FOLLOW_COLUMNS, LISTING_COLUMNS, ListingKind, STAT_COLUMNS, StatKind, Window, follow_id,
        listing_id, listing_label, listing_title, listings_wanted, market_picker_group,
        market_picker_group_listings, required_windows, stat_column, stat_label,
    },
    window::MarketWindow,
};

type ScopedStats = Option<(String, Arc<StatsIndex>, bool)>;

pub type ListingIndex = HashMap<(i32, bool), ItemListingStats>;

/// One current-listing body for one scope. `fetched_unix` is the clock the
/// age columns count from, captured once so cells stay pure functions of
/// the payload rather than re-reading the clock on every render.
#[derive(Clone, Debug, PartialEq)]
pub struct ListingSlot {
    pub scope: String,
    pub index: Arc<ListingIndex>,
    /// The request failed; the empty index is not evidence of an empty board.
    pub failed: bool,
    pub fetched_unix: i64,
}

type ScopedListings = Option<ListingSlot>;

pub fn listing_index(stats: &[ItemListingStats]) -> ListingIndex {
    stats.iter().map(|s| ((s.item_id, s.hq), *s)).collect()
}

/// A cheap reactive handle; the payloads are cloned only by Arc. One slot
/// per server window, indexed by `Window::index()`. The seven-day body keeps
/// native cadence consumers compatible; other windows load when a visible or
/// hidden query column, or a registered price input, needs them.
#[derive(Clone, Copy)]
pub struct MarketData {
    pub scope: Signal<String>,
    pub window: MarketWindow,
    stats: [RwSignal<ScopedStats>; Window::ALL.len()],
    wanted: [RwSignal<bool>; Window::ALL.len()],
    /// The alive set is window-independent: one slot, one gate.
    listings: RwSignal<ScopedListings>,
    listings_wanted: RwSignal<bool>,
}

impl MarketData {
    /// Hand the loader a body the page already fetched for `scope_name`,
    /// so a shared or pinned column reads it instead of requesting the
    /// same window again. Only meaningful for a window named in
    /// `provided`; a window the loader owns is overwritten on its next run.
    pub fn supply(self, window: Window, scope_name: String, stats: Arc<StatsIndex>, failed: bool) {
        self.stats[window.index()].set(Some((scope_name, stats, failed)));
    }

    /// The current-listing body for the present scope, once it has landed.
    /// A failed request is `Some` with `failed` set and an empty index.
    pub fn listings(self) -> Option<ListingSlot> {
        let scope = self.scope.get();
        self.listings
            .with(|v| v.as_ref().filter(|slot| slot.scope == scope).cloned())
    }

    /// Ask for the current-listing body. Idempotent; never un-wants.
    fn want_listings(self) {
        if !self.listings_wanted.get_untracked() {
            self.listings_wanted.set(true);
        }
    }

    pub fn stats(self, window: Window) -> Option<Arc<StatsIndex>> {
        let scope = self.scope.get();
        self.stats[window.index()].with(|v| {
            v.as_ref()
                .filter(|(name, _, _)| name == &scope)
                .map(|(_, stats, _)| stats.clone())
        })
    }

    pub fn stats_failed(self, window: Window) -> bool {
        let scope = self.scope.get();
        self.stats[window.index()].with(|v| {
            v.as_ref()
                .is_some_and(|(name, _, failed)| name == &scope && *failed)
        })
    }

    /// Statistics for sale-based prices and follow-window columns.
    pub fn selected_stats(self) -> Option<Arc<StatsIndex>> {
        self.stats(self.window.selected.get())
    }

    pub fn stats7(self) -> Option<Arc<StatsIndex>> {
        self.stats(Window::D7)
    }

    /// Register a price input once at setup. Its sale basis follows the page window;
    /// concurrent columns/inputs share the same request slot.
    pub fn require_price_basis(self, basis: Signal<PriceSignal>) {
        Effect::new(move |_| {
            if basis.get().sale_stat().is_some() {
                self.want(self.window.selected.get());
            }
        });
    }

    /// Ask for a window's body. Idempotent; never un-wants. `MarketGrid`
    /// calls this for its shared columns; a page calls it for a native
    /// column that reads a window the page does not fetch itself.
    pub fn want(self, window: Window) {
        let flag = self.wanted[window.index()];
        if !flag.get_untracked() {
            flag.set(true);
        }
    }

    /// Subscribe the caller to every slot without copying a payload.
    fn track_all(self) {
        for slot in self.stats {
            slot.with(|_| ());
        }
        self.listings.with(|_| ());
    }
}

/// Both SSR and the initial hydrated render use listing fallbacks. Register
/// each price input with `require_price_basis` and resolve it against
/// `selected_stats`; pass `window` to the page control and price controls.
/// `MarketGrid` registers visible and hidden query requirements itself.
/// Failed requests settle with an explicit failure flag and an empty index,
/// preserving price fallbacks while grid queries remain unavailable.
pub fn use_market_data(scope: Signal<String>) -> MarketData {
    use_market_data_with_window(
        scope,
        MarketWindow::new(Window::D7, &Window::ALL),
        Some(Window::D7),
    )
}

/// Load statistics only when a visible column or active query requests them.
pub fn use_market_data_on_demand(scope: Signal<String>) -> MarketData {
    use_market_data_with_window(scope, MarketWindow::new(Window::D7, &Window::ALL), None)
}

/// A page-owned window with optional prefetch, such as Trends' 30-day default.
pub fn use_market_data_with_window(
    scope: Signal<String>,
    window: MarketWindow,
    prefetch: Option<Window>,
) -> MarketData {
    use_market_data_configured(scope, window, prefetch, Signal::derive(Vec::new))
}

/// Share windows fetched by the page's SSR gate; remaining windows load on demand.
pub fn use_market_data_with(
    scope: Signal<String>,
    window: MarketWindow,
    provided: Signal<Vec<Window>>,
) -> MarketData {
    use_market_data_configured(scope, window, Some(Window::D7), provided)
}

fn use_market_data_configured(
    scope: Signal<String>,
    window: MarketWindow,
    prefetch: Option<Window>,
    provided: Signal<Vec<Window>>,
) -> MarketData {
    // `RwSignal` is `Copy`: a `[RwSignal::new(None); 4]` literal would be one
    // signal four times over.
    let market = MarketData {
        scope,
        window,
        stats: std::array::from_fn(|_| RwSignal::new(None)),
        wanted: std::array::from_fn(|i| RwSignal::new(prefetch.is_some_and(|w| w.index() == i))),
        listings: RwSignal::new(None),
        listings_wanted: RwSignal::new(false),
    };
    for window in Window::ALL {
        fetch_stats(
            scope,
            market.stats[window.index()],
            market.wanted[window.index()].into(),
            Signal::derive(move || provided.with(|p| p.contains(&window))),
            window.days(),
        );
    }
    fetch_listing_stats(scope, market.listings, market.listings_wanted.into());
    market
}

/// Same scope-change guard as `fetch_stats`. Unlike `sale_stats`, an empty
/// board is a successful `200 {"stats":[]}`: only a transport error sets
/// `failed`, so cells can tell "nothing alive" from "could not ask".
fn fetch_listing_stats(
    scope: Signal<String>,
    output: RwSignal<ScopedListings>,
    wanted: Signal<bool>,
) {
    let generation = StoredValue::new(0u64);
    Effect::new(move |_| {
        let name = scope.get();
        let wanted = wanted.get();
        generation.update_value(|n| *n = n.wrapping_add(1));
        let epoch = generation.get_value();
        output.set(None);
        if !wanted {
            return;
        }
        if name.is_empty() {
            output.set(Some(ListingSlot {
                scope: name,
                index: Arc::new(ListingIndex::new()),
                failed: true,
                fetched_unix: 0,
            }));
            return;
        }
        leptos::task::spawn_local(async move {
            let result = get_listing_stats(&name)
                .await
                .map(|body| listing_index(&body.stats));
            let failed = result.is_err();
            let index = result.unwrap_or_default();
            if scope.try_get_untracked().as_ref() != Some(&name)
                || generation.try_get_value() != Some(epoch)
            {
                return;
            }
            let _ = output.try_set(Some(ListingSlot {
                scope: name,
                index: Arc::new(index),
                failed,
                fetched_unix: chrono::Utc::now().timestamp(),
            }));
        });
    });
}

fn fetch_stats(
    scope: Signal<String>,
    output: RwSignal<ScopedStats>,
    wanted: Signal<bool>,
    provided: Signal<bool>,
    days: u16,
) {
    let generation = StoredValue::new(0u64);
    Effect::new(move |_| {
        let name = scope.get();
        let wanted = wanted.get();
        let provided = provided.get();
        generation.update_value(|n| *n = n.wrapping_add(1));
        let epoch = generation.get_value();
        // The page owns this window: its `supply` fills the slot, and a
        // scope change is hidden by the scope check every read applies.
        if provided {
            return;
        }
        // A body the page supplied for this very scope is the body this
        // request would fetch; keep it when the page stops supplying it.
        if wanted
            && output.with_untracked(|v| {
                v.as_ref()
                    .is_some_and(|(held, _, failed)| held == &name && !failed)
            })
        {
            return;
        }
        output.set(None);
        if !wanted {
            return;
        }
        if name.is_empty() {
            output.set(Some((name, Arc::new(StatsIndex::new()), true)));
            return;
        }
        leptos::task::spawn_local(async move {
            let result = get_sale_stats(&name, days)
                .await
                .map(|body| stats_index(&body));
            let failed = result.is_err();
            let index = result.unwrap_or_default();
            if scope.try_get_untracked().as_ref() != Some(&name)
                || generation.try_get_value() != Some(epoch)
            {
                return;
            }
            let _ = output.try_set(Some((name, Arc::new(index), failed)));
        });
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPrice {
    pub price: i32,
    pub hq: bool,
    /// The actual listing's world. Zero means the statistic has no listing.
    pub world_id: i32,
    pub fallback: bool,
}

/// Resolve one exact quality, or the cheaper quality when `hq` is None.
/// Statistics never invent a listing location and absent/zero statistics
/// retain the existing listing rather than pricing an input at zero.
pub fn resolve_price(
    listings: &CheapestListingsMap,
    stats: Option<&StatsIndex>,
    item_id: i32,
    hq: Option<bool>,
    basis: PriceSignal,
) -> Option<ResolvedPrice> {
    let quality = |hq| {
        let listing = listings
            .map
            .get(&CheapestListingMapKey { item_id, hq })
            .filter(|v| v.price > 0);
        let price = basis
            .sale_stat()
            .and_then(|stat| stat_only(stats?, item_id, hq, stat));
        Some(ResolvedPrice {
            price: price.or_else(|| listing.map(|v| v.price))?,
            hq,
            world_id: listing.map_or(0, |v| v.world_id),
            fallback: basis != PriceSignal::ListingMin && price.is_none(),
        })
    };
    match hq {
        Some(hq) => quality(hq),
        None => match (quality(false), quality(true)) {
            (Some(nq), Some(hq)) => Some(if hq.price < nq.price { hq } else { nq }),
            (nq, hq) => nq.or(hq),
        },
    }
}

#[component]
pub fn MarketPriceControls(
    window: MarketWindow,
    #[prop(into)] basis: Signal<PriceSignal>,
    on_change: Callback<PriceSignal>,
    #[prop(into)] label: String,
    #[prop(optional, into)] listing_label: String,
    #[prop(default = true)] show_fallback_note: bool,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let listing_label = if listing_label.is_empty() {
        t_string!(i18n, market_listing_basis).to_string()
    } else {
        listing_label
    };
    let options = move || {
        [
            (PriceSignal::ListingMin, listing_label.clone()),
            (
                PriceSignal::SaleMin,
                stat_label(StatKind::Min, window.selected.get()),
            ),
            (
                PriceSignal::SaleMedian,
                stat_label(StatKind::Median, window.selected.get()),
            ),
            (
                PriceSignal::SaleAvg,
                stat_label(StatKind::Average, window.selected.get()),
            ),
        ]
    };
    view! {
        <div class="flex flex-col gap-1">
            <label class="filter-chip">
                <span>{label}</span>
                <select class="filter-chip-value" prop:value=move || basis.get().to_string()
                    on:change=move |ev| {
                        if let Ok(value) = event_target_value(&ev).parse() { on_change.run(value); }
                    }>
                    {move || options().into_iter().map(|(value, label)| view! {
                        <option value=value.to_string() selected=move || basis.get() == value>{label}</option>
                    }).collect_view()}
                </select>
            </label>
            {show_fallback_note.then(|| view! {
                <span class="text-xs text-[color:var(--color-text-muted)]">{t_string!(i18n, market_fallback_note)}</span>
            })}
        </div>
    }
}

/// The market-facing part of a row. A project/turn-in tool can name the
/// ingredient whose market columns it presents, rather than implying its
/// project itself trades. Location always comes from an actual listing.
#[derive(Clone, Debug, PartialEq)]
pub struct MarketSubject {
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    pub label: String,
    pub listing_price: Option<i32>,
}

impl MarketSubject {
    pub fn new(item_id: i32, hq: bool, world_id: i32) -> Self {
        Self {
            item_id,
            hq,
            world_id,
            label: String::new(),
            listing_price: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarketMetric {
    Subject,
    Scope,
    Quality,
    World,
    Datacenter,
    Listing,
    /// One statistic of one window; ids and labels come from `STAT_COLUMNS`.
    Stat(StatKind, Window),
    Follow(StatKind),
    LastSold,
    Confidence,
    TrendWorld,
    Trend7,
    Drift7,
    /// The board as it stands now; ids and labels come from `LISTING_COLUMNS`.
    Listings(ListingKind),
}

impl MarketMetric {
    fn id(self) -> &'static str {
        match self {
            Self::Listings(kind) => listing_id(kind),
            Self::Subject => "market-subject",
            Self::Scope => "market-scope",
            Self::Quality => "market-quality",
            Self::World => "market-world",
            Self::Datacenter => "market-datacenter",
            Self::Listing => "market-listing",
            Self::Stat(kind, window) => stat_column(kind, window).id,
            Self::Follow(kind) => follow_id(kind),
            Self::LastSold => "market-last-sold",
            Self::Confidence => "market-confidence",
            Self::TrendWorld => "market-trend-world",
            Self::Trend7 => "market-trend-7",
            Self::Drift7 => "market-drift-7",
        }
    }

    fn text(self) -> bool {
        matches!(
            self,
            Self::Subject
                | Self::Scope
                | Self::Quality
                | Self::World
                | Self::Datacenter
                | Self::Confidence
                | Self::LastSold
                | Self::TrendWorld
        )
    }

    fn partial(self) -> bool {
        matches!(self, Self::Trend7 | Self::Drift7)
    }

    /// The bulk body this metric reads. Last-sold and confidence are
    /// seven-day facts, as before.
    fn window(self, selected: Window) -> Option<Window> {
        match self {
            Self::Stat(_, window) => Some(window),
            Self::Follow(_) => Some(selected),
            Self::LastSold | Self::Confidence => Some(Window::D7),
            _ => None,
        }
    }
}

const LEADING_METRICS: [MarketMetric; 6] = [
    MarketMetric::Subject,
    MarketMetric::Scope,
    MarketMetric::Quality,
    MarketMetric::World,
    MarketMetric::Datacenter,
    MarketMetric::Listing,
];

const TRAILING_METRICS: [MarketMetric; 5] = [
    MarketMetric::LastSold,
    MarketMetric::Confidence,
    MarketMetric::TrendWorld,
    MarketMetric::Trend7,
    MarketMetric::Drift7,
];

/// Every shared column in default (appended) order: identity, then the
/// window × statistic table, then the seven-day text and trend columns.
fn market_metrics() -> impl Iterator<Item = MarketMetric> {
    LEADING_METRICS
        .into_iter()
        .chain(
            FOLLOW_COLUMNS
                .iter()
                .map(|(kind, _)| MarketMetric::Follow(*kind)),
        )
        .chain(
            STAT_COLUMNS
                .iter()
                .map(|c| MarketMetric::Stat(c.kind, c.window)),
        )
        .chain(TRAILING_METRICS)
        .chain(
            LISTING_COLUMNS
                .iter()
                .map(|(kind, _)| MarketMetric::Listings(*kind)),
        )
}

fn metric_by_id(id: &str) -> Option<MarketMetric> {
    market_metrics().find(|m| m.id() == id)
}

/// Header hover text; only the age columns carry one.
fn metric_title(metric: MarketMetric) -> Option<String> {
    match metric {
        MarketMetric::Listings(kind) => listing_title(kind),
        _ => None,
    }
}

fn metric_label(metric: MarketMetric, selected: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match metric {
        MarketMetric::Listings(kind) => return listing_label(kind),
        MarketMetric::Subject => t_string!(i18n, market_subject),
        MarketMetric::Scope => t_string!(i18n, market_scope),
        MarketMetric::Quality => t_string!(i18n, market_quality),
        MarketMetric::World => t_string!(i18n, market_world),
        MarketMetric::Datacenter => t_string!(i18n, market_datacenter),
        MarketMetric::Listing => t_string!(i18n, market_listing),
        MarketMetric::Stat(kind, window) => return stat_label(kind, window),
        MarketMetric::Follow(kind) => return stat_label(kind, selected),
        MarketMetric::LastSold => t_string!(i18n, market_last_sold),
        MarketMetric::Confidence => t_string!(i18n, market_confidence),
        MarketMetric::TrendWorld => t_string!(i18n, market_trend_world),
        MarketMetric::Trend7 => t_string!(i18n, market_trend_7),
        MarketMetric::Drift7 => t_string!(i18n, market_drift_7),
    }
    .to_string()
}

fn number(value: Option<f64>) -> GridValue {
    value
        .filter(|v| v.is_finite())
        .map_or(GridValue::Missing, GridValue::Number)
}

fn stats_value(metric: MarketMetric, stats: Option<ItemSaleStats>) -> GridValue {
    let Some(s) = stats else {
        return GridValue::Missing;
    };
    let positive = |v: i32| (v > 0).then_some(f64::from(v));
    match metric {
        MarketMetric::Confidence => match s.confidence {
            ConfidenceBand::Unknown => GridValue::Missing,
            band => GridValue::Text(format!("{band:?}")),
        },
        MarketMetric::LastSold => chrono::DateTime::from_timestamp(s.last_sold_unix, 0)
            .filter(|_| s.last_sold_unix > 0)
            .map_or(GridValue::Missing, |time| {
                GridValue::Text(time.format("%Y-%m-%d %H:%M UTC").to_string())
            }),
        MarketMetric::Stat(kind, _) | MarketMetric::Follow(kind) => number(match kind {
            StatKind::Min => positive(s.min_price),
            StatKind::Median => positive(s.median_price),
            StatKind::Average => positive(s.avg_price),
            StatKind::SalesPerDay => Some(f64::from(s.sales_per_day)),
            StatKind::Cadence => (s.sales_per_day > 0.0).then(|| 24.0 / f64::from(s.sales_per_day)),
            StatKind::Units => Some(s.units_sold as f64),
            StatKind::Sales => Some(s.num_sold as f64),
            StatKind::Vwap => positive(s.vwap),
            // Zero is an old server (serde default), not a free market.
            StatKind::GilVolume => (s.gil_volume > 0).then_some(s.gil_volume as f64),
        }),
        _ => GridValue::Missing,
    }
}

/// A row absent from a successful body has no alive listings Ultros knows
/// of. Ages count from the retainer's last review; a zero review time is an
/// unknown timestamp, not a listing from 1970.
fn listing_value(
    kind: ListingKind,
    stats: Option<&ItemListingStats>,
    fetched_unix: i64,
) -> GridValue {
    let Some(s) = stats else {
        return GridValue::Missing;
    };
    let alive = s.alive_count > 0;
    match kind {
        ListingKind::Alive => GridValue::Number(f64::from(s.alive_count)),
        ListingKind::AliveUnits => GridValue::Number(s.alive_units as f64),
        ListingKind::Sellers => GridValue::Number(f64::from(s.distinct_retainers)),
        ListingKind::MedianAge => number(alive.then_some(f64::from(s.median_age_secs))),
        ListingKind::OldestAge => number(
            (alive && s.oldest_reviewed_unix > 0)
                .then(|| (fetched_unix - s.oldest_reviewed_unix).max(0) as f64),
        ),
    }
}

type WorldNames = Arc<HashMap<i32, (String, String)>>;
type MarketSparkKey = (i32, bool, i32);
type MarketSparkStore = Enrichment<MarketSparkKey, MarketSpark>;

#[derive(Clone, Debug, PartialEq)]
enum MarketSpark {
    Ready(SparkValue),
    Unavailable,
}

impl Absorb for MarketSpark {
    fn absorb(&mut self, newer: Self) {
        *self = newer;
    }
}

/// Failures settle every requested key without asserting that it has no
/// history. An empty successful response settles to Missing via the store.
fn spark_response(
    requested: &[(i32, bool)],
    world_id: i32,
    response: Option<SparklinesResponse>,
) -> Vec<(MarketSparkKey, MarketSpark)> {
    match response {
        Some(body) => body
            .series
            .into_iter()
            .map(|series| {
                (
                    (series.item_id, series.hq, series.world_id),
                    MarketSpark::Ready(SparkValue {
                        delta_pct: crate::analysis::first_to_last_pct(
                            series.first_price,
                            series.last_price,
                        ),
                        points: series.points,
                    }),
                )
            })
            .collect(),
        None => requested
            .iter()
            .map(|&(item_id, hq)| ((item_id, hq, world_id), MarketSpark::Unavailable))
            .collect(),
    }
}

fn spark_metric_value(store: &MarketSparkStore, key: &MarketSparkKey) -> GridValue {
    match store.get(key) {
        Some(MarketSpark::Ready(s)) => number(s.delta_pct.map(f64::from)),
        Some(MarketSpark::Unavailable) => GridValue::Unavailable,
        None if store.is_settled(key) => GridValue::Missing,
        None => GridValue::Pending,
    }
}

fn spark_key(subject: &MarketSubject, scope_world: Option<i32>) -> (i32, bool, i32) {
    (
        subject.item_id,
        subject.hq,
        scope_world.unwrap_or(subject.world_id),
    )
}

fn market_value(
    metric: MarketMetric,
    subject: &MarketSubject,
    market: MarketData,
    sparks: RwSignal<MarketSparkStore>,
    scope_world: Memo<Option<i32>>,
    worlds: &WorldNames,
) -> GridValue {
    let text = |value: Option<String>| {
        value
            .filter(|v| !v.is_empty())
            .map_or(GridValue::Missing, GridValue::Text)
    };
    match metric {
        MarketMetric::Subject => text(Some(subject.label.clone())),
        MarketMetric::Scope => text(Some(market.scope.get())),
        MarketMetric::Quality => GridValue::Text(if subject.hq { "HQ" } else { "NQ" }.into()),
        MarketMetric::World => text(worlds.get(&subject.world_id).map(|v| v.0.clone())),
        MarketMetric::Datacenter => text(worlds.get(&subject.world_id).map(|v| v.1.clone())),
        MarketMetric::TrendWorld => text(
            worlds
                .get(&spark_key(subject, scope_world.get()).2)
                .map(|v| v.0.clone()),
        ),
        MarketMetric::Listing => number(subject.listing_price.filter(|v| *v > 0).map(f64::from)),
        MarketMetric::Trend7 | MarketMetric::Drift7 => sparks.with(|store| {
            let key = spark_key(subject, scope_world.get());
            spark_metric_value(store, &key)
        }),
        MarketMetric::Listings(kind) => match market.listings() {
            None => GridValue::Pending,
            Some(slot) if slot.failed => GridValue::Unavailable,
            Some(slot) => listing_value(
                kind,
                slot.index.get(&(subject.item_id, subject.hq)),
                slot.fetched_unix,
            ),
        },
        _ => {
            let Some(window) = metric.window(market.window.selected.get()) else {
                return GridValue::Missing;
            };
            if market.stats_failed(window) {
                return GridValue::Unavailable;
            }
            match market.stats(window) {
                None => GridValue::Pending,
                Some(stats) => {
                    let value =
                        stats_value(metric, stats.get(&(subject.item_id, subject.hq)).copied());
                    if matches!(metric, MarketMetric::Confidence)
                        && matches!(value, GridValue::Text(_))
                    {
                        GridValue::Text(display_value(metric, value))
                    } else {
                        value
                    }
                }
            }
        }
    }
}

fn display_value(metric: MarketMetric, value: GridValue) -> String {
    match value {
        GridValue::Number(n) if matches!(metric, MarketMetric::Trend7 | MarketMetric::Drift7) => {
            format!("{n:+.1}%")
        }
        GridValue::Number(n) if matches!(metric, MarketMetric::Listings(kind) if kind.is_age()) => {
            format_duration_short(n.max(0.0).round() as u64)
        }
        GridValue::Number(n)
            if matches!(
                metric,
                MarketMetric::Stat(StatKind::SalesPerDay | StatKind::Cadence, _)
                    | MarketMetric::Follow(StatKind::SalesPerDay | StatKind::Cadence)
            ) =>
        {
            format!("{n:.2}")
        }
        GridValue::Number(n) => (n.round() as i64).separate_with_commas(),
        GridValue::Text(s) if matches!(metric, MarketMetric::Confidence) => {
            let i18n = crate::i18n_fallback::use_i18n_or_default();
            match s.as_str() {
                "High" => t_string!(i18n, confidence_band_high).to_string(),
                "Medium" => t_string!(i18n, confidence_band_medium).to_string(),
                "Low" => t_string!(i18n, confidence_band_low).to_string(),
                "Unusable" => t_string!(i18n, confidence_band_unusable).to_string(),
                _ => s,
            }
        }
        GridValue::Text(s) => s,
        GridValue::Set(s) => s.join(", "),
        GridValue::Pending => {
            let i18n = crate::i18n_fallback::use_i18n_or_default();
            t_string!(i18n, market_loading).to_string()
        }
        GridValue::Missing | GridValue::Unavailable => "—".into(),
    }
}

/// Adapts custom analyzer rows to the same grid, preserving native cell
/// renderers and all layout/view interactions. Custom metrics take the
/// same GridMetric path as these common market metrics.
#[component]
pub fn MarketGrid<T, K, KF, H, F, M>(
    #[prop(into)] each: Signal<Vec<T>>,
    #[prop(into)] columns: Signal<Vec<GridColumn>>,
    key: KF,
    header: H,
    view: F,
    measure: M,
    #[prop(default = Signal::derive(|| 0), into)] measure_version: Signal<u64>,
    market: MarketData,
    subject: Arc<dyn Fn(&T) -> MarketSubject + Send + Sync>,
    #[prop(optional)] metrics: Vec<GridMetric<T>>,
    #[prop(optional)] on_rows: Option<Callback<Vec<T>>>,
    #[prop(default = true)] show_saved_views: bool,
    #[prop(default = 40.0)] row_height: f64,
    #[prop(optional)] visible_range: Option<RwSignal<(usize, usize)>>,
    #[prop(into)] id: String,
    #[prop(into)] label: String,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
    KF: Fn(&T) -> K + Send + Sync + 'static,
    H: Fn(&'static str) -> AnyView + Send + Sync + 'static,
    F: Fn(T, &'static str) -> AnyView + Send + Sync + 'static,
    M: Fn(&T, &'static str) -> (String, f64) + Send + Sync + 'static,
{
    let worlds: WorldNames = Arc::new(
        use_context::<LocalWorldData>()
            .and_then(|v| v.0.ok())
            .map(|helper| {
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
                    .collect()
            })
            .unwrap_or_default(),
    );
    let range = visible_range.unwrap_or_else(|| RwSignal::new((0, 0)));
    let filtered = RwSignal::new(Vec::<T>::new());
    let sparks = RwSignal::new(MarketSparkStore::default());
    // Providers update independently of row identities and query results.
    // Track their revisions without copying payloads or measuring every row
    // in a reactive effect; the grid performs one debounced, chunked pass.
    let sizing_version = Memo::new(move |previous: Option<&u64>| {
        measure_version.get();
        market.scope.with(|_| ());
        market.window.selected.get();
        market.track_all();
        sparks.with(|_| ());
        previous.copied().unwrap_or_default().wrapping_add(1)
    });
    let worlds_scope = worlds.clone();
    let scope_world = Memo::new(move |_| {
        let scope = market.scope.get();
        worlds_scope
            .iter()
            .find(|(_, (name, _))| name.eq_ignore_ascii_case(&scope))
            .map(|(id, _)| *id)
    });
    let query = use_location_or_default().query;
    let all_columns = Memo::new(move |_| {
        let mut result = columns.get();
        for metric in market_metrics() {
            let position = result.iter().position(|col| col.id == metric.id());
            let column = if let Some(position) = position {
                &mut result[position]
            } else {
                result.push(GridColumn::new(
                    metric.id(),
                    String::new(),
                    160.0,
                    true,
                    false,
                ));
                result.last_mut().unwrap()
            };
            // Pages own placement, initial width and default visibility, while
            // shared columns retain the same labels and picker groups everywhere.
            column.label = metric_label(metric, market.window.selected.get());
            column.picker_group = match metric {
                MarketMetric::Follow(_) => Some(market_picker_group(None)),
                MarketMetric::Stat(_, window) => Some(market_picker_group(Some(window))),
                MarketMetric::Listings(_) => Some(market_picker_group_listings()),
                _ => None,
            };
        }
        result
    });
    let filter_registry =
        use_context::<crate::components::virtual_grid::registry::FilterRegistry>();
    let needs = Memo::new(move |_| {
        let mut wanted = query.with(|q| {
            filter_registry
                .map(|r| r.filters(q).into_keys().collect())
                .unwrap_or_else(|| active_metric_columns(q.get("gf").as_deref()))
        });
        query.with(|q| {
            if let Some(cols) = q.get("cols") {
                wanted.extend(cols.split(',').map(str::to_owned));
            }
            let sort = q.get("sort");
            let sort = match filter_registry {
                Some(registry) => registry.sort_column(sort.as_deref()),
                None => {
                    crate::components::virtual_grid::registry::resolve_sort(sort.as_deref(), &[])
                }
            };
            if let Some(sort) = sort {
                wanted.insert(sort);
            }
        });
        for col in all_columns.get().iter().filter(|c| c.visible) {
            wanted.insert(col.id.to_string());
        }
        wanted
    });
    Effect::new(move |_| {
        needs.with(|n| {
            for window in required_windows(n, market.window.selected.get(), false) {
                market.want(window);
            }
            if listings_wanted(n) {
                market.want_listings();
            }
        });
    });
    let subject_rows = subject.clone();
    let spark_rows = Signal::derive(move || {
        if !needs.with(|n| n.contains("market-trend-7") || n.contains("market-drift-7")) {
            return Vec::new();
        }
        filtered.with(|rows| {
            rows.iter()
                .map(|row| {
                    let s = subject_rows(row);
                    spark_key(&s, scope_world.get())
                })
                .collect()
        })
    });
    let worlds_fetch = worlds.clone();
    use_visible_enrichment(
        sparks,
        spark_rows,
        range.into(),
        market.scope,
        |key| *key,
        move |_scope, keys| {
            let worlds = worlds_fetch.clone();
            async move {
                let mut by_world = HashMap::<i32, Vec<(i32, bool)>>::new();
                for (item_id, hq, world_id) in keys {
                    if worlds.contains_key(&world_id) {
                        by_world.entry(world_id).or_default().push((item_id, hq));
                    }
                }
                let requests = by_world.into_iter().map(|(world_id, items)| {
                    let world = worlds[&world_id].0.clone();
                    async move {
                        let requested = items.clone();
                        let response = post_sparklines(
                            &world,
                            SparklinesRequest {
                                items,
                                hours: Some(168),
                            },
                        )
                        .await
                        .ok();
                        spark_response(&requested, world_id, response)
                    }
                });
                futures::future::join_all(requests)
                    .await
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
            }
        },
        EnrichmentConfig {
            prefetch_margin: PREFETCH_MARGIN,
            debounce_ms: DEBOUNCE_MS,
            max_keys_per_request: 200,
        },
    );
    let mut all_metrics = metrics;
    for metric in market_metrics() {
        if all_metrics.iter().any(|m| m.id == metric.id()) {
            continue;
        }
        let subject = subject.clone();
        let worlds = worlds.clone();
        let value = move |row: &T| {
            market_value(metric, &subject(row), market, sparks, scope_world, &worlds)
        };
        let def = if metric.text() {
            GridMetric::text(metric.id(), value)
        } else {
            GridMetric::number(metric.id(), value)
        };
        all_metrics.push(if metric.partial() { def.partial() } else { def });
    }
    let sortable = StoredValue::new(
        all_metrics
            .iter()
            .filter(|m| !m.partial)
            .map(|m| m.id)
            .collect::<Vec<_>>(),
    );
    let native_header = StoredValue::new(header);
    let native_view = StoredValue::new(view);
    let native_measure = StoredValue::new(measure);
    let subject_measure = subject.clone();
    let worlds_measure = worlds.clone();
    let handle_rows = Callback::new(move |rows: Vec<T>| {
        if !filtered.with_untracked(|current| current == &rows) {
            filtered.set(rows.clone());
        }
        if let Some(callback) = on_rows {
            callback.run(rows);
        }
    });
    view! {
        <QueryGrid each columns=all_columns key row_height visible_range=range id label metrics=all_metrics on_rows=handle_rows show_saved_views measure_version=sizing_version
            header=move |id| match metric_by_id(id) {
                Some(metric) if !metric.partial() && sortable.with_value(|ids| ids.contains(&id)) => view! {
                    <span title=metric_title(metric)>
                        <MetricSortHeader column=id label=Signal::derive(move || metric_label(metric, market.window.selected.get())) />
                    </span>
                }.into_any(),
                Some(metric) => (move || metric_label(metric, market.window.selected.get())).into_any(),
                None => native_header.with_value(|header| header(id)),
            }
            view=move |row: T, id| {
                let Some(metric) = metric_by_id(id) else {
                    return native_view.with_value(|view| view(row, id));
                };
                let subject = subject(&row);
                let worlds = worlds.clone();
                let title_subject = subject.clone();
                let title_worlds = worlds.clone();
                view! { <div class="px-3 flex h-full items-center tabular-nums" title=move || {
                    if metric.partial() {
                        let world = title_worlds.get(&spark_key(&title_subject, scope_world.get()).2)
                            .map(|v| v.0.clone()).unwrap_or_else(|| "—".into());
                        format!("{}: {world}", metric_label(MarketMetric::TrendWorld, market.window.selected.get()))
                    } else { String::new() }
                }>{move || {
                    if matches!(metric, MarketMetric::Trend7)
                        && let Some(MarketSpark::Ready(value)) = sparks.with(|s| s.get(&spark_key(&subject, scope_world.get())).cloned()) {
                        return view! { <Sparkline points=value.points pct_change=value.delta_pct.unwrap_or_default() width=120 /> }.into_any();
                    }
                    display_value(metric, market_value(metric, &subject, market, sparks, scope_world, &worlds)).into_any()
                }}</div> }.into_any()
            }
            measure=move |row: &T, id| match metric_by_id(id) {
                Some(metric) => {
                    let subject = subject_measure(row);
                    if matches!(metric, MarketMetric::Trend7)
                        && sparks.with(|store| matches!(store.get(&spark_key(&subject, scope_world.get())), Some(MarketSpark::Ready(_)))) {
                        // This cell renders a 120px SVG, rather than its numeric delta.
                        (String::new(), 144.0)
                    } else {
                        (display_value(metric, market_value(metric, &subject, market, sparks, scope_world, &worlds_measure)), 24.0)
                    }
                },
                None => native_measure.with_value(|measure| measure(row, id)),
            }
        />
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn on_demand_market_data_wants_no_window_until_asked() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let scope = RwSignal::new("Gilgamesh".to_owned());
            let lazy = use_market_data_on_demand(scope.into());
            assert!(lazy.wanted.iter().all(|w| !w.get_untracked()));
            let eager = use_market_data(scope.into());
            assert!(eager.wanted[Window::D7.index()].get_untracked());
            lazy.want(Window::D30);
            assert!(lazy.wanted[Window::D30.index()].get_untracked());
            assert!(!lazy.wanted[Window::D7.index()].get_untracked());
        });
    }
    use super::*;
    use ultros_api_types::cheapest_listings::CheapestListingData;

    #[test]
    fn hidden_legacy_filters_request_their_window_before_any_edit() {
        use crate::components::virtual_grid::{
            metrics::FilterOp,
            registry::{FilterAlias, resolve_filters},
        };
        let mut query = leptos_router::params::ParamsMap::new();
        query.insert("legacy-median", "100".into());
        let aliases = [FilterAlias::integer(
            "legacy-median",
            "market-sale-median-30",
            FilterOp::Gte,
        )];
        let needs = resolve_filters(&query, &aliases).into_keys().collect();
        assert_eq!(
            required_windows(&needs, Window::D7, false),
            vec![Window::D30]
        );
    }

    #[test]
    fn selected_prices_and_columns_follow_window_without_reusing_other_scope_data() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let scope = RwSignal::new("Gilgamesh".to_owned());
            let selected = RwSignal::new(Window::D7);
            let mut market = use_market_data(scope.into());
            market.window.selected = Memo::new(move |_| selected.get());
            let make_stats = |price| {
                Arc::new(
                    [(
                        (42, false),
                        ItemSaleStats {
                            item_id: 42,
                            min_price: price - 10,
                            median_price: price,
                            avg_price: price + 10,
                            ..Default::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                )
            };
            market.stats[Window::D7.index()].set(Some(("Gilgamesh".into(), make_stats(70), false)));
            market.stats[Window::D30.index()].set(Some((
                "Gilgamesh".into(),
                make_stats(300),
                false,
            )));
            let price = |basis| {
                resolve_price(
                    &listing(),
                    market.selected_stats().as_deref(),
                    42,
                    Some(false),
                    basis,
                )
                .unwrap()
            };
            assert_eq!(price(PriceSignal::SaleMedian).price, 70);
            selected.set(Window::D30);
            assert_eq!(price(PriceSignal::SaleMin).price, 290);
            assert_eq!(price(PriceSignal::SaleMedian).price, 300);
            assert_eq!(price(PriceSignal::SaleAvg).price, 310);
            assert_eq!(price(PriceSignal::ListingMin).price, 100);
            assert_eq!(market.stats7().unwrap()[&(42, false)].median_price, 70);
            assert_eq!(
                metric_by_id("market-sale-median")
                    .unwrap()
                    .window(selected.get()),
                Some(Window::D30)
            );
            assert_eq!(
                metric_by_id("market-sale-median-7")
                    .unwrap()
                    .window(selected.get()),
                Some(Window::D7)
            );

            let sparks = RwSignal::new(MarketSparkStore::default());
            let scope_world = Memo::new(|_| None);
            let subject = MarketSubject::new(42, false, 7);
            let worlds = Arc::new(HashMap::new());
            let value = || {
                market_value(
                    MarketMetric::Follow(StatKind::Median),
                    &subject,
                    market,
                    sparks,
                    scope_world,
                    &worlds,
                )
            };
            assert_eq!(value(), GridValue::Number(300.0));
            scope.set("Cactuar".into());
            assert!(market.selected_stats().is_none());
            assert_eq!(value(), GridValue::Pending);
            // An old scope completing late cannot supply either prices or cells.
            market.stats[Window::D30.index()].set(Some((
                "Gilgamesh".into(),
                make_stats(999),
                false,
            )));
            assert_eq!(value(), GridValue::Pending);
            assert!(price(PriceSignal::SaleMedian).fallback);
            market.stats[Window::D30.index()].set(Some((
                "Cactuar".into(),
                Arc::new(StatsIndex::new()),
                false,
            )));
            assert_eq!(value(), GridValue::Missing);
            assert_eq!(price(PriceSignal::SaleMedian).price, 100);
            market.stats[Window::D30.index()].set(Some((
                "Cactuar".into(),
                Arc::new(StatsIndex::new()),
                true,
            )));
            assert_eq!(value(), GridValue::Unavailable);
            assert!(market.stats_failed(Window::D30));
        });
    }

    #[test]
    fn listing_columns_distinguish_pending_failed_empty_and_absent_rows() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let scope = RwSignal::new("Gilgamesh".to_owned());
            let market = use_market_data(scope.into());
            let sparks = RwSignal::new(MarketSparkStore::default());
            let scope_world = Memo::new(|_| None);
            let worlds = Arc::new(HashMap::new());
            let subject = MarketSubject::new(42, true, 7);
            let value = |kind| {
                market_value(
                    MarketMetric::Listings(kind),
                    &subject,
                    market,
                    sparks,
                    scope_world,
                    &worlds,
                )
            };
            // Nothing wanted yet: the slot is empty and cells wait.
            assert!(!market.listings_wanted.get_untracked());
            assert_eq!(value(ListingKind::Alive), GridValue::Pending);
            let slot = |scope: &str, rows: Vec<ItemListingStats>, failed| {
                Some(ListingSlot {
                    scope: scope.into(),
                    index: Arc::new(listing_index(&rows)),
                    failed,
                    fetched_unix: 1_000_000,
                })
            };
            // A body from another scope never satisfies this scope's cells.
            market
                .listings
                .set(slot("Cactuar", vec![row(42, true, 3)], false));
            assert_eq!(value(ListingKind::Alive), GridValue::Pending);
            market.listings.set(slot("Gilgamesh", Vec::new(), true));
            assert!(market.listings().is_some_and(|slot| slot.failed));
            assert_eq!(value(ListingKind::Alive), GridValue::Unavailable);
            assert_eq!(value(ListingKind::OldestAge), GridValue::Unavailable);
            // An empty successful body is an empty board, not a failure.
            market.listings.set(slot("Gilgamesh", Vec::new(), false));
            assert!(market.listings().is_some_and(|slot| !slot.failed));
            assert_eq!(value(ListingKind::Alive), GridValue::Missing);
            // Exact quality: an NQ row does not answer for the HQ subject.
            market
                .listings
                .set(slot("Gilgamesh", vec![row(42, false, 3)], false));
            assert_eq!(value(ListingKind::Sellers), GridValue::Missing);
            market
                .listings
                .set(slot("Gilgamesh", vec![row(42, true, 3)], false));
            assert_eq!(value(ListingKind::Alive), GridValue::Number(3.0));
            assert_eq!(value(ListingKind::AliveUnits), GridValue::Number(30.0));
            assert_eq!(value(ListingKind::Sellers), GridValue::Number(2.0));
            assert_eq!(value(ListingKind::MedianAge), GridValue::Number(3_600.0));
            assert_eq!(
                value(ListingKind::OldestAge),
                GridValue::Number(f64::from(1_000_000 - 913_600))
            );
            scope.set("Cactuar".into());
            assert_eq!(value(ListingKind::Alive), GridValue::Pending);
        });
    }

    fn row(item_id: i32, hq: bool, alive_count: u32) -> ItemListingStats {
        ItemListingStats {
            item_id,
            hq,
            alive_count,
            alive_units: u64::from(alive_count) * 10,
            distinct_retainers: 2,
            oldest_reviewed_unix: 913_600,
            median_age_secs: 3_600,
            floor_alive: 500,
            window: None,
        }
    }

    #[test]
    fn listing_ages_treat_unknown_timestamps_and_empty_boards_as_missing() {
        let now = 2_000_000;
        let zero = ItemListingStats {
            alive_count: 0,
            ..row(42, false, 0)
        };
        assert_eq!(
            listing_value(ListingKind::Alive, Some(&zero), now),
            GridValue::Number(0.0)
        );
        assert_eq!(
            listing_value(ListingKind::MedianAge, Some(&zero), now),
            GridValue::Missing
        );
        assert_eq!(
            listing_value(ListingKind::OldestAge, Some(&zero), now),
            GridValue::Missing
        );
        let unknown = ItemListingStats {
            oldest_reviewed_unix: 0,
            ..row(42, false, 2)
        };
        assert_eq!(
            listing_value(ListingKind::OldestAge, Some(&unknown), now),
            GridValue::Missing
        );
        assert_eq!(
            listing_value(ListingKind::MedianAge, Some(&unknown), now),
            GridValue::Number(3_600.0)
        );
        // A review time ahead of the fetch clock (skew) is an age of zero, not negative.
        let future = ItemListingStats {
            oldest_reviewed_unix: now + 60,
            ..row(42, false, 2)
        };
        assert_eq!(
            listing_value(ListingKind::OldestAge, Some(&future), now),
            GridValue::Number(0.0)
        );
        assert_eq!(
            listing_value(ListingKind::Alive, None, now),
            GridValue::Missing
        );
    }

    #[test]
    fn listing_ages_display_as_durations_and_counts_as_integers() {
        assert_eq!(
            display_value(
                MarketMetric::Listings(ListingKind::OldestAge),
                GridValue::Number(90_000.0)
            ),
            "1d 1h"
        );
        assert_eq!(
            display_value(
                MarketMetric::Listings(ListingKind::MedianAge),
                GridValue::Number(59.4)
            ),
            "59s"
        );
        assert_eq!(
            display_value(
                MarketMetric::Listings(ListingKind::AliveUnits),
                GridValue::Number(12345.0)
            ),
            "12,345"
        );
        assert_eq!(
            display_value(
                MarketMetric::Listings(ListingKind::Alive),
                GridValue::Unavailable
            ),
            "—"
        );
        for id in [
            "market-alive",
            "market-alive-units",
            "market-sellers",
            "market-listing-age",
            "market-oldest-listing",
        ] {
            let metric = metric_by_id(id).unwrap();
            assert!(matches!(metric, MarketMetric::Listings(_)), "{id}");
            assert!(!metric.text() && !metric.partial(), "{id} sorts globally");
            assert_eq!(metric.window(Window::D30), None, "{id} needs no window");
        }
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            assert!(metric_title(metric_by_id("market-alive").unwrap()).is_none());
            assert!(
                metric_title(metric_by_id("market-listing-age").unwrap())
                    .is_some_and(|t| t.contains("retainer last touched"))
            );
            assert_eq!(
                metric_label(metric_by_id("market-sellers").unwrap(), Window::D7),
                "Sellers"
            );
        });
    }

    #[test]
    fn failed_hourly_requests_remain_unknown_while_empty_success_is_missing() {
        let key = (42, true, 7);
        let mut store = MarketSparkStore::default();
        assert_eq!(spark_metric_value(&store, &key), GridValue::Pending);
        store.merge(&[key], spark_response(&[(42, true)], 7, None));
        assert!(store.is_settled(&key));
        assert_eq!(spark_metric_value(&store, &key), GridValue::Unavailable);

        let mut store = MarketSparkStore::default();
        store.merge(
            &[key],
            spark_response(
                &[(42, true)],
                7,
                Some(SparklinesResponse {
                    world_id: 7,
                    series: Vec::new(),
                }),
            ),
        );
        assert!(store.is_settled(&key));
        assert_eq!(spark_metric_value(&store, &key), GridValue::Missing);
    }

    #[test]
    fn hourly_history_uses_scope_world_or_real_listing_world() {
        let subject = MarketSubject::new(42, true, 7);
        assert_eq!(spark_key(&subject, Some(99)), (42, true, 99));
        assert_eq!(spark_key(&subject, None), (42, true, 7));
        let absent = MarketSubject::new(42, false, 0);
        assert_eq!(spark_key(&absent, None), (42, false, 0));
    }

    #[test]
    fn shared_column_ids_are_unique_and_include_every_window() {
        let ids: Vec<_> = market_metrics().map(|m| m.id()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len());
        for id in [
            "market-sale-median-7",
            "market-sale-median-30",
            "market-gil-1",
            "market-vwap-90",
            "market-trend-7",
        ] {
            assert!(unique.contains(id), "{id}");
            assert_eq!(metric_by_id(id).map(|m| m.id()), Some(id));
        }
        assert_eq!(metric_by_id("roi"), None);
    }

    #[test]
    fn rates_format_with_two_decimals_and_gil_with_commas() {
        assert_eq!(
            display_value(
                MarketMetric::Stat(StatKind::SalesPerDay, Window::D30),
                GridValue::Number(2.5)
            ),
            "2.50"
        );
        assert_eq!(
            display_value(
                MarketMetric::Stat(StatKind::GilVolume, Window::D7),
                GridValue::Number(1234567.0)
            ),
            "1,234,567"
        );
    }

    fn listing() -> CheapestListingsMap {
        CheapestListingsMap {
            map: [(
                CheapestListingMapKey {
                    item_id: 42,
                    hq: false,
                },
                CheapestListingData {
                    price: 100,
                    world_id: 7,
                },
            )]
            .into(),
        }
    }

    #[test]
    fn sale_price_preserves_quality_location_and_explicit_fallback() {
        let mut stats = StatsIndex::new();
        stats.insert(
            (42, true),
            ItemSaleStats {
                item_id: 42,
                hq: true,
                median_price: 80,
                ..Default::default()
            },
        );
        let nq = resolve_price(
            &listing(),
            Some(&stats),
            42,
            Some(false),
            PriceSignal::SaleMedian,
        )
        .unwrap();
        assert_eq!(
            (nq.price, nq.world_id, nq.hq, nq.fallback),
            (100, 7, false, true)
        );
        let any =
            resolve_price(&listing(), Some(&stats), 42, None, PriceSignal::SaleMedian).unwrap();
        assert_eq!(
            (any.price, any.world_id, any.hq, any.fallback),
            (80, 0, true, false)
        );
        assert_eq!(
            resolve_price(&listing(), None, 99, None, PriceSignal::SaleMedian),
            None
        );
    }

    #[test]
    fn counts_and_velocity_describe_sales_separately_from_units() {
        let stats = Some(ItemSaleStats {
            num_sold: 14,
            units_sold: 140,
            sales_per_day: 2.0,
            gil_volume: 7_000,
            ..Default::default()
        });
        let stat = |kind| stats_value(MarketMetric::Stat(kind, Window::D7), stats);
        assert_eq!(stat(StatKind::Units), GridValue::Number(140.0));
        assert_eq!(stat(StatKind::Sales), GridValue::Number(14.0));
        assert_eq!(stat(StatKind::Cadence), GridValue::Number(12.0));
        assert_eq!(stat(StatKind::GilVolume), GridValue::Number(7_000.0));
        assert_eq!(stat(StatKind::Median), GridValue::Missing);
        assert_eq!(
            stats_value(
                MarketMetric::Stat(StatKind::GilVolume, Window::D7),
                Some(ItemSaleStats::default())
            ),
            GridValue::Missing,
            "an old server's zero is unknown, not free"
        );
    }
}
