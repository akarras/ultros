use crate::api::get_worlds;
use crate::components::{listings_table::*, sale_history_table::*};
use crate::global_state::LocalWorldData;
use crate::item_icon::*;
use leptos::*;
use leptos_meta::*;
use leptos_router::use_params_map;
use ultros_api_types::world_helper::{AnyResult, WorldHelper};
use xiv_gen::ItemId;

use crate::{api::get_listings, item_icon::IconSize};

#[component]
fn WorldButton(cx: Scope, world_name: String, item_id: i32) -> impl IntoView {
    view! { cx, <a class="btn btn-secondary" href=format!("/listings/{}/{item_id}", world_name)>{world_name}</a>}
}

#[component]
fn WorldMenu(cx: Scope, world_name: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    // for some reason the context version doesn't work
    let worlds = create_resource(
        cx,
        move || {},
        move |_| async move {
            let world_data = get_worlds(cx).await;
            world_data.map(|data| WorldHelper::new(data))
        },
    );
    view! {cx,
        <Suspense fallback=move || view!{ cx, <p>"Loading worlds..."</p>}>
        {move || {
            match worlds() {
                Some(Some(worlds)) => {
                    if let Some(world) = worlds.lookup_world_by_name(&world_name()) {
                        let create_world_button = move |name| view!{cx, <WorldButton world_name=name item_id=item_id()/>};
                        match world {
                            AnyResult::World(world) => {
                                // display the region, datacenter, and sibling worlds to this datacenter (excluding this world)
                                let region = worlds.get_region(AnyResult::World(world));
                                let datacenters = worlds.get_datacenters(&AnyResult::World(world));
                                let views : Vec<_> = [region.name.to_string()]
                                    .into_iter()
                                    .chain(datacenters.iter().map(|dc| dc.name.to_string()))
                                    .chain(datacenters.iter().flat_map(|dc| dc.worlds.iter().filter(|w| w.name != world.name).map(|world| world.name.to_string())))
                                        .map(move |name| view!{cx, <WorldButton world_name=name item_id=item_id()/>})
                                    .collect();
                                view!{cx, {views}}.into_view(cx)
                            },
                            AnyResult::Datacenter(dc) => {
                                // show all worlds in this datacenter, other datacenters in this region, the region this datacenter belongs to
                                let region = worlds.get_region(AnyResult::Datacenter(dc));
                                let views : Vec<_> = [region.name.to_string()].into_iter()
                                    .chain(region.datacenters.iter().filter(|d| dc.name != d.name).map(|d| d.name.to_string()))
                                    .chain(dc.worlds.iter().map(|w| w.name.to_string()))
                                    .map(create_world_button)
                                    .collect();
                                view!{cx, {views}}.into_view(cx)
                            },
                            AnyResult::Region(region) => {
                                // show all regions, and datacenters in this region
                                let datacenters = worlds.get_datacenters(&AnyResult::Region(region));
                                let views : Vec<_> = datacenters.iter()
                                    .map(|dc| dc.name.to_string())
                                    .map(move |name| view!{cx, <WorldButton world_name=name item_id=item_id()/>}).collect();
                                view!{cx, {views}}.into_view(cx)
                            }
                        }
                    } else {
                        view!{cx, <div>"No worlds"</div>}.into_view(cx)
                    }
                }
                _ => view!{cx, <p>"Loading worlds..."</p>}.into_view(cx)
            }
        }}
        </Suspense>
    }
}

#[component]
fn ListingsContent(cx: Scope, item_id: Memo<i32>, world: Memo<String>) -> impl IntoView {
    let listing_resource = create_resource(
        cx,
        move || (item_id(), world()),
        move |(item_id, world)| async move { get_listings(cx, item_id, &world).await },
    );
    view! { cx,
        <div>
            <Suspense fallback=|| view!{ cx, "Loading"}>
            {move || listing_resource().map(|listings| {
                match listings {
                    None => view!{ cx, <div>"Error getting listings"</div>},
                    Some(currently_shown) => {
                        let hq_listings : Vec<_> = currently_shown.listings.iter().cloned().filter(|(listing, _)| listing.hq).collect();
                        let lq_listings : Vec<_> = currently_shown.listings.iter().cloned().filter(|(listing, _)| !listing.hq).collect();
                        view! { cx,
                            <div class="flex flex-wrap">
                                {if !hq_listings.is_empty() {
                                    view!{ cx, <div class="content-well"><span class="content-title">"high quality listings"</span><ListingsTable listings=hq_listings /></div> }.into_any()
                                } else {
                                    view!{ cx, <div></div> }.into_any()
                                }}
                                <div class="content-well"><span class="content-title">"low quality listings"</span><ListingsTable listings=lq_listings /></div>
                                <div class="content-well"><span class="content-title">"sale history"</span><SaleHistoryTable sale_history=currently_shown.sales /></div>
                            </div>
                        }
                    }
                }
            })}
            </Suspense>
        </div>
    }
}

#[component]
pub fn Listings(cx: Scope) -> impl IntoView {
    // get world and item id from scope
    let params = use_params_map(cx);

    let item_id = create_memo(cx, move |_| {
        params()
            .get("id")
            .map(|id| id.parse::<i32>().ok())
            .flatten()
            .unwrap_or_default()
    });
    let items = &xiv_gen_db::decompress_data().items;
    let world = create_memo(cx, move |_| {
        params.with(|p| p.get("world").cloned().unwrap_or_default())
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
    let categories = &xiv_gen_db::decompress_data().item_ui_categorys;
    let description = create_memo(cx, move |_| {
        format!(
            "Current marketboard listings for world {} for {}",
            world(),
            item_name()
        )
    });
    view! {
        cx,
        <Meta name="description" content=move || description()/>
        <div class="container">
            <div class="flex-column">
                <div class="flex-row" style="background-color: rgb(16, 10, 18); margin-bottom: 15px; border-radius: 12px; padding: 14px; line-height: .9;">
                    {move || view!{cx, <ItemIcon item_id=item_id() icon_size=IconSize::Large />}}
                    <div style="padding: 5px">
                        <div class="flex-row" style="padding: 5px">
                            <span style="font-size: 36px; padding: 5px">{move || item_name()}</span>
                            <span style="font-size: 16px; padding: 5px">{move || items.get(&ItemId(item_id())).map(|item| categories.get(&item.item_ui_category)).flatten().map(|i| i.name.as_str()).unwrap_or_default()}</span>
                        </div>
                        <span>{move || item_description()}</span>
                    </div>
                </div>
                <div class="flex-wrap">
                    <WorldMenu world_name=world item_id />
                </div>
            </div>
            <div class="main-content flex-wrap">
                <div class="content-well">
                    <ListingsContent item_id world />
                </div>
            </div>
        </div>
    }
}
