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
use crate::global_state::home_world::{get_price_zone, use_home_world};
use chrono::{TimeDelta, Utc};
use leptos::prelude::*;
use leptos_meta::{Link, Meta};
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use leptos_router::location::Url;
use std::sync::Arc;
use ultros_api_types::CurrentlyShownItem;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::world_helper::{AnyResult, OwnedResult};
use xiv_gen::ItemId;

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
            "text-lg font-bold text-brand-200 px-4 py-2",
        ),
        OwnedResult::Datacenter(_) => (
            "bg-brand-500/15",
            "text-base font-semibold text-brand-300 px-3 py-1.5",
        ),
        OwnedResult::World(_) => ("bg-transparent", "text-sm px-2 py-1"),
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
                    "rounded-md text-[color:var(--color-text)] flex items-center gap-2 transition-all duration-200",
                    bg_color,
                    other_styles,
                    "hover:scale-105 hover:shadow-lg shadow-brand-900/20",
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
    view! {
        <div class="flex flex-col gap-2 rounded-lg bg-brand-900/20 p-2">
            <h2 class="text-lg font-bold text-brand-200 px-2 py-1">
                "Datacenter"
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
                        <h2 class="text-lg font-bold text-brand-200 px-2 py-1">
                            "Worlds"
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

    view! {
        <div class="sticky top-0 z-10">
            <div class="container mx-auto px-4">
                <div class="panel">
                    <div class="flex flex-col gap-2 py-3">
                        {move || {
                            let world = world_name();
                            let world_name = Url::unescape(&world);
                            let all_regions = world_data.get_inner_data().regions.iter().map(|r| {
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
                                <div class="flex flex-wrap items-center gap-1">
                                    {all_regions.collect_view()}
                                    {(!home_world_in_region)
                                        .then(|| {
                                            view! { <HomeWorldButton current_world item_id /> }
                                        })}
                                </div>
                                <div class="w-full h-px bg-brand-700/50 my-1"></div>
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
fn SummaryCards(listing_resource: Resource<Result<CurrentlyShownItem, AppError>>) -> impl IntoView {
    view! {
        <Transition fallback=move || view! { <BoxSkeleton /> }>
            {move || {
                let data_ref = listing_resource.get();
                if let Some(Ok(data)) = data_ref.as_ref() {
                    let cheapest_nq = data.listings
                        .iter()
                        .filter(|(l, _)| !l.hq)
                        .min_by_key(|(l, _)| l.price_per_unit)
                        .cloned();

                    let cheapest_hq = data.listings
                        .iter()
                        .filter(|(l, _)| l.hq)
                        .min_by_key(|(l, _)| l.price_per_unit)
                        .cloned();

                    let recent_sales = &data.sales;
                    let avg_price = if !recent_sales.is_empty() {
                        recent_sales.iter().map(|s| s.price_per_item as i64).sum::<i64>() / recent_sales.len() as i64
                    } else {
                        0
                    };
                    let listings_count = data.listings.len();
                    let has_nq = cheapest_nq.is_some();

                    view! {
                         <div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
                            // Card 1: Cheapest Found
                             <a href="#listings" class="panel p-4 border-l-4 border-l-brand-500 hover:scale-[1.02] transition-all cursor-pointer group bg-gradient-to-br from-brand-900/50 to-transparent">
                                 <div class="flex justify-between items-start">
                                     <div>
                                         <div class="text-xs font-bold text-brand-300 uppercase tracking-wider mb-2">"Cheapest Found"</div>
                                         <div class="flex flex-col gap-3">
                                             // NQ Display
                                             {if let Some((listing, _retainer)) = cheapest_nq {
                                                     view! {
                                                         <div>
                                                             <div class="flex items-center gap-2">
                                                                 <span class="text-xs font-bold text-brand-400 bg-brand-900/50 px-1.5 py-0.5 rounded border border-brand-700/50">"NQ"</span>
                                        <div class="text-xl font-bold text-[color:var(--color-text)]">
                                                                     <Gil amount=listing.price_per_unit />
                                                                 </div>
                                                             </div>
                                                         <div class="text-xs text-brand-200 mt-0.5 flex items-center gap-1 opacity-80">
                                                             <Icon icon=icondata::FaGlobeSolid attr:class="text-[10px]" />
                                                             <WorldName id=AnySelector::World(listing.world_id) />
                                                         </div>
                                                     </div>
                                                 }.into_any()
                                             } else {
                                                 // Don't show "No NQ" if HQ exists to avoid clutter, or maybe small text?
                                                 // If ONLY HQ exists, it will pop.
                                                 match cheapest_hq {
                                                    None => view! { <div class="text-lg text-gray-400 italic">"No listings"</div> }.into_any(),
                                                    _ => ().into_any()
                                                 }
                                             }}

                                             // HQ Display
                                             {if let Some((listing, _retainer)) = cheapest_hq {
                                                 view! {
                                                     <div class="relative">
                                                         // Add a separator if NQ also exists
                                                         <Show when=move || has_nq>
                                                             <div class="absolute -top-1.5 left-0 w-8 border-t border-brand-700/30"></div>
                                                         </Show>
                                                         <div class="flex items-center gap-2">
                                                             <span class="text-xs font-bold text-[#95c521] bg-[#95c521]/10 px-1.5 py-0.5 rounded border border-[#95c521]/20 flex items-center gap-1">
                                                                 <Icon icon=icondata::FaStarSolid attr:class="text-[9px]" />
                                                                 "HQ"
                                                             </span>
                                        <div class="text-xl font-bold text-[color:var(--color-text)]">
                                                                 <Gil amount=listing.price_per_unit />
                                                             </div>
                                                         </div>
                                                         <div class="text-xs text-brand-200 mt-0.5 flex items-center gap-1 opacity-80">
                                                             <Icon icon=icondata::FaGlobeSolid attr:class="text-[10px]" />
                                                             <WorldName id=AnySelector::World(listing.world_id) />
                                                         </div>
                                                     </div>
                                                 }.into_any()
                                             } else {
                                                 ().into_any()
                                             }}
                                         </div>
                                     </div>
                                     <Icon icon=icondata::FaCoinsSolid attr:class="text-3xl text-brand-500/20 group-hover:text-brand-500/40 transition-colors" />
                                 </div>
                             </a>

                            // Card 2: Recent History
                            <a href="#history" class="panel p-4 border-l-4 border-l-blue-500 hover:scale-[1.02] transition-all cursor-pointer group bg-gradient-to-br from-blue-900/20 to-transparent">
                                 <div class="flex justify-between items-start">
                                     <div>
                                    <div class="text-xs font-bold text-blue-700 dark:text-blue-300 uppercase tracking-wider mb-1">"Recent Average"</div>
                                    <div class="text-2xl font-bold text-[color:var(--color-text)]">
                                            {if avg_price > 0 {
                                                view! { <Gil amount=avg_price as i32 /> }.into_any()
                                            } else {
                                                view! { <span class="text-gray-400">"No Data"</span> }.into_any()
                                            }}
                                         </div>
                                         <div class="text-sm text-blue-700 dark:text-blue-200 mt-1">
                                             {format!("Based on {} sales", recent_sales.len())}
                                         </div>
                                         <div class="text-sm text-blue-700 dark:text-blue-200 mt-1">
                                             {
                                                 if recent_sales.len() > 1 {
                                                     let newest = recent_sales.first().unwrap().sold_date;
                                                     let oldest = recent_sales.last().unwrap().sold_date;
                                                     let seconds = (newest - oldest).num_seconds().abs();
                                                     let count = recent_sales.len() - 1;

                                                     if seconds > 0 {
                                                         let seconds_per_sale = seconds as f64 / count as f64;
                                                         if seconds_per_sale < 60.0 {
                                                             format!("Sells ~{:.1} times per minute", 60.0 / seconds_per_sale)
                                                         } else if seconds_per_sale < 3600.0 {
                                                             format!("Sells ~{:.1} times per hour", 3600.0 / seconds_per_sale)
                                                         } else if seconds_per_sale < 86400.0 {
                                                             format!("Sells ~{:.1} times per day", 86400.0 / seconds_per_sale)
                                                         } else {
                                                             format!("Sells ~1 every {:.1} days", seconds_per_sale / 86400.0)
                                                         }
                                                     } else {
                                                         "Very high frequency".to_string()
                                                     }
                                                 } else {
                                                     "Not enough data".to_string()
                                                 }
                                             }
                                         </div>
                                     </div>
                                     <Icon icon=icondata::FaChartLineSolid attr:class="text-3xl text-blue-500/20 group-hover:text-blue-500/40 transition-colors" />
                                 </div>
                            </a>

                            // Card 3: Active Listings
                            <a href="#listings" class="panel p-4 border-l-4 border-l-emerald-500 hover:scale-[1.02] transition-all cursor-pointer group bg-gradient-to-br from-emerald-900/20 to-transparent">
                                 <div class="flex justify-between items-start">
                                     <div>
                                    <div class="text-xs font-bold text-emerald-700 dark:text-emerald-300 uppercase tracking-wider mb-1">"Active Listings"</div>
                                    <div class="text-2xl font-bold text-[color:var(--color-text)]">
                                             {listings_count}
                                         </div>
                                    <div class="text-sm text-emerald-700 dark:text-emerald-200 mt-1">
                                             "Available now"
                                         </div>
                                     </div>
                                     <Icon icon=icondata::FaListSolid attr:class="text-3xl text-emerald-500/20 group-hover:text-emerald-500/40 transition-colors" />
                                 </div>
                            </a>
                         </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}
        </Transition>
    }.into_any()
}

#[component]
pub fn ChartWrapper(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
    item_id: Memo<i32>,
    world: Memo<String>,
) -> impl IntoView {
    let (hq_only, set_hq_only) = signal(false);
    let (days_range, set_days_range) = signal(30i32); // 0 = All

    /* moved into Transition branch to avoid reading resource outside Suspense/Transition */

    view! {
        <Transition fallback=move || {
            view! {
                <div class="animate-pulse panel h-[35em] text-[color:var(--color-text)]">
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
                            <strong class="font-semibold">"Error:"</strong>
                            <span class="ml-2">{msg}</span>
                            <div class="text-sm text-red-300/80 mt-1">"Unable to load recent sales. Please try refreshing."</div>
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
                        <div class="space-y-4">
                            <div class="panel p-4 text-[color:var(--color-text)]">
                                <div class="flex flex-wrap items-center justify-between gap-3">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <div class="inline-flex rounded-md overflow-hidden border border-[color:var(--color-outline)]">
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors",
                                                    if days_range() == 7 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(7)
                                            >
                                                "7d"
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 30 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(30)
                                            >
                                                "30d"
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 90 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(90)
                                            >
                                                "90d"
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 0 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(0)
                                            >
                                                "All"
                                            </button>
                                        </div>
                                        <div class="ml-2">
                                            <Toggle
                                                checked=hq_only
                                                set_checked=set_hq_only
                                                checked_label="HQ only"
                                                unchecked_label="All qualities"
                                            />
                                        </div>
                                    </div>
                                    <a
                                        class="btn-primary"
                                        target="_blank"
                                        href=move || format!("/itemcard/{}/{}", world(), item_id())
                                    >
                                        "Download PNG"
                                    </a>
                                </div>
                            </div>

                            {move || {
                                if filtered_sales.with(|s| s.is_empty()) {
                                    view! {
                                        <div role="status" class="bg-amber-900/30 text-amber-200 border border-amber-700/40 rounded-xl p-4">
                                            "No sales found for the selected filters. Try expanding the time range or disabling HQ-only."
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="panel p-6 text-[color:var(--color-text)]">
                                            <PriceHistoryChart sales=filtered_sales />
                                        </div>
                                    }.into_any()
                                }
                            }}

                            {move || {
                                let no_listings = listing_resource.with(|l| {
                                    l.as_ref().and_then(|r| r.as_ref().ok()).map(|l| l.listings.is_empty()).unwrap_or(false)
                                });
                                no_listings.then(|| view! {
                                    <div role="status" class="bg-amber-900/30 text-amber-200 border border-amber-700/40 rounded-xl p-4">
                                        "No active listings found for this world. Try checking other worlds or come back later."
                                    </div>
                                })
                            }}
                        </div>
                    }.into_any()
                }
            }}
        </Transition>
    }.into_any()
}

#[component]
fn HighQualityTable(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
) -> impl IntoView {
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
                            class="panel p-4 sm:p-6"
                            class:hidden=move || hq_listings.with(|l| l.is_empty())
                        >
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                "High Quality Listings"
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
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
) -> impl IntoView {
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
                            class="panel p-4 sm:p-6"
                            class:hidden=move || lq_listings.with(|l| l.is_empty())
                        >
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                "Low Quality Listings"
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
fn SalesDetails(listing_resource: Resource<Result<CurrentlyShownItem, AppError>>) -> impl IntoView {
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
                        <div class="panel p-4 sm:p-6 flex-1">
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                "Sale History"
                            </h2>
                            <SaleHistoryTable sales=sales.into() />
                        </div>

                        <div class="panel p-4 sm:p-6">
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

#[component]
fn ListingsContent(item_id: Memo<i32>, world: Memo<String>) -> impl IntoView {
    let listing_resource = Resource::new(
        move || (item_id(), world()),
        |(item_id, world)| async move {
            get_listings(item_id, world.as_str())
                .await
                .inspect_err(|e| tracing::error!(error = ?e, "Error getting value"))
        },
    );
    Effect::new(move |_| {
        let val = listing_resource.get();
        tracing::info!(?val, "Listings updated");
    });
    view! {
        <div class="w-full py-8 text-[color:var(--color-text)]">
            <SummaryCards listing_resource />

            <div id="listings" class="grid grid-cols-1 gap-6">
                <HighQualityTable listing_resource />
                <LowQualityTable listing_resource />
            </div>

            <div id="history" class="grid grid-cols-1 lg:grid-cols-2 gap-6 mt-8">
                 <div class="lg:sticky lg:top-24 h-fit"> // Make chart sticky? Or just normal col.
                     <ChartWrapper listing_resource item_id world />
                 </div>
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
pub fn ItemView() -> impl IntoView {
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

    let data = &xiv_gen_db::data();
    let items = &data.items;
    let categories = &data.item_ui_categorys;
    let search_categories = &data.item_search_categorys;
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

    let item_name = move || {
        items
            .get(&ItemId(item_id()))
            .map(|item| item.name.as_str())
            .unwrap_or_default()
    };

    let item = move || items.get(&ItemId(item_id()));

    let item_description = move || {
        items
            .get(&ItemId(item_id()))
            .map(|item| item.description.as_str())
            .unwrap_or_default()
    };

    let item_category = move || {
        items
            .get(&ItemId(item_id()))
            .and_then(|item| categories.get(&item.item_ui_category))
    };

    let item_search_category = move || {
        items
            .get(&ItemId(item_id()))
            .and_then(|item| search_categories.get(&item.item_search_category))
    };

    let description = Memo::new(move |_| {
        format!(
            "Current market board listings for {} within {}. Find the lowest prices in your region.",
            item_name(),
            world(),
        )
    });

    view! {
        <MetaDescription text=description />
        <MetaTitle title=move || {
            format!("{} - 🌍{} - Market board - Ultros", item_name(), world())
        } />
        <Meta name="twitter:card" content="summary_large_image" />
        <MetaImage url=move || format!("https://ultros.app/itemcard/{}/{}", world(), item_id()) />
        <Meta
            property="thumbnail"
            content=move || format!("https://ultros.app/static/itemicon/{}?size=Large", item_id())
        />
        <Link rel="canonical" prop:href=move || format!("https://ultros.app/item/{}", item_id()) />
        <div class="min-h-screen">
            <div class="w-full px-0 sm:px-4 py-4 sm:py-6">
                <div class="flex flex-col gap-6 p-4 sm:p-6 panel">
                    <div class="flex flex-col md:flex-row items-start gap-4">
                        <div class="flex items-center gap-4 flex-1">
                            <ItemIcon item_id icon_size=IconSize::Large />
                            <div class="flex flex-col">
                                <h1 class="text-3xl font-bold text-[color:var(--color-text)] flex items-center gap-2">
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
                            </div>
                        </div>

                        <div class="flex flex-wrap gap-2 items-center">
                            <div class="cursor-pointer"><AddToList item_id /></div>
                            <a
                                class="btn-primary"
                                target="_blank"
                                rel="noopener noreferrer"
                                aria-label="Open Universalis market page in a new tab"
                                href=move || format!("https://universalis.app/market/{}", item_id())
                            >
                                "Universalis"
                            </a>
                            <a
                                class="btn-primary"
                                target="_blank"
                                rel="noopener noreferrer"
                                aria-label="Open Garlandtools item page in a new tab"
                                href=move || format!("https://garlandtools.org/db/#item/{}", item_id())
                            >
                                "Garlandtools"
                            </a>
                        </div>
                    </div>

                    // Moved Description and Item Level here
                    <div class="space-y-3 pt-4 border-t border-[color:var(--color-outline)] text-[color:var(--color-text)]/90">
                        <div class="flex items-center gap-2">
                            <span class="text-brand-300 font-medium tracking-wide text-sm uppercase">Item Level</span>
                            <span class="bg-brand-900/40 text-brand-100 px-2 py-0.5 rounded text-sm font-bold border border-brand-700/50">
                                {move || item().map(|item| item.level_item.0).unwrap_or_default()}
                            </span>
                            <div class="flex-grow"></div>
                             <div>{move || view! { <ItemStats item_id=ItemId(item_id()) /> }}</div>
                        </div>
                        <div
                            class=""
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
                <div class="mt-6 panel p-3">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
        </div>
    }.into_any()
}
