use crate::components::{listings_table::*, sale_history_table::*};
use leptos::*;
use leptos_meta::*;
use leptos_router::use_params_map;
use xiv_gen::ItemId;
use crate::item_icon::*;

use crate::{api::get_listings, item_icon::IconSize};

#[component]
pub fn Listings(cx: Scope) -> impl IntoView {
    // get world and item id from scope
    let params = use_params_map(cx);

    let item_id = create_memo(cx, move |_| params().get("id")
                .map(|id| id.parse::<i32>().ok())
                .flatten()
                .unwrap_or_default());
    let items = &xiv_gen_db::decompress_data().items;
    let listings = create_resource(
        cx,
        move || {
            params.with(|p| {
                let item_id = p
                    .get("id")
                    .map(|id| id.parse::<i32>().ok())
                    .flatten()
                    .unwrap_or_default();
                let world = p.get("world").cloned().unwrap_or_default();
                (item_id, world)
            })
        },
        move |(item_id, world)| async move { get_listings(cx, item_id, &world).await },
    );
    let world = create_memo(cx, move |_| {
        params.with(|p| p.get("world").cloned().unwrap_or_default())
    });
    let item_name = move || items.get(&ItemId(item_id())).map(|item| item.name.as_str()).unwrap_or_default();
    let categories = &xiv_gen_db::decompress_data().item_ui_categorys;
    
    let description = create_memo(cx, move |_|
        format!("Current listings for world {} for {}", world(), item_name())
    );
    view! {
        cx,
        <Meta name="description" content=move || description()/>
        <div class="container">
            <div class="flex-row">
                
            <div class="search-result">
                // <ItemIcon item_id=move || item_id() icon_size=IconSize::Large />
                <div class="search-result-details">
                    <span class="item-name">{move || item_name()}</span>
                    <span class="item-type">{move || items.get(&ItemId(item_id())).map(|item| categories.get(&item.item_ui_category)).flatten().map(|i| i.name.as_str()).unwrap_or_default()}</span>
                </div>
            </div>
                // <ItemSearchResult item_id=(move || item_id())  icon_size=IconSize::Large />
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
