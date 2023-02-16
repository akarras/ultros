use std::cmp::Reverse;

use crate::components::{cheapest_price::*, fonts::*, item_icon::*, tooltip::*};
use leptos::*;
use ultros_api_types::icon_size::IconSize;
use leptos_router::*;
use urlencoding::{decode, encode};

/// Displays buttons of categories
#[component]
fn CategoryView(cx: Scope, category: u8) -> impl IntoView {
    let data = xiv_gen_db::decompress_data();
    let search_categories = &data.item_search_categorys;
    // let item_ui_category = &data.item_ui_categorys;
    let mut categories = search_categories
        .iter()
        .filter(|(_, cat)| cat.category == category)
        .map(|(id, cat)| {
            // lookup the ID for the map
            (cat.order, &cat.name, id)
        })
        .collect::<Vec<_>>();
    categories.sort_by_key(|(order, _, _)| *order);
    view! {cx,
        <div class="flex flex-row flex-wrap">
        {categories.into_iter()
            .map(|(_, name, id)| view! {cx,
                <Tooltip tooltip_text=name.to_string()>
                    <A  href=format!("/items/{}", encode(name))>
                        <ItemSearchCategoryIcon id=*id />
                    </A>
                </Tooltip>
            })
            .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn JobGearView(cx: Scope) -> impl IntoView {
    
}

#[component]
pub fn ItemExplorer(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let data = xiv_gen_db::decompress_data();
    view! {cx,
        <div class="container">
            <div class="main-content flex">
                <div class="flex-column" style="width: 250px">
                    "Weapons:"
                    <CategoryView category=1 />
                    "Armor"
                    <CategoryView category=2 />
                    "Items"
                    <CategoryView category=3 />
                    "Housing"
                    <CategoryView category=4 />
                </div>
                <div class="flex-column">
                    {move || {
                        let cat = params().get("category")?.clone();
                        let cat = decode(&cat).ok()?.into_owned();
                        let category = data.item_search_categorys.iter().find(|(_id, category)| category.name == cat);
                        category.map(|(id, _)| {
                            let mut items = data.items
                                .iter()
                                .filter(|(_, item)| item.item_search_category == *id)
                                .collect::<Vec<_>>();
                            items.sort_by_key(|(_, item)| Reverse(item.level_item.0));
                            items.into_iter().map(|(id, item)| view!{cx, <div class="flex-row">
                                    <ItemIcon item_id=id.0 icon_size=IconSize::Small />
                                    <span style="width: 250px">{&item.name}</span>
                                    <span style="color: #f3a; width: 50px">{item.level_item.0}</span>
                                    <CheapestPrice item_id=*id hq=None />
                                </div>
                            })
                            .collect::<Vec<_>>()
                        })

                    }}
                </div>
            </div>
        </div>
    }
}
