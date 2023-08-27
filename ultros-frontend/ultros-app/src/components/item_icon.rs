use leptos::*;
pub use ultros_api_types::icon_size::IconSize;
use xiv_gen::ItemId;

#[component]
pub fn ItemIcon(item_id: i32, icon_size: IconSize) -> impl IntoView {
    // Currently I only have icons for marketboard items, assume that anything without an item search category won't have an icon
    let valid_icon = xiv_gen_db::data()
        .items
        .get(&ItemId(item_id))
        .map(|item| item.item_search_category.0 > 0)
        .unwrap_or_default();
    let (failed, set_failed) = create_signal(!valid_icon);
    view! {

        <img class=icon_size.get_class()
            src=move || { if !failed() {
                format!("/static/itemicon/{item_id}?size={}", icon_size)
            } else {
                "/static/itemicon/fallback".to_string()
            } } loading="lazy" on:error=move |_| {
            set_failed(true);
        } />
    }
}
