use leptos::*;
use leptos_router::use_params_map;

/// Displays buttons of categories
#[component]
pub fn CategoryView(cx: Scope, category: u8) -> impl IntoView {
    let data = xiv_gen_db::decompress_data();
    let search_categories = &data.item_search_categorys;
    let mut categories = search_categories
        .iter()
        .filter(|(_, cat)| cat.category == category)
        .map(|(_, cat)| (cat.order, cat.name.to_string()))
        .collect::<Vec<_>>();
    categories.sort_by_key(|(order, _)| *order);
    view! {cx, <div class="flex flex-wrap">
        {
            categories.into_iter()
            .map(|(_, name)| view! {cx, <a href=format!("/items/{}", name)>{&name}</a>})
            .collect::<Vec<_>>()}
    </div>
    }
}

#[component]
pub fn ItemExplorer(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let data = xiv_gen_db::decompress_data();
    view! {cx,
        <div class="container">
            <div class="main-content flex-wrap">
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
                        let category = data.item_search_categorys.iter().find(|(_id, category)| category.name == cat);
                        category.map(|(id, _)| {
                            data.items
                                .iter()
                                .filter(|(_, item)| item.item_search_category == *id)
                                .map(|(_, item)| view!{cx, <div>{&item.name}</div>})
                                .collect::<Vec<_>>()
                        })

                    }}
                </div>
            </div>
        </div>
    }
}
