use crate::item_icon::*;
use leptos::*;

#[component]
pub fn ItemSearchResult(cx: Scope, item_id: i32, item: &'static xiv_gen::Item) -> impl IntoView {
    let item_name = &item.name;
    let categories = &xiv_gen_db::decompress_data().item_ui_categorys;
    let icon_size = IconSize::Medium;
    view! {
        cx,
        <div class="search-result">
            <a href=format!("/listings/North-America/{item_id}")> // this needs to be updated to be able to point to any region
                <ItemIcon item_id icon_size />
                <div class="search-result-details">
                    <span class="item-name">{item_name}</span>
                    <span class="item-type">{categories.get(&item.item_ui_category).map(|i| i.name.as_str()).unwrap_or_default()}</span>
                </div>
            </a>
        </div>
    }
}
