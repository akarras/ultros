//! Shared market inputs and optional grid columns for every analyzer.
//!
//! Bulk statistics describe the selected scope and exact quality. Expensive
//! hourly history is fetched for the displayed window, accumulated for the
//! life of that scope, and advertises partial filter coverage to QueryGrid.

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
};

use leptos::prelude::*;
use thousands::Separable;
use ultros_api_types::{
    cheapest_listings::{CheapestListingMapKey, CheapestListingsMap},
    floor_history::{FloorHistoryBatch, FloorHistoryRequest, FloorInterval, ItemFloorHistory},
    listing_stats::{ItemListingStats, ListingWindowStats, StockStatus},
    sale_stats::ItemSaleStats,
    sparklines::{SparklinesRequest, SparklinesResponse},
    trends::ConfidenceBand,
};

use crate::{
    analysis::format_duration_short,
    api::{
        get_listing_stats, get_listing_stats_window, get_sale_stats, post_floor_history,
        post_sparklines,
    },
    components::{
        app_link::use_location_or_default,
        sparkline::Sparkline,
        virtual_grid::{
            GridColumn,
            metrics::{GridMetric, GridValue, active_metric_columns},
            query_grid::{MetricSortHeader, QueryGrid},
            units::Unit,
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
    hour_profile::{self, HourStrip, local_offset_hours},
    signals::{StatsIndex, stat_only, stats_index},
    stat_columns::{
        FLOOR_TREND_ID, FLOOR_TREND_WINDOW, FOLLOW_COLUMNS, LISTING_COLUMNS,
        LISTING_WINDOW_COLUMNS, ListingKind, ListingWindowKind, STAT_COLUMNS, StatKind, Window,
        floor_trend_label, floor_trend_title, follow_id, history_observed_note, listing_id,
        listing_label, listing_title, listing_window_id, listing_window_label,
        listing_window_title, listing_window_wanted, listings_wanted, market_picker_group,
        market_picker_group_listings, required_windows, stat_column, stat_label, stat_picker_hint,
        stat_picker_label,
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
    /// How far back a windowed body's observations reach. `None` on the
    /// alive set and on a body with no history rows.
    pub reach: Option<HistoryReach>,
}

type ScopedListings = Option<ListingSlot>;

pub fn listing_index(stats: &[ItemListingStats]) -> ListingIndex {
    stats.iter().map(|s| ((s.item_id, s.hq), *s)).collect()
}

/// The window a body describes and the earliest observation any of its rows
/// holds, per source. A single row's coverage says little (a quiet item has
/// one event), but the earliest across the whole scope is where Ultros's
/// history begins, which is what bounds every row's statistic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryReach {
    pub from: i64,
    pub to: i64,
    pub listings: Option<i64>,
    pub receipts: Option<i64>,
}

impl HistoryReach {
    /// Whole days of the window Ultros has observed, when that is at least
    /// a day short of the window. `None` when the window is covered or
    /// nothing was observed to date it by.
    pub fn observed_days(self, counts_receipts: bool) -> Option<u16> {
        const DAY: i64 = 86_400;
        let first = if counts_receipts {
            self.receipts
        } else {
            self.listings
        }?;
        let span = self.to - self.from;
        if first <= self.from + DAY || span <= 0 {
            return None;
        }
        let days = (self.to - first.min(self.to) + DAY / 2) / DAY;
        (days < (span + DAY / 2) / DAY).then_some(days as u16)
    }
}

pub fn history_reach(stats: &[ItemListingStats]) -> Option<HistoryReach> {
    let mut windows = stats.iter().filter_map(|s| s.window.as_ref());
    let first = windows.next()?;
    let mut reach = HistoryReach {
        from: first.from,
        to: first.to,
        listings: first.listing_coverage.first_observed_unix,
        receipts: first.matches.receipt_coverage.first_observed_unix,
    };
    let earliest = |held: Option<i64>, seen: Option<i64>| match (held, seen) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    for window in windows {
        reach.listings = earliest(reach.listings, window.listing_coverage.first_observed_unix);
        reach.receipts = earliest(
            reach.receipts,
            window.matches.receipt_coverage.first_observed_unix,
        );
    }
    Some(reach)
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
    /// Listing history per window, fetched with `?window=N` only when a
    /// windowed listing column wants the selected window. Each window body
    /// is a separate multi-megabyte fetch, so nothing pins a window.
    listing_windows: [RwSignal<ScopedListings>; Window::ALL.len()],
    listing_windows_wanted: [RwSignal<bool>; Window::ALL.len()],
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

    /// The windowed listing body for the present scope and `window`, once it
    /// has landed. `failed` is set only after the retry ladder is exhausted.
    pub fn listing_window(self, window: Window) -> Option<ListingSlot> {
        let scope = self.scope.get();
        self.listing_windows[window.index()]
            .with(|v| v.as_ref().filter(|slot| slot.scope == scope).cloned())
    }

    /// Ask for a window's listing history. Idempotent; never un-wants.
    pub fn want_listing_window(self, window: Window) {
        let flag = self.listing_windows_wanted[window.index()];
        if !flag.get_untracked() {
            flag.set(true);
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
        for slot in self.listing_windows {
            slot.with(|_| ());
        }
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
        listing_windows: std::array::from_fn(|_| RwSignal::new(None)),
        listing_windows_wanted: std::array::from_fn(|_| RwSignal::new(false)),
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
    fetch_listing_stats(scope, market.listings, market.listings_wanted.into(), None);
    for window in Window::ALL {
        fetch_listing_stats(
            scope,
            market.listing_windows[window.index()],
            market.listing_windows_wanted[window.index()].into(),
            Some(window.days()),
        );
    }
    market
}

/// Same scope-change guard as `fetch_stats`. Unlike `sale_stats`, an empty
/// board is a successful `200 {"stats":[]}`: only a transport error sets
/// `failed`, so cells can tell "nothing alive" from "could not ask". A
/// windowed body (`days` is `Some`) retries a failure three times before it
/// settles as failed.
fn fetch_listing_stats(
    scope: Signal<String>,
    output: RwSignal<ScopedListings>,
    wanted: Signal<bool>,
    days: Option<u16>,
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
                reach: None,
            }));
            return;
        }
        leptos::task::spawn_local(async move {
            // A cold scope/window pair is 503 until the snapshot worker
            // publishes (about a minute for a world, longer for a DC), so a
            // windowed body waits and retries; the alive set never retries.
            const RETRY_MS: [u32; 3] = [15_000, 30_000, 60_000];
            let mut attempt = 0usize;
            let result = loop {
                let result = match days {
                    Some(days) => get_listing_stats_window(&name, days).await,
                    None => get_listing_stats(&name).await,
                };
                if result.is_ok() || days.is_none() || attempt == RETRY_MS.len() {
                    break result;
                }
                gloo_timers::future::TimeoutFuture::new(RETRY_MS[attempt]).await;
                attempt += 1;
                if scope.try_get_untracked().as_ref() != Some(&name)
                    || generation.try_get_value() != Some(epoch)
                {
                    return;
                }
            };
            let result =
                result.map(|body| (listing_index(&body.stats), history_reach(&body.stats)));
            let failed = result.is_err();
            let (index, reach) = result.unwrap_or_default();
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
                reach,
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
    ListingAssessment,
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
    /// Listing history over the selected window; ids and labels come from
    /// `LISTING_WINDOW_COLUMNS`.
    ListingWindow(ListingWindowKind),
    /// The pinned 30-day scope floor sparkline; its value is the first to
    /// last observed floor change, in percent.
    FloorTrend,
}

impl MarketMetric {
    fn id(self) -> &'static str {
        match self {
            Self::Listings(kind) => listing_id(kind),
            Self::ListingWindow(kind) => listing_window_id(kind),
            Self::FloorTrend => FLOOR_TREND_ID,
            Self::Subject => "market-subject",
            Self::Scope => "market-scope",
            Self::Quality => "market-quality",
            Self::World => "market-world",
            Self::Datacenter => "market-datacenter",
            Self::Listing => "market-listing",
            Self::ListingAssessment => "market-listing-assessment",
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
                | Self::TrendWorld
                | Self::ListingAssessment
        )
    }

    /// Filled per visible row, so it never sorts or filters the whole list.
    fn partial(self) -> bool {
        matches!(self, Self::Trend7 | Self::Drift7 | Self::FloorTrend)
    }

    /// How a filter on this column reads and prints its bounds.
    fn unit(self) -> Unit {
        let stat = |kind| match kind {
            StatKind::Min
            | StatKind::Median
            | StatKind::Average
            | StatKind::Vwap
            | StatKind::GilVolume => Unit::Gil,
            StatKind::SalesPerDay => Unit::Rate,
            StatKind::Cadence => Unit::Hours,
            StatKind::Units | StatKind::Sales => Unit::Plain,
        };
        match self {
            Self::Listing => Unit::Gil,
            Self::Stat(kind, _) | Self::Follow(kind) => stat(kind),
            Self::LastSold => Unit::Timestamp,
            Self::Trend7 | Self::Drift7 | Self::FloorTrend => Unit::Percent,
            Self::Listings(kind) if kind.is_age() => Unit::Seconds,
            Self::ListingWindow(kind) => match kind {
                ListingWindowKind::FloorMin | ListingWindowKind::FloorMax => Unit::Gil,
                ListingWindowKind::TimeToSell => Unit::Seconds,
                ListingWindowKind::UndercutsPerDay => Unit::Rate,
                ListingWindowKind::UndercutMedian => Unit::Percent,
                ListingWindowKind::Additions
                | ListingWindowKind::Removals
                | ListingWindowKind::DaysOfStock
                | ListingWindowKind::ListingHours
                | ListingWindowKind::UndercutHours => Unit::Plain,
            },
            _ => Unit::Plain,
        }
    }

    /// The bulk body this metric reads. Last-sold and confidence are
    /// seven-day facts, as before.
    fn window(self, selected: Window) -> Option<Window> {
        match self {
            Self::Stat(_, window) => Some(window),
            Self::Follow(_) | Self::ListingAssessment => Some(selected),
            Self::LastSold | Self::Confidence => Some(Window::D7),
            Self::ListingWindow(_) => Some(selected),
            _ => None,
        }
    }
}

const LEADING_METRICS: [MarketMetric; 7] = [
    MarketMetric::Subject,
    MarketMetric::Scope,
    MarketMetric::Quality,
    MarketMetric::World,
    MarketMetric::Datacenter,
    MarketMetric::Listing,
    MarketMetric::ListingAssessment,
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
        .chain(
            LISTING_WINDOW_COLUMNS
                .iter()
                .map(|(kind, _)| MarketMetric::ListingWindow(*kind)),
        )
        .chain([MarketMetric::FloorTrend])
}

fn metric_by_id(id: &str) -> Option<MarketMetric> {
    market_metrics().find(|m| m.id() == id)
}

/// Header hover text; only the age columns carry one.
fn metric_title(metric: MarketMetric) -> Option<String> {
    match metric {
        MarketMetric::ListingAssessment => {
            let i18n = crate::i18n_fallback::use_i18n_or_default();
            Some(t_string!(i18n, market_listing_assessment_hint).to_string())
        }
        MarketMetric::Listings(kind) => listing_title(kind),
        MarketMetric::ListingWindow(kind) => Some(listing_window_title(kind)),
        MarketMetric::FloorTrend => Some(floor_trend_title()),
        _ => None,
    }
}

/// The sortable header's hover text: a windowed listing column also says
/// how much of the selected window Ultros has actually observed.
fn metric_header_title(metric: MarketMetric, market: MarketData) -> Option<String> {
    let title = metric_title(metric);
    let MarketMetric::ListingWindow(kind) = metric else {
        return title;
    };
    let window = market.window.selected.get();
    let note = market
        .listing_window(window)
        .and_then(|slot| slot.reach)
        .and_then(|reach| reach.observed_days(kind.counts_receipts()))
        .map(|days| history_observed_note(days, window));
    match (title, note) {
        (Some(title), Some(note)) => Some(format!("{title} {note}")),
        (title, note) => title.or(note),
    }
}

fn metric_label(metric: MarketMetric, selected: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match metric {
        MarketMetric::Listings(kind) => return listing_label(kind),
        MarketMetric::ListingWindow(kind) => return listing_window_label(kind, selected),
        MarketMetric::FloorTrend => return floor_trend_label(),
        MarketMetric::Subject => t_string!(i18n, market_subject),
        MarketMetric::Scope => t_string!(i18n, market_scope),
        MarketMetric::Quality => t_string!(i18n, market_quality),
        MarketMetric::World => t_string!(i18n, market_world),
        MarketMetric::Datacenter => t_string!(i18n, market_datacenter),
        MarketMetric::Listing => t_string!(i18n, market_listing),
        MarketMetric::ListingAssessment => t_string!(i18n, market_listing_assessment),
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

/// A successful recent-history response with no matching sales is unknown,
/// whereas a failed response cannot support a numerical demand estimate.
pub fn recent_sample_value(value: f64, feed_available: bool, sample_size: usize) -> GridValue {
    if !feed_available {
        GridValue::Unavailable
    } else if sample_size == 0 {
        GridValue::Missing
    } else {
        number(Some(value))
    }
}

fn listing_assessment(price: Option<i32>, median: Option<i32>) -> GridValue {
    match (price.filter(|p| *p > 0), median.filter(|m| *m > 0)) {
        (Some(price), Some(median)) => GridValue::Text(
            if crate::analysis::is_troll_listing(price, median) {
                "suspicious"
            } else {
                "plausible"
            }
            .into(),
        ),
        _ => GridValue::Text("unverified".into()),
    }
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
        // Unix seconds, so a filter can ask for "within the last 7 days";
        // `display_value` prints the same date the column always showed.
        MarketMetric::LastSold => {
            if s.last_sold_unix > 0 {
                GridValue::Number(s.last_sold_unix as f64)
            } else {
                GridValue::Missing
            }
        }
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

/// A row absent from a successful body, or one the server synthesized for a
/// newly alive key without a snapshot row (`window` is `None`), has no
/// history to show. A zero rate or count is a real zero; a missing median
/// means nothing happened to take one of. `undercut_median` is a fraction on
/// the wire; it is scaled to a percentage here and `display_value` adds the
/// `%`. A floor is an observed floor: never an unknown stretch read as 0 gil.
/// Days of stock exists only as an estimate; no sales is not "0 days".
fn listing_window_value(kind: ListingWindowKind, stats: Option<&ItemListingStats>) -> GridValue {
    let Some(window) = stats.and_then(|s| s.window.as_ref()) else {
        return GridValue::Missing;
    };
    let floor = |price: Option<u32>| number(price.filter(|p| *p > 0).map(f64::from));
    match kind {
        ListingWindowKind::FloorMin => floor(window.floor_min),
        ListingWindowKind::FloorMax => floor(window.floor_max),
        ListingWindowKind::Additions => GridValue::Number(window.additions as f64),
        ListingWindowKind::Removals => GridValue::Number(window.removals as f64),
        ListingWindowKind::TimeToSell => {
            number(window.matches.median_time_to_sell_secs.map(|s| s as f64))
        }
        ListingWindowKind::DaysOfStock => number(
            (window.stock_status == StockStatus::Estimated)
                .then_some(window.days_of_stock)
                .flatten(),
        ),
        ListingWindowKind::UndercutsPerDay => number(window.undercuts_per_day),
        ListingWindowKind::UndercutMedian => number(window.undercut_median.map(|m| m * 100.0)),
        ListingWindowKind::ListingHours | ListingWindowKind::UndercutHours => number(
            hour_profile::peak_hour(&local_hours(kind, window, local_offset_hours()))
                .map(|h| h as f64),
        ),
    }
}

/// The strip's counts in the viewer's day.
fn local_hours(kind: ListingWindowKind, window: &ListingWindowStats, offset: i64) -> [u32; 24] {
    let utc = match kind {
        ListingWindowKind::UndercutHours => &window.undercut_hours,
        _ => &window.new_listing_hours,
    };
    hour_profile::to_local(utc, offset)
}

/// What an hour-strip cell draws: local counts, whether the placement is too
/// loose to trust, and the hover text. `None` when there is nothing to bin,
/// so the cell falls back to its dash.
fn hour_strip(
    kind: ListingWindowKind,
    stats: Option<&ItemListingStats>,
    offset: i64,
) -> Option<([u32; 24], bool, String)> {
    let window = stats.and_then(|s| s.window.as_ref())?;
    let hours = local_hours(kind, window, offset);
    let peak = hour_profile::peak_hour(&hours)?;
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let mut label = t_string!(
        i18n,
        market_hours_peak,
        hour = hour_profile::hour_label(peak)
    )
    .to_string();
    let mut faded = false;
    if kind == ListingWindowKind::ListingHours && window.new_listings > 0 {
        let share = window.new_listings_pinned as f64 / window.new_listings as f64;
        let pct = (share * 100.0).round().to_string();
        label = format!(
            "{label} · {}",
            t_string!(i18n, market_hours_pinned, pct = pct)
        );
        if share < hour_profile::PINNED_FLOOR {
            faded = true;
            label = format!("{label}. {}", t_string!(i18n, market_hours_uncertain));
        }
    }
    Some((hours, faded, label))
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

/// A floor series is per scope, so its key needs no world.
type FloorKey = (i32, bool);
type FloorStore = Enrichment<FloorKey, MarketSpark>;

/// The server's per-request caps (`FloorHistoryRequest::valid`).
const FLOOR_IDS_PER_REQUEST: usize = 20;

/// The end of the requested range: an hour boundary, so every row and every
/// visitor within the hour shares the server's cache entry, five minutes
/// back so a client clock slightly ahead of the server's is not refused.
fn floor_trend_to(now_unix: i64) -> i64 {
    (now_unix - 300).div_euclid(3_600) * 3_600
}

/// Daily floors, oldest first, from the first day Ultros knew the whole
/// scope's board. Earlier days are unknown rather than empty, so they are
/// dropped instead of drawn; a known-empty day inside the series (no
/// listings at all) is a gap the sparkline bridges. The change runs from the
/// first observed floor to the last one.
fn floor_spark(series: &ItemFloorHistory) -> SparkValue {
    let unknown: HashSet<i64> = series.unknown_timestamps.iter().copied().collect();
    let points: Vec<u32> = series
        .history
        .points
        .iter()
        .skip_while(|point| unknown.contains(&point.timestamp))
        .map(|point| point.price.unwrap_or(0))
        .collect();
    let first = points.iter().copied().find(|p| *p > 0).unwrap_or(0);
    let last = points.iter().copied().rfind(|p| *p > 0).unwrap_or(0);
    SparkValue {
        delta_pct: crate::analysis::first_to_last_pct(first, last),
        points,
    }
}

/// Every requested key settles: a failed batch as Unavailable, a key the
/// server left out of a successful batch as Missing (via the store).
fn floor_response(
    requested: &[FloorKey],
    response: Option<FloorHistoryBatch>,
) -> Vec<(FloorKey, MarketSpark)> {
    match response {
        Some(body) => body
            .series
            .iter()
            .map(|series| {
                (
                    (series.item_id, series.hq),
                    MarketSpark::Ready(floor_spark(series)),
                )
            })
            .collect(),
        None => requested
            .iter()
            .map(|key| (*key, MarketSpark::Unavailable))
            .collect(),
    }
}

/// One daily-cadence request per quality and per 20 items: the key set a
/// request settles, and the request.
fn floor_requests(keys: &[FloorKey], now_unix: i64) -> Vec<(Vec<FloorKey>, FloorHistoryRequest)> {
    let to = floor_trend_to(now_unix);
    let from = to - i64::from(FLOOR_TREND_WINDOW.days()) * 86_400;
    let mut out = Vec::new();
    for hq in [false, true] {
        // Row order (nearest the viewport first), without repeats.
        let mut seen = HashSet::new();
        let ids: Vec<i32> = keys
            .iter()
            .filter(|k| k.1 == hq && seen.insert(k.0))
            .map(|k| k.0)
            .collect();
        for chunk in ids.chunks(FLOOR_IDS_PER_REQUEST) {
            out.push((
                chunk.iter().map(|id| (*id, hq)).collect(),
                FloorHistoryRequest {
                    item_ids: chunk.to_vec(),
                    from,
                    to,
                    interval: FloorInterval::Daily,
                    hq: Some(hq),
                },
            ));
        }
    }
    out
}

/// The planned requests, sent one after another: the server answers 503
/// rather than queue a fifth uncached batch, so a burst of parallel requests
/// from one grid would mostly fail. A failed request retries twice before
/// its keys settle as unavailable. `alive` is checked before each request so
/// a grid that moved to another scope stops asking.
async fn fetch_floor_trends(
    scope: String,
    keys: Vec<FloorKey>,
    alive: impl Fn() -> bool,
) -> Vec<(FloorKey, MarketSpark)> {
    const RETRY_MS: [u32; 2] = [1_500, 4_000];
    let mut out = Vec::with_capacity(keys.len());
    for (requested, request) in floor_requests(&keys, chrono::Utc::now().timestamp()) {
        let mut attempt = 0;
        let response = loop {
            if !alive() {
                break None;
            }
            match post_floor_history(&scope, request.clone()).await {
                Ok(body) => break Some(body),
                Err(_) if attempt < RETRY_MS.len() => {
                    gloo_timers::future::TimeoutFuture::new(RETRY_MS[attempt]).await;
                    attempt += 1;
                }
                Err(_) => break None,
            }
        };
        out.extend(floor_response(&requested, response));
    }
    out
}

fn spark_metric_value<K: Copy + Eq + Hash>(
    store: &Enrichment<K, MarketSpark>,
    key: &K,
) -> GridValue {
    match store.get(key) {
        Some(MarketSpark::Ready(s)) => number(s.delta_pct.map(f64::from)),
        Some(MarketSpark::Unavailable) => GridValue::Unavailable,
        None if store.is_settled(key) => GridValue::Missing,
        None => GridValue::Pending,
    }
}

/// The hour strip for a row, read from the selected window's listing body.
fn market_hour_strip(
    kind: ListingWindowKind,
    subject: &MarketSubject,
    market: MarketData,
) -> Option<([u32; 24], bool, String)> {
    let slot = market.listing_window(market.window.selected.get())?;
    if slot.failed {
        return None;
    }
    hour_strip(
        kind,
        slot.index.get(&(subject.item_id, subject.hq)),
        local_offset_hours(),
    )
}

fn spark_key(subject: &MarketSubject, scope_world: Option<i32>) -> (i32, bool, i32) {
    (
        subject.item_id,
        subject.hq,
        scope_world.unwrap_or(subject.world_id),
    )
}

/// The two per-row feeds a grid fills for its visible window.
#[derive(Clone, Copy)]
struct RowFeeds {
    /// Hourly VWAP for Trend/Drift, per world.
    sparks: RwSignal<MarketSparkStore>,
    /// Daily scope floors for the floor sparkline.
    floors: RwSignal<FloorStore>,
}

impl RowFeeds {
    fn new() -> Self {
        Self {
            sparks: RwSignal::new(MarketSparkStore::default()),
            floors: RwSignal::new(FloorStore::default()),
        }
    }

    fn track(self) {
        self.sparks.with(|_| ());
        self.floors.with(|_| ());
    }

    /// The floor series to draw, once one with an observed floor landed.
    fn floor_spark(self, subject: &MarketSubject) -> Option<SparkValue> {
        self.floors
            .with(|store| match store.get(&(subject.item_id, subject.hq)) {
                Some(MarketSpark::Ready(value)) if value.delta_pct.is_some() => Some(value.clone()),
                _ => None,
            })
    }
}

fn market_value(
    metric: MarketMetric,
    subject: &MarketSubject,
    market: MarketData,
    feeds: RowFeeds,
    scope_world: Memo<Option<i32>>,
    worlds: &WorldNames,
) -> GridValue {
    let sparks = feeds.sparks;
    let text = |value: Option<String>| {
        value
            .filter(|v| !v.is_empty())
            .map_or(GridValue::Missing, GridValue::Text)
    };
    match metric {
        MarketMetric::Subject => text(if subject.label.is_empty() {
            crate::global_state::xiv_data::tracked_data()
                .items
                .get(&xiv_gen::ItemId(subject.item_id))
                .map(|item| item.name.clone())
        } else {
            Some(subject.label.clone())
        }),
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
        MarketMetric::ListingAssessment => {
            let window = market.window.selected.get();
            if market.stats_failed(window) {
                return GridValue::Unavailable;
            }
            match market.stats(window) {
                None => GridValue::Pending,
                Some(stats) => listing_assessment(
                    subject.listing_price,
                    stats
                        .get(&(subject.item_id, subject.hq))
                        .map(|s| s.median_price),
                ),
            }
        }
        MarketMetric::Trend7 | MarketMetric::Drift7 => sparks.with(|store| {
            let key = spark_key(subject, scope_world.get());
            spark_metric_value(store, &key)
        }),
        MarketMetric::FloorTrend => feeds
            .floors
            .with(|store| spark_metric_value(store, &(subject.item_id, subject.hq))),
        MarketMetric::Listings(kind) => match market.listings() {
            None => GridValue::Pending,
            Some(slot) if slot.failed => GridValue::Unavailable,
            Some(slot) => listing_value(
                kind,
                slot.index.get(&(subject.item_id, subject.hq)),
                slot.fetched_unix,
            ),
        },
        MarketMetric::ListingWindow(kind) => {
            match market.listing_window(market.window.selected.get()) {
                None => GridValue::Pending,
                Some(slot) if slot.failed => GridValue::Unavailable,
                Some(slot) => {
                    listing_window_value(kind, slot.index.get(&(subject.item_id, subject.hq)))
                }
            }
        }
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
        GridValue::Number(n) if metric == MarketMetric::LastSold => {
            chrono::DateTime::from_timestamp(n as i64, 0)
                .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_default()
        }
        GridValue::Number(n)
            if matches!(
                metric,
                MarketMetric::Trend7 | MarketMetric::Drift7 | MarketMetric::FloorTrend
            ) =>
        {
            format!("{n:+.1}%")
        }
        GridValue::Number(n)
            if matches!(metric, MarketMetric::Listings(kind) if kind.is_age())
                || metric == MarketMetric::ListingWindow(ListingWindowKind::TimeToSell) =>
        {
            format_duration_short(n.max(0.0).round() as u64)
        }
        GridValue::Number(n) if matches!(metric, MarketMetric::ListingWindow(kind) if kind.is_hours()) => {
            hour_profile::hour_label(n.max(0.0) as usize)
        }
        GridValue::Number(n)
            if metric == MarketMetric::ListingWindow(ListingWindowKind::DaysOfStock) =>
        {
            format!("{n:.1}")
        }
        GridValue::Number(n)
            if matches!(
                metric,
                MarketMetric::ListingWindow(ListingWindowKind::UndercutMedian)
            ) =>
        {
            format!("{n:.1}%")
        }
        GridValue::Number(n)
            if matches!(
                metric,
                MarketMetric::Stat(StatKind::SalesPerDay | StatKind::Cadence, _)
                    | MarketMetric::Follow(StatKind::SalesPerDay | StatKind::Cadence)
                    | MarketMetric::ListingWindow(ListingWindowKind::UndercutsPerDay)
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
        GridValue::Text(s) if matches!(metric, MarketMetric::ListingAssessment) => {
            let i18n = crate::i18n_fallback::use_i18n_or_default();
            match s.as_str() {
                "suspicious" => t_string!(i18n, market_listing_suspicious).to_string(),
                "plausible" => t_string!(i18n, market_listing_plausible).to_string(),
                "unverified" => t_string!(i18n, market_listing_unverified).to_string(),
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
fn signal_for_metric(id: &str) -> Option<&'static str> {
    match metric_by_id(id)? {
        MarketMetric::Listing => Some("listing-min"),
        MarketMetric::Follow(StatKind::Min) => Some("sale-min"),
        MarketMetric::Follow(StatKind::Median) => Some("sale-median"),
        MarketMetric::Follow(StatKind::Average) => Some("sale-avg"),
        // Fixed-window columns cannot select a basis for a different window.
        _ => None,
    }
}

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
    /// Forwarded to the grid: data-row index (in the rows `on_rows` reports)
    /// to scroll into view.
    #[prop(optional, into)]
    reveal_index: Option<Signal<Option<usize>>>,
    #[prop(into)] id: String,
    #[prop(into)] label: String,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Ord + Hash + Send + Sync + 'static,
    KF: Fn(&T) -> K + Send + Sync + 'static,
    H: Fn(&'static str) -> AnyView + Send + Sync + 'static,
    F: Fn(T, &'static str) -> AnyView + Send + Sync + 'static,
    M: Fn(&T, &'static str) -> (String, f64) + Send + Sync + 'static,
{
    let calculation = use_context::<super::calculation::Calculation>();
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
    let feeds = RowFeeds::new();
    let sparks = feeds.sparks;
    // Providers update independently of row identities and query results.
    // Track their revisions without copying payloads or measuring every row
    // in a reactive effect; the grid performs one debounced, chunked pass.
    let sizing_version = Memo::new(move |previous: Option<&u64>| {
        measure_version.get();
        market.scope.with(|_| ());
        market.window.selected.get();
        market.track_all();
        feeds.track();
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
                MarketMetric::Listings(_)
                | MarketMetric::ListingWindow(_)
                | MarketMetric::FloorTrend => Some(market_picker_group_listings()),
                _ => None,
            };
            match metric {
                MarketMetric::Follow(kind) => {
                    column.picker_label =
                        Some(stat_picker_label(kind, market.window.selected.get(), true));
                    column.picker_hint = Some(format!(
                        "{}: {}",
                        market.scope.get(),
                        stat_picker_hint(market.window.selected.get(), true)
                    ));
                }
                MarketMetric::Stat(kind, window) => {
                    column.picker_label = Some(stat_picker_label(kind, window, false));
                    column.picker_hint = Some(format!(
                        "{}: {}",
                        market.scope.get(),
                        stat_picker_hint(window, false)
                    ));
                }
                _ => column.picker_hint = metric_title(metric),
            }
        }
        for column in &mut result {
            if let Some((calculation, term)) =
                calculation.and_then(|c| c.term(column.id).map(|term| (c, term)))
            {
                column.heading_adornments += 24.0;
                if let Some(key) = term.key {
                    column
                        .heading_lines
                        .push((calculation.selected_label(key), 0.0));
                }
            }
            if calculation.and_then(|c| c.market_input).is_some()
                && signal_for_metric(column.id).is_some()
            {
                column.heading_lines.push((String::new(), 60.0));
            }
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
            if n.contains("market-listing-assessment") {
                market.want(market.window.selected.get());
            }
            if listings_wanted(n) {
                market.want_listings();
            }
            if listing_window_wanted(n) {
                market.want_listing_window(market.window.selected.get());
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
    let subject_floors = subject.clone();
    let floor_rows = Signal::derive(move || {
        if !needs.with(|n| n.contains(FLOOR_TREND_ID)) {
            return Vec::new();
        }
        filtered.with(|rows| {
            rows.iter()
                .map(|row| {
                    let s = subject_floors(row);
                    (s.item_id, s.hq)
                })
                .collect()
        })
    });
    use_visible_enrichment(
        feeds.floors,
        floor_rows,
        range.into(),
        market.scope,
        |key| *key,
        move |scope: String, keys| {
            let alive = {
                let scope = scope.clone();
                move || market.scope.try_get_untracked().as_ref() == Some(&scope)
            };
            fetch_floor_trends(scope, keys, alive)
        },
        // One window's keys arrive in one call, which paces its own
        // 20-item requests; see `fetch_floor_trends`.
        EnrichmentConfig {
            prefetch_margin: PREFETCH_MARGIN,
            debounce_ms: DEBOUNCE_MS,
            max_keys_per_request: 200,
        },
    );
    let mut all_metrics = metrics;
    let confidence_stats = Memo::new(move |_| market.stats(Window::D7));
    for metric in market_metrics() {
        if all_metrics.iter().any(|m| m.id == metric.id()) {
            continue;
        }
        let comparator_subject = subject.clone();
        let subject = subject.clone();
        let worlds = worlds.clone();
        let value =
            move |row: &T| market_value(metric, &subject(row), market, feeds, scope_world, &worlds);
        let def = if metric.text() {
            GridMetric::text(metric.id(), value)
        } else {
            GridMetric::number(metric.id(), value)
        }
        .unit(metric.unit());
        let def = if metric == MarketMetric::Confidence {
            def.with_comparator(move |left, right, ascending| {
                // Key extraction above already tracks this scope and D7
                // provider once per row; comparisons only borrow the snapshot.
                confidence_stats.with_untracked(|stats| {
                    let confidence = |row: &T| {
                        let subject = comparator_subject(row);
                        stats
                            .as_ref()?
                            .get(&(subject.item_id, subject.hq))
                            .map(|s| s.confidence)
                    };
                    super::confidence::compare_confidence(
                        confidence(left),
                        confidence(right),
                        ascending,
                    )
                })
            })
        } else {
            def
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
        <QueryGrid each columns=all_columns key row_height visible_range=range reveal_index id label metrics=all_metrics on_rows=handle_rows show_saved_views measure_version=sizing_version
            header=move |id| {
                let header = match metric_by_id(id) {
                Some(metric) if !metric.partial() && sortable.with_value(|ids| ids.contains(&id)) => view! {
                    <span title=move || metric_header_title(metric, market)>
                        <MetricSortHeader column=id label=Signal::derive(move || metric_label(metric, market.window.selected.get())) />
                    </span>
                }.into_any(),
                Some(metric) => view! {
                    <span title=metric_title(metric)>
                        {move || metric_label(metric, market.window.selected.get())}
                    </span>
                }.into_any(),
                None => native_header.with_value(|header| header(id)),
                };
                let header = super::calculation::decorate_header(calculation, id, header);
                match calculation.and_then(|c| c.market_input.map(|key| (c, key))).zip(signal_for_metric(id)) {
                    Some(((calculation, key), value)) => view! {
                        <div class="flex flex-col items-start gap-1 min-w-0">
                            {header}
                            <super::calculation::UsePriceSignal calculation key value />
                        </div>
                    }.into_any(),
                    None => header,
                }
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
                    if matches!(metric, MarketMetric::Trend7 | MarketMetric::Drift7) {
                        let world = title_worlds.get(&spark_key(&title_subject, scope_world.get()).2)
                            .map(|v| v.0.clone()).unwrap_or_else(|| "—".into());
                        format!("{}: {world}", metric_label(MarketMetric::TrendWorld, market.window.selected.get()))
                    } else { String::new() }
                }>{move || {
                    if matches!(metric, MarketMetric::Trend7)
                        && let Some(MarketSpark::Ready(value)) = sparks.with(|s| s.get(&spark_key(&subject, scope_world.get())).cloned()) {
                        return view! { <Sparkline points=value.points pct_change=value.delta_pct.unwrap_or_default() width=120 /> }.into_any();
                    }
                    if matches!(metric, MarketMetric::FloorTrend)
                        && let Some(value) = feeds.floor_spark(&subject) {
                        return view! { <Sparkline points=value.points pct_change=value.delta_pct.unwrap_or_default() width=120 hours_per_point=24 /> }.into_any();
                    }
                    if let MarketMetric::ListingWindow(kind) = metric
                        && kind.is_hours()
                        && let Some((hours, faded, label)) = market_hour_strip(kind, &subject, market) {
                        return view! { <HourStrip hours faded label /> }.into_any();
                    }
                    display_value(metric, market_value(metric, &subject, market, feeds, scope_world, &worlds)).into_any()
                }}</div> }.into_any()
            }
            measure=move |row: &T, id| match metric_by_id(id) {
                Some(metric) => {
                    let subject = subject_measure(row);
                    let svg = match metric {
                        MarketMetric::Trend7 => sparks.with(|store| matches!(store.get(&spark_key(&subject, scope_world.get())), Some(MarketSpark::Ready(_)))),
                        MarketMetric::FloorTrend => feeds.floor_spark(&subject).is_some(),
                        MarketMetric::ListingWindow(kind) if kind.is_hours() => {
                            if market_hour_strip(kind, &subject, market).is_some() {
                                // 24 × 5px cells with 1px gaps, plus the cell padding.
                                return (String::new(), 168.0);
                            }
                            false
                        }
                        _ => false,
                    };
                    if svg {
                        // This cell renders a 120px SVG, rather than its numeric delta.
                        (String::new(), 144.0)
                    } else {
                        (display_value(metric, market_value(metric, &subject, market, feeds, scope_world, &worlds_measure)), 24.0)
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
    fn only_compatible_follow_window_price_columns_offer_use_shortcuts() {
        assert_eq!(
            super::signal_for_metric("market-listing"),
            Some("listing-min")
        );
        assert_eq!(
            super::signal_for_metric("market-sale-median"),
            Some("sale-median")
        );
        assert_eq!(
            super::signal_for_metric("market-sale-min"),
            Some("sale-min")
        );
        assert_eq!(
            super::signal_for_metric("market-sale-avg"),
            Some("sale-avg")
        );
        for id in [
            "market-sale-median-7",
            "market-vwap",
            "market-sales-per-day",
            "profit",
            "cost",
        ] {
            assert_eq!(super::signal_for_metric(id), None, "{id}");
        }
    }
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
    fn last_sold_is_a_timestamp_that_still_prints_its_date() {
        use crate::components::virtual_grid::metrics::{FilterOp, MetricFilter};
        let metric = MarketMetric::LastSold;
        assert!(!metric.text());
        assert_eq!(metric.unit(), Unit::Timestamp);
        let sold = 1_756_684_800.0;
        assert_eq!(
            display_value(metric, GridValue::Number(sold)),
            "2025-09-01 00:00 UTC"
        );
        // "Within the last day" now reaches this column.
        let within = MetricFilter::range(Some("-1d".into()), None).resolved(sold + 3_600.0);
        assert_eq!(within.op, FilterOp::Range);
        assert_eq!(within.matches(&GridValue::Number(sold), false), Some(true));
        assert_eq!(
            within.matches(&GridValue::Number(sold - 2.0 * 86_400.0), false),
            Some(false)
        );
    }

    #[test]
    fn market_units_follow_what_each_statistic_measures() {
        assert_eq!(MarketMetric::Listing.unit(), Unit::Gil);
        assert_eq!(
            MarketMetric::Stat(StatKind::Median, Window::D7).unit(),
            Unit::Gil
        );
        assert_eq!(
            MarketMetric::Follow(StatKind::SalesPerDay).unit(),
            Unit::Rate
        );
        assert_eq!(MarketMetric::Follow(StatKind::Cadence).unit(), Unit::Hours);
        assert_eq!(MarketMetric::Trend7.unit(), Unit::Percent);
        assert_eq!(
            MarketMetric::Listings(ListingKind::MedianAge).unit(),
            Unit::Seconds
        );
        assert_eq!(
            MarketMetric::Listings(ListingKind::Alive).unit(),
            Unit::Plain
        );
        assert_eq!(MarketMetric::Confidence.unit(), Unit::Plain);
    }

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

            let feeds = RowFeeds::new();
            let scope_world = Memo::new(|_| None);
            let subject = MarketSubject::new(42, false, 7);
            let worlds = Arc::new(HashMap::new());
            let value = || {
                market_value(
                    MarketMetric::Follow(StatKind::Median),
                    &subject,
                    market,
                    feeds,
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
            let feeds = RowFeeds::new();
            let scope_world = Memo::new(|_| None);
            let worlds = Arc::new(HashMap::new());
            let subject = MarketSubject::new(42, true, 7);
            let value = |kind| {
                market_value(
                    MarketMetric::Listings(kind),
                    &subject,
                    market,
                    feeds,
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
                    reach: None,
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

    #[test]
    fn listing_window_columns_follow_the_selected_window_and_its_states() {
        use ultros_api_types::listing_stats::ListingWindowStats;
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let scope = RwSignal::new("Gilgamesh".to_owned());
            let selected = RwSignal::new(Window::D7);
            let mut market = use_market_data(scope.into());
            market.window.selected = Memo::new(move |_| selected.get());
            let feeds = RowFeeds::new();
            let scope_world = Memo::new(|_| None);
            let worlds = Arc::new(HashMap::new());
            let subject = MarketSubject::new(42, true, 7);
            let value = |kind| {
                market_value(
                    MarketMetric::ListingWindow(kind),
                    &subject,
                    market,
                    feeds,
                    scope_world,
                    &worlds,
                )
            };
            let with_window = |undercuts_per_day, undercut_median| ItemListingStats {
                window: Some(ListingWindowStats {
                    window_days: 7,
                    undercuts: 3,
                    undercuts_per_day,
                    undercut_median,
                    ..Default::default()
                }),
                ..row(42, true, 3)
            };
            let slot = |scope: &str, rows: Vec<ItemListingStats>, failed| {
                Some(ListingSlot {
                    scope: scope.into(),
                    index: Arc::new(listing_index(&rows)),
                    failed,
                    fetched_unix: 1_000_000,
                    reach: None,
                })
            };
            // Nothing wanted yet: the slot is empty and cells wait.
            assert!(!market.listing_windows_wanted[Window::D7.index()].get_untracked());
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Pending
            );
            market.want_listing_window(Window::D7);
            assert!(market.listing_windows_wanted[Window::D7.index()].get_untracked());
            assert!(!market.listing_windows_wanted[Window::D30.index()].get_untracked());
            // Another scope's body never answers; a failed body is unknown.
            market.listing_windows[Window::D7.index()].set(slot(
                "Cactuar",
                vec![with_window(Some(0.5), Some(0.25))],
                false,
            ));
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Pending
            );
            market.listing_windows[Window::D7.index()].set(slot("Gilgamesh", Vec::new(), true));
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Unavailable
            );
            assert_eq!(
                value(ListingWindowKind::UndercutMedian),
                GridValue::Unavailable
            );
            // An empty successful body, a row without history, and an
            // NQ row for an HQ subject are all missing, never zero.
            market.listing_windows[Window::D7.index()].set(slot("Gilgamesh", Vec::new(), false));
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Missing
            );
            market.listing_windows[Window::D7.index()].set(slot(
                "Gilgamesh",
                vec![row(42, true, 3)],
                false,
            ));
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Missing
            );
            market.listing_windows[Window::D7.index()].set(slot(
                "Gilgamesh",
                vec![ItemListingStats {
                    hq: false,
                    ..with_window(Some(0.5), Some(0.25))
                }],
                false,
            ));
            assert_eq!(value(ListingWindowKind::UndercutMedian), GridValue::Missing);
            market.listing_windows[Window::D7.index()].set(slot(
                "Gilgamesh",
                vec![with_window(Some(0.5), Some(0.25))],
                false,
            ));
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Number(0.5)
            );
            assert_eq!(
                value(ListingWindowKind::UndercutMedian),
                GridValue::Number(25.0)
            );
            // No undercuts: the rate is a real zero, the median is missing.
            market.listing_windows[Window::D7.index()].set(slot(
                "Gilgamesh",
                vec![with_window(Some(0.0), None)],
                false,
            ));
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Number(0.0)
            );
            assert_eq!(value(ListingWindowKind::UndercutMedian), GridValue::Missing);
            // The column reads the selected window's slot, not the seven-day one.
            selected.set(Window::D30);
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Pending
            );
            assert_eq!(
                metric_by_id("market-undercuts")
                    .unwrap()
                    .window(selected.get()),
                Some(Window::D30)
            );
            market.listing_windows[Window::D30.index()].set(slot(
                "Gilgamesh",
                vec![with_window(Some(1.5), Some(0.1))],
                false,
            ));
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Number(1.5)
            );
            scope.set("Cactuar".into());
            assert_eq!(
                value(ListingWindowKind::UndercutsPerDay),
                GridValue::Pending
            );
        });
    }

    #[test]
    fn undercut_cells_format_as_rate_and_percent() {
        assert_eq!(
            display_value(
                MarketMetric::ListingWindow(ListingWindowKind::UndercutsPerDay),
                GridValue::Number(0.29)
            ),
            "0.29"
        );
        assert_eq!(
            display_value(
                MarketMetric::ListingWindow(ListingWindowKind::UndercutMedian),
                GridValue::Number(2.6)
            ),
            "2.6%"
        );
        assert_eq!(
            display_value(
                MarketMetric::ListingWindow(ListingWindowKind::UndercutMedian),
                GridValue::Missing
            ),
            "—"
        );
    }

    #[test]
    fn hour_columns_sort_by_the_busiest_local_hour_and_fade_loose_placement() {
        use ultros_api_types::listing_stats::ListingWindowStats;
        let mut new_listing_hours = [0; 24];
        new_listing_hours[3] = 9;
        new_listing_hours[4] = 2;
        let mut undercut_hours = [0; 24];
        undercut_hours[20] = 1;
        let stats = |pinned: u64| ItemListingStats {
            item_id: 7,
            window: Some(ListingWindowStats {
                new_listings: 11,
                new_listings_pinned: pinned,
                new_listing_hours,
                undercut_hours,
                ..Default::default()
            }),
            ..Default::default()
        };
        let trusted = stats(9);
        assert_eq!(
            listing_window_value(ListingWindowKind::ListingHours, Some(&trusted)),
            GridValue::Number(3.0)
        );
        assert_eq!(
            listing_window_value(ListingWindowKind::UndercutHours, Some(&trusted)),
            GridValue::Number(20.0)
        );
        assert_eq!(
            display_value(
                MarketMetric::ListingWindow(ListingWindowKind::ListingHours),
                GridValue::Number(3.0)
            ),
            "03:00"
        );
        let empty = ItemListingStats {
            window: Some(ListingWindowStats::default()),
            ..Default::default()
        };
        assert_eq!(
            listing_window_value(ListingWindowKind::ListingHours, Some(&empty)),
            GridValue::Missing
        );
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            // UTC-7: 03:00 UTC is the viewer's 20:00.
            let (hours, faded, label) =
                hour_strip(ListingWindowKind::ListingHours, Some(&trusted), -7).unwrap();
            assert_eq!((hours[20], faded), (9, false));
            assert!(label.contains("20:00") && label.contains("82%"), "{label}");
            let (_, faded, _) =
                hour_strip(ListingWindowKind::ListingHours, Some(&stats(2)), 0).unwrap();
            assert!(faded, "2 of 11 pinned mostly maps browsing");
            let (_, faded, _) =
                hour_strip(ListingWindowKind::UndercutHours, Some(&stats(2)), 0).unwrap();
            assert!(!faded, "undercut timing does not depend on first sight");
            assert!(hour_strip(ListingWindowKind::ListingHours, Some(&empty), 0).is_none());
            assert!(hour_strip(ListingWindowKind::ListingHours, None, 0).is_none());
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

    #[test]
    fn suspicious_listing_filter_requires_matching_positive_evidence() {
        assert_eq!(
            listing_assessment(Some(999_999_999), Some(10_000)),
            GridValue::Text("suspicious".into())
        );
        assert_eq!(
            listing_assessment(Some(500_000), Some(10_000)),
            GridValue::Text("plausible".into())
        );
        assert_eq!(
            listing_assessment(Some(500_001), Some(10_000)),
            GridValue::Text("suspicious".into())
        );
        for (price, median) in [
            (None, Some(100)),
            (Some(999_999_999), None),
            (Some(100), Some(0)),
        ] {
            let value = listing_assessment(price, median);
            assert_eq!(value, GridValue::Text("unverified".into()));
            let guard = crate::components::virtual_grid::metrics::MetricFilter {
                op: crate::components::virtual_grid::metrics::FilterOp::Ne,
                value: "suspicious".into(),
            };
            assert_eq!(guard.matches(&value, false), Some(true));
        }
    }

    #[test]
    fn empty_or_failed_sales_samples_are_not_zero_demand() {
        assert_eq!(recent_sample_value(0.0, false, 0), GridValue::Unavailable);
        assert_eq!(recent_sample_value(0.0, true, 0), GridValue::Missing);
        assert_eq!(recent_sample_value(2.5, true, 3), GridValue::Number(2.5));
    }

    fn history(window: ListingWindowStats) -> ItemListingStats {
        ItemListingStats {
            window: Some(window),
            ..row(42, false, 3)
        }
    }

    use ultros_api_types::listing_stats::{HistoryCoverage, ListingWindowStats, MatchedSalesStats};

    #[test]
    fn history_columns_read_observed_values_and_never_invent_zeros() {
        let value = |kind, window| listing_window_value(kind, Some(&history(window)));
        let observed = ListingWindowStats {
            window_days: 7,
            additions: 12,
            removals: 0,
            floor_min: Some(900),
            floor_max: Some(1_450),
            matches: MatchedSalesStats {
                median_time_to_sell_secs: Some(5_400),
                ..Default::default()
            },
            stock_status: StockStatus::Estimated,
            days_of_stock: Some(3.25),
            ..Default::default()
        };
        assert_eq!(
            value(ListingWindowKind::FloorMin, observed),
            GridValue::Number(900.0)
        );
        assert_eq!(
            value(ListingWindowKind::FloorMax, observed),
            GridValue::Number(1_450.0)
        );
        assert_eq!(
            value(ListingWindowKind::Additions, observed),
            GridValue::Number(12.0)
        );
        // Nothing left the board: a real zero.
        assert_eq!(
            value(ListingWindowKind::Removals, observed),
            GridValue::Number(0.0)
        );
        assert_eq!(
            value(ListingWindowKind::TimeToSell, observed),
            GridValue::Number(5_400.0)
        );
        assert_eq!(
            value(ListingWindowKind::DaysOfStock, observed),
            GridValue::Number(3.25)
        );
        // Unknown floors, no matched sales, no sales at all, or unknown sales.
        let unknown = ListingWindowStats {
            floor_min: None,
            floor_max: Some(0),
            matches: MatchedSalesStats::default(),
            stock_status: StockStatus::NoSales,
            days_of_stock: None,
            ..observed
        };
        for kind in [
            ListingWindowKind::FloorMin,
            ListingWindowKind::FloorMax,
            ListingWindowKind::TimeToSell,
            ListingWindowKind::DaysOfStock,
        ] {
            assert_eq!(value(kind, unknown), GridValue::Missing, "{kind:?}");
        }
        let unavailable = ListingWindowStats {
            stock_status: StockStatus::Unavailable,
            ..observed
        };
        assert_eq!(
            value(ListingWindowKind::DaysOfStock, unavailable),
            GridValue::Missing,
            "a stale estimate is not shown once the server calls stock unavailable"
        );
        // No history row at all.
        assert_eq!(
            listing_window_value(ListingWindowKind::Additions, Some(&row(42, false, 3))),
            GridValue::Missing
        );
    }

    #[test]
    fn history_columns_format_units_and_follow_the_window() {
        let metric = MarketMetric::ListingWindow;
        assert_eq!(metric(ListingWindowKind::FloorMin).unit(), Unit::Gil);
        assert_eq!(metric(ListingWindowKind::TimeToSell).unit(), Unit::Seconds);
        assert_eq!(metric(ListingWindowKind::Additions).unit(), Unit::Plain);
        assert_eq!(
            display_value(
                metric(ListingWindowKind::TimeToSell),
                GridValue::Number(5_400.0)
            ),
            "1h 30m"
        );
        assert_eq!(
            display_value(
                metric(ListingWindowKind::DaysOfStock),
                GridValue::Number(3.26)
            ),
            "3.3"
        );
        assert_eq!(
            display_value(
                metric(ListingWindowKind::FloorMax),
                GridValue::Number(1_450.0)
            ),
            "1,450"
        );
        for id in [
            "market-floor-min",
            "market-floor-max",
            "market-listings-added",
            "market-listings-removed",
            "market-time-to-sell",
            "market-days-of-stock",
            "market-listing-hours",
            "market-undercut-hours",
        ] {
            let found = metric_by_id(id).unwrap();
            assert!(matches!(found, MarketMetric::ListingWindow(_)), "{id}");
            assert!(!found.partial(), "{id} sorts the whole list");
            assert_eq!(found.window(Window::D30), Some(Window::D30), "{id}");
            assert!(listing_window_wanted(&[id.to_owned()].into()), "{id}");
        }
    }

    fn coverage(first: Option<i64>) -> HistoryCoverage {
        HistoryCoverage {
            first_observed_unix: first,
            ..Default::default()
        }
    }

    #[test]
    fn history_reach_is_the_earliest_observation_across_the_scope() {
        const DAY: i64 = 86_400;
        let to = 100 * DAY;
        let from = to - 30 * DAY;
        let window = |listings: Option<i64>, receipts: Option<i64>| ListingWindowStats {
            window_days: 30,
            from,
            to,
            listing_coverage: coverage(listings),
            matches: MatchedSalesStats {
                receipt_coverage: coverage(receipts),
                ..Default::default()
            },
            ..Default::default()
        };
        // Tracking began 12 days ago; receipts 10 days ago. A quiet item's
        // late first event does not shorten the scope's reach.
        let rows = [
            history(window(Some(to - 12 * DAY), None)),
            history(window(Some(to - 2 * DAY), Some(to - 10 * DAY + 3_600))),
            row(43, false, 1),
        ];
        let reach = history_reach(&rows).unwrap();
        assert_eq!(reach.listings, Some(to - 12 * DAY));
        assert_eq!(reach.receipts, Some(to - 10 * DAY + 3_600));
        assert_eq!(reach.observed_days(false), Some(12));
        assert_eq!(reach.observed_days(true), Some(10));
        // A full window, give or take a few hours, says nothing.
        let full = HistoryReach {
            listings: Some(from + 3_600),
            ..reach
        };
        assert_eq!(full.observed_days(false), None);
        // Nothing observed at all dates nothing.
        let none = HistoryReach {
            receipts: None,
            ..reach
        };
        assert_eq!(none.observed_days(true), None);
        assert_eq!(history_reach(&[row(42, false, 3)]), None);
        assert_eq!(history_reach(&[]), None);
    }

    #[test]
    fn a_short_history_is_named_in_the_header_title() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let scope = RwSignal::new("Gilgamesh".to_owned());
            let selected = RwSignal::new(Window::D30);
            let mut market = use_market_data(scope.into());
            market.window.selected = Memo::new(move |_| selected.get());
            let metric = MarketMetric::ListingWindow(ListingWindowKind::FloorMin);
            let plain = metric_header_title(metric, market).unwrap();
            assert!(!plain.contains("observed about"));
            let to = 100 * 86_400;
            market.listing_windows[Window::D30.index()].set(Some(ListingSlot {
                scope: "Gilgamesh".into(),
                index: Arc::new(ListingIndex::new()),
                failed: false,
                fetched_unix: to,
                reach: Some(HistoryReach {
                    from: to - 30 * 86_400,
                    to,
                    listings: Some(to - 12 * 86_400),
                    receipts: None,
                }),
            }));
            let noted = metric_header_title(metric, market).unwrap();
            assert!(noted.starts_with(&plain), "{noted}");
            assert!(
                noted.ends_with("Ultros has observed about 12 of these 30 days so far."),
                "{noted}"
            );
            // Time to sell dates itself by receipts, which this body lacks.
            let receipts = metric_header_title(
                MarketMetric::ListingWindow(ListingWindowKind::TimeToSell),
                market,
            )
            .unwrap();
            assert!(!receipts.contains("observed about"), "{receipts}");
            // Current-listing columns never carry a window note.
            assert_eq!(
                metric_header_title(MarketMetric::Listings(ListingKind::Alive), market),
                None
            );
        });
    }

    fn floor_series(points: &[(i64, Option<u32>)], unknown: &[i64]) -> ItemFloorHistory {
        use ultros_api_types::floor_history::{FloorBounds, FloorHistory, FloorPoint};
        ItemFloorHistory {
            item_id: 42,
            hq: true,
            history: FloorHistory {
                from: points.first().map_or(0, |p| p.0),
                to: points.last().map_or(0, |p| p.0),
                bucket_seconds: 86_400,
                points: points
                    .iter()
                    .map(|&(timestamp, price)| FloorPoint { timestamp, price })
                    .collect(),
            },
            bounds: FloorBounds::default(),
            unknown_timestamps: unknown.to_vec(),
        }
    }

    #[test]
    fn floor_sparkline_drops_days_before_tracking_and_keeps_empty_days_as_gaps() {
        let spark = floor_spark(&floor_series(
            &[
                (0, None),
                (1, None),
                (2, Some(1_000)),
                (3, None),
                (4, Some(1_100)),
            ],
            &[0, 1],
        ));
        assert_eq!(spark.points, vec![1_000, 0, 1_100]);
        assert_eq!(spark.delta_pct, Some(10.0));
        // Nothing known: nothing to draw, and no change to sort by.
        let unknown = floor_spark(&floor_series(&[(0, None), (1, None)], &[0, 1]));
        assert!(unknown.points.is_empty());
        assert_eq!(unknown.delta_pct, None);
        // Known but always empty: points to lay out, still no change.
        let empty = floor_spark(&floor_series(&[(0, None), (1, None)], &[]));
        assert_eq!(empty.points, vec![0, 0]);
        assert_eq!(empty.delta_pct, None);
    }

    #[test]
    fn floor_trend_failures_are_unknown_and_absent_series_are_missing() {
        let key = (42, true);
        let mut store = FloorStore::default();
        assert_eq!(spark_metric_value(&store, &key), GridValue::Pending);
        store.merge(&[key], floor_response(&[key], None));
        assert_eq!(spark_metric_value(&store, &key), GridValue::Unavailable);
        let mut store = FloorStore::default();
        store.merge(
            &[key, (43, true)],
            floor_response(
                &[key, (43, true)],
                Some(FloorHistoryBatch {
                    series: vec![floor_series(&[(0, Some(200)), (1, Some(150))], &[])],
                }),
            ),
        );
        assert_eq!(spark_metric_value(&store, &key), GridValue::Number(-25.0));
        assert_eq!(spark_metric_value(&store, &(43, true)), GridValue::Missing);
        assert_eq!(
            display_value(MarketMetric::FloorTrend, GridValue::Number(-25.0)),
            "-25.0%"
        );
    }

    #[test]
    fn floor_trend_requests_stay_inside_the_server_caps() {
        let now = 1_758_600_000 + 1_234;
        let to = floor_trend_to(now);
        assert_eq!(to % 3_600, 0);
        assert!(to <= now - 300 && to > now - 300 - 3_600);
        // 45 NQ keys (one repeated) and 3 HQ keys.
        let mut keys: Vec<FloorKey> = (1..=45).map(|id| (id, false)).collect();
        keys.push((7, false));
        keys.extend([(1, true), (2, true), (3, true)]);
        let requests = floor_requests(&keys, now);
        let sizes: Vec<_> = requests.iter().map(|(k, _)| k.len()).collect();
        assert_eq!(sizes, vec![20, 20, 5, 3]);
        for (requested, request) in &requests {
            assert!(request.valid(), "{request:?}");
            assert_eq!(request.to - request.from, 30 * 86_400);
            assert_eq!(request.interval, FloorInterval::Daily);
            assert!(requested.iter().all(|k| Some(k.1) == request.hq));
            assert_eq!(
                requested.iter().map(|k| k.0).collect::<Vec<_>>(),
                request.item_ids
            );
        }
        // Row order survives, so the rows nearest the viewport go first.
        assert_eq!(requests[0].1.item_ids[..3], [1, 2, 3]);
    }

    #[test]
    fn floor_trend_is_a_pinned_partial_listing_column() {
        let metric = metric_by_id("market-floor-30").unwrap();
        assert_eq!(metric, MarketMetric::FloorTrend);
        assert!(
            metric.partial(),
            "filled per visible row, never a global sort"
        );
        assert_eq!(metric.unit(), Unit::Percent);
        assert_eq!(metric.window(Window::D7), None, "reads no bulk body");
        assert!(!listing_window_wanted(
            &["market-floor-30".to_owned()].into()
        ));
        assert!(
            required_windows(&["market-floor-30".to_owned()].into(), Window::D7, false).is_empty()
        );
    }
}
