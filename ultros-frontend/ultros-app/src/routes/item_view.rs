use crate::api::get_listings;
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::price_history_chart::PriceHistoryChart;
use crate::components::world_name::WorldName;
use crate::components::{
    ad::Ad, add_to_list::AddToList, clipboard::*, item_icon::*, listings_table::*, meta::*,
    recently_viewed::RecentItems, related_items::*, sale_history_table::*, skeleton::BoxSkeleton,
    stats_display::*, toggle::Toggle, ui_text::*,
};
use crate::error::AppError;
use crate::global_state::LocalWorldData;
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::global_state::home_world::{get_price_zone, locale_preferred_region, use_home_world};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string};
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use chrono::{TimeDelta, Utc};
use leptos::prelude::*;
use leptos_meta::{Link, Meta};
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use leptos_router::location::Url;
use std::sync::Arc;
use ultros_api_types::websocket::{
    EventType, FilterPredicate, ListingEventData, SaleEventData, ServerClient, SocketMessageType,
};
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::world_helper::{AnyResult, OwnedResult};
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer, SaleHistory};
use xiv_gen::{ItemId, ItemSearchCategoryId, ItemUiCategoryId};

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
    let (bg_color, other_styles) = match world {
        OwnedResult::Region(_) => (
            "bg-brand-500/10",
            "text-sm font-bold text-brand-200 px-3 py-1.5",
        ),
        OwnedResult::Datacenter(_) => (
            "bg-brand-500/15",
            "text-sm font-semibold text-brand-300 px-2.5 py-1",
        ),
        OwnedResult::World(_) => ("bg-transparent", "text-xs px-2 py-1"),
    };
    let is_selected = move || current_world.with(|w| w == world_3.as_str());
    let home_world_emphasis = move || {
        is_home_world.with(|w| {
            if *w {
                "border-2 border-brand-400 shadow-lg"
            } else {
                ""
            }
        })
    };
    view! {
        <A
            attr:class=move || {
                [
                    "rounded-md text-[color:var(--color-text)] flex items-center gap-1.5 transition-all duration-200 whitespace-nowrap",
                    bg_color,
                    other_styles,
                    "hover:bg-brand-500/15 hover:shadow-lg shadow-brand-900/20",
                    if is_selected() { "bg-brand-500/25 font-bold" } else { "" },
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
        <div class="flex flex-col gap-2 rounded-lg bg-brand-900/15 p-2">
            <h2 class="text-xs font-bold text-brand-200 px-1 uppercase tracking-wide">
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
            {active_datacenter
                .map(|dc| {
                    view! {
                        <h2 class="text-xs font-bold text-brand-200 px-1 uppercase tracking-wide">
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
        <div class="sticky top-0 z-10">
            <div class="container mx-auto px-4">
                <div class="panel">
                    <div class="flex flex-col gap-2 p-2">
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
                                <div class="w-full h-px bg-brand-700/40"></div>
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
        </div>
    }
    .into_any()
}

#[component]
fn MarketStatsPanel(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let cheapest_prices = use_context::<CheapestPrices>();

    view! {
        <Transition fallback=move || view! { <BoxSkeleton /> }>
            {move || {
                listing_resource
                    .with(|data_ref| {
                        if let Some(Ok(data)) = data_ref.as_ref() {
                            let data = data.clone();
                            let cheapest_nq = data
                                .listings
                                .iter()
                                .filter(|(listing, _)| !listing.hq)
                                .min_by_key(|(listing, _)| listing.price_per_unit)
                                .cloned();
                            let cheapest_hq = data
                                .listings
                                .iter()
                                .filter(|(listing, _)| listing.hq)
                                .min_by_key(|(listing, _)| listing.price_per_unit)
                                .cloned();
                            let listings_count = data.listings.len();
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
                                prices.sort_unstable();
                                Some(prices[prices.len() / 2])
                            };
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
                                            "text-amber-300 border-amber-400/40 bg-amber-500/10",
                                        )
                                    } else if exchange_exists {
                                        (
                                            t_string!(i18n, exchange_available).to_string(),
                                            view! { <span>{t!(i18n, exchange_available)}</span> }.into_any(),
                                            icondata::BsArrowLeftRight,
                                            "#exchange-sources",
                                            "text-purple-300 border-purple-400/40 bg-purple-500/10",
                                        )
                                    } else if recipe_exists {
                                        let summary_view = view! {
                                            <Suspense fallback=move || t_string!(i18n, craftable).to_string()>
                                                {move || {
                                                    if let Some(recipe) = recipe_tree_iter(ItemId(item_id)).next() {
                                                        if let Some(prices) = cheapest_prices.as_ref() {
                                                            prices.read_listings.with(|prices| {
                                                                let prices = prices.as_ref().and_then(|prices| prices.as_ref().ok());
                                                                if let Some(prices) = prices {
                                                                    let prices = prices.clone();
                                                                    let (hq, lq) = calculate_crafting_cost(recipe, &prices);
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
                                            "text-orange-300 border-orange-400/40 bg-orange-500/10",
                                        )
                                    } else {
                                        (
                                            t_string!(i18n, levequest_reward).to_string(),
                                            view! { t!(i18n, obtainable_via_levequest) }.into_any(),
                                            icondata::FaScrollSolid,
                                            "#leve-sources",
                                            "text-pink-300 border-pink-400/40 bg-pink-500/10",
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
                                            <h2 class="text-lg sm:text-xl font-bold text-[color:var(--color-text)] leading-tight">
                                                {t!(i18n, cheapest_found)}
                                            </h2>
                                            <p class="text-sm text-[color:var(--color-text-muted)]">
                                                {move || t!(i18n, based_on_sales, count = recent_sales.len())}
                                            </p>
                                        </div>
                                        <Icon icon=icondata::FaCoinsSolid attr:class="text-xl sm:text-2xl text-brand-300/70" />
                                    </div>

                                    <div class="grid grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2 gap-2 sm:gap-3">
                                        <a href="#listings" class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] p-2 sm:p-3 min-h-24">
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

                                        <a href="#listings" class="rounded-lg border border-[color:var(--color-outline)] bg-[#95c521]/10 p-2 sm:p-3 min-h-24">
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

                                        <a href="#history" class="rounded-lg border border-[color:var(--color-outline)] p-2 sm:p-3 min-h-24">
                                            <div class="text-xs font-bold uppercase text-blue-300 mb-1">{t!(i18n, recent_average)}</div>
                                            <div class="text-xl sm:text-2xl font-bold leading-none">
                                                {avg_price
                                                    .map(|price| view! { <Gil amount=price /> }.into_any())
                                                    .unwrap_or_else(|| view! { <span class="text-[color:var(--color-text-muted)]">{t!(i18n, no_data)}</span> }.into_any())}
                                            </div>
                                            <div class="text-xs text-[color:var(--color-text-muted)] mt-2">
                                                {t!(i18n, median_label)}
                                                " "
                                                {median_price
                                                    .map(|price| view! { <Gil amount=price /> }.into_any())
                                                    .unwrap_or_else(|| view! { <span>{t!(i18n, no_data)}</span> }.into_any())}
                                            </div>
                                        </a>

                                        <a href="#listings" class="rounded-lg border border-[color:var(--color-outline)] p-2 sm:p-3 min-h-24">
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
                                                <div role="status" class="rounded-lg border border-amber-700/40 bg-amber-900/30 px-3 py-2 text-sm text-amber-200">
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
    item_id: Memo<i32>,
    world: Memo<String>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let (hq_only, set_hq_only) = signal(false);
    let (filter_outliers, set_filter_outliers) = signal(true);
    let (days_range, set_days_range) = signal(30i32); // 0 = All

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
                        <div role="alert" class="bg-red-900/30 text-red-200 border border-red-700/40 rounded-xl p-4">
                            <strong class="font-semibold">{move || t_string!(i18n, error).to_string()} ":"</strong>
                            <span class="ml-2">{msg}</span>
                            <div class="text-sm text-red-300/80 mt-1">{move || t_string!(i18n, unable_to_load_recent_sales).to_string()}</div>
                        </div>
                    }.into_any()
                } else {
                    let base_sales = Memo::new(move |_| {
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
                        let days = days_range();
                        if days > 0 {
                            let cutoff = (Utc::now() - TimeDelta::days(days as i64)).naive_utc();
                            sales.retain(|s| s.sold_date >= cutoff);
                        }
                        sales
                    });

                    view! {
                        <div class="rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4 text-[color:var(--color-text)] h-full">
                            <div class="flex flex-col gap-3">
                                <div class="flex flex-wrap items-start justify-between gap-3">
                                    <div>
                                        <h2 class="text-xl font-bold leading-tight">{move || t_string!(i18n, sale_history).to_string()}</h2>
                                        <p class="text-sm text-[color:var(--color-text-muted)]">
                                            {move || t!(i18n, based_on_sales, count = base_sales.with(|sales| sales.len()))}
                                        </p>
                                    </div>
                                    <div class="flex flex-wrap items-center justify-end gap-2">
                                        <div class="inline-flex rounded-md overflow-hidden border border-[color:var(--color-outline)]">
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors",
                                                    if days_range() == 7 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(7)
                                            >
                                                {t!(i18n, chart_range_7d)}
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 30 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(30)
                                            >
                                                {t!(i18n, chart_range_30d)}
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 90 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(90)
                                            >
                                                {t!(i18n, chart_range_90d)}
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 0 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(0)
                                            >
                                                {t!(i18n, chart_range_all)}
                                            </button>
                                        </div>
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
                                        <a
                                            class="btn-primary text-sm"
                                            target="_blank"
                                            href=move || format!("/itemcard/{}/{}", world(), item_id())
                                        >
                                            {move || t_string!(i18n, download_png).to_string()}
                                        </a>
                                    </div>
                                </div>

                                {move || {
                                    if filtered_sales.with(|sales| sales.is_empty()) {
                                        view! {
                                            <div role="status" class="bg-amber-900/30 text-amber-200 border border-amber-700/40 rounded-xl p-4">
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
                                    let no_listings = listing_resource.with(|listing| {
                                        listing
                                            .as_ref()
                                            .and_then(|result| result.as_ref().ok())
                                            .map(|listing| listing.listings.is_empty())
                                            .unwrap_or(false)
                                    });
                                    no_listings.then(|| view! {
                                        <div role="status" class="bg-amber-900/30 text-amber-200 border border-amber-700/40 rounded-xl px-3 py-2 text-sm">
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
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        <div class="space-y-6">
            <Transition fallback=move || {
                view! { <BoxSkeleton /> }
            }>
                {move || {
                    let hq_listings = Memo::new(move |_| {
                        listing_resource
                            .with(|l| {
                                l.as_ref()
                                    .and_then(|l| {
                                        l.as_ref()
                                            .ok()
                                            .map(|l| {
                                                l.listings
                                                    .iter()
                                                    .filter(|(l, _)| l.hq)
                                                    .map(|(l, r)| (l.clone(), Arc::new(r.clone())))
                                                    .collect::<Vec<_>>()
                                            })
                                    })
                            })
                            .unwrap_or_default()
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
                }}
            </Transition>
        </div>
    }
    .into_any()
}

#[component]
fn LowQualityTable(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        <div class="space-y-6">
            <Transition fallback=move || {
                view! { <BoxSkeleton /> }
            }>
                {move || {
                    let lq_listings = Memo::new(move |_| {
                        listing_resource
                            .with(|l| {
                                l.as_ref()
                                    .and_then(|l| {
                                        l.as_ref()
                                            .ok()
                                            .map(|l| {
                                                l.listings
                                                    .iter()
                                                    .filter(|(l, _)| !l.hq)
                                                    .map(|(l, r)| (l.clone(), Arc::new(r.clone())))
                                                    .collect::<Vec<_>>()
                                            })
                                    })
                            })
                            .unwrap_or_default()
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

fn apply_listing_event(data: &mut CurrentlyShownItem, event: EventType<ListingEventData>) {
    match event {
        EventType::Added(event) | EventType::Updated(event) => {
            upsert_listings(data, event.listings);
        }
        EventType::Removed(event) => {
            remove_listings(data, event.listings);
        }
    }
    data.listings
        .sort_by_key(|(listing, _)| (listing.hq, listing.price_per_unit));
}

fn upsert_listings(data: &mut CurrentlyShownItem, listings: Vec<(ActiveListing, Retainer)>) {
    for incoming in listings {
        data.listings
            .retain(|(listing, _)| listing.id != incoming.0.id);
        data.listings.push(incoming);
    }
}

fn remove_listings(data: &mut CurrentlyShownItem, listings: Vec<(ActiveListing, Retainer)>) {
    for (removed, _) in listings {
        data.listings
            .retain(|(listing, _)| listing.id != removed.id);
    }
}

fn apply_sales_event(data: &mut CurrentlyShownItem, event: EventType<SaleEventData>) {
    match event {
        EventType::Added(event) | EventType::Updated(event) => {
            upsert_sales(
                data,
                event
                    .sales
                    .into_iter()
                    .map(|(sale, _)| sale)
                    .collect::<Vec<_>>(),
            );
        }
        EventType::Removed(event) => {
            for (removed, _) in event.sales {
                data.sales.retain(|sale| sale.id != removed.id);
            }
        }
    }
    data.sales
        .sort_by_key(|sale| std::cmp::Reverse(sale.sold_date));
    data.sales.truncate(200);
}

fn upsert_sales(data: &mut CurrentlyShownItem, sales: Vec<SaleHistory>) {
    for incoming in sales {
        data.sales.retain(|sale| sale.id != incoming.id);
        data.sales.push(incoming);
    }
}

#[component]
fn ListingsContent(item_id: Memo<i32>, world: Memo<String>) -> impl IntoView {
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
                ServerClient::Listings(event) => {
                    update_current_item(listing_resource, |data| {
                        apply_listing_event(data, event);
                    });
                }
                ServerClient::Stale { .. } => listing_resource.refetch(),
                _ => {}
            },
        );
        let sales_subscription = realtime.subscribe_market(
            filter,
            SocketMessageType::Sales,
            move |message| match message {
                ServerClient::Sales(event) => {
                    update_current_item(listing_resource, |data| {
                        apply_sales_event(data, event);
                    });
                }
                ServerClient::Stale { .. } => listing_resource.refetch(),
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
            <div id="history" class="grid grid-cols-1 xl:grid-cols-[minmax(320px,0.85fr)_minmax(0,1.45fr)] gap-4 sm:gap-6">
                <MarketStatsPanel listing_resource item_id />
                <ChartWrapper listing_resource item_id world />
            </div>

            <div id="listings" class="grid grid-cols-1 gap-6 mt-6">
                <HighQualityTable listing_resource />
                <LowQualityTable listing_resource />
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
    #[prop(into)] world_name: Signal<String>,
) -> impl IntoView {
    let command = Signal::derive(move || {
        format!(
            "/ffxiv prices current item:{} world:{}",
            item_name.get(),
            world_name.get(),
        )
    });
    view! {
        <div class="inline-flex items-center gap-2 rounded-md border border-brand-500/30 bg-black/30 px-2.5 py-1 text-xs">
            <span class="text-[color:var(--color-text-muted)]">"Discord:"</span>
            <code class="font-mono">{move || command.get()}</code>
            <Clipboard clipboard_text=command />
        </div>
    }
}

#[component]
pub fn ItemView() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let params = use_params_map();
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
                            <span class="bg-brand-900/40 text-brand-100 px-2 py-0.5 rounded text-sm font-bold border border-brand-700/50">
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
                <ListingsContent item_id world />
                <div class="mt-6">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
        </div>
    }.into_any()
}
