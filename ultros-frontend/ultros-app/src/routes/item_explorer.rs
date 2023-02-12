use std::cmp::Reverse;

use leptos::*;
use leptos_router::use_params_map;
use urlencoding::{encode, decode};
use crate::components::{item_icon::*, fonts::*, tooltip::*};

/// Displays buttons of categories
#[component]
pub fn CategoryView(cx: Scope, category: u8) -> impl IntoView {
    let data = xiv_gen_db::decompress_data();
    let search_categories = &data.item_search_categorys;
    let item_ui_category = &data.item_ui_categorys;
    let mut categories = search_categories
        .iter()
        .filter(|(_, cat)| cat.category == category)
        .flat_map(|(_id, cat)| {
            // lookup the ID for the map
            let category = item_ui_category.iter().find(|(_key, category)| {
                category.icon.0 == cat.icon.0
            }).map(|(key, _)| key);
            if let None = category {
                log::error!("category {cat:?}");   
            }
            category.map(|category| (cat.order, &cat.name, category))
        }
        )
        .collect::<Vec<_>>();
    categories.sort_by_key(|(order, _, _)| *order);
    view! {cx, 
        <div class="flex flex-wrap">
        {categories.into_iter()
            .map(|(_, name, id)| view! {cx, <Tooltip tooltip_text=name.to_string()><a href=format!("/items/{}", encode(name))><ItemUiCategoryIcon id=*id /></a></Tooltip>})
            .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn ItemExplorer(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let data = xiv_gen_db::decompress_data();
    log::info!("item explorer created");
    view! {cx,
        <div class="container">
            <div class="main-content flex">
                <div class="flex-column content-well">
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
                            items.into_iter().map(|(id, item)| view!{cx, <div>
                                    <ItemIcon item_id=id.0 icon_size=IconSize::Small />
                                    {&item.name}
                                    <span style="color: #f3a">{item.level_item.0}</span>
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
