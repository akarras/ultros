use crate::api::get_listings;
use crate::components::ad::Ad;
use crate::components::add_to_list::AddToList;
use crate::components::recently_viewed::RecentItems;
use crate::components::skeleton::BoxSkeleton;
use crate::components::{
    clipboard::*, gil::*, item_icon::*, listings_table::*, meta::*, price_history_chart::*,
    related_items::*, sale_history_table::*, stats_display::*, ui_text::*,
};
use crate::global_state::home_world::{get_price_zone, use_home_world};
use crate::global_state::LocalWorldData;
use leptos::*;
use leptos_icons::Icon;
use leptos_meta::Link;
use leptos_meta::Meta;
use leptos_router::*;
use ultros_api_types::world_helper::AnyResult;
use xiv_gen::ItemId;

#[component]
fn WorldButton<'a>(world: AnyResult<'a>, item_id: i32) -> impl IntoView {
    let (home_world, _) = use_home_world();
    let world_name = world.get_name().to_string();
    let is_home_world = Signal::derive(move || {
        home_world
            .with(|w| w.as_ref().map(|w| &w.name == &world_name))
            .unwrap_or_default()
    });
    let world_name = world.get_name().to_owned();
    let bg_color = match world {
        AnyResult::Region(_r) => "bg-violet-950 hover:bg-violet-700",
        AnyResult::Datacenter(_d) => "bg-violet-900 hover:bg-violet-700",
        AnyResult::World(_w) => "bg-violet-800 hover:bg-violet-700",
    };
    view! {
        <A
            class=[
                "rounded-t-md text-sm p-2 text-md aria-current:font-bold aria-current:text-white mx-1 flex flex-row color-white-950 ",
                bg_color,
            ]
                .concat()
            href=format!("/item/{}/{item_id}", escape(&world_name))
        >
            {move || is_home_world.get().then(|| view! { <Icon icon=icondata::AiHomeFilled/><div class="w-1"></div> })}

            {world_name}
        </A>
    }
}

#[component]
fn WorldMenu(world_name: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    // for some reason the context version doesn't work
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let (home_world, _) = use_home_world();
    let home_world_button = Signal::derive(move || {
        home_world
            .get()
            .map(|world| view! { <WorldButton world=AnyResult::World(&world) item_id=item_id()/> })
    });
    view! {
        <div class="content-nav">
        {move || {
        let world = world_name();
        let world_name = escape(&world);
        if let Some(world) = world_data.lookup_world_by_name(&world_name) {
            let create_world_button = move |world| view! { <WorldButton world item_id=item_id()/> };
            match world {
                AnyResult::World(world) => {
                    // display the region, datacenter, and sibling worlds to this datacenter (excluding this world)
                    let region = world_data.get_region(AnyResult::World(world));
                    let datacenters = world_data.get_datacenters(&AnyResult::World(world));
                    let views: Vec<_> = [AnyResult::Region(region)]
                        .into_iter()
                        .chain(datacenters.iter().map(|dc| AnyResult::Datacenter(dc)))
                        .chain(
                            datacenters
                                .iter()
                                .flat_map(|dc| dc.worlds.iter().map(AnyResult::World)),
                        )
                        .map(move |world| view! { <WorldButton world item_id=item_id()/> })
                        .collect();
                    views.into_view()
                }
                AnyResult::Datacenter(dc) => {
                    // show all worlds in this datacenter, other datacenters in this region, the region this datacenter belongs to
                    let region = world_data.get_region(AnyResult::Datacenter(dc));
                    let views: Vec<_> = [AnyResult::Region(region)]
                        .into_iter()
                        .map(create_world_button)
                        .chain([view! { <div class="w-2"></div> }.into_view()].into_iter())
                        .chain(region.datacenters.iter().map(AnyResult::Datacenter).map(create_world_button))
                        .chain([view! { <div class="w-2"></div> }.into_view()].into_iter())
                        .chain(dc.worlds.iter().map(AnyResult::World).map(create_world_button))
                        .collect();
                    let should_show_homeworld = !dc.worlds.iter().any(|w| home_world.with(|world| world.as_ref().map(|world| world.name == w.name).unwrap_or_default()));
                    view! {
                        {views}
                        // padding
                        <div class="w-2"></div>
                        {should_show_homeworld.then(|| home_world_button)}
                    }.into_view()
                }
                AnyResult::Region(region) => {
                    // show all regions, and datacenters in this region
                    let regions = world_data
                        .get_inner_data()
                        .regions
                        .iter()
                        .map(AnyResult::Region)
                        .map(create_world_button);
                    let datacenters = world_data.get_datacenters(&AnyResult::Region(region));
                    let views: Vec<_> = regions
                        .chain([view! { <div class="w-2"></div> }.into_view()].into_iter())
                        .chain(datacenters.iter().map(|dc| AnyResult::Datacenter(dc)).map(create_world_button))
                        .collect();
                    view! {
                        {views}
                        // padding
                        <div class="w-2"></div>
                        {home_world_button}
                    }.into_view()
                }
            }
        } else {
            let regions = world_data
                .get_inner_data()
                .regions
                .iter()
                .map(|r| AnyResult::Region(r));
            let region = world_data.lookup_world_by_name("North-America").unwrap();
            let datacenters = world_data.get_datacenters(&region);
            let datacenters = datacenters.iter().map(|dc| AnyResult::Datacenter(dc));
            let views: Vec<_> = regions
                .chain(datacenters)
                .map(move |world| view! { <WorldButton world item_id=item_id()/> })
                .collect();
            views.into_view()
        }
    }}
    </div>
    }
}

#[component]
fn ListingsContent(item_id: Memo<i32>, world: Memo<String>) -> impl IntoView {
    let listing_resource = create_resource(
        move || (item_id(), world()),
        move |(item_id, world)| async move { get_listings(item_id, &world).await },
    );
    let _class_opacity = "opacity-0 opacity-50"; // this is just here to get tailwind to compile
    view! {
        <Transition fallback=move || {
            view! {
                <div class="h-[35em] grow w-screen md:w-[780px]">
                    <BoxSkeleton/>
                </div>
            }
        }>
            {move || {
                let sales = create_memo(move |_| {
                    listing_resource
                        .with(|l| l.as_ref().and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok()))
                        .unwrap_or_default()
                });
                view! {
                    <div class="content-well max-h-[35em] overflow-y-auto grow">
                        <PriceHistoryChart sales=MaybeSignal::from(sales)/>
                    </div>
                }
            }}

        </Transition>
        <Transition fallback=move || {
            view! {
                <div class="h-[35em] grow w-screen md:w-[780px]">
                    <BoxSkeleton/>
                </div>
            }
        }>
            {move || {
                let hq_listings = create_memo(move |_| {
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
                        class="content-well max-h-[35em] overflow-y-auto grow"
                        class:hidden=move || hq_listings.with(|l| l.is_empty())
                    >
                        <div class="content-title text-center">"high quality listings"</div>
                        <ListingsTable listings=hq_listings/>
                    </div>
                }
            }}

        </Transition>
        <Transition fallback=move || {
            view! {
                <div class="h-[35em] grow w-screen md:w-[780px]">
                    <BoxSkeleton/>
                </div>
            }
        }>
            {move || {
                let lq_listings = create_memo(move |_| {
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
                        class="content-well max-h-[35em] overflow-y-auto grow"
                        class:hidden=move || lq_listings.with(|l| l.is_empty())
                    >
                        <div class="content-title text-center">"low quality listings"</div>
                        <ListingsTable listings=lq_listings/>
                    </div>
                }
            }}

        </Transition>
        <Transition fallback=move || {
            view! {
                <div class="h-[35em] grow w-screen md:w-[780px] xl:basis-1/2">
                    <BoxSkeleton/>
                </div>
            }
        }>
            {move || {
                let sales = create_memo(move |_| {
                    listing_resource
                        .with(|l| l.as_ref().and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok()))
                        .unwrap_or_default()
                });
                view! {
                    <div class="content-well max-h-[35em] overflow-y-auto xl:basis-1/2">
                        <div class="content-title text-center">"sale history"</div>
                        <div>
                            <SaleHistoryTable sales=Signal::from(sales)/>
                        </div>
                    </div>
                }
            }}

        </Transition>
        <Transition fallback=move || {
            view! {
                <div class="h-[35em] grow w-screen md:w-[780px]">
                    <BoxSkeleton/>
                </div>
            }
        }>
            {move || {
                let sales = create_memo(move |_| {
                    listing_resource
                        .with(|l| l.as_ref().and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok()))
                        .unwrap_or_default()
                });
                view! {
                    <div class="content-well max-h-[35em] overflow-y-auto xl:basis-1/2">
                        <SalesInsights sales=Signal::from(sales)/>
                    </div>
                }
            }}

        </Transition>
        <Ad class="h-[336px] w-[280px]"/>
    }
}

#[component]
pub fn ItemView() -> impl IntoView {
    // get world and item id from scope
    let params = use_params_map();

    let item_id = create_memo(move |_| {
        params()
            .get("id")
            .and_then(|id| id.parse::<i32>().ok())
            .unwrap_or_default()
    });
    let recently_viewed = use_context::<RecentItems>().unwrap();
    create_effect(move |_| {
        recently_viewed.add_item(item_id());
    });
    let data = &xiv_gen_db::data();
    let items = &data.items;
    let (price_zone, _) = get_price_zone();
    let world = create_memo(move |_| {
        params.with(|p| {
            p.get("world").cloned().unwrap_or_else(move || {
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
    let item_description = move || {
        items
            .get(&ItemId(item_id()))
            .map(|item| item.description.as_str())
            .unwrap_or_default()
    };
    let categories = &data.item_ui_categorys;
    let search_categories = &data.item_search_categorys;

    let region_type = move || {
        let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
        let target = world_data.lookup_world_by_name(&world());
        match target {
            Some(AnyResult::Region(_)) => "region",
            Some(AnyResult::Datacenter(_)) => "datacenter",
            Some(AnyResult::World(_)) => "world",
            None => "unknown",
        }
    };
    let description = create_memo(move |_| {
        format!(
            "Current market board listings the item {} within the {} {}. Find the lowest prices in your region. View all crafting recipes & associated cost to craft. Explore related items",
            item_name(),
            region_type(),
            world(),
        )
    });
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
    let item = move || items.get(&ItemId(item_id()));
    view! {
        <MetaDescription text=description/>
        <MetaTitle title=move || {
            format!("{} - 🌍{} - Market board - Ultros", item_name(), world())
        }/>
        // TODO: probably shouldn't hard code the domain here
        <Meta name="twitter:card" content="summary_large_image"/>
        <MetaImage url=move || {
            format!("https://ultros.app/itemcard/{}/{}", world(), item_id())
        }/>
        <Meta
            property="thumbnail"
            content=move || {
                format!("https://ultros.app/static/itemicon/{}?size=Large", item_id())
            }
        />

        {move || {
            view! { <Link rel="canonical" href=format!("https://ultros.app/item/{}", item_id())/> }
        }}

        <div class="flex flex-column bg-gradient-to-r from-slate-950 -mt-96 pt-96 ">
            <div class="flex flex-row rounded-l items-start p-2">
                <div class="flex flex-column grow" style="padding: 5px">
                    <div class="flex md:flex-row flex-col flex-wrap">
                        <div class="flex flex-row text-2xl gap-1">
                            <ItemIcon item_id icon_size=IconSize::Large/>
                            <div class="flex flex-col">
                                <h1>
                                    {item_name}
                                    <div class="sr-only">
                                        " ffxiv marketboard prices for " {world}
                                    </div>
                                </h1>
                                <div style="font-size: 16px">
                                    <div>
                                        {move || {
                                            item_category()
                                                .and_then(|c| item_search_category().map(|s| (c, s)))
                                                .map(|(c, s)| {
                                                    view! {
                                                        <a
                                                            class="text-fuchsia-300 a:text-fuchsia-600"
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
                            <Clipboard clipboard_text=MaybeSignal::derive(move || {
                                item_name().to_string()
                            })/>
                        </div>
                        <div class="md:ml-auto flex flex-row">
                            <AddToList item_id/>
                            <a
                                class="btn"
                                href=move || format!("https://universalis.app/market/{}", item_id())
                            >
                                "Universalis"
                            </a>
                            <a
                                class="btn text-center"
                                href=move || {
                                    format!("https://garlandtools.org/db/#item/{}", item_id())
                                }
                            >

                                "Garlandtools"
                            </a>
                        </div>
                    </div>
                    <div class="flex flex-row gap-1">
                        <div
                            class="flex flex-row"
                            class:collapse=move || {
                                item().map(|i| i.price_low == 0).unwrap_or_default()
                            }
                        >

                            "Sells to a vendor for: "
                            <Gil amount=MaybeSignal::derive(move || {
                                item().map(|i| i.price_low).unwrap_or_default() as i32
                            })/>
                        </div>
                    </div>
                    <div>
                        "Item level: "
                        <span style="color: #abc; width: 50px;">
                            {move || item().map(|item| item.level_item.0).unwrap_or_default()}
                        </span>
                    </div>
                    <div>{move || view! { <UIText text=item_description().to_string()/> }}</div>
                    <div>{move || view! { <ItemStats item_id=ItemId(item_id())/> }}</div>
                    <Ad class="h-[90px] w-full"/>
                </div>
            </div>
            <WorldMenu world_name=world item_id/>
        </div>
        <div class="main-content flex-wrap">
            <ListingsContent item_id world/>
            <RelatedItems item_id=Signal::from(item_id)/>
        </div>
    }
}
