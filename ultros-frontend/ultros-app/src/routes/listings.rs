use leptos::*;
use leptos_router::use_params_map;
use xiv_gen::ItemId;
use leptos_meta::*;

use crate::api::get_listings;

#[component]
pub fn Listings(cx: Scope) -> impl IntoView {
    // get world and item id from scope
    let params = use_params_map(cx);
    
    let item_id : i32 = params.get().get("id").map(|id| id.parse().ok()).flatten().unwrap_or_default();
    let item = &xiv_gen_db::decompress_data().items;
    let item = match item.get(&ItemId(item_id)) {
        Some(i) => i,
        None => panic!("unsupported item id") //return view!{cx, <div>"Unable to get item!"</div>},
    };

    let listings = create_resource(cx, move || {
        let map = params.get();
        let world = map.get("world").cloned().unwrap_or_default();
        world
    }, move |world| async move {
        get_listings(cx, item_id, &world).await
    });
    let world = params.get().get("world").cloned().unwrap_or_default();
    let description = format!("Current listings for world {world} for {}", item.name);
    view! {
        cx,
        <Meta name="description" content=description/>
        <div class="container">
            <div class="flex-row">
                
            </div>
            <div class="main-content flex-wrap">
                <div class="content-well">
                    <Suspense fallback=|| view!{ cx, "Loading"}>
                        {move || listings.read().map(|listings| {
                            match listings {
                                None => view!{ cx, "No listing"},
                                Some(listings) => view! { cx, "Listings {listings:?}"}
                            }
                        })}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}
