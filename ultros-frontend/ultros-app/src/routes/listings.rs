use crate::components::{listings_table::*, sale_history_table::*};
use crate::item_icon::*;
use leptos::*;
use leptos_meta::*;
use leptos_router::use_params_map;
use xiv_gen::ItemId;

use crate::{api::get_listings, item_icon::IconSize};

#[component]
pub fn Listings(cx: Scope) -> impl IntoView {
    // get world and item id from scope
    let params = use_params_map(cx);

    let item_id: i32 = params
        .get()
        .get("id")
        .map(|id| id.parse().ok())
        .flatten()
        .unwrap_or_default();
    let item = &xiv_gen_db::decompress_data().items;
    let item = match item.get(&ItemId(item_id)) {
        Some(i) => i,
        None => panic!("unsupported item id"), //return view!{cx, <div>"Unable to get item!"</div>},
    };

    let listings = create_resource(
        cx,
        move || {
            let map = params.get();
            let world = map.get("world").cloned().unwrap_or_default();
            world
        },
        move |world| async move { get_listings(cx, item_id, &world).await },
    );
    let world = params.get().get("world").cloned().unwrap_or_default();
    let description = format!("Current listings for world {world} for {}", item.name);
    let icon_size = IconSize::Large;
    let item_name = &item.name;
    view! {
        cx,
        <Meta name="description" content=description/>
        <div class="container">
            <div class="flex-row">
                <ItemIcon item_id icon_size />
                <span>{item_name}</span>
            </div>
            <div class="main-content flex-wrap">
                <div class="content-well">
                    <Suspense fallback=|| view!{ cx, "Loading"}>
                        {move || listings.read().map(|listings| {
                            match listings {
                                None => view!{ cx, <div>"No listing"</div>},
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
            </div>
        </div>
    }
}
