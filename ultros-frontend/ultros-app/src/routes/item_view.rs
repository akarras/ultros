use crate::api::get_listings;
use crate::components::price_history_chart::PriceHistoryChart;
use crate::components::{
    ad::Ad, add_to_list::AddToList, clipboard::*, item_icon::*, listings_table::*, meta::*,
    recently_viewed::RecentItems, related_items::*, sale_history_table::*, skeleton::BoxSkeleton,
    stats_display::*, ui_text::*,
};
use crate::error::AppError;
use crate::global_state::home_world::{get_price_zone, use_home_world};
use crate::global_state::LocalWorldData;
use leptos::either::{Either, EitherOf3};
use leptos::prelude::*;
use leptos_icons::Icon;
use leptos_meta::{Link, Meta};
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use leptos_router::location::Url;
use ultros_api_types::world_helper::{AnyResult, OwnedResult};
use ultros_api_types::CurrentlyShownItem;
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
                .with(|w| w.as_ref().map(|w| &w.name == &world_2))
                .unwrap_or_default()
        }
    });
    let bg_color = match world {
        OwnedResult::Region(_) => "bg-brand-600/15 hover:bg-brand-600/25",
        OwnedResult::Datacenter(_) => "bg-brand-600/15 hover:bg-brand-600/25",
        OwnedResult::World(_) => "bg-brand-600/15 hover:bg-brand-600/25",
    };
    let is_selected = move || current_world.with(|w| w == world_3.as_str());
    view! {
        <div>
            <A
                attr:class=move || {
                    [
                        "rounded-md text-sm px-4 py-2 text-[color:var(--color-text)] mx-1 flex items-center gap-2 transition-all duration-200",
                        bg_color,
                        "hover:scale-105 hover:shadow-lg shadow-brand-900/20",
                        if is_selected() { "font-bold" } else { "" },
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
        </div>
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
fn WorldMenu(world_name: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    let current_world = world_name;
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let (home_world, _) = use_home_world();

    view! {
        <div class="sticky top-0 z-10">
            <div class="container mx-auto px-4">
                <div class="panel">
                <div class="flex flex-wrap gap-2 py-3">
                    {move || {
                        let world = world_name();
                        let world_name = Url::unescape(&world);
                        if let Some(world) = world_data.lookup_world_by_name(&world_name) {
                            Either::Left(
                                match world {
                                    AnyResult::World(world) => {
                                        let region = world_data.get_region(AnyResult::World(world));
                                        let datacenters = world_data
                                            .get_datacenters(&AnyResult::World(world));
                                        let views = [AnyResult::Region(region)]
                                            .into_iter()
                                            .chain(
                                                datacenters.iter().map(|dc| AnyResult::Datacenter(dc)),
                                            )
                                            .chain(
                                                datacenters
                                                    .iter()
                                                    .flat_map(|dc| dc.worlds.iter().map(AnyResult::World)),
                                            )
                                            .map(move |world| {
                                                view! {
                                                    <WorldButton current_world world item_id=item_id() />
                                                }
                                            })
                                            .collect_view();
                                        EitherOf3::A(views)
                                    }
                                    AnyResult::Datacenter(dc) => {
                                        let region = world_data
                                            .get_region(AnyResult::Datacenter(dc));
                                        let views = [AnyResult::Region(region)]
                                            .into_iter()
                                            .map(|w| Either::Left(
                                                view! {
                                                    <WorldButton current_world world=w item_id=item_id() />
                                                },
                                            ))
                                            .chain([Either::Right(view! { <div class="w-2"></div> })])
                                            .chain(
                                                region
                                                    .datacenters
                                                    .iter()
                                                    .map(|dc| Either::Left(
                                                        view! {
                                                            <WorldButton
                                                                current_world
                                                                world=AnyResult::Datacenter(dc)
                                                                item_id=item_id()
                                                            />
                                                        },
                                                    )),
                                            )
                                            .chain([Either::Right(view! { <div class="w-2"></div> })])
                                            .chain(
                                                dc
                                                    .worlds
                                                    .iter()
                                                    .map(|w| Either::Left(
                                                        view! {
                                                            <WorldButton
                                                                current_world
                                                                world=AnyResult::World(w)
                                                                item_id=item_id()
                                                            />
                                                        },
                                                    )),
                                            )
                                            .collect_view();
                                        let should_show_homeworld = !dc
                                            .worlds
                                            .iter()
                                            .any(|w| {
                                                home_world
                                                    .with_untracked(|world| {
                                                        world
                                                            .as_ref()
                                                            .map(|world| world.name == w.name)
                                                            .unwrap_or_default()
                                                    })
                                            });
                                        EitherOf3::B(
                                            view! {
                                                {views}
                                                <div class="w-2"></div>
                                                {should_show_homeworld
                                                    .then(|| {
                                                        view! { <HomeWorldButton current_world item_id /> }
                                                    })}
                                            },
                                        )
                                    }
                                    AnyResult::Region(region) => {
                                        let regions = world_data
                                            .get_inner_data()
                                            .regions
                                            .iter()
                                            .map(|r| {
                                                Either::Left(
                                                    view! {
                                                        <WorldButton
                                                            current_world
                                                            world=AnyResult::Region(r)
                                                            item_id=item_id()
                                                        />
                                                    },
                                                )
                                            });
                                        let datacenters = world_data
                                            .get_datacenters(&AnyResult::Region(region));
                                        let views = regions
                                            .chain([Either::Right(view! { <div class="w-2"></div> })])
                                            .chain(
                                                datacenters
                                                    .iter()
                                                    .map(|dc| Either::Left(
                                                        view! {
                                                            <WorldButton
                                                                current_world
                                                                world=AnyResult::Datacenter(dc)
                                                                item_id=item_id()
                                                            />
                                                        },
                                                    )),
                                            )
                                            .collect_view();
                                        EitherOf3::C(
                                            view! {
                                                {views}
                                                <div class="w-2"></div>
                                                <HomeWorldButton current_world item_id />
                                            },
                                        )
                                    }
                                },
                            )
                        } else {
                            let regions = world_data
                                .get_inner_data()
                                .regions
                                .iter()
                                .map(|r| {
                                    view! {
                                        <WorldButton
                                            current_world
                                            world=AnyResult::Region(r)
                                            item_id=item_id()
                                        />
                                    }
                                });
                            let region = world_data.lookup_world_by_name("North-America").unwrap();
                            let datacenters = world_data.get_datacenters(&region);
                            let views = regions
                                .chain(
                                    datacenters
                                        .iter()
                                        .map(|dc| {
                                            view! {
                                                <WorldButton
                                                    current_world
                                                    world=AnyResult::Datacenter(dc)
                                                    item_id=item_id()
                                                />
                                            }
                                        }),
                                )
                                .collect_view();
                            Either::Right(views)
                        }
                    }}
                </div>
            </div>
            </div>
        </div>
    }.into_any()
}

#[component]
pub fn ChartWrapper(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
) -> impl IntoView {
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
                    let sales = Memo::new(move |_| {
                        listing_resource
                            .with(|l| l.as_ref().and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok()))
                            .unwrap_or_default()
                    });
                    view! {
                        <div class="space-y-4">
                            <div class="panel p-6 text-[color:var(--color-text)]">
                                <PriceHistoryChart sales=sales />
                            </div>
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
                                                    .cloned()
                                                    .filter(|(l, _)| l.hq)
                                                    .collect::<Vec<_>>()
                                            })
                                    })
                            })
                            .unwrap_or_default()
                    });
                    view! {
                        <div
                            class="panel p-6"
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
                                                    .cloned()
                                                    .filter(|(l, _)| !l.hq)
                                                    .collect::<Vec<_>>()
                                            })
                                    })
                            })
                            .unwrap_or_default()
                    });
                    view! {
                        <div
                            class="panel p-6"
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
        <div class="mt-8 space-y-6">
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
                        <div class="space-y-6">
                            <div class="panel p-6">
                                <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                    "Sale History"
                                </h2>
                                <SaleHistoryTable sales=sales.into() />
                            </div>

                            <div class="panel p-6">
                                <SalesInsights sales=sales.into() />
                            </div>
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
        <div class="container mx-auto px-4 py-8">
            <div class="grid grid-cols-1 gap-6">
                <ChartWrapper listing_resource />
                <HighQualityTable listing_resource />
                <LowQualityTable listing_resource />
            </div>
            <SalesDetails listing_resource />

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
            <div class="container mx-auto px-4 py-6">
                <div class="flex flex-col md:flex-row items-start gap-4 p-4 panel">
                    <div class="flex items-center gap-4">
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

                    <div class="md:ml-auto flex flex-wrap gap-2 items-center">
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

                <div class="mt-4 space-y-3 text-[color:var(--color-text)]/90">
                    <div class="flex items-center gap-2">
                        <span class="text-brand-300">Item Level:</span>
                        <span class="bg-brand-900/30 px-2 py-1 rounded">
                            {move || item().map(|item| item.level_item.0).unwrap_or_default()}
                        </span>
                    </div>
                    <div
                        class="panel p-4 "
                        class:hidden=move || { item_description().is_empty() }
                    >
                        {move || view! { <UIText text=item_description().to_string() /> }}
                    </div>
                    <div>{move || view! { <ItemStats item_id=ItemId(item_id()) /> }}</div>
                </div>
            </div>

            <WorldMenu world_name=world item_id />

            <div class="main-content">
                <ListingsContent item_id world />
                <div class="mt-6 panel p-3">
                    <RelatedItems item_id=Signal::from(item_id) />
                </div>
            </div>
        </div>
    }.into_any()
}
