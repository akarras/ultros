use crate::api::get_listings;
use crate::components::recently_viewed::RecentItems;
use crate::components::{
    ad::Ad, clipboard::*, item_icon::*, listings_table::*, loading::*, meta::*,
    price_history_chart::*, related_items::*, sale_history_table::*, stats_display::*, ui_text::*,
};
use crate::global_state::home_world::get_price_zone;
use crate::global_state::LocalWorldData;
use leptos::*;
use leptos_meta::Link;
use leptos_meta::Meta;
use leptos_router::escape;
use leptos_router::*;
use ultros_api_types::world_helper::{AnyResult, OwnedResult};
use xiv_gen::ItemId;

#[component]
fn WorldButton<'a>(world: AnyResult<'a>, item_id: i32) -> impl IntoView {
    let world_name = world.get_name().to_owned();
    let bg_color = match world {
        AnyResult::Region(_r) => "bg-violet-950 hover:bg-violet-900",
        AnyResult::Datacenter(_d) => "bg-violet-800 hover:bg-violet-700",
        AnyResult::World(_w) => "bg-violet-600 hover:bg-violet-500",
    };
    view! { <A class=["rounded-t-lg text-sm p-1 aria-current:font-bold aria-current:text-white mx-1 ", bg_color].concat() href=format!("/item/{}/{item_id}", escape(&world_name))>{world_name}</A>}
}

#[component]
fn WorldMenu(world_name: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    // for some reason the context version doesn't work
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    move || {
        let world = world_name();
        let world_name = escape(&world);
        if let Some(world) = world_data.lookup_world_by_name(&world_name) {
            let create_world_button = move |world| view! {<WorldButton world item_id=item_id()/>};
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
                        .map(move |world| view! {<WorldButton world item_id=item_id()/>})
                        .collect();
                    views.into_view()
                }
                AnyResult::Datacenter(dc) => {
                    // show all worlds in this datacenter, other datacenters in this region, the region this datacenter belongs to
                    let region = world_data.get_region(AnyResult::Datacenter(dc));
                    let views: Vec<_> = [AnyResult::Region(region)]
                        .into_iter()
                        .chain(region.datacenters.iter().map(AnyResult::Datacenter))
                        .chain(dc.worlds.iter().map(AnyResult::World))
                        .map(create_world_button)
                        .collect();
                    views.into_view()
                }
                AnyResult::Region(region) => {
                    // show all regions, and datacenters in this region
                    let regions = world_data
                        .get_inner_data()
                        .regions
                        .iter()
                        .map(AnyResult::Region);
                    let datacenters = world_data.get_datacenters(&AnyResult::Region(region));
                    let views: Vec<_> = regions
                        .chain(datacenters.iter().map(|dc| AnyResult::Datacenter(dc)))
                        .map(move |world| view! {<WorldButton world item_id=item_id()/>})
                        .collect();
                    views.into_view()
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
                .map(move |world| view! {<WorldButton world item_id=item_id()/>})
                .collect();
            views.into_view()
        }
    }
}

#[component]
fn ListingsContent(item_id: Memo<i32>, world: Memo<String>) -> impl IntoView {
    let listing_resource = create_resource(
        move || (item_id(), world()),
        move |(item_id, world)| async move { get_listings(item_id, &world).await },
    );
    let (_pending, set_pending) = create_signal(false);
    let _class_opacity = "opacity-0 opacity-50"; // this is just here to get tailwind to compile
    view! {
        // <LargeLoading pending />
        <Transition set_pending=set_pending fallback=Loading>
        {move || listing_resource.get().map(|listings| {
            match listings {
                Err(e) => view!{ <div>{format!("Error getting listings\n{e}")}</div>}.into_view(),
                Ok(currently_shown) => {
                    let hq_listings = currently_shown.listings.iter().cloned().filter(|(listing, _)| listing.hq).collect::<Vec<_>>();
                    let lq_listings = currently_shown.listings.iter().cloned().filter(|(listing, _)| !listing.hq).collect::<Vec<_>>();
                    let sales = create_memo(move |_| currently_shown.sales.clone());
                    view! {
                        <div class="content-well max-h-[35em] overflow-y-auto">
                            <PriceHistoryChart sales=MaybeSignal::from(sales) />
                        </div>
                        {(!hq_listings.is_empty()).then(move || {
                            view!{ <div class="content-well max-h-[35em] overflow-y-auto">
                                <span class="content-title">"high quality listings"</span>
                                <ListingsTable listings=hq_listings />
                            </div> }.into_view()
                        })}
                        <div class="content-well max-h-[35em] overflow-y-auto">
                            <span class="content-title">"low quality listings"</span>
                            <ListingsTable listings=lq_listings />
                        </div>
                        <div class="content-well max-h-[35em] overflow-y-auto">
                            <span class="content-title">"sale history"</span>
                            <SaleHistoryTable sales=Signal::from(sales) />
                        </div>
                    }.into_view()
                }
            }
        })}
        </Transition>
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
    let region_type = move || match get_price_zone().0() {
        Some(OwnedResult::Region(_)) => "region",
        Some(OwnedResult::Datacenter(_)) => "datacenter",
        Some(OwnedResult::World(_)) => "world",
        None => "unknown",
    };
    let description = create_memo(move |_| {
        format!(
            "Current lowest prices and sale history for the item {} within the {} {}. Discover related items and view crafting recipes. {}",
            item_name(),
            region_type(),
            world(),
            item_description()
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
    view! {
        <MetaDescription text=description/>
        <MetaTitle title=move || format!("{} - Market view", item_name())/>
        // TODO: probably shouldn't hard code the domain here
        <Meta name="twitter:card" content="summary_large_image"/>
        <MetaImage url=move || { format!("https://ultros.app/itemcard/{}/{}", world(), item_id()) }/>
        <Meta property="og:image" content=move || { format!("https://ultros.app/static/itemicon/{}?size=Large", item_id())} />
        {move || view!{ <Link rel="canonical" href=format!("https://ultros.app/item/{}", item_id()) /> }}
        <div class="flex flex-column bg-gradient-to-r from-slate-950 -mt-96 pt-96 ">
            <div class="flex flex-row grow p-6 rounded-l ">
                <div class="flex flex-column grow" style="padding: 5px">
                    <div class="flex md:flex-row flex-col flex-wrap">
                        <span class="flex flex-row text-2xl gap-1">
                            <ItemIcon item_id icon_size=IconSize::Large />
                            <div class="flex flex-col">
                                <span>{item_name}</span>
                                <span style="font-size: 16px">{move || item_category().and_then(|c| item_search_category().map(|s| (c, s))).map(|(c, s)| view!{<a class="text-fuchsia-300 a:text-fuchsia-600" href=["/items/category/", &s.name].concat()>
                                    {c.name.as_str()}
                                </a>})}
                            </span></div><Clipboard clipboard_text=MaybeSignal::derive(move || item_name().to_string())/></span>
                        <div class="md:ml-auto flex flex-row" style="align-items:start">
                            <a style="height: 45px" class="btn" href=move || format!("https://universalis.app/market/{}", item_id())>"Universalis"</a>
                            <a style="height: 45px" class="btn" href=move || format!("https://garlandtools.org/db/#item/{}", item_id())>"Garland Tools"</a>
                        </div>
                    </div>
                    <span>{move || view!{<UIText text=item_description().to_string()/>}}</span>
                    {move || view!{<ItemStats item_id=ItemId(item_id()) />}}
                </div>
            </div>
            <div class="content-nav">
                <WorldMenu world_name=world item_id />
            </div>
        </div>
        <div class="main-content flex-wrap">
            <ListingsContent item_id world />
            <Ad class="h-96"/>
            <RelatedItems item_id=Signal::from(item_id) />
        </div>
    }
}
