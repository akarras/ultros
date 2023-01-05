use crate::item_icon::*;
use leptos::*;
use xiv_gen::ItemId;

#[component]
pub fn ItemSearchResult(cx: Scope, item_id: i32, icon_size: IconSize) -> impl IntoView {
    let data = xiv_gen_db::decompress_data();
    let categories = &data.item_ui_categorys;
    let items = &data.items;
    let item = items.get(&ItemId(item_id));
    view! {
        cx,
        {if let Some(item) = item {
            view!{cx, <div class="search-result"><a href=format!("/listings/North-America/{item_id}")> // this needs to be updated to be able to point to any region
            <ItemIcon item_id icon_size />
            <div class="search-result-details">
                <span class="item-name">{&item.name}</span>
                <span class="item-type">{categories.get(&item.item_ui_category).map(|i| i.name.as_str()).unwrap_or_default()}</span>
            </div>
        </a></div>}
        } else {
            view!{cx, <div class="search-result"></div>}
        }}
    }
}
