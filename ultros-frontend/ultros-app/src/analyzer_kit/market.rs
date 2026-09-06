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
    sale_stats::ItemSaleStats,
    sparklines::{SparklinesRequest, SparklinesResponse},
    trends::ConfidenceBand,
};

use crate::{
    api::{get_sale_stats, post_sparklines},
    components::{
        app_link::use_location_or_default,
        sparkline::Sparkline,
        virtual_grid::{
            GridColumn,
            metrics::{GridMetric, GridValue, active_metric_columns},
            query_grid::QueryGrid,
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
};

type ScopedStats = Option<(String, Arc<StatsIndex>, bool)>;

/// A cheap reactive handle; the payloads are cloned only by Arc.
#[derive(Clone, Copy)]
pub struct MarketData {
    pub scope: Signal<String>,
    stats_7: RwSignal<ScopedStats>,
    stats_30: RwSignal<ScopedStats>,
    want_30: RwSignal<bool>,
}

impl MarketData {
    pub fn stats7(self) -> Option<Arc<StatsIndex>> {
        let scope = self.scope.get();
        self.stats_7.with(|v| {
            v.as_ref()
                .filter(|(name, _, _)| name == &scope)
                .map(|(_, stats, _)| stats.clone())
        })
    }

    pub fn stats30(self) -> Option<Arc<StatsIndex>> {
        let scope = self.scope.get();
        self.stats_30.with(|v| {
            v.as_ref()
                .filter(|(name, _, _)| name == &scope)
                .map(|(_, stats, _)| stats.clone())
        })
    }

    pub fn stats7_failed(self) -> bool {
        let scope = self.scope.get();
        self.stats_7.with(|v| {
            v.as_ref()
                .is_some_and(|(name, _, failed)| name == &scope && *failed)
        })
    }

    pub fn stats30_failed(self) -> bool {
        let scope = self.scope.get();
        self.stats_30.with(|v| {
            v.as_ref()
                .is_some_and(|(name, _, failed)| name == &scope && *failed)
        })
    }
}

/// Both SSR and the initial hydrated render use listing fallbacks. The
/// client fills the shared seven-day body after mounting; optional thirty-
/// day data never delays the first table. Failed requests settle to empty.
pub fn use_market_data(scope: Signal<String>) -> MarketData {
    let market = MarketData {
        scope,
        stats_7: RwSignal::new(None),
        stats_30: RwSignal::new(None),
        want_30: RwSignal::new(false),
    };
    fetch_stats(scope, market.stats_7, Signal::derive(|| true), 7);
    fetch_stats(scope, market.stats_30, market.want_30.into(), 30);
    market
}

fn fetch_stats(
    scope: Signal<String>,
    output: RwSignal<ScopedStats>,
    wanted: Signal<bool>,
    days: u16,
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
    let options = [
        (PriceSignal::ListingMin, listing_label),
        (
            PriceSignal::SaleMin,
            t_string!(i18n, market_sale_min_7).to_string(),
        ),
        (
            PriceSignal::SaleMedian,
            t_string!(i18n, market_sale_median_7).to_string(),
        ),
        (
            PriceSignal::SaleAvg,
            t_string!(i18n, market_sale_avg_7).to_string(),
        ),
    ];
    view! {
        <div class="flex flex-col gap-1">
            <label class="filter-chip">
                <span>{label}</span>
                <select class="filter-chip-value" prop:value=move || basis.get().to_string()
                    on:change=move |ev| {
                        if let Ok(value) = event_target_value(&ev).parse() { on_change.run(value); }
                    }>
                    {options.into_iter().map(|(value, label)| view! {
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

#[derive(Clone, Copy)]
enum MarketMetric {
    Subject,
    Scope,
    Quality,
    World,
    Datacenter,
    Listing,
    Minimum7,
    Median7,
    Average7,
    SalesPerDay7,
    Cadence7,
    Units7,
    Sales7,
    Vwap7,
    LastSold,
    Confidence,
    Units30,
    Sales30,
    Vwap30,
    TrendWorld,
    Trend7,
    Drift7,
}

impl MarketMetric {
    fn id(self) -> &'static str {
        match self {
            Self::Subject => "market-subject",
            Self::Scope => "market-scope",
            Self::Quality => "market-quality",
            Self::World => "market-world",
            Self::Datacenter => "market-datacenter",
            Self::Listing => "market-listing",
            Self::Minimum7 => "market-sale-min-7",
            Self::Median7 => "market-sale-median-7",
            Self::Average7 => "market-sale-avg-7",
            Self::SalesPerDay7 => "market-sales-per-day-7",
            Self::Cadence7 => "market-cadence-7",
            Self::Units7 => "market-units-7",
            Self::Sales7 => "market-sales-7",
            Self::Vwap7 => "market-vwap-7",
            Self::LastSold => "market-last-sold",
            Self::Confidence => "market-confidence",
            Self::Units30 => "market-units-30",
            Self::Sales30 => "market-sales-30",
            Self::Vwap30 => "market-vwap-30",
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

    fn thirty_days(self) -> bool {
        matches!(self, Self::Units30 | Self::Sales30 | Self::Vwap30)
    }

    fn partial(self) -> bool {
        matches!(self, Self::Trend7 | Self::Drift7)
    }
}

const MARKET_METRICS: [MarketMetric; 22] = [
    MarketMetric::Subject,
    MarketMetric::Scope,
    MarketMetric::Quality,
    MarketMetric::World,
    MarketMetric::Datacenter,
    MarketMetric::Listing,
    MarketMetric::Minimum7,
    MarketMetric::Median7,
    MarketMetric::Average7,
    MarketMetric::SalesPerDay7,
    MarketMetric::Cadence7,
    MarketMetric::Units7,
    MarketMetric::Sales7,
    MarketMetric::Vwap7,
    MarketMetric::LastSold,
    MarketMetric::Confidence,
    MarketMetric::Units30,
    MarketMetric::Sales30,
    MarketMetric::Vwap30,
    MarketMetric::TrendWorld,
    MarketMetric::Trend7,
    MarketMetric::Drift7,
];

fn metric_label(metric: MarketMetric) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match metric {
        MarketMetric::Subject => t_string!(i18n, market_subject),
        MarketMetric::Scope => t_string!(i18n, market_scope),
        MarketMetric::Quality => t_string!(i18n, market_quality),
        MarketMetric::World => t_string!(i18n, market_world),
        MarketMetric::Datacenter => t_string!(i18n, market_datacenter),
        MarketMetric::Listing => t_string!(i18n, market_listing),
        MarketMetric::Minimum7 => t_string!(i18n, market_sale_min_7),
        MarketMetric::Median7 => t_string!(i18n, market_sale_median_7),
        MarketMetric::Average7 => t_string!(i18n, market_sale_avg_7),
        MarketMetric::SalesPerDay7 => t_string!(i18n, market_sales_per_day_7),
        MarketMetric::Cadence7 => t_string!(i18n, market_cadence_7),
        MarketMetric::Units7 => t_string!(i18n, market_units_7),
        MarketMetric::Sales7 => t_string!(i18n, market_sales_7),
        MarketMetric::Vwap7 => t_string!(i18n, market_vwap_7),
        MarketMetric::LastSold => t_string!(i18n, market_last_sold),
        MarketMetric::Confidence => t_string!(i18n, market_confidence),
        MarketMetric::Units30 => t_string!(i18n, market_units_30),
        MarketMetric::Sales30 => t_string!(i18n, market_sales_30),
        MarketMetric::Vwap30 => t_string!(i18n, market_vwap_30),
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
    if matches!(metric, MarketMetric::Confidence) {
        return match s.confidence {
            ConfidenceBand::Unknown => GridValue::Missing,
            band => GridValue::Text(format!("{band:?}")),
        };
    }
    if matches!(metric, MarketMetric::LastSold) {
        return chrono::DateTime::from_timestamp(s.last_sold_unix, 0)
            .filter(|_| s.last_sold_unix > 0)
            .map_or(GridValue::Missing, |time| {
                GridValue::Text(time.format("%Y-%m-%d %H:%M UTC").to_string())
            });
    }
    let positive = |v: i32| (v > 0).then_some(f64::from(v));
    number(match metric {
        MarketMetric::Minimum7 => positive(s.min_price),
        MarketMetric::Median7 => positive(s.median_price),
        MarketMetric::Average7 => positive(s.avg_price),
        MarketMetric::SalesPerDay7 => Some(f64::from(s.sales_per_day)),
        MarketMetric::Cadence7 => {
            (s.sales_per_day > 0.0).then(|| 24.0 / f64::from(s.sales_per_day))
        }
        MarketMetric::Units7 | MarketMetric::Units30 => Some(s.units_sold as f64),
        MarketMetric::Sales7 | MarketMetric::Sales30 => Some(s.num_sold as f64),
        MarketMetric::Vwap7 | MarketMetric::Vwap30 => positive(s.vwap),
        _ => None,
    })
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
        _ => {
            if if metric.thirty_days() {
                market.stats30_failed()
            } else {
                market.stats7_failed()
            } {
                return GridValue::Unavailable;
            }
            let stats = if metric.thirty_days() {
                market.stats30()
            } else {
                market.stats7()
            };
            match stats {
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
        GridValue::Number(n)
            if matches!(metric, MarketMetric::SalesPerDay7 | MarketMetric::Cadence7) =>
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
    market: MarketData,
    subject: Arc<dyn Fn(&T) -> MarketSubject + Send + Sync>,
    #[prop(optional)] metrics: Vec<GridMetric<T>>,
    #[prop(optional)] on_rows: Option<Callback<Vec<T>>>,
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
        for metric in MARKET_METRICS {
            if !result.iter().any(|col| col.id == metric.id()) {
                result.push(GridColumn::new(
                    metric.id(),
                    metric_label(metric),
                    160.0,
                    true,
                    false,
                ));
            }
        }
        result
    });
    let needs = Memo::new(move |_| {
        let mut wanted = query.with(|q| active_metric_columns(q.get("gf").as_deref()));
        query.with(|q| {
            if let Some(cols) = q.get("cols") {
                wanted.extend(cols.split(',').map(str::to_owned));
            }
            if let Some(sort) = q
                .get("sort")
                .and_then(|s| s.strip_prefix("grid:").map(str::to_owned))
            {
                wanted.insert(sort);
            }
        });
        for col in all_columns.get().iter().filter(|c| c.visible) {
            wanted.insert(col.id.to_string());
        }
        wanted
    });
    Effect::new(move |_| {
        if !market.want_30.get_untracked()
            && needs.with(|n| {
                MARKET_METRICS
                    .iter()
                    .any(|m| m.thirty_days() && n.contains(m.id()))
            })
        {
            market.want_30.set(true);
        }
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
    for metric in MARKET_METRICS {
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
        <QueryGrid each columns=all_columns key row_height visible_range=range id label metrics=all_metrics on_rows=handle_rows
            header=move |id| match MARKET_METRICS.into_iter().find(|m| m.id() == id) {
                Some(metric) => metric_label(metric).into_any(),
                None => native_header.with_value(|header| header(id)),
            }
            view=move |row: T, id| {
                let Some(metric) = MARKET_METRICS.into_iter().find(|m| m.id() == id) else {
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
                        format!("{}: {world}", metric_label(MarketMetric::TrendWorld))
                    } else { String::new() }
                }>{move || {
                    if matches!(metric, MarketMetric::Trend7)
                        && let Some(MarketSpark::Ready(value)) = sparks.with(|s| s.get(&spark_key(&subject, scope_world.get())).cloned()) {
                        return view! { <Sparkline points=value.points pct_change=value.delta_pct.unwrap_or_default() width=120 /> }.into_any();
                    }
                    display_value(metric, market_value(metric, &subject, market, sparks, scope_world, &worlds)).into_any()
                }}</div> }.into_any()
            }
            measure=move |row: &T, id| match MARKET_METRICS.into_iter().find(|m| m.id() == id) {
                Some(metric) => (display_value(metric, market_value(metric, &subject_measure(row), market, sparks, scope_world, &worlds_measure)), 24.0),
                None => native_measure.with_value(|measure| measure(row, id)),
            }
        />
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::cheapest_listings::CheapestListingData;

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
    fn shared_column_ids_are_unique() {
        let ids: std::collections::HashSet<_> = MARKET_METRICS.iter().map(|m| m.id()).collect();
        assert_eq!(ids.len(), MARKET_METRICS.len());
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
            ..Default::default()
        });
        assert_eq!(
            stats_value(MarketMetric::Units7, stats),
            GridValue::Number(140.0)
        );
        assert_eq!(
            stats_value(MarketMetric::Sales7, stats),
            GridValue::Number(14.0)
        );
        assert_eq!(
            stats_value(MarketMetric::Cadence7, stats),
            GridValue::Number(12.0)
        );
        assert_eq!(
            stats_value(MarketMetric::Median7, stats),
            GridValue::Missing
        );
    }
}
