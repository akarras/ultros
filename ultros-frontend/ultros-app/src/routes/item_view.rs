use crate::api::get_listings;
use crate::api::get_worlds;
use crate::components::{
    clipboard::*, item_icon::*, listings_table::*, loading::*, meta::*, price_history_chart::*,
    related_items::*, sale_history_table::*, ui_text::*,
};
use leptos::*;
use leptos_router::*;
use ultros_api_types::world_helper::{AnyResult, WorldHelper};
use xiv_gen::ItemId;

#[component]
fn WorldButton(world_name: String, item_id: i32) -> impl IntoView {
    view! { <A class="btn-secondary" href=format!("/item/{}/{item_id}", urlencoding::encode(&world_name))>{world_name}</A>}
}

#[component]
fn WorldMenu(world_name: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    // for some reason the context version doesn't work
    let worlds = create_resource(
        move || {},
        move |_| async move {
            let world_data = get_worlds().await;
            world_data.map(WorldHelper::new)
        },
    );
    view! {
        <Suspense fallback=move || view!{ <Loading/>}>
        {move || {
            match worlds.get() {
                Some(Ok(worlds)) => {
                    let world = world_name();
                    let world_name = urlencoding::decode(&world).unwrap_or_default();
                    if let Some(world) = worlds.lookup_world_by_name(&world_name) {
                        let create_world_button = move |name| view!{<WorldButton world_name=name item_id=item_id()/>};
                        match world {
                            AnyResult::World(world) => {
                                // display the region, datacenter, and sibling worlds to this datacenter (excluding this world)
                                let region = worlds.get_region(AnyResult::World(world));
                                let datacenters = worlds.get_datacenters(&AnyResult::World(world));
                                let views : Vec<_> = [region.name.to_string()]
                                    .into_iter()
                                    .chain(datacenters.iter().map(|dc| dc.name.to_string()))
                                    .chain(datacenters.iter().flat_map(|dc| dc.worlds.iter()
                                        .map(|world| world.name.to_string())))
                                        .map(move |name| view!{<WorldButton world_name=name item_id=item_id()/>})
                                    .collect();
                                views.into_view()
                            },
                            AnyResult::Datacenter(dc) => {
                                // show all worlds in this datacenter, other datacenters in this region, the region this datacenter belongs to
                                let region = worlds.get_region(AnyResult::Datacenter(dc));
                                let views : Vec<_> = [region.name.to_string()].into_iter()
                                    .chain(region.datacenters.iter().map(|d| d.name.to_string()))
                                    .chain(dc.worlds.iter().map(|w| w.name.to_string()))
                                    .map(create_world_button)
                                    .collect();
                                views.into_view()
                            },
                            AnyResult::Region(region) => {
                                // show all regions, and datacenters in this region
                                let regions = worlds.get_all().regions.iter().map(|r| r.name.to_string());
                                let datacenters = worlds.get_datacenters(&AnyResult::Region(region));
                                let views : Vec<_> = regions.chain(datacenters.iter()
                                    .map(|dc| dc.name.to_string()))
                                    .map(move |name| view!{<WorldButton world_name=name item_id=item_id()/>}).collect();
                                views.into_view()
                            }
                        }
                    } else {
                        view!{<div>"No worlds"</div>}.into_view()
                    }
                }
                _ => view!{<Loading/>}.into_view()
            }
        }}
        </Suspense>
    }
}

#[component]
fn ListingsContent(item_id: Memo<i32>, world: Memo<String>) -> impl IntoView {
    let listing_resource = create_resource(
        move || (item_id(), world()),
        move |(item_id, world)| async move { get_listings(item_id, &world).await },
    );
    view! {
        <Suspense fallback=move || view!{ <Loading/>}>
        {move || listing_resource.get().map(|listings| {
            match listings {
                Err(e) => view!{ <div>{format!("Error getting listings\n{e}")}</div>}.into_view(),
                Ok(currently_shown) => {

                    let hq_listings = currently_shown.listings.iter().cloned().filter(|(listing, _)| listing.hq).collect::<Vec<_>>();
                    let lq_listings = currently_shown.listings.iter().cloned().filter(|(listing, _)| !listing.hq).collect::<Vec<_>>();
                    let sales = create_memo(move |_| currently_shown.sales.clone());
                    view! {
                        <PriceHistoryChart sales=MaybeSignal::from(sales) />
                        {(!hq_listings.is_empty()).then(move || {
                            view!{ <div class="content-well">
                                <span class="content-title">"high quality listings"</span>
                                <ListingsTable listings=hq_listings />
                            </div> }.into_view()
                        })}
                        <div class="content-well">
                            <span class="content-title">"low quality listings"</span>
                            <ListingsTable listings=lq_listings />
                        </div>
                        <div class="content-well">
                            <span class="content-title">"sale history"</span>
                            <SaleHistoryTable sales=Signal::from(sales) />
                        </div>
                    }.into_view()
                }
            }
        })}
        </Suspense>
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
    let items = &xiv_gen_db::data().items;
    let world = create_memo(move |_| params.with(|p| p.get("world").cloned().unwrap_or_default()));
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
    let categories = &xiv_gen_db::data().item_ui_categorys;
    let description =
        create_memo(move |_| format!("Current listings for {} on {}", item_name(), world(),));
    view! {
        <MetaDescription text=description/>
        <MetaTitle title=move || format!("{} - Market view", item_name())/>
        // TODO: probably shouldn't hard code the domain here
        <MetaImage url=move || { format!("https://ultros.app/static/itemicon/{}?size=Large", item_id())}/>
        <div class="flex-column">
            <div class="flex-wrap" style="background-color: rgb(16, 10, 18); margin-bottom: 15px; border-radius: 12px; padding: 14px; line-height: .9; justify-content: space-between;">
                <div class="flex-row">
                    {move || view!{<ItemIcon item_id=item_id() icon_size=IconSize::Large />}}
                    <div class="flex-column" style="padding: 5px">
                        <span class="flex-row" style="font-size: 36px; line-height 0.5;">{item_name}{move || view!{<Clipboard clipboard_text=item_name().to_string()/>}}</span>
                        <span style="font-size: 16px">{move || items.get(&ItemId(item_id())).and_then(|item| categories.get(&item.item_ui_category)).map(|i| i.name.as_str()).unwrap_or_default()}</span>
                        <span>{move || view!{<UIText text=item_description().to_string()/>}}</span>
                    </div>
                </div>
                <div class="flex-row" style="align-items:start">
                    <a style="height: 45px" class="btn" href=move || format!("https://universalis.app/market/{}", item_id())>"Universalis"</a>
                    <a style="height: 45px" class="btn" href=move || format!("https://garlandtools.org/db/#item/{}", item_id())>"Garland Tools"</a>
                </div>
            </div>
            <div class="content-nav">
                <WorldMenu world_name=world item_id />
            </div>
        </div>
        <div class="main-content flex-wrap">
            <ListingsContent item_id world />
            <RelatedItems item_id=Signal::from(item_id) />
        </div>
    }
}
