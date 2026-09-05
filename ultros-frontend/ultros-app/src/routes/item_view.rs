use crate::api::{get_item_stats, get_listings, get_price_density, get_price_series};
use crate::components::app_link::AppLink;
use crate::components::chart_query::{
    RangeDecision, RangePreset, SaleProbe, decide_range, effective_preset,
};
use crate::components::confidence_badge::ConfidenceBadge;
use crate::components::freshness_badge::FreshnessBadge;
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::listing_filters::filter_listing_rows;
use crate::components::price_history_chart::PriceHistoryChart;
use crate::components::sales_cadence_badge::SalesCadenceBadge;
use crate::components::world_name::WorldName;
use crate::components::{
    ad::Ad, add_to_list::AddToList, clipboard::*, item_icon::*, item_tooltip::ItemTooltip,
    listings_panel::ListingsPanel, meta::*, realtime_status::RealtimeStatus,
    recently_viewed::RecentItems, related_items::*, sale_history_table::*, section_nav::SectionNav,
    skeleton::BoxSkeleton, stats_display::*, toggle::Toggle,
};
use crate::error::AppError;
use crate::global_state::LocalWorldData;
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::global_state::home_world::{get_price_zone, locale_preferred_region, use_home_world};
use crate::global_state::xiv_data::{resolve_item_id, tracked_data};
use crate::i18n::{t, t_string};
use crate::query_defaults::filter_query_signal;
use crate::routes::item_view_scope::{COMPARE_BUY_FROM_PARAM, item_href};
use crate::routes::not_found::NotFound;
use crate::script_escape::escape_for_script_tag;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use leptos::prelude::*;
use leptos_meta::Meta;
use leptos_router::hooks::{use_params_map, use_query_map};
use leptos_router::location::Url;
use leptos_use::signal_debounced;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use ultros_api_types::cheapest_listings::{CheapestListingData, PriceSummary};
use ultros_api_types::price_series::{HqFilter, SeriesGroup};
use ultros_api_types::websocket::{FilterPredicate, ServerClient, SocketMessageType};
use ultros_api_types::world::Datacenter;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::world_helper::{AnyResult, OwnedResult};
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer};
use ultros_charts::charts::ChartMode;
use ultros_charts::data::grouping::{GroupLevel, default_group_level};
use xiv_gen::{ItemId, ItemSearchCategoryId, ItemUiCategoryId};

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

const MEANINGFUL_CROSS_WORLD_SAVINGS_GIL: i32 = 1_000;

/// Applies `fun` to a reactive value that may already have been disposed,
/// falling back to `fallback` instead of panicking.
///
/// The `<Suspense>`/`<Transition>` bodies on this page are walked by tachys'
/// `dry_resolve` twice: once inline, and again from the detached
/// `Effect::new_isomorphic` leptos keeps alive until the boundary resolves.
/// That second walk can outlive the owner that created props like
/// `filtered_listings`, and `With::with` on a disposed signal panics.
///
/// A panic there aborts `to_html_async` partway through the body, so the
/// server ships a *truncated* document and the browser hydrates a half-written
/// DOM — the tachys `unreachable!()` flood in GlitchTip #6831. The request is
/// already being torn down whenever this fires, so degrading to `fallback`
/// costs nothing user-visible and keeps the response whole.
pub(crate) fn with_or<S, U>(signal: &S, fallback: U, fun: impl FnOnce(&S::Value) -> U) -> U
where
    S: With,
{
    signal.try_with(fun).unwrap_or(fallback)
}

/// [`Get`] counterpart to [`with_or`] for values that are cloned out anyway.
pub(crate) fn get_or_default<S>(signal: &S) -> S::Value
where
    S: Get,
    S::Value: Default,
{
    signal.try_get().unwrap_or_default()
}

#[component]
fn WorldButton(
    current_world: Memo<String>,
    #[prop(into)] world: OwnedResult,
    item_id: i32,
) -> impl IntoView {
    let (home_world, _) = use_home_world();
    let world_name = world.get_name().to_string();
    let label = world_name.clone();
    let query = use_query_map();
    // Only the params this route actually owns are carried forward, so a
    // stale or hostile query key can't be reflected back into a link.
    let search = Signal::derive(move || {
        query.with(|query| {
            carried_world_switch_query(
                query.get("exclude-worlds").as_deref(),
                query.get(COMPARE_BUY_FROM_PARAM).as_deref(),
            )
        })
    });
    let world_2 = world_name.clone();
    let world_3 = world_name.clone();
    let is_home_world = Signal::derive({
        move || {
            home_world
                .with(|w| w.as_ref().map(|w| w.name == world_2))
                .unwrap_or_default()
        }
    });
    // Sizing is always applied; the type color only when not selected, so it
    // can't fight the filled pill's text color on CSS specificity.
    let (size_styles, color_styles) = match world {
        OwnedResult::Region(_) => ("text-sm font-bold px-3 py-1.5", "text-brand-200"),
        OwnedResult::Datacenter(_) => ("text-sm font-semibold px-2.5 py-1", "text-brand-300"),
        OwnedResult::World(_) => ("text-xs px-2 py-1", "text-[color:var(--color-text)]"),
    };
    let is_selected = Signal::derive(move || current_world.with(|w| w == world_3.as_str()));
    let home_world_emphasis = move || {
        is_home_world.with(|w| {
            if *w && !is_selected.get() {
                "border border-brand-300/70"
            } else {
                ""
            }
        })
    };
    view! {
        <AppLink
            attr:class=move || {
                [
                    "rounded-md flex items-center gap-1.5 transition-colors duration-150 whitespace-nowrap border border-transparent",
                    size_styles,
                    if is_selected.get() {
                        // `!` important is required: the global anchor rule in
                        // style/tailwind.css
                        //   a:not(.nav-link):not(.btn):not(.btn-primary)...
                        // has specificity (0,5,1) and hard-sets
                        // `background-color: transparent` + `rounded-md`, which
                        // beats a plain (0,1,0) utility class. Same idiom as the
                        // analyzer tabs' `active_classes`.
                        "font-bold !rounded-full !bg-[color:var(--brand-bg)] !text-[color:var(--brand-fg)]"
                    } else {
                        color_styles
                    },
                    if is_selected.get() {
                        ""
                    } else {
                        "hover:border-[color:var(--color-outline)] hover:text-brand-100"
                    },
                    home_world_emphasis(),
                ]
                    .join(" ")
            }
                attr:aria-current=move || is_selected.get().then_some("page")
                href=move || search.with(|search| item_href(&world_name, item_id, search))
            >
                {move || {
                    is_home_world
                        .get()
                        .then(|| {
                            view! {
                                <Icon icon=icondata::AiHomeFilled attr:class="text-brand-200" />
                                <div class="w-1"></div>
                            }
                        })
                }}
                {label}
            </AppLink>
    }.into_any()
}

#[component]
fn HomeWorldButton(current_world: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    let (home_world, _) = use_home_world();
    home_world
        .get_untracked()
        .map(move |world| {
            view! { <WorldButton current_world world=AnyResult::World(&world) item_id=item_id() /> }
        })
        .into_any()
}

#[component]
fn WorldGrouping(
    region: OwnedResult,
    active_datacenter: Option<ultros_api_types::world::Datacenter>,
    current_world: Memo<String>,
    item_id: i32,
) -> impl IntoView {
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let datacenters = world_data.get_datacenters(&region.as_ref());
    let i18n = crate::i18n::use_i18n();
    view! {
        <div class="flex flex-col gap-2">
            <div class="flex flex-wrap items-center gap-2">
                <h2 class="text-xs font-bold text-brand-200 uppercase tracking-wide">
                    {t!(i18n, datacenter)}
                </h2>
                <div class="flex flex-wrap gap-1">
                    {datacenters
                        .iter()
                        .map(|dc| {
                            view! {
                                <WorldButton
                                    current_world=current_world
                                    world=AnyResult::Datacenter(dc)
                                    item_id=item_id
                                />
                            }
                        })
                        .collect_view()}
                </div>
            </div>
            {active_datacenter
                .map(|dc| {
                    view! {
                        <div class="flex flex-wrap items-center gap-2">
                            <h2 class="text-xs font-bold text-brand-200 uppercase tracking-wide">
                                {t!(i18n, worlds)}
                            </h2>
                            <div class="flex flex-wrap gap-1">
                                {dc
                                    .worlds
                                    .iter()
                                    .map(|w| {
                                        view! {
                                            <WorldButton
                                                current_world=current_world
                                                world=AnyResult::World(w)
                                                item_id=item_id
                                            />
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </div>
                    }
                })}
        </div>
    }
}

#[component]
fn WorldMenu(world_name: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    let current_world = world_name;
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let (home_world, _) = use_home_world();
    let i18n = crate::i18n::use_i18n();

    view! {
        <div class="border-y border-[color:var(--color-outline)]">
            <div class="w-full px-3 sm:px-4">
                <div class="flex flex-col gap-2 py-2">
                        {move || {
                            let world = world_name();
                            let world_name = Url::unescape(&world);
                            let preferred = locale_preferred_region(i18n.get_locale());
                            let ordered_regions = world_data.regions_ordered(preferred);
                            let all_regions = ordered_regions.into_iter().map(|r| {
                                view! {
                                    <WorldButton
                                        current_world=current_world
                                        world=AnyResult::Region(r)
                                        item_id=item_id()
                                    />
                                }
                            });
                            let selected_any_result = world_data.lookup_world_by_name(&world_name);
                            let region = if let Some(world) = selected_any_result {
                                world_data.get_region(world)
                            } else {
                                let region_result = world_data
                                    .lookup_world_by_name("North-America")
                                    .unwrap();
                                world_data.get_region(region_result)
                            };

                            let active_datacenter = if let Some(any_result) = selected_any_result {
                                match any_result {
                                    AnyResult::World(world) => world_data
                                        .get_datacenters(&AnyResult::World(world))
                                        .first()
                                        .map(|dc| (*dc).clone()),
                                    AnyResult::Datacenter(dc) => Some((*dc).clone()),
                                    AnyResult::Region(_) => None,
                                }
                            } else {
                                None
                            };

                            let home_world_in_region = home_world
                                .with_untracked(|home| {
                                    home
                                        .as_ref()
                                        .map(|home| {
                                            region
                                                .datacenters
                                                .iter()
                                                .any(|dc| dc.worlds.iter().any(|w| w.id == home.id))
                                        })
                                        .unwrap_or(true)
                                });

                            view! {
                                <div class="flex items-center gap-1 overflow-x-auto pb-1">
                                    {all_regions.collect_view()}
                                    {(!home_world_in_region)
                                        .then(|| {
                                            view! { <HomeWorldButton current_world item_id /> }
                                        })}
                                </div>
                                <div class="w-full h-px bg-[color:var(--color-outline)]"></div>
                                <WorldGrouping
                                    region=OwnedResult::Region(region.clone())
                                    active_datacenter
                                    current_world
                                    item_id=item_id()
                                />
                            }
                        }}
                    </div>
                </div>
            </div>
    }
    .into_any()
}

#[component]
pub fn DatacenterExclusionControls(
    world: Memo<String>,
    excluded_datacenters: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();

    let datacenters = Memo::new({
        let world_data = world_data.clone();
        move |_| {
            let world_name = Url::unescape(&world());
            let mut datacenters = world_data
                .lookup_world_by_name(&world_name)
                .map(|result| {
                    world_data
                        .get_datacenters(&result)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            // Keep an exclusion from a previously selected scope removable,
            // but render every datacenter exactly once.
            excluded_datacenters.with(|excluded| {
                for name in excluded {
                    let already_visible = datacenters
                        .iter()
                        .any(|datacenter| datacenter.name == *name);
                    if !already_visible
                        && let Some(datacenter) = world_data
                            .lookup_world_by_name(name)
                            .and_then(|result| result.as_datacenter())
                    {
                        datacenters.push(datacenter.clone());
                    }
                }
            });
            datacenters.sort_by(|a, b| a.name.cmp(&b.name));
            datacenters
        }
    });

    view! {
        {move || {
            let has_controls = datacenters.with(|datacenters| !datacenters.is_empty());
            has_controls.then(|| {
                view! {
                    <div
                        class="flex flex-wrap items-center gap-2"
                        role="group"
                        aria-label=move || t_string!(i18n, item_view_exclude_datacenters).to_string()
                    >
                            {move || {
                                datacenters
                                    .get()
                                    .into_iter()
                                    .map(|datacenter: Datacenter| {
                                        let name = datacenter.name.clone();
                                        let label_name = name.clone();
                                        let state_name = name.clone();
                                        let click_name = name.clone();
                                        let hook_name = name.clone();
                                        let is_excluded = Signal::derive(move || {
                                            excluded_datacenters.with(|set| set.contains(&state_name))
                                        });
                                        view! {
                                            <button
                                                type="button"
                                                data-datacenter=hook_name
                                                aria-pressed=move || is_excluded().to_string()
                                                aria-label=move || {
                                                    if is_excluded() {
                                                        t_string!(i18n, item_view_include_datacenter_aria, datacenter = label_name.clone()).to_string()
                                                    } else {
                                                        t_string!(i18n, item_view_exclude_datacenter_aria, datacenter = label_name.clone()).to_string()
                                                    }
                                                }
                                                class=move || {
                                                    [
                                                        "inline-flex min-h-10 items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[color:var(--brand-ring)]",
                                                        if is_excluded() {
                                                            "border-brand-300/60 bg-[color:var(--brand-bg)] font-semibold text-[color:var(--brand-fg)]"
                                                        } else {
                                                            "border-[color:var(--color-outline)] bg-[color:var(--color-background-elevated)] text-[color:var(--color-text)] hover:border-brand-300/60"
                                                        },
                                                    ]
                                                        .join(" ")
                                                }
                                                on:click=move |_| {
                                                    excluded_datacenters.update(|set| {
                                                        if !set.remove(&click_name) {
                                                            set.insert(click_name.clone());
                                                        }
                                                    });
                                                }
                                            >
                                                {move || {
                                                    is_excluded()
                                                        .then(|| view! { <Icon icon=icondata::MdiClose attr:class="text-sm" /> })
                                                }}
                                                <span>{name.clone()}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                            {move || {
                                (!excluded_datacenters.with(|set| set.is_empty()))
                                    .then(|| {
                                        view! {
                                            <button
                                                type="button"
                                                class="inline-flex min-h-10 items-center gap-1.5 rounded-md px-3 py-1.5 text-sm text-[color:var(--color-text-muted)] transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] hover:text-[color:var(--color-text)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[color:var(--brand-ring)]"
                                                data-testid="clear-datacenter-exclusions"
                                                on:click=move |_| {
                                                    excluded_datacenters.update(|set| set.clear());
                                                }
                                            >
                                                <Icon icon=icondata::MdiClose attr:class="text-sm" />
                                                {t!(i18n, clear_all)}
                                            </button>
                                        }
                                    })
                            }}
                    </div>
                }
            })
        }}
    }
    .into_any()
}

fn cheapest_listing_for_quality(
    listings: &ListingRows,
    hq: bool,
) -> Option<(ActiveListing, Arc<Retainer>)> {
    listings
        .iter()
        .filter(|(listing, _)| listing.hq == hq)
        .min_by_key(|(listing, _)| listing.price_per_unit)
        .cloned()
}

/// Cross-world savings hint derived from the zone-wide cheapest map.
///
/// Replaces the listings-payload `SavingsVerdict`: a world-scoped listings
/// request only contains that world (world_cache.rs `get_all_worlds_in`),
/// so the old cross-world comparison could never fire.
#[derive(Clone, Debug, PartialEq)]
struct ZoneSavings {
    cheapest: CheapestListingData,
    hq: bool,
    savings: i32,
    savings_percent: f64,
}

fn zone_savings_for_quality(
    local_floor: Option<i32>,
    zone_cheapest: Option<CheapestListingData>,
    hq: bool,
    current_world_id: i32,
) -> Option<ZoneSavings> {
    let local = local_floor?;
    let cheapest = zone_cheapest?;
    if cheapest.world_id == current_world_id || cheapest.price <= 0 || local <= 0 {
        return None;
    }
    let savings = local - cheapest.price;
    if savings < MEANINGFUL_CROSS_WORLD_SAVINGS_GIL {
        return None;
    }
    Some(ZoneSavings {
        cheapest,
        hq,
        savings,
        savings_percent: (savings as f64 / local as f64) * 100.0,
    })
}

fn zone_savings(
    local_floor_nq: Option<i32>,
    local_floor_hq: Option<i32>,
    summary: &PriceSummary,
    current_world_id: i32,
) -> Option<ZoneSavings> {
    [
        zone_savings_for_quality(local_floor_nq, summary.lq, false, current_world_id),
        zone_savings_for_quality(local_floor_hq, summary.hq, true, current_world_id),
    ]
    .into_iter()
    .flatten()
    .max_by_key(|savings| savings.savings)
}

fn format_savings_percent(percent: f64) -> String {
    if percent >= 10.0 {
        format!("{percent:.0}")
    } else {
        format!("{percent:.1}")
    }
}

/// Builds the query string a world-switch link carries forward: only the
/// params this route owns are allowed through, so a stale or hostile query
/// key can't be reflected back into a link (same allowlist idiom as
/// `parse_excluded_world_ids`). `compare-buy-from` is included so clicking a
/// world button doesn't silently dismiss an open flip-verification card —
/// the spec requires "changing the sell world keeps the comparison alive".
fn carried_world_switch_query(
    exclude_worlds: Option<&str>,
    compare_buy_from: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(worlds) = exclude_worlds
        && !worlds.is_empty()
    {
        parts.push(format!("exclude-worlds={}", Url::escape(worlds)));
    }
    if let Some(buy_from) = compare_buy_from
        && !buy_from.is_empty()
    {
        parts.push(format!(
            "{COMPARE_BUY_FROM_PARAM}={}",
            Url::escape(buy_from)
        ));
    }
    parts.join("&")
}

fn parse_excluded_world_ids(raw: Option<&str>) -> HashSet<i32> {
    raw.unwrap_or_default()
        .split(',')
        .filter_map(|world| world.trim().parse::<i32>().ok())
        .collect()
}

#[component]
fn DecisionHeader(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let cheapest_prices = use_context::<CheapestPrices>();
    let (compare_world, set_compare_world) = filter_query_signal::<String>(COMPARE_BUY_FROM_PARAM);

    // Same idiom as `MarketStatsPanel` (see the long comment above it): the
    // zone-cheapest resource must read as unavailable during SSR and the
    // initial hydration render, or the SSR/CSR DOM shapes mismatch and
    // tachys panics.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });

    view! {
        <Transition fallback=move || view! { <BoxSkeleton /> }>
            {move || {
                listing_resource
                    .with(|data_ref| {
                        if let Some(Ok(data)) = data_ref.as_ref() {
                            let listings = get_or_default(&filtered_listings);
                            let scope = {
                                let world_name = Url::unescape(&world());
                                world_data.lookup_world_by_name(&world_name)
                            };
                            let current_world_id = scope
                                .as_ref()
                                .and_then(|result| result.as_world().map(|world| world.id));
                            // Number of worlds the page selector covers (1 on a
                            // world page, ~8 on a DC, more on a region).
                            let world_count = scope
                                .as_ref()
                                .map(|result| result.all_worlds().count())
                                .unwrap_or(1);
                            let savings = current_world_id.and_then(|world_id| {
                                let local_floor = |hq: bool| {
                                    listings
                                        .iter()
                                        .filter(|(listing, _)| {
                                            listing.hq == hq && listing.world_id == world_id
                                        })
                                        .map(|(listing, _)| listing.price_per_unit)
                                        .min()
                                };
                                let summary = if hydrated.get() {
                                    cheapest_prices.as_ref().and_then(|prices| {
                                        prices.read_listings.with(|r| {
                                            let map = r.as_ref().and_then(|r| r.as_ref().ok());
                                            map.map(|map| map.find_matching_listings(item_id()))
                                        })
                                    })
                                } else {
                                    None
                                };
                                summary.and_then(|summary| {
                                    zone_savings(local_floor(false), local_floor(true), &summary, world_id)
                                })
                            });
                            let recent_sales = &data.sales;

                            // Freshness is judged on when Ultros last ingested the
                            // board (`last_updated`), not on the sellers' re-list
                            // times carried by `ActiveListing::timestamp`.
                            let freshness_inputs = crate::freshness::derive_freshness_inputs(
                                &data.last_updated,
                                recent_sales,
                                world_count,
                                chrono::Utc::now().naive_utc(),
                            );
                            let age = freshness_inputs.age;

                            let freshness_verdict = ultros_api_types::freshness::calculate_freshness_verdict(
                                age,
                                freshness_inputs.per_world_sales_per_day,
                            );
                            // The cadence badge describes the whole scope, so it
                            // keeps the unnormalized velocity.
                            let scope_sales_per_day = freshness_inputs
                                .scope_sales_per_day
                                .unwrap_or_default();
                            let cadence_verdict = crate::analysis::get_sales_cadence(
                                scope_sales_per_day,
                                recent_sales.len(),
                            );

                            view! {
                                <div class="flex flex-col gap-3 mb-4">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <FreshnessBadge verdict=freshness_verdict age=age />
                                        <SalesCadenceBadge
                                            cadence=cadence_verdict
                                            sales_per_day=scope_sales_per_day
                                        />
                                    </div>
                                    {savings
                                        .and_then(|savings| {
                                            let buy_world_name = world_data
                                                .lookup_selector(AnySelector::World(savings.cheapest.world_id))
                                                .map(|w| w.get_name().to_string())?;
                                            // Don't advertise the Compare card when it's
                                            // already open for this world.
                                            let already_open = compare_world
                                                .get()
                                                .map(|raw| Url::unescape(&raw))
                                                .is_some_and(|current| {
                                                    current.eq_ignore_ascii_case(&buy_world_name)
                                                });
                                            if already_open {
                                                return None;
                                            }
                                            Some((savings, buy_world_name))
                                        })
                                        .map(|(savings, buy_world_name)| {
                                            let quality_label = if savings.hq {
                                                t_string!(i18n, hq).to_string()
                                            } else {
                                                t_string!(i18n, nq).to_string()
                                            };
                                            let percent = format_savings_percent(savings.savings_percent);
                                            let cheapest_world_id = savings.cheapest.world_id;
                                            let cheapest_price = savings.cheapest.price;
                                            let saved_amount = savings.savings;
                                            view! {
                                                <button
                                                    type="button"
                                                    class="flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg border border-emerald-400/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-100 transition-colors hover:border-emerald-300/70"
                                                    on:click=move |_| {
                                                        set_compare_world.set(Some(buy_world_name.clone()));
                                                    }
                                                >
                                                    <Icon icon=icondata::FaGlobeSolid attr:class="text-sm shrink-0" />
                                                    <span class="font-semibold">
                                                        {t!(i18n, item_view_savings_cheapest_on)}
                                                    </span>
                                                    <span class="inline-flex items-center gap-1">
                                                        <WorldName id=AnySelector::World(cheapest_world_id) />
                                                        <span class="rounded border border-emerald-300/40 px-1 text-[10px] font-bold leading-4 text-emerald-100">
                                                            {quality_label}
                                                        </span>
                                                    </span>
                                                    <span class="text-[color:var(--color-text-muted)]">":"</span>
                                                    <div class="font-bold text-[color:var(--color-text)]">
                                                        <Gil amount=cheapest_price />
                                                    </div>
                                                    <span class="text-[color:var(--color-text-muted)]">"-"</span>
                                                    <span>{t!(i18n, item_view_savings_save)}</span>
                                                    <div class="font-bold text-[color:var(--color-text)]">
                                                        <Gil amount=saved_amount />
                                                    </div>
                                                    <span class="text-[color:var(--color-text-muted)]">
                                                        "("{percent}"%)"
                                                    </span>
                                                    <span class="font-semibold underline">
                                                        {t!(i18n, item_compare_action)}
                                                    </span>
                                                </button>
                                            }
                                            .into_any()
                                        })
                                        .unwrap_or_else(|| ().into_any())}
                                </div>
                            }
                            .into_any()
                        } else {
                            ().into_any()
                        }
                    })
            }}
        </Transition>
    }
    .into_any()
}

#[component]
fn MarketStatsPanel(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    item_id: Memo<i32>,
    realtime_status: Signal<String>,
    last_update_at: Signal<Option<chrono::DateTime<chrono::Utc>>>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();

    view! {
        <Transition fallback=move || view! { <BoxSkeleton /> }>
            {move || {
                listing_resource
                    .with(|data_ref| {
                        if let Some(Ok(data)) = data_ref.as_ref() {
                            let data = data.clone();
                            let listings = get_or_default(&filtered_listings);
                            let cheapest_nq = cheapest_listing_for_quality(&listings, false);
                            let cheapest_hq = cheapest_listing_for_quality(&listings, true);
                            let listings_count = listings.len();
                            let recent_sales = data.sales.clone();
                            let avg_price = if recent_sales.is_empty() {
                                None
                            } else {
                                Some(
                                    recent_sales
                                        .iter()
                                        .map(|sale| sale.price_per_item as i64)
                                        .sum::<i64>() as i32
                                        / recent_sales.len() as i32,
                                )
                            };
                            let median_price = if recent_sales.is_empty() {
                                None
                            } else {
                                let mut prices = recent_sales
                                    .iter()
                                    .map(|sale| sale.price_per_item)
                                    .collect::<Vec<_>>();
                                let len = prices.len();
                                // ⚡ Bolt: Optimization: Use select_nth_unstable instead of sort_unstable for median calculation.
                                let (_, &mut median, _) = prices.select_nth_unstable(len / 2);
                                Some(median)
                            };
                            let vendor_price = tracked_data()
                                .items
                                .get(&ItemId(item_id()))
                                .map(|item| item.price_mid as i32)
                                .filter(|p| *p > 0);

                            let real = crate::analysis::real_price(
                                &recent_sales
                                    .iter()
                                    .map(|s| (s.price_per_item, s.quantity, s.hq))
                                    .collect::<Vec<_>>(),
                                vendor_price,
                            );
                            let real_primary = real.primary();
                            let real_secondary = real.secondary();
                            let sales_cadence = if recent_sales.len() > 1 {
                                let newest = recent_sales.first().unwrap().sold_date;
                                let oldest = recent_sales.last().unwrap().sold_date;
                                let seconds = (newest - oldest).num_seconds().abs();
                                let count = recent_sales.len() - 1;

                                if seconds > 0 {
                                    let seconds_per_sale = seconds as f64 / count as f64;
                                    if seconds_per_sale < 60.0 {
                                        t!(i18n, sells_per_minute, count = format!("{:.1}", 60.0 / seconds_per_sale)).into_any()
                                    } else if seconds_per_sale < 3600.0 {
                                        t!(i18n, sells_per_hour, count = format!("{:.1}", 3600.0 / seconds_per_sale)).into_any()
                                    } else if seconds_per_sale < 86400.0 {
                                        t!(i18n, sells_per_day, count = format!("{:.1}", 86400.0 / seconds_per_sale)).into_any()
                                    } else {
                                        t!(i18n, sells_every_days, count = format!("{:.1}", seconds_per_sale / 86400.0)).into_any()
                                    }
                                } else {
                                    t!(i18n, very_high_frequency).into_any()
                                }
                            } else {
                                t!(i18n, not_enough_data).into_any()
                            };


                            view! {
                                <div class="flex flex-col rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4">
                                    <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1 mb-1.5">
                                        <h2 class="text-lg sm:text-xl font-bold text-[color:var(--color-text)] leading-tight">
                                            {t!(i18n, cheapest_found)}
                                        </h2>
                                        <RealtimeStatus
                                            status=realtime_status
                                            last_update=last_update_at
                                        />
                                        <p class="text-sm text-[color:var(--color-text-muted)]">
                                            {move || t!(i18n, based_on_sales, count = recent_sales.len())}
                                        </p>
                                    </div>

                                    // Flat stat strip: 2x2 grid with hairline separators on
                                    // mobile, one row of 4 with left dividers at lg+.
                                    <div class="grid grid-cols-2 lg:grid-cols-4 [&>a]:border-[color:var(--color-outline)] [&>a:nth-child(even)]:border-l lg:[&>a:not(:first-child)]:border-l [&>a:nth-child(n+3)]:border-t lg:[&>a]:border-t-0">
                                        <a href="#listings" class="px-3 py-1.5 sm:px-4 transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]">
                                            <div class="text-xs font-bold uppercase text-brand-300 mb-1">{t!(i18n, nq)}</div>
                                            {if let Some((listing, _)) = cheapest_nq.clone() {
                                                view! {
                                                    <div>
                                                        <div class="text-lg sm:text-xl font-bold leading-none"><Gil amount=listing.price_per_unit /></div>
                                                        <div class="text-xs text-[color:var(--color-text-muted)] mt-1 flex items-center gap-1">
                                                            <Icon icon=icondata::FaGlobeSolid attr:class="text-[10px]" />
                                                            <WorldName id=AnySelector::World(listing.world_id) />
                                                        </div>
                                                    </div>
                                                }
                                                .into_any()
                                            } else {
                                                view! { <div class="text-base sm:text-lg text-[color:var(--color-text-muted)]">{t!(i18n, no_data)}</div> }.into_any()
                                            }}
                                        </a>

                                        <a href="#listings" class="px-3 py-1.5 sm:px-4 transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]">
                                            <div class="text-xs font-bold uppercase text-[#95c521] mb-1 flex items-center gap-1">
                                                <Icon icon=icondata::FaStarSolid attr:class="text-[10px]" />
                                                {t!(i18n, hq)}
                                            </div>
                                            {if let Some((listing, _)) = cheapest_hq.clone() {
                                                view! {
                                                    <div>
                                                        <div class="text-lg sm:text-xl font-bold leading-none"><Gil amount=listing.price_per_unit /></div>
                                                        <div class="text-xs text-[color:var(--color-text-muted)] mt-1 flex items-center gap-1">
                                                            <Icon icon=icondata::FaGlobeSolid attr:class="text-[10px]" />
                                                            <WorldName id=AnySelector::World(listing.world_id) />
                                                        </div>
                                                    </div>
                                                }
                                                .into_any()
                                            } else {
                                                view! { <div class="text-base sm:text-lg text-[color:var(--color-text-muted)]">{t!(i18n, no_data)}</div> }.into_any()
                                            }}
                                        </a>

                                        <a href="#history" class="px-3 py-1.5 sm:px-4 transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]">
                                            <div class="text-xs font-bold uppercase text-blue-300 mb-1 flex items-center gap-1">
                                                {t!(i18n, real_price)}
                                                {real_primary
                                                    .map(|(is_hq, _)| {
                                                        if is_hq {
                                                            view! { <span class="text-[10px] text-[color:var(--color-text-muted)]">{t!(i18n, hq)}</span> }.into_any()
                                                        } else {
                                                            view! { <span class="text-[10px] text-[color:var(--color-text-muted)]">{t!(i18n, nq)}</span> }.into_any()
                                                        }
                                                    })
                                                    .unwrap_or_else(|| ().into_any())}
                                            </div>
                                            <div class="text-lg sm:text-xl font-bold leading-none">
                                                {match real_primary {
                                                    Some((_, est)) => view! { <Gil amount=est.value /> }.into_any(),
                                                    None => view! { <span class="text-[color:var(--color-text-muted)]">{t!(i18n, no_data)}</span> }.into_any(),
                                                }}
                                            </div>
                                            {match real_secondary {
                                                Some((is_hq, est)) => {
                                                    let tag = if is_hq {
                                                        view! { <span class="font-semibold">{t!(i18n, hq)}</span> }.into_any()
                                                    } else {
                                                        view! { <span class="font-semibold">{t!(i18n, nq)}</span> }.into_any()
                                                    };
                                                    view! {
                                                        <div class="text-xs text-[color:var(--color-text-muted)] mt-1 flex items-center gap-1">
                                                            {tag}
                                                            <Gil amount=est.value />
                                                        </div>
                                                    }
                                                    .into_any()
                                                }
                                                None => ().into_any(),
                                            }}
                                            <div class="text-[10px] text-[color:var(--color-text-muted)] mt-1">
                                                {match real_primary {
                                                    Some((_, est)) => {
                                                        view! {
                                                            <span>
                                                                {t!(i18n, real_price_basis, used = est.used, total = est.total, excluded = est.excluded)}
                                                                " · "
                                                            </span>
                                                        }
                                                        .into_any()
                                                    }
                                                    None => ().into_any(),
                                                }}
                                                {t!(i18n, recent_average)}
                                                " "
                                                {avg_price
                                                    .map(|price| view! { <Gil amount=price /> }.into_any())
                                                    .unwrap_or_else(|| view! { <span>{t!(i18n, no_data)}</span> }.into_any())}
                                                " · "
                                                {t!(i18n, median_label)}
                                                " "
                                                {median_price
                                                    .map(|price| view! { <Gil amount=price /> }.into_any())
                                                    .unwrap_or_else(|| view! { <span>{t!(i18n, no_data)}</span> }.into_any())}
                                            </div>
                                        </a>

                                        <a href="#listings" class="px-3 py-1.5 sm:px-4 transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]">
                                            <div class="text-xs font-bold uppercase text-emerald-300 mb-1">{t!(i18n, active_listings)}</div>
                                            <div class="text-lg sm:text-xl font-bold leading-none">{listings_count}</div>
                                            <div class="text-xs text-[color:var(--color-text-muted)] mt-1">
                                                {sales_cadence}
                                            </div>
                                        </a>
                                    </div>

                                    <div class="mt-2 flex flex-wrap items-center gap-2" class:hidden={move || listings_count > 0}>
                                        {if listings_count == 0 {
                                            view! {
                                                <div role="status" class="rounded-lg border border-amber-500/40 px-3 py-2 text-sm text-amber-200">
                                                    {move || t_string!(i18n, no_active_listings_found).to_string()}
                                                </div>
                                            }
                                            .into_any()
                                        } else {
                                            ().into_any()
                                        }}
                                    </div>
                                </div>
                            }
                            .into_any()
                        } else {
                            ().into_any()
                        }
                    })
            }}
        </Transition>
    }
    .into_any()
}

/// Per-world share of the currently listed quantity. Only rendered when the
/// selected scope is a datacenter or region — on a single world there is
/// nothing to compare.
#[component]
fn WorldMarketShare(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    world: Memo<String>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    view! {
        <Transition fallback=move || ()>
            {move || {
                // Suspend on `listing_resource` here too (see ListingsPanel) so the
                // server and the hydrating client agree on the rendered structure —
                // the tachys hydration mismatch behind GlitchTip #6831.
                if !listing_resource.with(|r| matches!(r, Some(Ok(_)))) {
                    return ().into_any();
                }
                let is_multi_world = world.with(|w| {
                    world_data
                        .lookup_world_by_name(&Url::unescape(w))
                        .map(|scope| scope.as_world().is_none())
                        .unwrap_or(false)
                });
                if !is_multi_world {
                    return ().into_any();
                }
                let shares = Memo::new(move |_| {
                    let mut per_world: HashMap<i32, (u64, usize)> = HashMap::new();
                    with_or(&filtered_listings, (), |listings| {
                        for (listing, _) in listings {
                            let entry = per_world.entry(listing.world_id).or_default();
                            entry.0 += listing.quantity.max(0) as u64;
                            entry.1 += 1;
                        }
                    });
                    let mut rows: Vec<(i32, u64, usize)> = per_world
                        .into_iter()
                        .map(|(world_id, (quantity, listings))| (world_id, quantity, listings))
                        .collect();
                    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                    rows
                });
                view! {
                    <div
                        class="rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4"
                        class:hidden=move || shares.with(|s| s.is_empty())
                    >
                        <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1 mb-1.5">
                            <h2 class="text-lg sm:text-xl font-bold text-[color:var(--color-text)] leading-tight">
                                {t!(i18n, market_share_title)}
                            </h2>
                            <p class="text-sm text-[color:var(--color-text-muted)]">
                                {t!(i18n, market_share_subtitle)}
                            </p>
                        </div>
                        <div class="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-1">
                            {move || {
                                let rows = shares.get();
                                let max_quantity = rows.iter().map(|(_, q, _)| *q).max().unwrap_or(0).max(1);
                                rows.into_iter()
                                    .map(|(world_id, quantity, listings)| {
                                        let percent = quantity as f64 / max_quantity as f64 * 100.0;
                                        view! {
                                            <div class="flex items-center gap-2 text-xs">
                                                <span class="w-24 shrink-0 truncate">
                                                    <WorldName id=AnySelector::World(world_id) />
                                                </span>
                                                <div class="flex-1 h-2.5 rounded-full bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]">
                                                    <div
                                                        class="h-full rounded-full bg-[color:var(--brand-ring)]/70"
                                                        style:width=format!("{percent:.1}%")
                                                    ></div>
                                                </div>
                                                <span class="w-32 shrink-0 text-right text-[color:var(--color-text-muted)]">
                                                    {t!(i18n, market_share_row, quantity = quantity, listings = listings)}
                                                </span>
                                            </div>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>
                }
                    .into_any()
            }}
        </Transition>
    }
    .into_any()
}

/// The sale-history panel's loading state: a placeholder title/badge row, a
/// toggle+button row, and a large block standing in for the price chart.
///
/// Mirrors the panel `ChartWrapper` renders once its data arrives — same
/// `panel h-[26rem]` frame — so the swap from loading to loaded is a content
/// change, not a layout jump.
#[component]
fn ChartWrapperSkeleton() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        <div class="panel h-[26rem] flex flex-col gap-3 p-3 sm:p-4" role="status">
            <div class="skeleton-shimmer flex flex-col gap-3 h-full flex-1 min-h-0" aria-hidden="true">
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div class="flex flex-col gap-2">
                        <div class="flex items-center gap-2">
                            <div class="skeleton-block h-5 w-32 rounded"></div>
                            <div class="skeleton-block h-4 w-16 rounded-full"></div>
                        </div>
                        <div class="skeleton-block h-3 w-40 rounded"></div>
                    </div>
                    <div class="flex items-center gap-2">
                        <div class="skeleton-block h-6 w-24 rounded-lg"></div>
                        <div class="skeleton-block h-6 w-20 rounded-lg"></div>
                    </div>
                </div>
                <div class="skeleton-block flex-1 w-full rounded-lg"></div>
            </div>
            <span class="sr-only">{t!(i18n, loading)}</span>
        </div>
    }
}

#[component]
pub fn ChartWrapper(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    item_id: Memo<i32>,
    world: Memo<String>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    // `?hq=true`, absent means off. Only written when true, so the default
    // never appears in the URL.
    let (hq_param, set_hq_param) = filter_query_signal::<bool>("hq");
    let hq_only = Signal::derive(move || hq_param.get().unwrap_or(false));
    let set_hq_only = SignalSetter::map(move |on: bool| {
        set_hq_param.set(on.then_some(true));
    });

    // Per-item analyzer stats (ClickHouse-backed). LocalResource = client-
    // only — the badge isn't part of SSR output, so we avoid a hydration
    // mismatch when the resource resolves at different times on server vs
    // client. Soft-fails: if the endpoint errors or returns no variant,
    // the badge simply doesn't render and the rest of the chart works.
    let item_stats_resource = LocalResource::new(move || {
        let id = item_id();
        let w = world();
        async move { get_item_stats(&w, id).await }
    });

    // Chart data now comes pre-bucketed from ClickHouse rather than a raw
    // sale pull re-bucketed in the browser (see `ultros_api_types::price_series`
    // doc comment). `group`/`hq` mirror the chart's own controls so the
    // request always matches what's on screen; `selected_range` is the
    // timeline slicer's committed selection (`None` = full history).
    // Grouping is a derived read over `?group=`, not a signal: an absent
    // param means "the broadest level this scope offers", computed at read
    // time. That gives a region page region lines instead of ~70 world lines
    // (it used to hardcode World, which is valid at every scope, so the
    // corrective Effect in the chart never fired).
    //
    // Filtering by the scope's available levels means a shared `?group=region`
    // link opened on a *world* page degrades to World rather than requesting
    // a grouping the scope cannot serve. Deriving rather than seeding also
    // means navigating region -> world needs no write and cannot lose a race
    // with the world picker's mount-time rebuild.
    let (group_param, set_group_param) = filter_query_signal::<GroupLevel>("group");
    let group_helper = world_data.clone();
    let group_default_helper = world_data.clone();
    let group = Signal::derive(move || {
        let scope = world.get();
        group_param
            .get()
            .filter(|level| {
                ultros_charts::data::grouping::available_group_levels(&group_helper, &scope)
                    .contains(level)
            })
            .unwrap_or_else(|| default_group_level(&group_default_helper, &scope))
    });
    let set_group = SignalSetter::map(move |level: GroupLevel| {
        set_group_param.set(Some(level));
    });
    // `?mode=`, absent means Price. Deriving rather than seeding keeps the
    // URL clean until the user actually picks a mode, and means a shared
    // link and a fresh visit agree on what the chart shows. Mode switches
    // never touch the time window or grouping -- spec: "switching mode
    // preserves the time window and grouping".
    let (mode_param, set_mode_param) = filter_query_signal::<ChartMode>("mode");
    let mode = Signal::derive(move || mode_param.get().unwrap_or_default());
    let set_mode = SignalSetter::map(move |next: ChartMode| {
        set_mode_param.set(Some(next));
    });
    let hq = Signal::derive(move || {
        if hq_only.get() {
            HqFilter::Hq
        } else {
            HqFilter::Any
        }
    });
    // The time window has two URL shapes. A preset click writes `?range=1mo`,
    // so the link keeps meaning "the last month" indefinitely; a slicer drag
    // has no relative meaning, so it writes absolute `?from=&to=` epoch
    // seconds. `decide_range` applies the precedence, and with neither
    // shape present the window defaults dynamically from the item's newest
    // sale (see `sale_probe_state` below).
    let (range_param, set_range_param) = filter_query_signal::<RangePreset>("range");
    let (from_param, set_from_param) = filter_query_signal::<i64>("from");
    let (to_param, set_to_param) = filter_query_signal::<i64>("to");

    // Resolved once per mount rather than continuously: a chart does not
    // need to slide in real time, and re-resolving on every tick would
    // refetch. Computed during SSR too (this is just a component body), but
    // nothing SSR-rendered ever consumes it: `selected_range` only reaches
    // `debounced_decision` -> a client-only `LocalResource`, and
    // `selected_domain`, which short-circuits on `available_domain` --
    // itself derived from a client-only `LocalResource` and additionally
    // gated behind `<Show when=available_domain.is_some()>`.
    let now = StoredValue::new(chrono::Utc::now().timestamp());

    // Newest-sale probe for the dynamic default range: with no range params
    // in the URL, a hot item (sold within the last week) defaults to the
    // week window instead of full history. The probe reads the listings
    // payload the page already fetches, so deciding costs no extra request.
    //
    // Latched per (item, world): once the first payload answers, realtime
    // sale events prepended into `listing_resource` must not re-run the
    // decision — a live sale on a rarely-traded item would otherwise
    // suddenly narrow a full-history chart to one week mid-view. The
    // latched arm reads only the identity signals, so later resource
    // updates don't even re-run the memo until the identity changes.
    let sale_probe_state = Memo::new(move |prev: Option<&(i32, String, SaleProbe)>| {
        let key = (item_id.get(), world.get());
        if let Some((prev_item, prev_world, SaleProbe::Known(newest))) = prev
            && *prev_item == key.0
            && *prev_world == key.1
        {
            return (key.0, key.1, SaleProbe::Known(*newest));
        }
        // `with_or`, not `with`: this memo is created during SSR too, and
        // `With::with` panics on a disposed signal — the truncated-response
        // failure the helper's own docs describe. `Pending` is the right
        // degradation, since nothing SSR-rendered consumes the probe.
        let probe = with_or(&listing_resource, SaleProbe::Pending, |value| match value {
            Some(Ok(data)) => SaleProbe::Known(
                data.sales
                    .iter()
                    .map(|sale| sale.sold_date.and_utc().timestamp())
                    .max(),
            ),
            // A failed listings fetch must not leave the chart waiting
            // forever — fall back to the full-history default.
            Some(Err(_)) => SaleProbe::Known(None),
            None => SaleProbe::Pending,
        });
        (key.0, key.1, probe)
    });
    let sale_probe = Signal::derive(move || sale_probe_state.get().2);

    // What the chart should fetch: explicit URL params win, the dynamic
    // default fills their absence, and `Pending` holds the fetch until the
    // probe answers — fetching full history first and narrowing after would
    // flash exactly the misleading view this default exists to avoid.
    let range_decision = Signal::derive(move || {
        let from_to = from_param.get().zip(to_param.get());
        decide_range(
            range_param.get(),
            from_to,
            sale_probe.get(),
            now.get_value(),
        )
    });
    let selected_range = Signal::derive(move || match range_decision.get() {
        RangeDecision::Resolved(range) => range,
        RangeDecision::Pending => None,
    });
    // The preset button that should render pressed — `?range=` when set,
    // else whatever the dynamic default landed on.
    let chart_preset = Signal::derive(move || {
        let from_to = from_param.get().zip(to_param.get());
        effective_preset(
            range_param.get(),
            from_to,
            sale_probe.get(),
            now.get_value(),
        )
    });

    // A drag commits absolute bounds and clears any preset. Writing all
    // three together keeps the two shapes from coexisting in one URL.
    // ("All" no longer comes through here — it is an explicit preset now,
    // so it goes through `set_range_preset` below.)
    let set_selected_range = Callback::new(move |next: Option<(i64, i64)>| {
        set_range_param.set(None);
        match next {
            Some((from, to)) => {
                set_from_param.set(Some(from));
                set_to_param.set(Some(to));
            }
            None => {
                set_from_param.set(None);
                set_to_param.set(None);
            }
        }
    });

    // Selecting a preset clears the absolute bounds for the same reason.
    let set_range_preset = Callback::new(move |preset: Option<RangePreset>| {
        set_from_param.set(None);
        set_to_param.set(None);
        set_range_param.set(preset);
    });

    // A different item/world makes any absolute-timestamp selection from the
    // previous item meaningless (and possibly outside the new item's data
    // entirely) — drop back to full range before the next request goes out.
    // `range` deliberately survives: a relative preset like `?range=1mo`
    // means "the last month" and stays just as meaningful on the new
    // item/world, so clearing it here would silently downgrade a scope
    // switch into a full-history refetch. Also does *not* track
    // `group`/`hq`: changing those shouldn't discard an in-progress zoom.
    //
    // Guarded on an actual change (not just the first run): `Effect::new`
    // fires unconditionally on mount, and now that these setters write
    // straight to the URL, an unguarded first run would strip `from`/`to`
    // out of a freshly-loaded shared link before its first fetch even
    // finishes.
    Effect::new(move |prev: Option<(i32, String)>| {
        let key = (item_id.get(), world.get());
        if prev.is_some_and(|p| p != key) {
            set_from_param.set(None);
            set_to_param.set(None);
        }
        key
    });
    // Debounce so dragging a slicer handle fires one request after the drag
    // settles rather than one per pointer move; the slicer's own handle
    // rendering reads the undebounced `selected_range` so it still tracks
    // the pointer at full rate.
    let debounced_decision = signal_debounced(range_decision, 300.0);

    // LocalResource = client-only, same rationale as `item_stats_resource`
    // above: avoids a hydration mismatch when the fetch resolves at
    // different times on server vs. client. Resolves to `None` (no request
    // sent) while the range decision is still pending on the sale probe.
    let series_resource = LocalResource::new(move || {
        let id = item_id.get();
        let world_name = world.get();
        let series_group = SeriesGroup::from(group.get());
        let hq_filter = hq.get();
        let decision = debounced_decision.get();
        async move {
            match decision {
                RangeDecision::Pending => None,
                RangeDecision::Resolved(range) => {
                    Some(get_price_series(id, &world_name, series_group, hq_filter, range).await)
                }
            }
        }
    });
    let series = Signal::derive(move || series_resource.get().flatten().and_then(|r| r.ok()));

    // Fetched only while density mode is active — the mode is the gate, so
    // flipping to Density triggers the fetch and every other mode costs
    // nothing. Same LocalResource/hydration rationale as series_resource.
    let density_resource = LocalResource::new(move || {
        let active = mode.get() == ChartMode::Density;
        let id = item_id.get();
        let world_name = world.get();
        let hq_filter = hq.get();
        // With no slicer selection, bound the request to the domain the
        // series response reported (its `to` is the last bucket's *start*,
        // so extend one bucket width to keep the newest sales). An unbounded
        // request would make the server derive its bucket from the default
        // multi-year window, yielding a couple of month-wide columns no
        // matter how little history actually exists — the same
        // one-data-point failure the price series had, so keep both charts
        // on the same window.
        let decision = debounced_decision.get();
        let range = match decision {
            RangeDecision::Resolved(range) => range.or_else(|| {
                series.get().filter(|s| !s.is_empty()).map(|s| {
                    (
                        s.from.and_utc().timestamp(),
                        s.to.and_utc().timestamp() + s.bucket_seconds,
                    )
                })
            }),
            // Still waiting on the sale probe — don't fetch (guard below).
            RangeDecision::Pending => None,
        };
        async move {
            if !active || decision == RangeDecision::Pending {
                return None;
            }
            get_price_density(id, &world_name, hq_filter, range, 32)
                .await
                .ok()
        }
    });
    let density = Signal::derive(move || density_resource.get().flatten());

    view! {
        <Transition fallback=ChartWrapperSkeleton>
            {move || {
                let error = listing_resource
                    .with(|l| l.as_ref().and_then(|r| r.as_ref().err()).map(|e| e.to_string()));
                if let Some(msg) = error {
                    view! {
                        <div role="alert" class="text-red-200 border border-red-500/40 rounded-xl p-4">
                            <strong class="font-semibold">{move || t_string!(i18n, error).to_string()} ":"</strong>
                            <span class="ml-2">{msg}</span>
                            <div class="text-sm text-red-300/80 mt-1">{move || t_string!(i18n, unable_to_load_recent_sales).to_string()}</div>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4 text-[color:var(--color-text)] h-full">
                            <div class="flex flex-col gap-3">
                                <div class="flex flex-wrap items-start justify-between gap-3">
                                    <div>
                                        <div class="flex items-center gap-2 flex-wrap">
                                            <h2 class="text-xl font-bold leading-tight">{move || t_string!(i18n, sale_history).to_string()}</h2>
                                            // Analyzer confidence chip — reflects ClickHouse-rolled
                                            // sample size + launder suspicion over 30 days.
                                            // Picks HQ or NQ variant based on the current toggle so
                                            // users see the band that matches what they're looking at.
                                            {move || {
                                                let want_hq = hq_only();
                                                item_stats_resource
                                                    .get()
                                                    .and_then(|s| s.as_ref().as_ref().ok().and_then(|r| r.variant_for(want_hq).cloned()))
                                                    .map(|variant| view! {
                                                        <ConfidenceBadge
                                                            band=variant.confidence_band
                                                            sample_size=variant.sample_size_30d
                                                        />
                                                    })
                                            }}
                                        </div>
                                        <p class="text-sm text-[color:var(--color-text-muted)]">
                                            {move || {
                                                series
                                                    .get()
                                                    .map(|s| {
                                                        let n: usize = s
                                                            .series
                                                            .iter()
                                                            .flat_map(|entry| entry.buckets.iter())
                                                            .map(|b| b.sales as usize)
                                                            .sum();
                                                        t!(i18n, based_on_sales, count = n)
                                                    })
                                            }}
                                        </p>
                                    </div>
                                    <div class="flex flex-wrap items-center justify-end gap-2">
                                        <Toggle
                                            checked=hq_only
                                            set_checked=set_hq_only
                                            checked_label=t_string!(i18n, hq_only).to_string()
                                            unchecked_label=t_string!(i18n, all_qualities).to_string()
                                        />
                                        <a
                                            class="btn-primary text-sm"
                                            target="_blank"
                                            href=move || crate::social_meta::social_image_path(
                                                i18n.get_locale(),
                                                &crate::social_card::SocialCardKind::Item(item_id()),
                                                Some(&world()),
                                            )
                                        >
                                            {move || t_string!(i18n, download_png).to_string()}
                                        </a>
                                    </div>
                                </div>

                                {move || {
                                    series_resource
                                        .get()
                                        .flatten()
                                        .and_then(|r| r.err())
                                        .map(|e| view! {
                                            <div role="alert" class="bg-red-900/30 text-red-200 border border-red-700/40 rounded-xl px-3 py-2 text-sm">
                                                {e.to_string()}
                                            </div>
                                        })
                                }}

                                {move || {
                                    let is_empty = series.get().map(|s| s.is_empty()).unwrap_or(false);
                                    is_empty.then(|| view! {
                                        <div role="status" class="text-amber-200 border border-amber-500/40 rounded-xl p-4">
                                            {move || t_string!(i18n, no_sales_found).to_string()}
                                        </div>
                                    })
                                }}

                                <PriceHistoryChart
                                    series=series
                                    density=density
                                    scope_name=world
                                    mode=mode
                                    set_mode=set_mode
                                    group=group
                                    set_group=set_group
                                    selected_range=selected_range
                                    on_range_change=set_selected_range
                                    range_preset=chart_preset
                                    set_range_preset=set_range_preset
                                />

                                {move || {
                                    let no_listings = with_or(
                                        &filtered_listings,
                                        true,
                                        |listings| listings.is_empty(),
                                    );
                                    no_listings.then(|| view! {
                                        <div role="status" class="text-amber-200 border border-amber-500/40 rounded-xl px-3 py-2 text-sm">
                                            {move || t_string!(i18n, no_active_listings_found).to_string()}
                                        </div>
                                    })
                                }}
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </Transition>
    }.into_any()
}

#[component]
fn SalesDetails(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        // Removed mt-8 and space-y-6 wrapper to let grid control layout
        <Transition fallback=move || {
            view! { <BoxSkeleton /> }
        }>
            {move || {
                let sales = Memo::new(move |_| {
                    listing_resource
                        .with(|l| {
                            l.as_ref().and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok())
                        })
                        .unwrap_or_default()
                });

                view! {
                    <div class="flex flex-col rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4 h-full">
                        <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                            {move || t_string!(i18n, sale_history).to_string()}
                        </h2>
                        <SaleHistoryTable sales=sales.into() />
                    </div>
                }
                    .into_any()
            }}
        </Transition>
    }
    .into_any()
}

fn update_current_item(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    update: impl FnOnce(&mut CurrentlyShownItem),
) {
    listing_resource.update(|current| {
        if let Some(Ok(current)) = current {
            let mut updated = current.as_ref().clone();
            update(&mut updated);
            *current = Arc::new(updated);
        }
    });
}

#[component]
fn ListingsContent(
    item_id: Memo<i32>,
    world: Memo<String>,
    #[prop(into, default = Signal::derive(HashSet::new))] excluded_worlds: Signal<HashSet<i32>>,
) -> impl IntoView {
    let (realtime_status, set_realtime_status) = signal("connecting".to_string());
    let (last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);
    let listing_resource = Resource::new(
        move || (item_id(), world()),
        |(item_id, world)| async move {
            get_listings(item_id, world.as_str())
                .await
                .map(Arc::new) // Keep large listing payloads cheap to share across page sections.
                .inspect_err(|e| {
                    // Only *our* side breaking is worth error-level reporting.
                    // A world segment the API can't resolve is a 404 it is
                    // right to return, already logged with its status and path
                    // by the fetch layer -- re-reporting it here is what filled
                    // GlitchTip issue 2210. See `AppError::is_api_response`.
                    // A loopback timeout is the same story one layer down:
                    // already logged by the fetch layer, transient, and the
                    // other half of GlitchTip issue 2210's volume.
                    if e.is_api_response() || e.is_transient_transport() {
                        tracing::warn!(error = ?e, item_id, %world, "Error getting value");
                    } else {
                        tracing::error!(error = ?e, item_id, %world, "Error getting value");
                    }
                })
        },
    );
    Effect::new(move |_| {
        let val = listing_resource.get();
        tracing::info!(?val, "Listings updated");
    });
    let realtime = use_realtime();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let excluded_datacenters = RwSignal::new(HashSet::<String>::new());
    let filtered_listings = Memo::new({
        // Every read in here goes through a `try_*` accessor. `ArcMemo` `take()`s
        // its cached value before running this closure, so a panic in the body
        // leaves the memo permanently holding `None` — every later read then dies
        // on the `t.as_ref().unwrap()` inside `try_read_untracked` (GlitchTip
        // #6865), including reads that go through `try_get`. Keeping the body
        // infallible is what stops that cascade.
        move |_| {
            let listings = with_or(&listing_resource, None, |listing| {
                listing.as_ref().and_then(|result| {
                    result.as_ref().ok().map(|item| {
                        item.listings
                            .iter()
                            .map(|(listing, retainer)| {
                                (listing.clone(), Arc::new(retainer.clone()))
                            })
                            .collect::<ListingRows>()
                    })
                })
            })
            .unwrap_or_default();
            // Datacenter exclusions belong to the Active Listings panel. Keeping
            // them out of this page-wide dataset prevents a table preference from
            // changing the price summary, chart, and per-world supply breakdown.
            filter_listing_rows(
                listings,
                None,
                &get_or_default(&excluded_worlds),
                &HashSet::new(),
            )
        }
    });
    let market_subscriptions = StoredValue::new(Vec::<RealtimeSubscription>::new());
    Effect::new(move |_| {
        market_subscriptions.update_value(|subscriptions| subscriptions.clear());
        let item_id = item_id();
        let world = Url::unescape(&world());
        let Some(realtime) = realtime.clone() else {
            return;
        };
        let Some(selector) = world_data
            .lookup_world_by_name(&world)
            .map(|world| AnySelector::from(&world))
        else {
            return;
        };
        if item_id == 0 {
            return;
        }

        let filter = FilterPredicate::World(selector).and(FilterPredicate::Item(item_id));
        let listings_subscription = realtime.subscribe_market(
            filter.clone(),
            SocketMessageType::Listings,
            move |message| match message {
                ServerClient::Subscribed { .. } => {
                    set_realtime_status.set("live".to_string());
                }
                ServerClient::Listings(event) => {
                    set_realtime_status.set("live".to_string());
                    set_last_update_at.set(Some(chrono::Utc::now()));
                    update_current_item(listing_resource, |data| {
                        data.apply_listing_event(item_id, event);
                    });
                }
                ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                    set_realtime_status.set("reconnecting".to_string());
                    set_last_update_at.set(Some(chrono::Utc::now()));
                    listing_resource.refetch();
                }
                _ => {}
            },
        );
        let sales_subscription = realtime.subscribe_market(
            filter,
            SocketMessageType::Sales,
            move |message| match message {
                ServerClient::Subscribed { .. } => {
                    set_realtime_status.set("live".to_string());
                }
                ServerClient::Sales(event) => {
                    set_realtime_status.set("live".to_string());
                    set_last_update_at.set(Some(chrono::Utc::now()));
                    update_current_item(listing_resource, |data| {
                        data.apply_sales_event(item_id, event);
                    });
                }
                ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                    set_realtime_status.set("reconnecting".to_string());
                    set_last_update_at.set(Some(chrono::Utc::now()));
                    listing_resource.refetch();
                }
                _ => {}
            },
        );
        market_subscriptions.set_value(vec![listings_subscription, sales_subscription]);
    });
    on_cleanup(move || {
        market_subscriptions.update_value(|subscriptions| subscriptions.clear());
    });
    view! {
        <div class="w-full py-4 sm:py-6 text-[color:var(--color-text)]">
            <div id="overview" class="scroll-mt-16">
                <crate::routes::item_compare::FlipRouteCard item_id world listing_resource />
                <DecisionHeader listing_resource filtered_listings world item_id />
                <MarketStatsPanel
                    listing_resource
                    filtered_listings
                    item_id
                    realtime_status=realtime_status.into()
                    last_update_at=last_update_at.into()
                />
            </div>
            // Tables before the chart: the listings and recent sales are what
            // most visitors came for, so they come right after the overview.
            // Both tables force `min-w-[720px]`, so two columns only fit when
            // the content area is ~1500px wide — roughly a 1440p display once
            // the sidebar and ad rail take their cut. Gating on the container
            // (not the viewport) keeps this correct when the sidebar is
            // collapsed or the ad rail is hidden. `minmax(0,1fr)` keeps a wide
            // table from blowing the grid past the container.
            <div class="@container">
                <div class="grid grid-cols-1 @min-[94rem]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-6 mt-6">
                    <div id="listings" class="scroll-mt-16 min-w-0">
                        <ListingsPanel
                            listing_resource
                            filtered_listings
                            world
                            excluded_datacenters
                        />
                    </div>
                    <div id="history" class="scroll-mt-16 min-w-0">
                        <SalesDetails listing_resource />
                    </div>
                </div>
            </div>

            <div class="mt-6">
                <ChartWrapper listing_resource filtered_listings item_id world />
            </div>

            // Per-world supply distribution answers a research question, not
            // something every visitor should scroll past on the way to the
            // chart. It sits below the sale history for that reason.
            <div class="mt-6">
                <WorldMarketShare listing_resource filtered_listings world />
            </div>

            <div class="mt-6 mx-auto">
                <Ad class="h-[336px] w-[280px] rounded-xl overflow-hidden" />
            </div>
        </div>
    }
    .into_any()
}

/// Builds the item page's `BreadcrumbList` JSON-LD.
///
/// `category` is `(display_name, search_category_id)`. The id — not the
/// category's localized name — is what the URL is keyed on: #1001 moved
/// `/items/category/:category` to an id precisely because the name differs per
/// locale, so a name-keyed URL here would hand Google a link that doesn't
/// resolve. This must keep matching the visible category link in the view below.
fn build_breadcrumb_json_ld(
    item_name: &str,
    world_val: &str,
    item_id_val: i32,
    category: Option<(&str, i32)>,
) -> String {
    let mut items = vec![
        serde_json::json!({
            "@type": "ListItem",
            "position": 1,
            "name": "Home",
            "item": "https://ultros.app/"
        }),
        serde_json::json!({
            "@type": "ListItem",
            "position": 2,
            "name": "Item Explorer",
            "item": "https://ultros.app/items"
        }),
    ];

    if let Some((c_name, category_id)) = category {
        items.push(serde_json::json!({
            "@type": "ListItem",
            "position": 3,
            "name": c_name,
            "item": format!("https://ultros.app/items/category/{category_id}")
        }));
        items.push(serde_json::json!({
            "@type": "ListItem",
            "position": 4,
            "name": item_name,
            "item": format!("https://ultros.app/item/{world_val}/{item_id_val}")
        }));
    } else {
        items.push(serde_json::json!({
            "@type": "ListItem",
            "position": 3,
            "name": item_name,
            "item": format!("https://ultros.app/item/{world_val}/{item_id_val}")
        }));
    }

    let json_value = serde_json::json!({
        "@context": "https://schema.org",
        "@type": "BreadcrumbList",
        "itemListElement": items
    });

    escape_for_script_tag(&serde_json::to_string(&json_value).unwrap_or_default())
}

/// Gates the item page on the `:id` route param actually naming a real item.
/// A param that fails to parse, or parses to an id with no matching item,
/// previously fell through to `unwrap_or_default()` and silently rendered an
/// empty "item 0" page with a 200 status — an indexable junk page for every
/// garbage `/item/<id>` URL. Render `NotFound` (which sets the 404 status)
/// instead.
#[component]
pub fn ItemView() -> impl IntoView {
    let params = use_params_map();
    let item_id_valid =
        Memo::new(move |_| params.with(|p| resolve_item_id(p.get_str("id"))).is_some());

    view! {
        <Show when=move || item_id_valid.get() fallback=|| view! { <NotFound /> }.into_any()>
            <ItemViewContent />
        </Show>
    }
}

#[component]
fn ItemViewContent() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let params = use_params_map();
    let query = use_query_map();
    let item_id = Memo::new(move |_| {
        params()
            .get("id")
            .and_then(|id| id.parse::<i32>().ok())
            .unwrap_or_default()
    });

    let recently_viewed = use_context::<RecentItems>().unwrap();
    Effect::new(move |_| {
        recently_viewed.add_item(item_id());
    });

    let (price_zone, _) = get_price_zone();
    let excluded_worlds = Memo::new(move |_| {
        query.with(|query| parse_excluded_world_ids(query.get("exclude-worlds").as_deref()))
    });

    let world = Memo::new(move |_| {
        params.with(|p| {
            p.get("world").clone().unwrap_or_else(move || {
                price_zone
                    .get()
                    .map(|zone| zone.get_name().to_string())
                    .unwrap_or_else(|| "North-America".to_string())
            })
        })
    });

    // Each closure calls `tracked_data()` inside its own reactive scope so it
    // re-subscribes to `DataRevision` and re-reads after a locale swap.
    let item_name = move || {
        tracked_data()
            .items
            .get(&ItemId(item_id()))
            .map(|item| item.name.as_str())
            .unwrap_or_default()
            .to_string()
    };

    let item = move || tracked_data().items.get(&ItemId(item_id()));

    let item_category = move || {
        let data = tracked_data();
        data.items.get(&ItemId(item_id())).and_then(|item| {
            data.item_ui_categorys
                .get(&ItemUiCategoryId(item.item_ui_category))
        })
    };

    let item_search_category = move || {
        let data = tracked_data();
        data.items.get(&ItemId(item_id())).and_then(|item| {
            data.item_search_categorys
                .get(&ItemSearchCategoryId(item.item_search_category))
        })
    };

    let description = Memo::new(move |_| {
        t_string!(
            i18n,
            item_view_meta_description,
            name = item_name().to_string(),
            world = world()
        )
        .to_string()
    });

    // BreadcrumbList JSON-LD for Google Rich Results.
    // We only emit BreadcrumbList markup (Home -> Item Explorer -> {category} -> {item})
    // and purposely omit Product / AggregateOffer markup because:
    // 1. Google's Product rich-result guidelines target real-world purchasable products with real currencies.
    // 2. FFXIV gil is a fictional virtual currency and "GIL" is not a valid ISO 4217 code.
    // 3. Placing fictional virtual currency values in Product / AggregateOffer markup can trigger structured data spam manual actions.
    let json_ld = move || {
        let name_val = item_name();
        let world_val = world();
        let item_id_val = item_id();
        let category = item_category()
            .and_then(|c| item_search_category().map(|s| (c, s)))
            .map(|(c, s)| (c.name.as_str(), s.key_id.0));

        build_breadcrumb_json_ld(&name_val, &world_val, item_id_val, category)
    };

    view! {
        <MetaTitle title=move || {
            t_string!(i18n, item_view_meta_title, name = item_name().to_string(), world = world()).to_string()
        } />
        <MetaDescription text=description />
        <Meta
            property="thumbnail"
            content=move || format!("https://ultros.app/static/itemicon/{}?size=Large", item_id())
        />
        <MetaCanonical href=move || format!("https://ultros.app/item/{}", item_id()) />
        <script type="application/ld+json" inner_html=json_ld />
        <div class="min-h-screen">
            <div class="w-full px-0 sm:px-4 pt-4 sm:pt-5 pb-3">
                <div class="flex flex-col gap-4 p-3 sm:p-4 border-b border-[color:var(--color-outline)] pb-6">
                    <div class="flex flex-col md:flex-row items-start gap-4">
                        <div class="flex items-center gap-4 flex-1">
                            <ItemTooltip item_id=item_id>
                                // The hero icon is the LCP candidate on the item page —
                                // eager-load it; every other icon stays lazy.
                                <ItemIcon item_id icon_size=IconSize::Large loading="eager" />
                            </ItemTooltip>

                            <div class="flex flex-col min-w-0">
                                <h1 class="text-3xl sm:text-4xl font-bold text-[color:var(--color-text)] flex items-center gap-2 leading-tight">
                                    {item_name}
                                    <Clipboard clipboard_text=Signal::derive(move || {
                                        item_name().to_string()
                                    }) />
                                </h1>
                                <div class="text-brand-300 text-lg">
                                    {move || {
                                        item_category()
                                            .and_then(|c| item_search_category().map(|s| (c, s)))
                                            .map(|(c, s)| {
                                                view! {
                                                    <a
                                                        class="text-brand-300 hover:text-brand-200 transition-colors"
                                                        href=format!("/items/category/{}", s.key_id.0)
                                                    >
                                                        {c.name.as_str()}
                                                    </a>
                                                }
                                            })
                                    }}
                                </div>
                            </div>
                        </div>

                        <div class="flex flex-wrap gap-2 items-center">
                            <div class="cursor-pointer"><AddToList item_id /></div>
                            <a
                                class="btn-primary"
                                target="_blank"
                                rel="noopener noreferrer"
                                aria-label=move || t_string!(i18n, open_universalis_aria_label).to_string()
                                href=move || format!("https://universalis.app/market/{}", item_id())
                            >
                                {t!(i18n, universalis)}
                            </a>
                            <a
                                class="btn-primary"
                                target="_blank"
                                rel="noopener noreferrer"
                                aria-label=move || t_string!(i18n, open_garlandtools_aria_label).to_string()
                                href=move || format!("https://garlandtools.org/db/#item/{}", item_id())
                            >
                                {t!(i18n, garlandtools)}
                            </a>
                        </div>
                    </div>

                    // Stats are reference material, not market data — collapsed by
                    // default so listings and sales start higher on the page. Native
                    // <details> keeps the default state static and SSR-deterministic.
                    <details class="group pt-3 border-t border-[color:var(--color-outline)]">
                        <summary class="flex cursor-pointer list-none items-center gap-2 text-sm font-semibold text-brand-300 hover:text-[color:var(--brand-fg)]">
                            <Icon icon=icondata::BiChevronDownRegular attr:class="shrink-0 transition-transform group-open:rotate-180" />
                            {t!(i18n, item_view_item_details)}
                        </summary>
                        <div class="grid grid-cols-1 lg:grid-cols-[minmax(0,0.8fr)_minmax(320px,1.2fr)] gap-3 pt-3 text-[color:var(--color-text)]/90">
                            <div class="flex flex-wrap items-center gap-2">
                                <span class="text-brand-300 font-medium tracking-wide text-xs uppercase">{move || t_string!(i18n, item_level).to_string()}</span>
                                <span class="text-brand-100 px-2 py-0.5 rounded text-sm font-bold border border-brand-400/50">
                                    {move || item().map(|item| item.level_item).unwrap_or_default()}
                                </span>
                            </div>
                            <div>{move || view! { <ItemStats item_id=ItemId(item_id()) /> }}</div>
                        </div>
                    </details>
                </div>
            </div>

            <WorldMenu world_name=world item_id />

            <SectionNav item_id>
                <span class="text-sm font-bold text-brand-200 whitespace-nowrap">
                    {move || Url::unescape(&world())}
                </span>
            </SectionNav>

            <div class="main-content px-0 sm:px-4">
                <ListingsContent item_id world excluded_worlds />
                <div id="related" class="scroll-mt-16 mt-6">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
        </div>
    }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos_i18n::context::init_i18n_context;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};
    use ultros_api_types::world_helper::WorldHelper;

    fn world_data_for_exclusion_controls() -> LocalWorldData {
        LocalWorldData(Ok(Arc::new(WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".to_string(),
                datacenters: vec![
                    Datacenter {
                        id: 10,
                        name: "Aether".to_string(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 100,
                            name: "Gilgamesh".to_string(),
                            datacenter_id: 10,
                        }],
                    },
                    Datacenter {
                        id: 20,
                        name: "Primal".to_string(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 200,
                            name: "Excalibur".to_string(),
                            datacenter_id: 20,
                        }],
                    },
                ],
            }],
        }))))
    }

    fn listing(
        id: i32,
        world_id: i32,
        price_per_unit: i32,
        hq: bool,
    ) -> (ActiveListing, Arc<Retainer>) {
        (
            ActiveListing {
                id,
                world_id,
                item_id: 1,
                retainer_id: id,
                price_per_unit,
                quantity: 1,
                hq,
                timestamp: chrono::Utc::now().naive_utc(),
            },
            Arc::new(Retainer {
                id,
                world_id,
                name: format!("Retainer {id}"),
                retainer_city_id: 1,
            }),
        )
    }

    #[test]
    fn item_view_cheapest_listing_empty_exclusions_preserve_selection() {
        let listings = vec![listing(1, 100, 100, false), listing(2, 200, 200, false)];

        let result = cheapest_listing_for_quality(&listings, false).unwrap();

        assert_eq!(result.0.id, 1);
        assert_eq!(result.0.world_id, 100);
    }

    #[test]
    fn item_view_cheapest_listing_uses_pre_filtered_rows() {
        let listings = vec![
            listing(1, 100, 100, false),
            listing(2, 200, 200, false),
            listing(3, 300, 50, true),
        ];
        let filtered = listings
            .into_iter()
            .filter(|(listing, _)| listing.world_id != 100)
            .collect::<ListingRows>();

        let result = cheapest_listing_for_quality(&filtered, false).unwrap();

        assert_eq!(result.0.id, 2);
        assert_eq!(result.0.world_id, 200);
    }

    #[test]
    fn datacenter_exclusion_controls_render_each_datacenter_once() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            provide_context(world_data_for_exclusion_controls());
            let world = Memo::new(|_| "North-America".to_string());
            let excluded_datacenters = RwSignal::new(HashSet::from(["Aether".to_string()]));

            let html = view! {
                <DatacenterExclusionControls world excluded_datacenters />
            }
            .to_html();

            assert_eq!(
                html.matches("data-datacenter=\"Aether\"").count(),
                1,
                "{html}"
            );
            assert_eq!(
                html.matches("data-datacenter=\"Primal\"").count(),
                1,
                "{html}"
            );
            assert_eq!(html.matches("Clear all").count(), 1);
            assert!(html.contains("aria-pressed=\"true\""));
            assert!(!html.contains("<h2"));
        });
    }

    fn zone_listing(price: i32, world_id: i32) -> CheapestListingData {
        CheapestListingData { price, world_id }
    }

    #[test]
    fn zone_savings_reports_cheaper_other_world() {
        let summary = PriceSummary {
            lq: Some(zone_listing(3_000, 200)),
            hq: None,
        };
        let result = zone_savings(Some(5_000), None, &summary, 100).unwrap();
        assert_eq!(result.savings, 2_000);
        assert!(!result.hq);
        assert_eq!(result.cheapest.world_id, 200);
    }

    #[test]
    fn zone_savings_none_when_cheapest_is_current_world() {
        let summary = PriceSummary {
            lq: Some(zone_listing(3_000, 100)),
            hq: None,
        };
        assert!(zone_savings(Some(5_000), None, &summary, 100).is_none());
    }

    #[test]
    fn zone_savings_ignores_trivial_savings() {
        // Below MEANINGFUL_CROSS_WORLD_SAVINGS_GIL (1_000)
        let summary = PriceSummary {
            lq: Some(zone_listing(4_500, 200)),
            hq: None,
        };
        assert!(zone_savings(Some(5_000), None, &summary, 100).is_none());
    }

    #[test]
    fn zone_savings_picks_larger_quality_saving() {
        let summary = PriceSummary {
            lq: Some(zone_listing(3_000, 200)),  // saves 2_000
            hq: Some(zone_listing(10_000, 300)), // saves 30_000
        };
        let result = zone_savings(Some(5_000), Some(40_000), &summary, 100).unwrap();
        assert!(result.hq);
        assert_eq!(result.savings, 30_000);
    }

    #[test]
    fn zone_savings_none_without_local_floor() {
        // Nothing listed locally to compare against — no claim to make.
        let summary = PriceSummary {
            lq: Some(zone_listing(3_000, 200)),
            hq: None,
        };
        assert!(zone_savings(None, None, &summary, 100).is_none());
    }

    #[test]
    fn item_view_excluded_worlds_query_parses_world_ids() {
        let result = parse_excluded_world_ids(Some("100, 200,not-a-world,300"));

        assert_eq!(result, HashSet::from([100, 200, 300]));
    }

    #[test]
    fn item_view_excluded_worlds_query_absent_defaults_empty() {
        let result = parse_excluded_world_ids(None);

        assert!(result.is_empty());
    }

    #[test]
    fn test_format_savings_percent() {
        // Less than 10%, formatted to 1 decimal place
        assert_eq!(format_savings_percent(0.0), "0.0");
        assert_eq!(format_savings_percent(5.5), "5.5");
        assert_eq!(format_savings_percent(9.9), "9.9");
        assert_eq!(format_savings_percent(9.94), "9.9");
        // Due to floating point formatting, 9.95 rounded to 1 decimal place might be 9.9 or 10.0.
        // Let's test typical values.
        assert_eq!(format_savings_percent(9.96), "10.0");

        // Greater than or equal to 10%, formatted to 0 decimal places
        assert_eq!(format_savings_percent(10.0), "10");
        assert_eq!(format_savings_percent(15.5), "16"); // Rounds up
        assert_eq!(format_savings_percent(15.4), "15"); // Rounds down
        assert_eq!(format_savings_percent(99.9), "100");
    }

    /// Reproduces GlitchTip #6864/#6867: the server walks a `<Transition>`
    /// body after the owner that created the `filtered_listings` prop has been
    /// cleaned up. A bare `.with()`/`.get()` panics there and truncates the SSR
    /// response; the accessors used on this page must degrade instead.
    #[test]
    fn item_view_listing_reads_survive_a_disposed_owner() {
        let owner = Owner::new();
        let filtered_listings: Signal<ListingRows> = owner.with(|| {
            let rows = RwSignal::new(vec![listing(1, 100, 100, false)]);
            Memo::new(move |_| rows.get()).into()
        });

        // While the owner is alive the reads behave exactly like `.with()`/`.get()`.
        assert!(!with_or(&filtered_listings, true, |listings| listings.is_empty()));
        assert_eq!(get_or_default(&filtered_listings).len(), 1);

        owner.cleanup();

        // Once it is disposed they must fall back rather than panic.
        assert!(with_or(&filtered_listings, true, |listings| listings.is_empty()));
        assert!(get_or_default(&filtered_listings).is_empty());
    }

    #[test]
    fn carried_world_switch_query_forwards_exclude_worlds_only() {
        assert_eq!(
            carried_world_switch_query(Some("100,200"), None),
            "exclude-worlds=100%2C200",
        );
    }

    #[test]
    fn carried_world_switch_query_forwards_compare_buy_from_only() {
        assert_eq!(
            carried_world_switch_query(None, Some("Jenova")),
            "compare-buy-from=Jenova",
        );
    }

    #[test]
    fn carried_world_switch_query_forwards_both_params() {
        assert_eq!(
            carried_world_switch_query(Some("100,200"), Some("Jenova")),
            "exclude-worlds=100%2C200&compare-buy-from=Jenova",
        );
    }

    #[test]
    fn carried_world_switch_query_empty_when_neither_present() {
        assert_eq!(carried_world_switch_query(None, None), "");
    }

    #[test]
    fn carried_world_switch_query_ignores_empty_values() {
        assert_eq!(carried_world_switch_query(Some(""), Some("")), "");
    }

    #[test]
    fn test_build_breadcrumb_json_ld_with_category() {
        let json_str = build_breadcrumb_json_ld(
            "Excalibur",
            "Gilgamesh",
            12345,
            Some(("Two-Handed Sword", 2)),
        );
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["@context"], "https://schema.org");
        assert_eq!(parsed["@type"], "BreadcrumbList");

        let elements = parsed["itemListElement"].as_array().unwrap();
        assert_eq!(elements.len(), 4);

        assert_eq!(elements[0]["name"], "Home");
        assert_eq!(elements[0]["item"], "https://ultros.app/");

        assert_eq!(elements[1]["name"], "Item Explorer");
        assert_eq!(elements[1]["item"], "https://ultros.app/items");

        // The category link is keyed on the search-category id, matching both the
        // visible link in the view and the `/items/category/:category` route as of
        // #1001. Keying it on the localized name would emit a dead URL.
        assert_eq!(elements[2]["name"], "Two-Handed Sword");
        assert_eq!(elements[2]["item"], "https://ultros.app/items/category/2");

        assert_eq!(elements[3]["name"], "Excalibur");
        assert_eq!(
            elements[3]["item"],
            "https://ultros.app/item/Gilgamesh/12345"
        );
    }

    #[test]
    fn test_build_breadcrumb_json_ld_without_category() {
        let json_str = build_breadcrumb_json_ld("Excalibur", "Gilgamesh", 12345, None);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["@context"], "https://schema.org");
        assert_eq!(parsed["@type"], "BreadcrumbList");

        let elements = parsed["itemListElement"].as_array().unwrap();
        assert_eq!(elements.len(), 3);

        assert_eq!(elements[0]["name"], "Home");
        assert_eq!(elements[0]["item"], "https://ultros.app/");

        assert_eq!(elements[1]["name"], "Item Explorer");
        assert_eq!(elements[1]["item"], "https://ultros.app/items");

        assert_eq!(elements[2]["name"], "Excalibur");
        assert_eq!(
            elements[2]["item"],
            "https://ultros.app/item/Gilgamesh/12345"
        );
    }
}
