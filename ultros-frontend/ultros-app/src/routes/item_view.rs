use crate::api::{get_extended_sale_history, get_item_stats, get_listings};
use crate::components::confidence_badge::ConfidenceBadge;
use crate::components::freshness_badge::FreshnessBadge;
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::listing_filters::filter_listing_rows;
use crate::components::price_history_chart::PriceHistoryChart;
use crate::components::sales_cadence_badge::SalesCadenceBadge;
use crate::components::world_name::WorldName;
use crate::components::{
    ad::Ad, add_to_list::AddToList, clipboard::*, item_icon::*, listings_table::*, meta::*,
    realtime_status::RealtimeStatus, recently_viewed::RecentItems, related_items::*,
    sale_history_table::*, skeleton::BoxSkeleton, stats_display::*, toggle::Toggle, ui_text::*,
};
use crate::error::AppError;
use crate::global_state::LocalWorldData;
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::global_state::home_world::{get_price_zone, locale_preferred_region, use_home_world};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string};
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use leptos::prelude::*;
use leptos_meta::{Link, Meta};
use leptos_router::components::A;
use leptos_router::hooks::{use_params_map, use_query_map};
use leptos_router::location::Url;
use std::{collections::HashSet, sync::Arc};
use ultros_api_types::websocket::{FilterPredicate, ServerClient, SocketMessageType};
use ultros_api_types::world::Datacenter;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::world_helper::{AnyResult, OwnedResult};
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer, SaleHistory};
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
fn with_or<S, U>(signal: &S, fallback: U, fun: impl FnOnce(&S::Value) -> U) -> U
where
    S: With,
{
    signal.try_with(fun).unwrap_or(fallback)
}

/// [`Get`] counterpart to [`with_or`] for values that are cloned out anyway.
fn get_or_default<S>(signal: &S) -> S::Value
where
    S: Get,
    S::Value: Default,
{
    signal.try_get().unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
struct SavingsVerdict {
    cheapest_listing: ActiveListing,
    current_world_listing: ActiveListing,
    savings: i32,
    savings_percent: f64,
}

impl SavingsVerdict {
    fn new(cheapest_listing: ActiveListing, current_world_listing: ActiveListing) -> Option<Self> {
        if cheapest_listing.hq != current_world_listing.hq
            || cheapest_listing.world_id == current_world_listing.world_id
            || cheapest_listing.price_per_unit <= 0
            || current_world_listing.price_per_unit <= 0
        {
            return None;
        }

        let savings = current_world_listing.price_per_unit - cheapest_listing.price_per_unit;
        if savings < MEANINGFUL_CROSS_WORLD_SAVINGS_GIL {
            return None;
        }

        Some(Self {
            cheapest_listing,
            current_world_listing: current_world_listing.clone(),
            savings,
            savings_percent: (savings as f64 / current_world_listing.price_per_unit as f64) * 100.0,
        })
    }
}

#[component]
fn WorldButton(
    current_world: Memo<String>,
    #[prop(into)] world: OwnedResult,
    item_id: i32,
) -> impl IntoView {
    let (home_world, _) = use_home_world();
    let world_name = world.get_name().to_string();
    let world_2 = world_name.clone();
    let world_3 = world_name.clone();
    let is_home_world = Signal::derive({
        move || {
            home_world
                .with(|w| w.as_ref().map(|w| w.name == world_2))
                .unwrap_or_default()
        }
    });
    let other_styles = match world {
        OwnedResult::Region(_) => "text-sm font-bold text-brand-200 px-3 py-1.5",
        OwnedResult::Datacenter(_) => "text-sm font-semibold text-brand-300 px-2.5 py-1",
        OwnedResult::World(_) => "text-xs text-[color:var(--color-text)] px-2 py-1",
    };
    let is_selected = move || current_world.with(|w| w == world_3.as_str());
    let home_world_emphasis =
        move || is_home_world.with(|w| if *w { "border border-brand-300/70" } else { "" });
    view! {
        <A
            attr:class=move || {
                [
                    "rounded-md flex items-center gap-1.5 transition-colors duration-150 whitespace-nowrap border border-transparent",
                    other_styles,
                    "hover:border-[color:var(--color-outline)] hover:text-brand-100",
                    if is_selected() {
                        "font-bold text-brand-100 border-brand-300/70"
                    } else {
                        ""
                    },
                    home_world_emphasis(),
                ]
                    .join(" ")
            }
                href=format!("/item/{}/{item_id}", Url::escape(&world_name))
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
                {world_name}
            </A>
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
        <div class="sticky top-0 z-10 backdrop-blur bg-[color:color-mix(in_srgb,var(--color-background)_85%,transparent)] border-y border-[color:var(--color-outline)]">
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
fn DatacenterExclusionControls(
    world: Memo<String>,
    excluded_datacenters: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();

    let datacenters = Memo::new({
        let world_data = world_data.clone();
        move |_| {
            let world_name = Url::unescape(&world());
            world_data
                .lookup_world_by_name(&world_name)
                .map(|result| {
                    world_data
                        .get_datacenters(&result)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
    });
    let excluded_visible = Memo::new({
        let world_data = world_data.clone();
        move |_| {
            excluded_datacenters.with(|excluded| {
                let mut datacenters = excluded
                    .iter()
                    .filter_map(|name| {
                        world_data
                            .lookup_world_by_name(name)
                            .and_then(|result| result.as_datacenter())
                            .cloned()
                    })
                    .collect::<Vec<_>>();
                datacenters.sort_by(|a, b| a.name.cmp(&b.name));
                datacenters
            })
        }
    });

    view! {
        {move || {
            let has_controls = datacenters.with(|datacenters| !datacenters.is_empty())
                || excluded_visible.with(|datacenters| !datacenters.is_empty());
            has_controls.then(|| {
                view! {
                    <div class="rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4">
                        <div class="flex flex-wrap items-center justify-between gap-2">
                            <h2 class="text-sm font-bold uppercase text-brand-200">
                                {t!(i18n, item_view_exclude_datacenters)}
                            </h2>
                            <button
                                type="button"
                                class="btn-secondary h-8 px-2 text-xs"
                                class:hidden=move || excluded_datacenters.with(|set| set.is_empty())
                                on:click=move |_| {
                                    excluded_datacenters.update(|set| set.clear());
                                }
                            >
                                <Icon icon=icondata::MdiClose attr:class="text-sm" />
                                {t!(i18n, clear_all)}
                            </button>
                        </div>

                        <div class="mt-3 flex flex-wrap gap-2">
                            {move || {
                                datacenters
                                    .get()
                                    .into_iter()
                                    .map(|datacenter: Datacenter| {
                                        let name = datacenter.name.clone();
                                        let label_name = name.clone();
                                        let state_name = name.clone();
                                        let click_name = name.clone();
                                        let is_excluded = Signal::derive(move || {
                                            excluded_datacenters.with(|set| set.contains(&state_name))
                                        });
                                        view! {
                                            <button
                                                type="button"
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
                                                        "inline-flex min-h-9 items-center gap-1.5 rounded-md border px-2.5 py-1 text-sm transition-colors",
                                                        if is_excluded() {
                                                            "border-amber-300/60 bg-amber-500/10 text-amber-100"
                                                        } else {
                                                            "border-[color:var(--color-outline)] text-[color:var(--color-text)] hover:border-brand-300/60"
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
                                                        .then(|| view! { <Icon icon=icondata::BsCheck attr:class="text-sm" /> })
                                                }}
                                                <span>{name.clone()}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>

                        <div
                            class="mt-3 flex flex-wrap gap-2"
                            class:hidden=move || excluded_visible.with(|datacenters| datacenters.is_empty())
                        >
                            {move || {
                                excluded_visible
                                    .get()
                                    .into_iter()
                                    .map(|datacenter: Datacenter| {
                                        let name = datacenter.name.clone();
                                        let label_name = name.clone();
                                        let click_name = name.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="inline-flex min-h-8 items-center gap-1.5 rounded-md border border-amber-300/40 bg-amber-500/10 px-2 py-0.5 text-xs text-amber-100 transition-colors hover:border-amber-200/70"
                                                aria-label=move || t_string!(
                                                    i18n,
                                                    item_view_include_datacenter_aria,
                                                    datacenter = label_name.clone()
                                                )
                                                on:click=move |_| {
                                                    excluded_datacenters.update(|set| {
                                                        set.remove(&click_name);
                                                    });
                                                }
                                            >
                                                <Icon icon=icondata::MdiClose attr:class="text-sm" />
                                                <span>{name.clone()}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
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

fn savings_verdict_for_quality(
    listings: &ListingRows,
    current_world_id: i32,
    hq: bool,
) -> Option<SavingsVerdict> {
    let (cheapest_listing, _) = cheapest_listing_for_quality(listings, hq)?;
    let current_world_listing = listings
        .iter()
        .filter(|(listing, _)| listing.hq == hq && listing.world_id == current_world_id)
        .min_by_key(|(listing, _)| listing.price_per_unit)
        .map(|(listing, _)| listing.clone())?;

    SavingsVerdict::new(cheapest_listing, current_world_listing)
}

fn cheapest_savings_verdict(
    listings: &ListingRows,
    current_world_id: i32,
) -> Option<SavingsVerdict> {
    [false, true]
        .into_iter()
        .filter_map(|hq| savings_verdict_for_quality(listings, current_world_id, hq))
        .max_by(|left, right| {
            left.savings
                .cmp(&right.savings)
                .then_with(|| {
                    left.current_world_listing
                        .price_per_unit
                        .cmp(&right.current_world_listing.price_per_unit)
                })
                .then_with(|| left.cheapest_listing.hq.cmp(&right.cheapest_listing.hq))
        })
}

fn format_savings_percent(percent: f64) -> String {
    if percent >= 10.0 {
        format!("{percent:.0}")
    } else {
        format!("{percent:.1}")
    }
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
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();

    view! {
        <Transition fallback=move || view! { <BoxSkeleton /> }>
            {move || {
                listing_resource
                    .with(|data_ref| {
                        if let Some(Ok(data)) = data_ref.as_ref() {
                            let listings = get_or_default(&filtered_listings);
                            let current_world_id = {
                                let world_name = Url::unescape(&world());
                                world_data
                                    .lookup_world_by_name(&world_name)
                                    .and_then(|result| result.as_world().map(|world| world.id))
                            };
                            let savings_verdict = current_world_id
                                .and_then(|world_id| cheapest_savings_verdict(&listings, world_id));
                            let recent_sales = &data.sales;

                            let sales_per_day = if recent_sales.len() > 1 {
                                let newest = recent_sales.first().unwrap().sold_date;
                                let oldest = recent_sales.last().unwrap().sold_date;
                                let seconds = (newest - oldest).num_seconds().abs();
                                let count = recent_sales.len() - 1;
                                if seconds > 0 {
                                    Some((count as f32) / (seconds as f32 / 86400.0))
                                } else {
                                    Some(100.0) // high velocity
                                }
                            } else if recent_sales.is_empty() {
                                Some(0.0)
                            } else {
                                None
                            };

                            let latest_timestamp = listings
                                .iter()
                                .map(|(listing, _)| listing.timestamp)
                                .max();

                            let age = latest_timestamp.map(|t| chrono::Utc::now().naive_utc() - t);

                            let freshness_verdict = ultros_api_types::freshness::calculate_freshness_verdict(
                                age,
                                sales_per_day,
                            );
                            let cadence_verdict = crate::analysis::get_sales_cadence(
                                sales_per_day.unwrap_or_default(),
                                recent_sales.len(),
                            );

                            view! {
                                <div class="flex flex-col gap-3 mb-4">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <FreshnessBadge verdict=freshness_verdict age=age />
                                        <SalesCadenceBadge
                                            cadence=cadence_verdict
                                            sales_per_day=sales_per_day.unwrap_or_default()
                                        />
                                    </div>
                                    {savings_verdict
                                        .map(|verdict| {
                                            let quality_label = if verdict.cheapest_listing.hq {
                                                t_string!(i18n, hq).to_string()
                                            } else {
                                                t_string!(i18n, nq).to_string()
                                            };
                                            let percent = format_savings_percent(verdict.savings_percent);
                                            view! {
                                                <a
                                                    href="#listings"
                                                    class="flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg border border-emerald-400/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-100 transition-colors hover:border-emerald-300/70"
                                                >
                                                    <Icon icon=icondata::FaGlobeSolid attr:class="text-sm shrink-0" />
                                                    <span class="font-semibold">
                                                        {t!(i18n, item_view_savings_cheapest_on)}
                                                    </span>
                                                    <span class="inline-flex items-center gap-1">
                                                        <WorldName id=AnySelector::World(verdict.cheapest_listing.world_id) />
                                                        <span class="rounded border border-emerald-300/40 px-1 text-[10px] font-bold leading-4 text-emerald-100">
                                                            {quality_label}
                                                        </span>
                                                    </span>
                                                    <span class="text-[color:var(--color-text-muted)]">":"</span>
                                                    <div class="font-bold text-[color:var(--color-text)]">
                                                        <Gil amount=verdict.cheapest_listing.price_per_unit />
                                                    </div>
                                                    <span class="text-[color:var(--color-text-muted)]">"-"</span>
                                                    <span>{t!(i18n, item_view_savings_save)}</span>
                                                    <div class="font-bold text-[color:var(--color-text)]">
                                                        <Gil amount=verdict.savings />
                                                    </div>
                                                    <span class="text-[color:var(--color-text-muted)]">
                                                        "("{percent}"%)"
                                                    </span>
                                                </a>
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
    let cheapest_prices = use_context::<CheapestPrices>();

    // Defer the `cheapest_prices.read_listings`-driven recipe-cost chip until
    // after hydration. The chip lives inside an inner `<Suspense>` in
    // `source_callout`'s recipe branch and reads the resource via `.with()` —
    // which (same gotcha as #719) does NOT subscribe the wrapping Suspense, so
    // SSR proceeds with whatever state the resource happens to be in. When SSR
    // renders the text branch (`{t!(i18n, craftable)}` / `{t!(i18n,
    // used_in_crafting)}`) but the client-side serialised resource resolves to
    // `Some(prices)` with `min_cost > 0`, the first CSR render swaps in
    // `view! { <span>{t!(i18n, craft_for)} " ~" <Gil amount=min_cost /></span> }`
    // — an `<span>` element where the SSR'd DOM has a bare text node. tachys'
    // walker then hits `failed_to_cast_text_node` at
    // `tachys-0.2.15/src/hydration.rs:227` (the post-debug-strip `unreachable!()`
    // — see GlitchTip cluster on `/item/<world>/<id>`: issues 5270/5269/5268/
    // 5267/5266/…/5234 etc. on releases 51d31a9 and db795c3, plus the
    // long-running `RuntimeError: unreachable` mirrors 4 and 5147). The
    // wasm-bindgen-futures executor then cascades into `RefCell already
    // borrowed` from the same trace.
    //
    // Same idiom as #725 (chart), #719 (item-explorer), #712 (home),
    // #730 (relative-time): an `Effect`-driven `hydrated` flag (effects run
    // client-only, after first render) so SSR and the initial CSR hydration
    // render both treat prices as unavailable. Both sides emit the text
    // branches, shapes match, and a frame later the effect fires, the closure
    // re-runs with the real price map, and the chip reactively swaps to the
    // `<span>` form.
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

                            let source_callout = {
                                let game_data = tracked_data();
                                let cheapest_prices = cheapest_prices.clone();
                                let item_id = item_id();
                                let vendor_exists = is_vendor_item(item_id);
                                let exchange_exists = game_data
                                    .special_shops
                                    .values()
                                    .any(|shop| special_shop_has_item(shop, item_id));
                                let leve_exists = game_data.leves.values().any(|leve| {
                                    leve_rewards_item(
                                        leve,
                                        item_id,
                                        &game_data.leve_reward_items,
                                        &game_data.leve_reward_item_groups,
                                    )
                                });
                                let recipe_exists =
                                    recipe_tree_iter(ItemId(item_id)).next().is_some();

                                if vendor_exists || exchange_exists || recipe_exists || leve_exists {
                                    let (title, summary, icon, href, accent_class): (
                                        String,
                                        AnyView,
                                        icondata::Icon,
                                        &str,
                                        &str,
                                    ) = if vendor_exists {
                                        let price = game_data
                                            .items
                                            .get(&ItemId(item_id))
                                            .map(|item| {
                                                if item.price_mid > 0 {
                                                    item.price_mid
                                                } else {
                                                    item.price_low
                                                }
                                            })
                                            .unwrap_or(0);
                                        (
                                            t_string!(i18n, vendor_available).to_string(),
                                            view! { <span>{t!(i18n, sells_for)} <Gil amount=price as i32 /></span> }.into_any(),
                                            icondata::FaShopSolid,
                                            "#vendor-sources",
                                            "text-amber-300 border-amber-400/40",
                                        )
                                    } else if exchange_exists {
                                        (
                                            t_string!(i18n, exchange_available).to_string(),
                                            view! { <span>{t!(i18n, exchange_available)}</span> }.into_any(),
                                            icondata::BsArrowLeftRight,
                                            "#exchange-sources",
                                            "text-purple-300 border-purple-400/40",
                                        )
                                    } else if recipe_exists {
                                        let summary_view = view! {
                                            <Suspense fallback=move || t_string!(i18n, craftable).to_string()>
                                                {move || {
                                                    if let Some(recipe) = recipe_tree_iter(ItemId(item_id)).next() {
                                                        // Skip the price-aware branch entirely during the
                                                        // first (SSR-matching) render so SSR and CSR both
                                                        // pick the same text-only branches below. The effect
                                                        // above flips `hydrated` to true a frame later and
                                                        // the closure re-runs with the real price map.
                                                        if hydrated.get()
                                                            && let Some(prices) = cheapest_prices.as_ref()
                                                        {
                                                            prices.read_listings.with(|prices| {
                                                                let prices = prices.as_ref().and_then(|prices| prices.as_ref().ok());
                                                                if let Some(prices) = prices {
                                                                    let prices = prices.clone();
                                                                    let empty = crate::components::crafting_cost::EmptyOnHand;
                                                                    let recipes_by_output = std::collections::HashMap::new();
                                                                    // Read the user's shard preference so the chip stays
                                                                    // consistent with the cost line in the recipe panel.
                                                                    let opts_value = use_context::<crate::global_state::cookies::Cookies>()
                                                                        .map(|c| c.use_cookie_typed::<_, crate::global_state::craft_options::CraftOptions>(crate::global_state::craft_options::COOKIE_NAME).0.get().unwrap_or_default())
                                                                        .unwrap_or_default();
                                                                    let shards_mode = if opts_value.exclude_shards {
                                                                        crate::components::crafting_cost::ShardsMode::ExcludeShards
                                                                    } else {
                                                                        crate::components::crafting_cost::ShardsMode::IncludeMarket
                                                                    };
                                                                    let lq_opts = crate::components::crafting_cost::CraftingCostOptions {
                                                                        require_hq: false,
                                                                        max_subcraft_depth: 0,
                                                                        shards: shards_mode,
                                                                        on_hand: &empty,
                                                                    };
                                                                    let hq_opts = crate::components::crafting_cost::CraftingCostOptions {
                                                                        require_hq: true,
                                                                        max_subcraft_depth: 0,
                                                                        shards: shards_mode,
                                                                        on_hand: &empty,
                                                                    };
                                                                    let is_shard = crate::components::related_items::is_shard_item;
                                                                    let lq = crate::components::crafting_cost::compute_cost(recipe, &prices, &recipes_by_output, &lq_opts, &is_shard).cost;
                                                                    let hq = crate::components::crafting_cost::compute_cost(recipe, &prices, &recipes_by_output, &hq_opts, &is_shard).cost;
                                                                    let min_cost = if lq > 0 { lq } else { hq };
                                                                    if min_cost > 0 && recipe.item_result == item_id {
                                                                        view! { <span>{t!(i18n, craft_for)} " ~" <Gil amount=min_cost /></span> }.into_any()
                                                                    } else if recipe.item_result == item_id {
                                                                        t!(i18n, craftable).into_any()
                                                                    } else {
                                                                        t!(i18n, used_in_crafting).into_any()
                                                                    }
                                                                } else if recipe.item_result == item_id {
                                                                    t!(i18n, craftable).into_any()
                                                                } else {
                                                                    t!(i18n, used_in_crafting).into_any()
                                                                }
                                                            })
                                                        } else if recipe.item_result == item_id {
                                                            t!(i18n, craftable).into_any()
                                                        } else {
                                                            t!(i18n, used_in_crafting).into_any()
                                                        }
                                                    } else {
                                                        t!(i18n, craftable).into_any()
                                                    }
                                                }}
                                            </Suspense>
                                        }
                                        .into_any();
                                        (
                                            t_string!(i18n, crafting_recipe).to_string(),
                                            summary_view,
                                            icondata::FaHammerSolid,
                                            "#crafting-recipes",
                                            "text-orange-300 border-orange-400/40",
                                        )
                                    } else {
                                        (
                                            t_string!(i18n, levequest_reward).to_string(),
                                            view! { t!(i18n, obtainable_via_levequest) }.into_any(),
                                            icondata::FaScrollSolid,
                                            "#leve-sources",
                                            "text-pink-300 border-pink-400/40",
                                        )
                                    };

                                    Some(
                                        view! {
                                            <a
                                                href=href
                                                class=format!(
                                                    "flex items-center gap-3 rounded-lg border px-3 py-2 text-sm transition-colors hover:border-[color:var(--brand-ring)] {}",
                                                    accent_class,
                                                )
                                            >
                                                <Icon icon=icon attr:class="text-lg shrink-0" />
                                                <span class="min-w-0">
                                                    <span class="block font-semibold leading-tight">{title}</span>
                                                    <span class="block text-[color:var(--color-text)] leading-tight">{summary}</span>
                                                </span>
                                            </a>
                                        }
                                        .into_any(),
                                    )
                                } else {
                                    None
                                }
                            };

                            view! {
                                <div class="flex flex-col rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4 h-full">
                                    <div class="flex items-center justify-between gap-3 mb-2 sm:mb-3">
                                        <div>
                                            <div class="flex flex-wrap items-center gap-2">
                                                <h2 class="text-lg sm:text-xl font-bold text-[color:var(--color-text)] leading-tight">
                                                    {t!(i18n, cheapest_found)}
                                                </h2>
                                                <RealtimeStatus
                                                    status=realtime_status
                                                    last_update=last_update_at
                                                />
                                            </div>
                                            <p class="text-sm text-[color:var(--color-text-muted)]">
                                                {move || t!(i18n, based_on_sales, count = recent_sales.len())}
                                            </p>
                                        </div>
                                        <Icon icon=icondata::FaCoinsSolid attr:class="text-xl sm:text-2xl text-brand-300/70" />
                                    </div>

                                    <div class="grid grid-cols-2 gap-2 sm:gap-3">
                                        <a href="#listings" class="rounded-lg border border-[color:var(--color-outline)] hover:border-brand-300/60 transition-colors p-2 sm:p-3 min-h-24">
                                            <div class="text-xs font-bold uppercase text-brand-300 mb-1">{t!(i18n, nq)}</div>
                                            {if let Some((listing, _)) = cheapest_nq.clone() {
                                                view! {
                                                    <div>
                                                        <div class="text-xl sm:text-2xl font-bold leading-none"><Gil amount=listing.price_per_unit /></div>
                                                        <div class="text-xs text-[color:var(--color-text-muted)] mt-2 flex items-center gap-1">
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

                                        <a href="#listings" class="rounded-lg border border-[color:var(--color-outline)] hover:border-[#95c521]/60 transition-colors p-2 sm:p-3 min-h-24">
                                            <div class="text-xs font-bold uppercase text-[#95c521] mb-1 flex items-center gap-1">
                                                <Icon icon=icondata::FaStarSolid attr:class="text-[10px]" />
                                                {t!(i18n, hq)}
                                            </div>
                                            {if let Some((listing, _)) = cheapest_hq.clone() {
                                                view! {
                                                    <div>
                                                        <div class="text-xl sm:text-2xl font-bold leading-none"><Gil amount=listing.price_per_unit /></div>
                                                        <div class="text-xs text-[color:var(--color-text-muted)] mt-2 flex items-center gap-1">
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

                                        <a href="#history" class="rounded-lg border border-[color:var(--color-outline)] hover:border-blue-300/60 transition-colors p-2 sm:p-3 min-h-24">
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
                                            <div class="text-xl sm:text-2xl font-bold leading-none">
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
                                            {match real_primary {
                                                Some((_, est)) => {
                                                    view! {
                                                        <div class="text-[10px] text-[color:var(--color-text-muted)] mt-1">
                                                            {t!(i18n, real_price_basis, used = est.used, total = est.total, excluded = est.excluded)}
                                                        </div>
                                                    }
                                                    .into_any()
                                                }
                                                None => ().into_any(),
                                            }}
                                            <div class="text-xs text-[color:var(--color-text-muted)] mt-2">
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

                                        <a href="#listings" class="rounded-lg border border-[color:var(--color-outline)] hover:border-emerald-300/60 transition-colors p-2 sm:p-3 min-h-24">
                                            <div class="text-xs font-bold uppercase text-emerald-300 mb-1">{t!(i18n, active_listings)}</div>
                                            <div class="text-xl sm:text-2xl font-bold leading-none">{listings_count}</div>
                                            <div class="text-xs text-[color:var(--color-text-muted)] mt-2">
                                                {sales_cadence}
                                            </div>
                                        </a>
                                    </div>

                                    <div class="mt-3 sm:mt-4 space-y-2">
                                        {source_callout}
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

#[component]
pub fn ChartWrapper(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    item_id: Memo<i32>,
    world: Memo<String>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let (hq_only, set_hq_only) = signal(false);
    let (filter_outliers, set_filter_outliers) = signal(true);

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
    // When the user clicks "Load extended history", we replace the chart's sales with
    // a larger compact pull. Stored as Option<Vec<_>>: None means "use base resource".
    let (extended_sales, set_extended_sales) = signal::<Option<Vec<SaleHistory>>>(None);
    let (extended_loading, set_extended_loading) = signal(false);
    let (extended_error, set_extended_error) = signal::<Option<String>>(None);
    // Reset extended pull when the user navigates to a different item/world so the
    // chart doesn't keep stale data from the previous item.
    Effect::new(move |_| {
        let _ = item_id.get();
        let _ = world.get();
        set_extended_sales.set(None);
        set_extended_error.set(None);
    });

    /* moved into Transition branch to avoid reading resource outside Suspense/Transition */

    view! {
        <Transition fallback=move || {
            view! {
                <div class="animate-pulse panel h-[26rem] text-[color:var(--color-text)]">
                    <div class="h-full w-full flex items-center justify-center">
                        <div class="w-16 h-16 border-4 border-brand-400/40 border-t-transparent rounded-full animate-spin" />
                    </div>
                </div>
            }
        }>
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
                    let base_sales = Memo::new(move |_| {
                        if let Some(ext) = extended_sales.get() {
                            return ext;
                        }
                        listing_resource
                            .with(|l| {
                                l.as_ref()
                                    .and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok())
                            })
                            .unwrap_or_default()
                    });

                    let filtered_sales = Memo::new(move |_| {
                        let mut sales = base_sales();
                        if hq_only() {
                            sales.retain(|s| s.hq);
                        }
                        sales
                    });

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
                                            {move || t!(i18n, based_on_sales, count = base_sales.with(|sales| sales.len()))}
                                        </p>
                                    </div>
                                    <div class="flex flex-wrap items-center justify-end gap-2">
                                        <Toggle
                                            checked=hq_only
                                            set_checked=set_hq_only
                                            checked_label=t_string!(i18n, hq_only).to_string()
                                            unchecked_label=t_string!(i18n, all_qualities).to_string()
                                        />
                                        <Toggle
                                            checked=filter_outliers
                                            set_checked=set_filter_outliers
                                            checked_label=t_string!(i18n, filtering_outliers).to_string()
                                            unchecked_label=t_string!(i18n, no_filter).to_string()
                                        />
                                        <button
                                            class="btn-secondary text-sm disabled:opacity-50 disabled:cursor-not-allowed"
                                            type="button"
                                            disabled=move || extended_loading.get() || extended_sales.with(Option::is_some)
                                            on:click=move |_| {
                                                use leptos::task::spawn_local;
                                                set_extended_loading.set(true);
                                                set_extended_error.set(None);
                                                let world_name = world();
                                                let id = item_id();
                                                spawn_local(async move {
                                                    match get_extended_sale_history(id, &world_name, 5000).await {
                                                        Ok(payload) => {
                                                            let converted: Vec<SaleHistory> = payload
                                                                .sales
                                                                .into_iter()
                                                                .map(|s| SaleHistory {
                                                                    id: 0,
                                                                    quantity: s.quantity,
                                                                    price_per_item: s.price_per_item,
                                                                    buying_character_id: 0,
                                                                    hq: s.hq,
                                                                    sold_item_id: id,
                                                                    sold_date: s.sold_date,
                                                                    world_id: s.world_id,
                                                                    buyer_name: None,
                                                                })
                                                                .collect();
                                                            set_extended_sales.set(Some(converted));
                                                        }
                                                        Err(e) => {
                                                            set_extended_error.set(Some(e.to_string()));
                                                        }
                                                    }
                                                    set_extended_loading.set(false);
                                                });
                                            }
                                            title=move || t_string!(i18n, load_extended_history_help).to_string()
                                        >
                                            {move || {
                                                if extended_loading.get() {
                                                    t_string!(i18n, loading).to_string()
                                                } else if extended_sales.with(Option::is_some) {
                                                    let n = base_sales.with(|s| s.len());
                                                    t_string!(i18n, extended_history_loaded).to_string().replace("{n}", &n.to_string())
                                                } else {
                                                    t_string!(i18n, load_extended_history).to_string()
                                                }
                                            }}
                                        </button>
                                        <a
                                            class="btn-primary text-sm"
                                            target="_blank"
                                            href=move || format!("/itemcard/{}/{}", world(), item_id())
                                        >
                                            {move || t_string!(i18n, download_png).to_string()}
                                        </a>
                                    </div>
                                </div>

                                {move || extended_error.get().map(|msg| view! {
                                    <div role="alert" class="bg-red-900/30 text-red-200 border border-red-700/40 rounded-xl px-3 py-2 text-sm">
                                        {msg}
                                    </div>
                                })}

                                {move || {
                                    if filtered_sales.with(|sales| sales.is_empty()) {
                                        view! {
                                            <div role="status" class="text-amber-200 border border-amber-500/40 rounded-xl p-4">
                                                {move || t_string!(i18n, no_sales_found).to_string()}
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <PriceHistoryChart sales=filtered_sales filter_outliers scope_name=world />
                                        }.into_any()
                                    }
                                }}

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
fn HighQualityTable(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        <div class="space-y-6">
            <Transition fallback=move || {
                view! { <BoxSkeleton /> }
            }>
                {move || {
                    // Read `listing_resource` inside the Transition so this section
                    // actually suspends on it during SSR. `filtered_listings` is a Memo
                    // created outside any Suspense boundary, so reading it alone does NOT
                    // subscribe this Transition to the resource — the server would then
                    // render an empty table while the client hydrates a populated one,
                    // tripping the tachys hydration `unreachable!()` panic (GlitchTip #6831).
                    if !listing_resource.with(|r| matches!(r, Some(Ok(_)))) {
                        return ().into_any();
                    }
                    let hq_listings = Memo::new(move |_| {
                        with_or(&filtered_listings, Vec::new(), |listings| {
                            listings
                                .iter()
                                .filter(|(listing, _)| listing.hq)
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                    });
                    view! {
                        <div
                            class="flex flex-col gap-4 rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4"
                            class:hidden=move || hq_listings.with(|l| l.is_empty())
                        >
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                {move || t_string!(i18n, high_quality_listings).to_string()}
                            </h2>
                            <ListingsTable listings=hq_listings />
                        </div>
                    }
                        .into_any()
                }}
            </Transition>
        </div>
    }
    .into_any()
}

#[component]
fn LowQualityTable(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        <div class="space-y-6">
            <Transition fallback=move || {
                view! { <BoxSkeleton /> }
            }>
                {move || {
                    // Suspend on `listing_resource` here too (see HighQualityTable) so the
                    // server does not emit an empty table that the client then hydrates as
                    // populated — the tachys hydration mismatch behind GlitchTip #6831.
                    if !listing_resource.with(|r| matches!(r, Some(Ok(_)))) {
                        return ().into_any();
                    }
                    let lq_listings = Memo::new(move |_| {
                        with_or(&filtered_listings, Vec::new(), |listings| {
                            listings
                                .iter()
                                .filter(|(listing, _)| !listing.hq)
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                    });
                    view! {
                        <div
                            class="flex flex-col gap-4 rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4"
                            class:hidden=move || lq_listings.with(|l| l.is_empty())
                        >
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                {move || t_string!(i18n, low_quality_listings).to_string()}
                            </h2>
                            <ListingsTable listings=lq_listings />
                        </div>
                    }
                        .into_any()
                }}
            </Transition>
        </div>
    }
    .into_any()
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
                    <div class="flex flex-col gap-6 h-full"> // Use flex col to stack table and insights
                        <div class="flex flex-col rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4 flex-1">
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                {move || t_string!(i18n, sale_history).to_string()}
                            </h2>
                            <SaleHistoryTable sales=sales.into() />
                        </div>

                        <div class="flex flex-col rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4">
                            <SalesInsights sales=sales.into() />
                        </div>
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
                .inspect_err(|e| tracing::error!(error = ?e, "Error getting value"))
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
        let world_data = world_data.clone();
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
            filter_listing_rows(
                listings,
                Some(world_data.as_ref()),
                &get_or_default(&excluded_worlds),
                &get_or_default(&excluded_datacenters),
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
            <DecisionHeader listing_resource filtered_listings world />
            <div id="history" class="grid grid-cols-1 xl:grid-cols-[minmax(320px,0.85fr)_minmax(0,1.45fr)] gap-4 sm:gap-6">
                <MarketStatsPanel
                    listing_resource
                    filtered_listings
                    item_id
                    realtime_status=realtime_status.into()
                    last_update_at=last_update_at.into()
                />
                <ChartWrapper listing_resource filtered_listings item_id world />
            </div>

            <div id="listings" class="grid grid-cols-1 gap-6 mt-6">
                <DatacenterExclusionControls world excluded_datacenters />
                <HighQualityTable listing_resource filtered_listings />
                <LowQualityTable listing_resource filtered_listings />
            </div>

            <div class="grid grid-cols-1 gap-6 mt-8">
                <SalesDetails listing_resource />
            </div>

            <div class="mt-6 mx-auto">
                <Ad class="h-[336px] w-[280px] rounded-xl overflow-hidden" />
            </div>
        </div>
    }
    .into_any()
}

#[component]
fn DiscordCommandChip(
    #[prop(into)] item_name: Signal<String>,
    #[prop(into)] item_id: Signal<i32>,
    #[prop(into)] world_name: Signal<String>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    // The `item` slash-command parameter is typed as an INTEGER on the Discord side,
    // so a pasted command needs the item id, not a name. We show the name in the chip
    // for human readability and put the id in the clipboard payload.
    let display_command = Signal::derive(move || {
        format!(
            "/ffxiv prices current item:{} world:{}",
            item_name.get(),
            world_name.get(),
        )
    });
    let clipboard_command = Signal::derive(move || {
        format!(
            "/ffxiv prices current item:{} world:{}",
            item_id.get(),
            world_name.get(),
        )
    });
    view! {
        <div class="inline-flex items-center gap-2 rounded-md border border-brand-500/30 bg-black/30 px-2.5 py-1 text-xs">
            <span class="text-[color:var(--color-text-muted)]">{t!(i18n, item_view_discord_label)}</span>
            <code class="font-mono">{move || display_command.get()}</code>
            <Clipboard clipboard_text=clipboard_command />
        </div>
    }
}

#[component]
pub fn ItemView() -> impl IntoView {
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

    let item_description = move || {
        tracked_data()
            .items
            .get(&ItemId(item_id()))
            .map(|item| item.description.as_str())
            .unwrap_or_default()
            .to_string()
    };

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

    view! {
        <MetaTitle title=move || {
            t_string!(i18n, item_view_meta_title, name = item_name().to_string(), world = world()).to_string()
        } />
        <MetaDescription text=description />
        <MetaImage url=move || format!("https://ultros.app/itemcard/{}/{}", world(), item_id()) />
        <Meta
            property="thumbnail"
            content=move || format!("https://ultros.app/static/itemicon/{}?size=Large", item_id())
        />
        <Link rel="canonical" prop:href=move || format!("https://ultros.app/item/{}", item_id()) />
        <div class="min-h-screen">
            <div class="w-full px-0 sm:px-4 pt-4 sm:pt-5 pb-3">
                <div class="flex flex-col gap-4 p-3 sm:p-4 border-b border-[color:var(--color-outline)] pb-6">
                    <div class="flex flex-col md:flex-row items-start gap-4">
                        <div class="flex items-center gap-4 flex-1">
                            <ItemIcon item_id icon_size=IconSize::Large />
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
                                                        href=["/items/category/", &s.name.replace("/", "%2F")]
                                                            .concat()
                                                    >
                                                        {c.name.as_str()}
                                                    </a>
                                                }
                                            })
                                    }}
                                </div>
                                <div class="mt-1.5">
                                    <DiscordCommandChip
                                        item_name=Signal::derive(move || item_name().to_string())
                                        item_id=Signal::derive(move || item_id.get())
                                        world_name=Signal::derive(move || world.get())
                                    />
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

                    <div class="grid grid-cols-1 lg:grid-cols-[minmax(0,0.8fr)_minmax(320px,1.2fr)] gap-3 pt-3 border-t border-[color:var(--color-outline)] text-[color:var(--color-text)]/90">
                        <div class="flex flex-wrap items-center gap-2">
                            <span class="text-brand-300 font-medium tracking-wide text-xs uppercase">{move || t_string!(i18n, item_level).to_string()}</span>
                            <span class="text-brand-100 px-2 py-0.5 rounded text-sm font-bold border border-brand-400/50">
                                {move || item().map(|item| item.level_item).unwrap_or_default()}
                            </span>
                        </div>
                        <div>{move || view! { <ItemStats item_id=ItemId(item_id()) /> }}</div>
                        <div
                            class="lg:col-span-2 text-sm sm:text-base text-[color:var(--color-text-muted)] line-clamp-3"
                            class:hidden=move || { item_description().is_empty() }
                        >
                            {move || view! { <UIText text=item_description().to_string() /> }}
                        </div>
                    </div>
                </div>
            </div>

            <WorldMenu world_name=world item_id />

            <div class="main-content px-0 sm:px-4">
                <ListingsContent item_id world excluded_worlds />
                <div class="mt-6">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
        </div>
    }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn item_view_savings_verdict_no_listings() {
        let listings = Vec::new();

        let result = cheapest_savings_verdict(&listings, 100);

        assert!(result.is_none());
    }

    #[test]
    fn item_view_savings_verdict_same_world_cheapest() {
        let listings = vec![listing(1, 100, 5_000, false), listing(2, 200, 6_000, false)];

        let result = cheapest_savings_verdict(&listings, 100);

        assert!(result.is_none());
    }

    #[test]
    fn item_view_savings_verdict_cross_world_savings() {
        let listings = vec![listing(1, 100, 5_000, false), listing(2, 200, 3_000, false)];

        let result = cheapest_savings_verdict(&listings, 100).unwrap();

        assert_eq!(result.cheapest_listing.id, 2);
        assert_eq!(result.cheapest_listing.world_id, 200);
        assert!(!result.cheapest_listing.hq);
        assert_eq!(result.current_world_listing.id, 1);
        assert_eq!(result.savings, 2_000);
        assert_eq!(result.savings_percent, 40.0);
    }

    #[test]
    fn item_view_savings_verdict_ignores_trivial_savings() {
        let listings = vec![
            listing(1, 100, 10_000, false),
            listing(2, 200, 9_001, false),
        ];

        let result = cheapest_savings_verdict(&listings, 100);

        assert!(result.is_none());
    }

    #[test]
    fn item_view_savings_verdict_matches_quality() {
        let listings = vec![
            listing(1, 100, 10_000, false),
            listing(2, 200, 9_000, false),
            listing(3, 100, 50_000, true),
            listing(4, 200, 20_000, true),
        ];

        let result = cheapest_savings_verdict(&listings, 100).unwrap();

        assert!(result.cheapest_listing.hq);
        assert_eq!(result.cheapest_listing.id, 4);
        assert_eq!(result.current_world_listing.id, 3);
        assert_eq!(result.savings, 30_000);
    }

    #[test]
    fn item_view_savings_verdict_requires_current_world_matching_quality() {
        let listings = vec![listing(1, 100, 10_000, false), listing(2, 200, 2_000, true)];

        let result = savings_verdict_for_quality(&listings, 100, true);

        assert!(result.is_none());
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
}
